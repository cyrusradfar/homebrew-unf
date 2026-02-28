//! Sentinel watchdog for the UNFUDGED daemon.
//!
//! The sentinel is a lightweight process that monitors the daemon and ensures
//! the runtime registry (`projects.json`) stays in sync with the user's intent
//! (`intent.json`). It runs on a 15-second tick and:
//!
//! 1. Checks if the daemon is alive; respawns it if crashed
//! 2. Compares intent vs registry; reconciles any drift
//! 3. Removes stale stopped sentinel files for intended projects
//!
//! The sentinel is the single point of auto-restart. The OS KeepAlive
//! mechanism (launchd / systemd) keeps the sentinel alive; the sentinel
//! keeps the daemon alive.

use std::env;
use std::fs;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::audit;
use crate::error::UnfError;
use crate::intent::{self, Intent};
use crate::registry::{self, Registry};
use crate::storage;

/// Default tick interval for sentinel health checks (seconds).
const DEFAULT_TICK_INTERVAL_SECS: u64 = 15;

/// Returns the tick interval, allowing override via `UNF_SENTINEL_TICK_SECS`.
fn tick_interval_secs() -> u64 {
    std::env::var("UNF_SENTINEL_TICK_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_TICK_INTERVAL_SECS)
}

/// What kind of drift was detected between intent and registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftKind {
    /// Project is in intent but missing from registry.
    MissingFromRegistry,
    /// Project is in registry but not in intent (user unwatched it).
    ExtraInRegistry,
}

/// A single drift entry between intent and registry.
#[derive(Debug, Clone)]
pub struct DriftEntry {
    /// The project path.
    pub path: PathBuf,
    /// The kind of drift.
    pub kind: DriftKind,
}

/// Compares intent vs registry and returns a list of drifted entries.
///
/// Pure function — no I/O. Fully testable.
pub fn compute_drift(intent: &Intent, registry: &Registry) -> Vec<DriftEntry> {
    let mut drift = Vec::new();

    // Projects in intent but not in registry
    for entry in &intent.projects {
        if !registry.projects.iter().any(|p| p.path == entry.path) {
            drift.push(DriftEntry {
                path: entry.path.clone(),
                kind: DriftKind::MissingFromRegistry,
            });
        }
    }

    // Projects in registry but not in intent
    for entry in &registry.projects {
        if !intent.projects.iter().any(|p| p.path == entry.path) {
            drift.push(DriftEntry {
                path: entry.path.clone(),
                kind: DriftKind::ExtraInRegistry,
            });
        }
    }

    drift
}

