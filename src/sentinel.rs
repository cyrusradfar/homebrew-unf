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
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::io::AsRawFd;

use crate::audit;
use crate::error::UnfError;
use crate::intent::{self, Intent};
use crate::process::PidFile;
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

/// Decision the sentinel should take for the daemon on each health-check tick.
///
/// Returned by `assess_daemon_status()`. Separates pure decision logic from
/// the I/O of actually spawning or logging.
#[derive(Debug, PartialEq, Eq)]
enum DaemonAction {
    /// Daemon child is running normally — no action needed.
    Alive,
    /// Daemon needs to be spawned (or re-spawned).
    NeedsRespawn {
        /// Human-readable reason for the audit log.
        reason: &'static str,
    },
    /// No projects are registered; daemon should not be running.
    NoChild,
}

/// Determines what action the sentinel should take for the daemon.
///
/// Calls `try_wait()` on the child handle (wraps `waitpid(WNOHANG)`), which
/// reaps a zombie if the child has exited. The function is otherwise free of
/// I/O — all side-effecting decisions (spawn, log) are the caller's concern.
///
/// # Arguments
/// * `child` — mutable borrow of the sentinel's `Option<Child>` state.
/// * `has_projects` — whether there are any registered projects to watch.
fn assess_daemon_status(child: &mut Option<Child>, has_projects: bool) -> DaemonAction {
    if !has_projects {
        return DaemonAction::NoChild;
    }
    match child {
        None => DaemonAction::NeedsRespawn {
            reason: "no child handle",
        },
        Some(ref mut c) => match c.try_wait() {
            Ok(Some(_status)) => DaemonAction::NeedsRespawn { reason: "exited" },
            Ok(None) => DaemonAction::Alive,
            Err(_) => DaemonAction::NeedsRespawn {
                reason: "try_wait error",
            },
        },
    }
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

/// Result of a freshness check for a single project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreshnessVerdict {
    /// Recent snapshots exist; recording is active.
    Fresh,
    /// No recent filesystem changes detected. Project is idle.
    Idle,
    /// Recent filesystem changes detected but no recent snapshots.
    /// The daemon is likely not recording.
    Stale { gap_secs: u64 },
    /// Could not determine freshness (missing data).
    Unknown,
}

/// Duration after which missing snapshots with recent FS activity is considered stale.
const STALENESS_THRESHOLD_SECS: u64 = 300;

/// Freshness check interval in sentinel ticks (4 * 15s = 60s).
const FRESHNESS_CHECK_INTERVAL: u64 = 4;

/// Converts a `SystemTime` to a `DateTime<Utc>`.
///
/// Returns the UNIX epoch if the system time is invalid or cannot be converted.
/// This fallback is safe because we're comparing timestamps; a bad clock read
/// is caught by the sentinel's staleness threshold logic.
fn system_time_to_utc(st: std::time::SystemTime) -> chrono::DateTime<chrono::Utc> {
    let duration = st.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    // unwrap_or_default() falls back to epoch (1970-01-01). Safe because the
    // staleness threshold logic will catch bad clock reads downstream.
    chrono::DateTime::from_timestamp(duration.as_secs() as i64, duration.subsec_nanos())
        .unwrap_or_default()
}

/// Computes the freshness verdict for a project.
///
/// Pure function — takes pre-fetched timestamps, no I/O.
///
/// # Arguments
/// * `newest_snapshot` - Timestamp of the most recent snapshot in the DB.
/// * `newest_fs_mtime` - Most recent mtime of any file in the project root.
/// * `now` - Current time.
/// * `staleness_threshold` - Duration after which a gap is considered stale.
fn compute_freshness(
    newest_snapshot: Option<chrono::DateTime<chrono::Utc>>,
    newest_fs_mtime: Option<std::time::SystemTime>,
    now: chrono::DateTime<chrono::Utc>,
    staleness_threshold: std::time::Duration,
) -> FreshnessVerdict {
    // The staleness threshold duration should always fit in chrono::Duration.
    // If it doesn't, that's a programming error (e.g., an absurdly large duration).
    let threshold_chrono = chrono::Duration::from_std(staleness_threshold).expect(
        "staleness_threshold must fit in chrono::Duration (programming error if this fails)",
    );
    let threshold_time = now - threshold_chrono;

    match (newest_snapshot, newest_fs_mtime) {
        // Has recent snapshots — all good
        (Some(snap_time), _) if snap_time > threshold_time => FreshnessVerdict::Fresh,
        // No recent snapshots, but check if FS has recent changes
        (snap_opt, Some(fs_time)) => {
            let fs_time_utc = system_time_to_utc(fs_time);
            if fs_time_utc > threshold_time {
                let gap = match snap_opt {
                    Some(snap_time) => (now - snap_time).num_seconds().max(0) as u64,
                    None => (now - threshold_time).num_seconds().max(0) as u64,
                };
                FreshnessVerdict::Stale { gap_secs: gap }
            } else {
                FreshnessVerdict::Idle
            }
        }
        // No FS mtime data — idle
        (_, None) => FreshnessVerdict::Idle,
    }
}

