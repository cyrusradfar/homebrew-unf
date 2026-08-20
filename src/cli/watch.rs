//! `unf watch` command implementation.
//!
//! Registers a project for watching and manages the global daemon.
//! The watch command replaces the project-level logic from `unf init`
//! and integrates with the single global daemon architecture.

use std::env;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::cli::OutputFormat;
use crate::engine::Engine;
use crate::error::UnfError;
use crate::process::PidFile;
use crate::storage;

/// JSON output for the watch command.
#[derive(serde::Serialize)]
struct WatchOutput {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshots_preserved: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_restart: Option<bool>,
    /// Always serialized. Machine consumers must read the flag directly
    /// instead of treating an absent field as `false`.
    force_watch_gitignore: bool,
}

/// Prints the gitignore-override safety warning to stderr.
///
/// Line 1 uses the shared `warning:` prefix. Lines 2 to 4 use the same
/// two-space hint indent that `print_error` applies to its hint.
fn print_gitignore_override_warning() {
    super::output::print_warning(".gitignore protection is off for this project");
    eprintln!("  UNF records the files that .gitignore excludes, and hidden dotfiles.");
    eprintln!("  Secrets in those files go into the recording. Example: .env.local.");
    eprintln!("  Run `unf watch` with no flag to turn this off.");
}

/// Runs the `unf watch` command.
///
/// Registers a project for watching and manages the global daemon.
/// Unlike `init`, this command works with the global daemon architecture.
///
/// # Behavior
///
/// 1. Resolve storage dir for the project
/// 2. Remove stopped sentinel if present (re-activation)
/// 3. Initialize engine if needed (if storage doesn't exist, create it; else open)
/// 4. Register project in global registry
/// 5. Install auto-start capability
/// 6. Check if global daemon is running:
///    - If running: send SIGUSR1 signal to trigger registry reload
///    - If not running: spawn `unf __daemon` and write global PID file
/// 7. Output success message with status
///
/// # Arguments
///
/// * `project_root` - The root directory to watch (typically current directory)
/// * `format` - Output format (human or JSON)
/// * `force_watch_gitignore` - When true, record gitignored files and hidden
///   dotfiles for this project. The value is always written, so a plain
///   `unf watch` turns a previously set override back off.
///
/// # Returns
///
/// `Ok(())` on success, or `UnfError` if watch fails.
///
/// # Errors
///
/// - `UnfError::Db` if database operations fail
/// - `UnfError::Cas` if directory creation fails
/// - `UnfError::Watcher` if daemon spawn or signal operations fail
#[allow(clippy::cognitive_complexity)]
// TODO(v0.18): reduce complexity
pub fn run(
    project_root: &Path,
    format: OutputFormat,
    force_watch_gitignore: bool,
) -> Result<(), UnfError> {
    let storage_dir = storage::resolve_storage_dir(project_root)?;

    // Remove stopped markers (re-activation).
    // Per-project marker:
    let stopped_path = storage::stopped_path(&storage_dir);
    let _ = fs::remove_file(&stopped_path);
    // Global marker (created by `unf stop`; blocks sentinel startup):
    if let Ok(global_stopped) = storage::global_stopped_path() {
        let _ = fs::remove_file(&global_stopped);
    }

    // Initialize engine if needed
    let engine = if storage_dir.exists() {
        Engine::open(project_root, &storage_dir)?
    } else {
        Engine::init(project_root, &storage_dir)?
    };

    // Record user intent (source of truth for what should be watched)
    if let Err(e) = crate::intent::add_project(project_root, Some(force_watch_gitignore)) {
        super::output::print_warning(&format!("Failed to record intent: {}", e));
    }

    // Register project in global registry
    if let Err(e) = crate::registry::register_project(project_root, Some(force_watch_gitignore)) {
        super::output::print_warning(&format!("Failed to register project: {}", e));
    }

    // Install auto-start
    let auto_restart = match crate::autostart::install() {
        Ok(()) => crate::autostart::is_installed().unwrap_or(false),
        Err(e) => {
            super::output::print_warning(&format!("Failed to install auto-start: {}", e));
            false
        }
    };

    // Check if global daemon is already running
    let global_pid_path = storage::global_pid_path()?;
    let daemon_running = is_global_daemon_running(&global_pid_path);

    if daemon_running {
        // Send SIGUSR1 to trigger registry reload
        let pid_file = PidFile::new(global_pid_path.clone());
        if let Ok(Some(pid)) = pid_file.read() {
            if let Err(e) = crate::process::send_signal(pid, signal_hook::consts::SIGUSR1) {
                super::output::print_warning(&format!("Failed to signal daemon: {}", e));
            }
        }
    } else {
        // Spawn global daemon
        spawn_global_daemon(&global_pid_path)?;
    }

    // Start sentinel watchdog if not running
    if let Err(e) = crate::sentinel::ensure_sentinel_running() {
        super::output::print_warning(&format!("Failed to start sentinel: {}", e));
    }

    // Audit log
    crate::audit::log_event("WATCH", &project_root.display().to_string());

    // Also write per-project PID file for backward compatibility with status
    let per_project_pid = storage::pid_path(&storage_dir);
    let global_pid_file = PidFile::new(global_pid_path.clone());
    if let Ok(Some(pid)) = global_pid_file.read() {
        let _ = fs::write(&per_project_pid, pid.to_string());
    }

    // Get snapshot count to determine status
    let snapshot_count = engine.get_snapshot_count().unwrap_or(0);

    // Output
    let output = if snapshot_count > 0 {
        WatchOutput {
            status: "resumed".to_string(),
            snapshots_preserved: Some(snapshot_count),
            auto_restart: Some(auto_restart),
            force_watch_gitignore,
        }
    } else {
        WatchOutput {
            status: "started".to_string(),
            snapshots_preserved: None,
            auto_restart: Some(auto_restart),
            force_watch_gitignore,
        }
    };

    if format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else if snapshot_count > 0 {
        let subject = format!(
            "{} ({} snapshots preserved)",
            project_root.display(),
            snapshot_count
        );
        super::output::print_status("Watching", &subject);
    } else {
        super::output::print_status("Watching", &project_root.display().to_string());
    }

    // Human-format only: JSON consumers read `force_watch_gitignore` instead.
    if force_watch_gitignore && format != OutputFormat::Json {
        print_gitignore_override_warning();
    }

    Ok(())
}