/// Formats drift entries for audit logging.
fn format_drift(drift: &[DriftEntry]) -> String {
    drift
        .iter()
        .map(|d| {
            let kind = match d.kind {
                DriftKind::MissingFromRegistry => "+",
                DriftKind::ExtraInRegistry => "-",
            };
            format!("{}:{}", kind, d.path.display())
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Runs the sentinel watchdog loop.
///
/// This is the entry point for `unf __sentinel`. It:
/// 1. Writes a PID file at `~/.unfudged/sentinel.pid`
/// 2. Sets up SIGTERM handler for graceful shutdown
/// 3. Loops every 15 seconds checking daemon health and reconciling intent
///
/// Exits when SIGTERM is received or the global stopped marker exists.
pub fn run_sentinel() -> Result<(), UnfError> {
    // Check for global stopped marker
    let stopped_path = storage::global_stopped_path()?;
    if stopped_path.exists() {
        return Ok(());
    }

    // Write sentinel PID file
    let pid_path = storage::sentinel_pid_path()?;
    write_pid_file(&pid_path)?;

    // --- Boot-time initialization (replaces __boot one-shot agent) ---
    // Clear global stopped marker (fresh login = clean start)
    if let Ok(stopped_path) = storage::global_stopped_path() {
        let _ = std::fs::remove_file(&stopped_path);
    }

    // Clear per-project stopped files for intended projects
    if let Ok(intent) = intent::load() {
        for proj in &intent.projects {
            if let Ok(sd) = storage::resolve_storage_dir_canonical(&proj.path) {
                let _ = std::fs::remove_file(storage::stopped_path(&sd));
            }
        }
    }

    // Prune stale registry entries
    match registry::prune_stale_entries() {
        Ok(pruned) if pruned > 0 => {
            eprintln!("sentinel: pruned {} stale registry entries", pruned);
        }
        Err(e) => {
            eprintln!("sentinel: failed to prune registry: {}", e);
        }
        _ => {}
    }

    audit::log_event("SENTINEL_BOOT", "boot initialization completed");

    // Set up graceful shutdown
    let shutdown = Arc::new(AtomicBool::new(false));
    {
        let shutdown_flag = Arc::clone(&shutdown);
        let _ = ctrlc::set_handler(move || {
            shutdown_flag.store(true, Ordering::SeqCst);
        });
    }

    // Also handle SIGTERM via signal-hook for non-interactive termination
    let term_flag = Arc::clone(&shutdown);
    let _ = signal_hook::flag::register(signal_hook::consts::SIGTERM, term_flag);

    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        // Check for global stopped marker each tick
        if let Ok(sp) = storage::global_stopped_path() {
            if sp.exists() {
                break;
            }
        }

        // 1. Daemon health check
        if let Err(e) = check_daemon_health() {
            eprintln!("sentinel: daemon health check error: {}", e);
        }

        // 2. Intent reconciliation
        if let Err(e) = reconcile_intent() {
            eprintln!("sentinel: reconciliation error: {}", e);
        }

        // Sleep in small increments so shutdown is responsive
        let tick = tick_interval_secs();
        for _ in 0..(tick * 10) {
            if shutdown.load(Ordering::SeqCst) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    // Clean up PID file
    let _ = fs::remove_file(&pid_path);

    Ok(())
}

/// Checks if the daemon is alive; respawns if not.
fn check_daemon_health() -> Result<(), UnfError> {
    let global_pid_path = storage::global_pid_path()?;

    if is_daemon_alive(&global_pid_path) {
        return Ok(());
    }

    // Check if there are projects to watch
    if !registry::has_projects().unwrap_or(false) {
        // Also check intent
        if let Ok(intent) = intent::load() {
            if intent.projects.is_empty() {
                return Ok(());
            }
        } else {
            return Ok(());
        }
    }

    audit::log_event("DAEMON_CRASH", "detected dead daemon, respawning");
    spawn_daemon(&global_pid_path)?;

    if let Ok(pid_str) = fs::read_to_string(&global_pid_path) {
        audit::log_event("DAEMON_START", &format!("pid={}", pid_str.trim()));
    }

    Ok(())
}

/// Checks whether the daemon process is alive by reading its PID file.
fn is_daemon_alive(global_pid_path: &Path) -> bool {
    if let Ok(pid_str) = fs::read_to_string(global_pid_path) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            return crate::process::is_alive(pid);
        }
    }
    false
}

/// Reconciles intent.json against projects.json and clears stale stopped files.
fn reconcile_intent() -> Result<(), UnfError> {
    let intent = intent::load()?;
    let reg = registry::load()?;

    let drift = compute_drift(&intent, &reg);

    if !drift.is_empty() {
        audit::log_event("REGISTRY_DRIFT", &format_drift(&drift));
        reconcile(&drift)?;

        // Signal daemon to reload
        signal_daemon_reload()?;

        audit::log_event("SENTINEL_RECONCILE", &format_drift(&drift));
    }

    // Always clear stale stopped files for intended projects, even without drift.
    // A stopped file may be left behind by `unf stop` but the user re-watched.
    clear_stale_stopped_files(&intent);

    Ok(())
}

/// Removes per-project stopped files for projects the user intends to watch.
fn clear_stale_stopped_files(intent: &Intent) {
    for entry in &intent.projects {
        if let Ok(storage_dir) = storage::resolve_storage_dir_canonical(&entry.path) {
            let stopped = storage::stopped_path(&storage_dir);
            if stopped.exists() {
                let _ = fs::remove_file(&stopped);
            }
        }
    }
}

/// Applies drift corrections: adds missing projects to registry, removes extras,
/// and clears stale stopped sentinel files for intended projects.
fn reconcile(drift: &[DriftEntry]) -> Result<(), UnfError> {
    let mut reg = registry::load()?;
    let mut changed = false;

    for entry in drift {
        match entry.kind {
            DriftKind::MissingFromRegistry => {
                // Add to registry if not already present
                if !reg.projects.iter().any(|p| p.path == entry.path) {
                    reg.projects.push(registry::ProjectEntry {
                        path: entry.path.clone(),
                        registered: chrono::Utc::now(),
                    });
                    changed = true;
                }

                // Remove stopped sentinel file so daemon will watch this project
                if let Ok(storage_dir) = storage::resolve_storage_dir_canonical(&entry.path) {
                    let stopped = storage::stopped_path(&storage_dir);
                    let _ = fs::remove_file(&stopped);
                }
            }
            DriftKind::ExtraInRegistry => {
                let before = reg.projects.len();
                reg.projects.retain(|p| p.path != entry.path);
                if reg.projects.len() != before {
                    changed = true;
                }
            }
        }
    }

    if changed {
        registry::save(&reg)?;
    }

    Ok(())
}

/// Spawns the global daemon process, reusing the same pattern as cli/watch.rs.
fn spawn_daemon(global_pid_path: &Path) -> Result<(), UnfError> {
    let current_exe = env::current_exe().map_err(|e| {
        UnfError::Watcher(crate::error::WatcherError::Io(std::io::Error::other(
            format!("Failed to get current executable path: {}", e),
        )))
    })?;

    let child = Command::new(&current_exe)
        .arg("__daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| {
            UnfError::Watcher(crate::error::WatcherError::Io(std::io::Error::other(
                format!("Failed to spawn daemon: {}", e),
            )))
        })?;

    let pid = child.id();
    write_pid_file_with_pid(global_pid_path, pid)?;

    Ok(())
}

/// Sends SIGUSR1 to the daemon to trigger a registry reload.
fn signal_daemon_reload() -> Result<(), UnfError> {
    let global_pid_path = storage::global_pid_path()?;
    if let Ok(pid_str) = fs::read_to_string(&global_pid_path) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            if crate::process::is_alive(pid) {
                let _ = crate::process::send_signal(pid, signal_hook::consts::SIGUSR1);
            }
        }
    }
    Ok(())
}