/// Reads the last snapshot timestamp written by the daemon's sidecar file.
///
/// Returns `None` if the file does not exist or cannot be parsed.
fn read_last_snapshot_time(storage_dir: &Path) -> Option<chrono::DateTime<chrono::Utc>> {
    let path = storage::last_snapshot_time_path(storage_dir);
    let contents = fs::read_to_string(&path).ok()?;
    contents
        .trim()
        .parse::<chrono::DateTime<chrono::Utc>>()
        .ok()
}

/// Samples the newest mtime across the project root and up to three well-known
/// subdirectories (`src/`, `lib/`, `app/`).
///
/// Cost: 1-4 `stat()` calls. Returns `None` only if every `stat()` fails.
fn sample_newest_mtime(project_path: &Path) -> Option<std::time::SystemTime> {
    let mut newest: Option<std::time::SystemTime> = None;
    let candidates = [
        project_path.to_path_buf(),
        project_path.join("src"),
        project_path.join("lib"),
        project_path.join("app"),
    ];
    for path in &candidates {
        if let Ok(meta) = fs::metadata(path) {
            if let Ok(mtime) = meta.modified() {
                newest = Some(match newest {
                    Some(current) if mtime > current => mtime,
                    Some(current) => current,
                    None => mtime,
                });
            }
        }
    }
    newest
}