/// Checks if the global daemon is running.
///
/// Reads the global PID file and checks if the process is alive.
/// Returns false if the PID file doesn't exist or the process is dead.
fn is_global_daemon_running(global_pid_path: &Path) -> bool {
    let pid_file = PidFile::new(global_pid_path.to_path_buf());
    pid_file.is_running()
}

/// Spawns the global daemon process.
///
/// Forks a new process running `unf __daemon` (without --root) and writes
/// its PID to the global PID file at `~/.unfudged/daemon.pid`.
///
/// # Arguments
///
/// * `global_pid_path` - Path to the global daemon PID file
///
/// # Errors
///
/// Returns `UnfError::Watcher` if spawning or writing the PID file fails.
fn spawn_global_daemon(global_pid_path: &Path) -> Result<(), UnfError> {
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
        .process_group(0) // Detach so daemon survives parent exit
        .spawn()
        .map_err(|e| {
            UnfError::Watcher(crate::error::WatcherError::Io(std::io::Error::other(
                format!("Failed to spawn daemon: {}", e),
            )))
        })?;

    let pid = child.id();
    let pid_file = PidFile::new(global_pid_path.to_path_buf());
    pid_file.write(pid).map_err(|e| {
        UnfError::Watcher(crate::error::WatcherError::Io(std::io::Error::other(
            format!("Failed to write global PID file: {}", e),
        )))
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn is_global_daemon_running_nonexistent_returns_false() {
        let temp = TempDir::new().expect("create temp dir");
        let pid_path = temp.path().join("daemon.pid");
        assert!(!is_global_daemon_running(&pid_path));
    }

    #[test]
    fn is_global_daemon_running_invalid_pid_returns_false() {
        let temp = TempDir::new().expect("create temp dir");
        let pid_path = temp.path().join("daemon.pid");
        fs::write(&pid_path, "invalid").expect("write invalid pid");
        assert!(!is_global_daemon_running(&pid_path));
    }

    /// Serializes a `WatchOutput` to a JSON value for field assertions.
    fn to_json(output: &WatchOutput) -> serde_json::Value {
        serde_json::to_value(output).expect("serialize WatchOutput")
    }

    #[test]
    fn watch_output_always_serializes_gitignore_flag_when_false() {
        let json = to_json(&WatchOutput {
            status: "started".to_string(),
            snapshots_preserved: None,
            auto_restart: Some(true),
            force_watch_gitignore: false,
        });

        assert_eq!(json["force_watch_gitignore"], serde_json::json!(false));
        // Absent optional fields still collapse; the flag must not.
        assert!(json.get("snapshots_preserved").is_none());
    }

    #[test]
    fn watch_output_serializes_gitignore_flag_when_true() {
        let json = to_json(&WatchOutput {
            status: "resumed".to_string(),
            snapshots_preserved: Some(7),
            auto_restart: Some(false),
            force_watch_gitignore: true,
        });

        assert_eq!(json["force_watch_gitignore"], serde_json::json!(true));
        assert_eq!(json["status"], serde_json::json!("resumed"));
        assert_eq!(json["snapshots_preserved"], serde_json::json!(7));
    }
}