/// Writes the current process PID to a file.
fn write_pid_file(path: &Path) -> Result<(), UnfError> {
    write_pid_file_with_pid(path, std::process::id())
}

/// Writes a specific PID to a file.
fn write_pid_file_with_pid(path: &Path, pid: u32) -> Result<(), UnfError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            UnfError::InvalidArgument(format!("Failed to create PID file directory: {}", e))
        })?;
    }

    let mut file = fs::File::create(path)
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to create PID file: {}", e)))?;

    file.write_all(pid.to_string().as_bytes())
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to write PID file: {}", e)))?;

    Ok(())
}

/// Checks if the sentinel is currently running by reading its PID file.
pub fn is_sentinel_alive() -> bool {
    if let Ok(pid_path) = storage::sentinel_pid_path() {
        if let Ok(pid_str) = fs::read_to_string(&pid_path) {
            if let Ok(pid) = pid_str.trim().parse::<u32>() {
                return crate::process::is_alive(pid);
            }
        }
    }
    false
}

/// Spawns the sentinel process if it's not already running.
pub fn ensure_sentinel_running() -> Result<(), UnfError> {
    if is_sentinel_alive() {
        return Ok(());
    }

    // Check for global stopped marker
    let stopped_path = storage::global_stopped_path()?;
    if stopped_path.exists() {
        return Ok(());
    }

    let current_exe = env::current_exe().map_err(|e| {
        UnfError::Watcher(crate::error::WatcherError::Io(std::io::Error::other(
            format!("Failed to get current executable path: {}", e),
        )))
    })?;

    let child = Command::new(&current_exe)
        .arg("__sentinel")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| {
            UnfError::Watcher(crate::error::WatcherError::Io(std::io::Error::other(
                format!("Failed to spawn sentinel: {}", e),
            )))
        })?;

    let pid = child.id();
    let pid_path = storage::sentinel_pid_path()?;
    write_pid_file_with_pid(&pid_path, pid)?;

    Ok(())
}