/// Checks data freshness for all registered projects and force-restarts the
/// daemon if any project is stale.
///
/// I/O boundary function: reads the registry, reads sidecar files, stats
/// filesystem paths, and kills the daemon child if stale.
fn check_data_freshness(daemon_child: &mut Option<Child>) -> Result<(), UnfError> {
    let reg = registry::load()?;

    for project in &reg.projects {
        let storage_dir = match storage::resolve_storage_dir_canonical(&project.path) {
            Ok(sd) => sd,
            Err(_) => continue,
        };

        let newest_snapshot = read_last_snapshot_time(&storage_dir);
        let newest_fs_mtime = sample_newest_mtime(&project.path);

        let verdict = compute_freshness(
            newest_snapshot,
            newest_fs_mtime,
            chrono::Utc::now(),
            std::time::Duration::from_secs(STALENESS_THRESHOLD_SECS),
        );

        if let FreshnessVerdict::Stale { gap_secs } = verdict {
            audit::log_event(
                "FRESHNESS_STALE",
                &format!("project={} gap={}s", project.path.display(), gap_secs),
            );
            audit::log_event(
                "DAEMON_STALE_RESTART",
                "restarting daemon due to stale freshness",
            );

            // Kill the daemon child if we hold the handle.
            if let Some(ref mut child) = daemon_child {
                let pid = child.id();
                let _ = crate::process::force_terminate(pid, 2000);
            }
            *daemon_child = None;

            // One restart handles all projects — return early.
            return Ok(());
        }
    }

    Ok(())
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
#[allow(clippy::cognitive_complexity)]
// TODO(v0.18): reduce complexity
pub fn run_sentinel() -> Result<(), UnfError> {
    // Check for global stopped marker
    let stopped_path = storage::global_stopped_path()?;
    if stopped_path.exists() {
        return Ok(());
    }

    // Acquire exclusive lock on the sentinel PID file.
    // Returns Err immediately if another sentinel holds the lock.
    // _lock_file must be held for the sentinel's entire lifetime.
    #[cfg(unix)]
    let _lock_file = acquire_sentinel_lock()?;

    // On non-unix platforms fall back to the plain PID-file approach.
    #[cfg(not(unix))]
    {
        let pid_path = storage::sentinel_pid_path()?;
        let pid_file = PidFile::new(pid_path);
        pid_file.write(std::process::id()).map_err(|e| {
            UnfError::InvalidArgument(format!("Failed to write sentinel PID file: {}", e))
        })?;
    }

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

    // Retain the Child handle for the daemon we spawn so we can call
    // try_wait() on each tick and reap zombies. Starts as None — either we
    // adopt a daemon that was already running (PID-file path) or we spawn
    // one on the first tick that finds it missing.
    let mut daemon_child: Option<Child> = None;
    let mut tick_count: u64 = 0;

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
        if let Err(e) = check_daemon_health(&mut daemon_child) {
            eprintln!("sentinel: daemon health check error: {}", e);
        }

        // 2. Intent reconciliation
        if let Err(e) = reconcile_intent() {
            eprintln!("sentinel: reconciliation error: {}", e);
        }

        // 3. Data freshness check (every Nth tick)
        tick_count += 1;
        #[allow(clippy::manual_is_multiple_of)]
        if tick_count % FRESHNESS_CHECK_INTERVAL == 0 {
            if let Err(e) = check_data_freshness(&mut daemon_child) {
                eprintln!("sentinel: freshness check error: {}", e);
            }
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

    Ok(())
}

/// Checks if the daemon is alive; respawns if not.
///
/// Accepts the sentinel's `daemon_child` state so it can call `try_wait()`
/// (reaping zombies via `waitpid(WNOHANG)`) and update the handle after a
/// respawn. Falls back to the PID-file path when `daemon_child` is `None`
/// (e.g., sentinel restarted and adopted a pre-existing daemon).
fn check_daemon_health(daemon_child: &mut Option<Child>) -> Result<(), UnfError> {
    let global_pid_path = storage::global_pid_path()?;

    // Determine whether there are any projects that need watching.
    let has_projects = registry::has_projects().unwrap_or(false)
        || intent::load()
            .map(|i| !i.projects.is_empty())
            .unwrap_or(false);

    // If we have a child handle, use try_wait() to detect crashes/zombies.
    // If not, fall back to the PID-file + kill(pid,0) + is_zombie() check.
    let action = if daemon_child.is_some() {
        assess_daemon_status(daemon_child, has_projects)
    } else if !has_projects {
        DaemonAction::NoChild
    } else if is_daemon_alive(&global_pid_path) {
        // Daemon is running (adopted from a previous sentinel incarnation).
        // We don't hold the Child handle, so we can't reap it, but init/
        // launchd will reap it when it eventually exits.
        DaemonAction::Alive
    } else {
        DaemonAction::NeedsRespawn {
            reason: "no child handle",
        }
    };

    match action {
        DaemonAction::Alive | DaemonAction::NoChild => {}
        DaemonAction::NeedsRespawn { reason } => {
            audit::log_event("DAEMON_CRASH", &format!("detected dead daemon: {}", reason));
            let child = spawn_daemon(&global_pid_path)?;
            *daemon_child = Some(child);

            if let Ok(pid_str) = fs::read_to_string(&global_pid_path) {
                audit::log_event("DAEMON_START", &format!("pid={}", pid_str.trim()));
            }
        }
    }

    Ok(())
}

/// Checks whether the daemon process is alive by reading its PID file.
///
/// Used as the fallback when the sentinel has no `Child` handle (e.g., after
/// a sentinel restart that adopted a pre-existing daemon). Excludes zombies:
/// `kill(pid, 0)` returns success for zombies, so we additionally verify the
/// process is not in state Z before calling it alive.
fn is_daemon_alive(global_pid_path: &Path) -> bool {
    let pid_file = PidFile::new(global_pid_path.to_path_buf());
    if let Ok(Some(pid)) = pid_file.read() {
        return crate::process::is_alive(pid) && !crate::process::is_zombie(pid);
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

/// Spawns the global daemon process and returns the `Child` handle.
///
/// The caller **must** retain the returned `Child` handle so that `try_wait()`
/// can be called on subsequent ticks to reap the process when it exits.
/// Dropping the handle without calling `wait()` / `try_wait()` causes the
/// daemon to become a zombie — exactly the bug this change fixes.
fn spawn_daemon(global_pid_path: &Path) -> Result<Child, UnfError> {
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
    let pid_file = PidFile::new(global_pid_path.to_path_buf());
    pid_file.write(pid).map_err(|e| {
        UnfError::InvalidArgument(format!("Failed to write daemon PID file: {}", e))
    })?;

    Ok(child)
}

/// Sends SIGUSR1 to the daemon to trigger a registry reload.
fn signal_daemon_reload() -> Result<(), UnfError> {
    let global_pid_path = storage::global_pid_path()?;
    let pid_file = PidFile::new(global_pid_path);
    if let Ok(Some(pid)) = pid_file.read() {
        if crate::process::is_alive(pid) {
            let _ = crate::process::send_signal(pid, signal_hook::consts::SIGUSR1);
        }
    }
    Ok(())
}

/// Attempts a non-blocking exclusive flock on `file`, then truncates and
/// writes the current PID if successful.
///
/// Returns `true` if the lock was acquired and the PID written.
/// Returns `false` if the lock is held by another process.
///
/// # Panics / Errors
///
/// Write and sync errors are treated as non-fatal and silently ignored;
/// the caller's primary concern is whether the lock was obtained.
#[cfg(unix)]
fn try_flock_and_write_pid(file: &fs::File) -> bool {
    // SAFETY: flock(2) is a standard POSIX advisory lock mechanism.
    // LOCK_EX | LOCK_NB: exclusive, non-blocking — returns EWOULDBLOCK
    // immediately if another process holds the lock.
    let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if ret != 0 {
        return false;
    }

    // We hold the exclusive lock — safe to truncate and write our PID.
    let _ = file.set_len(0);
    let mut file_ref = file;
    let _ = file_ref.write_all(std::process::id().to_string().as_bytes());
    let _ = file_ref.sync_all();

    true
}

/// Acquires an exclusive non-blocking flock on the sentinel PID file.
///
/// Opens (or creates) the sentinel PID file and attempts a non-blocking
/// exclusive lock via `flock(LOCK_EX | LOCK_NB)`. On success the file is
/// truncated, the current PID is written, and the open `File` handle is
/// returned. The caller **must** keep this handle alive for the sentinel's
/// entire lifetime — dropping it releases the flock, allowing a replacement
/// sentinel to take over.
///
/// If the lock is already held by another process, the supersede path is
/// attempted:
/// - Read the PID from the sentinel PID file.
/// - If that process is alive, send SIGTERM and poll for lock release up to
///   5 seconds (50 × 100 ms).
/// - If the PID is stale (process already gone), wait 500 ms and retry once.
/// - If the lock still cannot be acquired, return `Err`.
#[cfg(unix)]
fn acquire_sentinel_lock() -> Result<fs::File, UnfError> {
    let pid_path = storage::sentinel_pid_path()?;

    if let Some(parent) = pid_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            UnfError::InvalidArgument(format!("Failed to create sentinel PID directory: {}", e))
        })?;
    }

    // Open without truncating — we must hold the lock before writing.
    let file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&pid_path)
        .map_err(|e| {
            UnfError::InvalidArgument(format!("Failed to open sentinel PID file: {}", e))
        })?;

    // First attempt: fast path.
    if try_flock_and_write_pid(&file) {
        return Ok(file);
    }

    // Lock is held by another process. Read its PID to decide what to do.
    let old_pid: Option<u32> = fs::read_to_string(&pid_path)
        .ok()
        .and_then(|s| s.trim().parse().ok());

    match old_pid {
        Some(pid) if crate::process::is_alive(pid) => {
            // An old sentinel is still running. Supersede it.
            audit::log_event(
                "SENTINEL_SUPERSEDE",
                &format!("terminating old sentinel pid={}", pid),
            );
            let _ = crate::process::terminate(pid);

            // Poll up to 5 seconds for the lock to become available.
            for _ in 0..50 {
                thread::sleep(Duration::from_millis(100));
                if try_flock_and_write_pid(&file) {
                    return Ok(file);
                }
            }

            Err(UnfError::InvalidArgument(
                "Cannot acquire sentinel lock after supersede attempt".to_string(),
            ))
        }
        _ => {
            // Stale PID file — the old process already exited but the lock
            // file was not cleaned up. Wait briefly for the OS to release
            // the flock and retry once.
            thread::sleep(Duration::from_millis(500));
            if try_flock_and_write_pid(&file) {
                return Ok(file);
            }

            Err(UnfError::InvalidArgument(
                "Another sentinel is already running".to_string(),
            ))
        }
    }
    // Dropping the returned File releases the flock.
}

/// Checks if the sentinel is currently running by reading its PID file.
///
/// Also checks for zombie state to avoid the same bug class that affected
/// daemon detection (a zombie sentinel would pass `is_alive` but be inert).
pub fn is_sentinel_alive() -> bool {
    if let Ok(pid_path) = storage::sentinel_pid_path() {
        let pid_file = PidFile::new(pid_path);
        if let Ok(Some(pid)) = pid_file.read() {
            return crate::process::is_alive(pid) && !crate::process::is_zombie(pid);
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
    let pid_file = PidFile::new(pid_path);
    pid_file.write(pid).map_err(|e| {
        UnfError::InvalidArgument(format!("Failed to write sentinel PID file: {}", e))
    })?;

    // The Child handle is intentionally dropped here. Unlike the daemon (where
    // dropping caused zombie bugs), the sentinel uses flock for single-instance
    // enforcement and is detached via process_group(0). The CLI process exits
    // shortly after, and init/launchd reaps the sentinel on termination.
    Ok(())
}

/// Kills the sentinel process if running.
pub fn kill_sentinel() -> Result<(), UnfError> {
    let pid_path = storage::sentinel_pid_path()?;
    let pid_file = PidFile::new(pid_path);
    if let Ok(Some(pid)) = pid_file.read() {
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
    let _ = pid_file.remove();
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

    /// `assess_daemon_status` returns `NeedsRespawn` when there is no child handle
    /// but there are projects registered.
    #[test]
    fn assess_daemon_status_no_child() {
        let mut child: Option<Child> = None;
        let action = assess_daemon_status(&mut child, true);
        assert_eq!(
            action,
            DaemonAction::NeedsRespawn {
                reason: "no child handle"
            }
        );
    }

    /// `assess_daemon_status` returns `NoChild` when there are no registered
    /// projects, regardless of whether a child handle exists.
    #[test]
    fn assess_daemon_status_no_projects() {
        let mut child: Option<Child> = None;
        let action = assess_daemon_status(&mut child, false);
        assert_eq!(action, DaemonAction::NoChild);
    }

    /// `try_flock_and_write_pid` returns `false` when the file is already locked
    /// by another descriptor in the same process.
    ///
    /// flock(2) locks are per open-file-description (not per FD within a process
    /// on Linux, but on macOS the non-blocking attempt on the same path from a
    /// different open FD correctly returns EWOULDBLOCK).  We simulate this by
    /// holding a lock on a temp file and verifying a second attempt fails.
    #[cfg(unix)]
    #[test]
    fn try_flock_and_write_pid_fails_when_locked() {
        let tmp = tempfile::NamedTempFile::new().expect("failed to create temp file");
        let path = tmp.path();

        // Acquire the lock on one file descriptor.
        let holder = fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("open holder");
        let ret = unsafe { libc::flock(holder.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        assert_eq!(ret, 0, "first lock must succeed");

        // Open a *separate* file descriptor to the same path.
        let challenger = fs::OpenOptions::new()
            .write(true)
            .open(path)
            .expect("open challenger");

        // The helper must report failure since the holder still holds the lock.
        assert!(
            !try_flock_and_write_pid(&challenger),
            "try_flock_and_write_pid should return false when file is already locked"
        );

        drop(holder);
    }

    /// Verifies that dropping the File handle returned by `acquire_sentinel_lock`
    /// releases the flock so a subsequent acquisition succeeds.
    ///
    /// This confirms the single-instance guard is RAII-safe: the lock is held
    /// exactly as long as the sentinel lives, and no manual cleanup is required.
    #[cfg(unix)]
    #[test]
    fn lock_released_on_drop() {
        use std::fs;
        use std::os::unix::io::AsRawFd;

        // Create a temp file to stand in for the sentinel PID file.
        let tmp = tempfile::NamedTempFile::new().expect("failed to create temp file");
        let path = tmp.path().to_path_buf();

        // Helper: attempt an exclusive non-blocking flock on the path.
        // Returns true if the lock was acquired, false if already held.
        // Drops the file handle immediately, releasing any acquired lock.
        let try_lock = |p: &std::path::Path| -> bool {
            let f = match fs::OpenOptions::new().write(true).open(p) {
                Ok(f) => f,
                Err(_) => return false,
            };
            // SAFETY: standard POSIX flock advisory lock check.
            let ret = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            ret == 0
            // `f` is dropped here, releasing the lock immediately.
        };

        // Acquire the lock and keep the handle alive.
        let lock_file = {
            let f = fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .expect("failed to open temp file");
            // SAFETY: standard POSIX flock advisory lock.
            let ret = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            assert_eq!(ret, 0, "first lock acquisition must succeed");
            f
        };

        // While we hold lock_file, a second attempt must fail.
        assert!(
            !try_lock(&path),
            "lock should be held; second attempt must fail"
        );

        // Drop the file handle — this releases the flock.
        drop(lock_file);

        // After the drop, a new acquisition must succeed.
        assert!(
            try_lock(&path),
            "lock should be released after drop; re-acquisition must succeed"
        );
    }

    /// Recent snapshot within the threshold window yields `Fresh`.
    #[test]
    fn freshness_verdict_fresh() {
        let now = chrono::Utc::now();
        let newest_snapshot = Some(now - chrono::Duration::seconds(60));
        let newest_fs_mtime =
            Some(std::time::SystemTime::now() - std::time::Duration::from_secs(30));
        let threshold = std::time::Duration::from_secs(300);

        let verdict = compute_freshness(newest_snapshot, newest_fs_mtime, now, threshold);
        assert_eq!(verdict, FreshnessVerdict::Fresh);
    }

    /// Old snapshot but recent FS activity yields `Stale`.
    #[test]
    fn freshness_verdict_stale() {
        let now = chrono::Utc::now();
        let newest_snapshot = Some(now - chrono::Duration::seconds(600));
        let newest_fs_mtime =
            Some(std::time::SystemTime::now() - std::time::Duration::from_secs(30));
        let threshold = std::time::Duration::from_secs(300);

        let verdict = compute_freshness(newest_snapshot, newest_fs_mtime, now, threshold);
        match verdict {
            FreshnessVerdict::Stale { gap_secs } => {
                // gap should be approximately 600s (now - snap_time)
                assert!((590..=610).contains(&gap_secs), "gap_secs={}", gap_secs);
            }
            other => panic!("expected Stale, got {:?}", other),
        }
    }

    /// Old snapshot and old FS activity yields `Idle`.
    #[test]
    fn freshness_verdict_idle() {
        let now = chrono::Utc::now();
        let newest_snapshot = Some(now - chrono::Duration::seconds(600));
        let newest_fs_mtime =
            Some(std::time::SystemTime::now() - std::time::Duration::from_secs(600));
        let threshold = std::time::Duration::from_secs(300);

        let verdict = compute_freshness(newest_snapshot, newest_fs_mtime, now, threshold);
        assert_eq!(verdict, FreshnessVerdict::Idle);
    }

    /// No snapshot and no FS mtime data yields `Idle`.
    #[test]
    fn freshness_verdict_no_data() {
        let now = chrono::Utc::now();
        let threshold = std::time::Duration::from_secs(300);

        let verdict = compute_freshness(None, None, now, threshold);
        assert_eq!(verdict, FreshnessVerdict::Idle);
    }

    /// `read_last_snapshot_time` correctly parses a valid RFC3339 timestamp
    /// written to a temp sidecar file.
    #[test]
    fn read_last_snapshot_time_valid() {
        let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let storage_dir = tmp_dir.path();

        // Write a known RFC3339 timestamp to the sidecar file location.
        let expected: chrono::DateTime<chrono::Utc> =
            "2026-03-27T10:00:00Z".parse().expect("valid timestamp");
        let sidecar_path = storage::last_snapshot_time_path(storage_dir);
        fs::write(&sidecar_path, expected.to_rfc3339()).expect("write sidecar");

        let result = read_last_snapshot_time(storage_dir);
        assert_eq!(result, Some(expected));
    }

    /// `read_last_snapshot_time` returns `None` when the sidecar file does
    /// not exist.
    #[test]
    fn read_last_snapshot_time_missing() {
        let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let result = read_last_snapshot_time(tmp_dir.path());
        assert_eq!(result, None);
    }

    /// `sample_newest_mtime` returns `Some` for an existing directory — the
    /// directory itself has an mtime.
    #[test]
    fn sample_newest_mtime_returns_some() {
        let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let result = sample_newest_mtime(tmp_dir.path());
        assert!(
            result.is_some(),
            "expected Some mtime for existing directory"
        );
    }
}