/// Kills the sentinel process if running.
pub fn kill_sentinel() -> Result<(), UnfError> {
    let pid_path = storage::sentinel_pid_path()?;
    if let Ok(pid_str) = fs::read_to_string(&pid_path) {
        if let Ok(pid) = pid_str.trim().parse::<u32>() {
            if crate::process::is_alive(pid) {
                let _ = crate::process::terminate(pid);
                // Wait briefly for exit
                for _ in 0..20 {
                    if !crate::process::is_alive(pid) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
    let _ = fs::remove_file(&pid_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::{Intent, IntentEntry};
    use crate::registry::{ProjectEntry, Registry};
    use std::path::PathBuf;

    #[test]
    fn compute_drift_empty_both() {
        let intent = Intent::default();
        let registry = Registry::default();
        let drift = compute_drift(&intent, &registry);
        assert!(drift.is_empty());
    }

    #[test]
    fn compute_drift_missing_from_registry() {
        let intent = Intent {
            projects: vec![IntentEntry {
                path: PathBuf::from("/foo/bar"),
                watched_at: chrono::Utc::now(),
            }],
        };
        let registry = Registry::default();
        let drift = compute_drift(&intent, &registry);
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].path, PathBuf::from("/foo/bar"));
        assert_eq!(drift[0].kind, DriftKind::MissingFromRegistry);
    }

    #[test]
    fn compute_drift_extra_in_registry() {
        let intent = Intent::default();
        let registry = Registry {
            projects: vec![ProjectEntry {
                path: PathBuf::from("/foo/bar"),
                registered: chrono::Utc::now(),
            }],
        };
        let drift = compute_drift(&intent, &registry);
        assert_eq!(drift.len(), 1);
        assert_eq!(drift[0].path, PathBuf::from("/foo/bar"));
        assert_eq!(drift[0].kind, DriftKind::ExtraInRegistry);
    }

    #[test]
    fn compute_drift_in_sync() {
        let path = PathBuf::from("/foo/bar");
        let intent = Intent {
            projects: vec![IntentEntry {
                path: path.clone(),
                watched_at: chrono::Utc::now(),
            }],
        };
        let registry = Registry {
            projects: vec![ProjectEntry {
                path: path.clone(),
                registered: chrono::Utc::now(),
            }],
        };
        let drift = compute_drift(&intent, &registry);
        assert!(drift.is_empty());
    }

    #[test]
    fn compute_drift_mixed() {
        let intent = Intent {
            projects: vec![
                IntentEntry {
                    path: PathBuf::from("/project/a"),
                    watched_at: chrono::Utc::now(),
                },
                IntentEntry {
                    path: PathBuf::from("/project/b"),
                    watched_at: chrono::Utc::now(),
                },
            ],
        };
        let registry = Registry {
            projects: vec![
                ProjectEntry {
                    path: PathBuf::from("/project/a"),
                    registered: chrono::Utc::now(),
                },
                ProjectEntry {
                    path: PathBuf::from("/project/c"),
                    registered: chrono::Utc::now(),
                },
            ],
        };
        let drift = compute_drift(&intent, &registry);
        assert_eq!(drift.len(), 2);

        let missing: Vec<_> = drift
            .iter()
            .filter(|d| d.kind == DriftKind::MissingFromRegistry)
            .collect();
        let extra: Vec<_> = drift
            .iter()
            .filter(|d| d.kind == DriftKind::ExtraInRegistry)
            .collect();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].path, PathBuf::from("/project/b"));
        assert_eq!(extra.len(), 1);
        assert_eq!(extra[0].path, PathBuf::from("/project/c"));
    }

    #[test]
    fn format_drift_output() {
        let drift = vec![
            DriftEntry {
                path: PathBuf::from("/project/a"),
                kind: DriftKind::MissingFromRegistry,
            },
            DriftEntry {
                path: PathBuf::from("/project/b"),
                kind: DriftKind::ExtraInRegistry,
            },
        ];
        let formatted = format_drift(&drift);
        assert!(formatted.contains("+:/project/a"));
        assert!(formatted.contains("-:/project/b"));
    }

    #[test]
    fn format_drift_empty() {
        let drift: Vec<DriftEntry> = Vec::new();
        let formatted = format_drift(&drift);
        assert!(formatted.is_empty());
    }
}
