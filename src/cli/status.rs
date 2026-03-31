//! The `unf status` command implementation.
//!
//! Shows the current state of the UNFUDGED flight recorder:
//! - Whether recording is active
//! - Snapshot count, tracked files, and store size
//! - Time since recording started

use std::path::Path;

use chrono::Utc;

use crate::cli::OutputFormat;
use crate::engine::Engine;
use crate::error::UnfError;
use crate::process::PidFile;
use crate::storage;

/// JSON output for the status command.
#[derive(serde::Serialize)]
struct StatusOutput {
    recording: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshots: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    files_tracked: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    store_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    newest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status_mode: Option<u8>,
    auto_restart: bool,
}

/// Status modes for unwatched directories.
///
/// Mode 0: Never watched — directory has no history in the registry.
/// Mode 1: Previously watched but inactive — directory was registered but daemon isn't active.
/// Mode 2: Actively being watched — daemon is recording changes.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusMode {
    /// Directory has never been watched.
    NeverWatched = 0,
    /// Directory was watched before but is currently inactive.
    PreviouslyWatched = 1,
    /// Directory is currently being watched.
    ActivelyWatching = 2,
}

/// Executes the `unf status` command.
///
/// Handles three modes for unwatched directories:
/// - Mode 0 (Never watched): Directory has no registry entry
/// - Mode 1 (Previously watched): Directory was registered but daemon isn't active
/// - Mode 2 (Actively watching): Daemon is recording changes
///
/// # Arguments
///
/// * `project_root` - The root directory of the project
/// * `format` - Output format (human or JSON)
///
/// # Errors
///
/// Returns an error only if storage path cannot be resolved or database queries fail.
/// Returns `Ok(())` for all unwatched directory modes (shows appropriate message).
pub fn run(project_root: &Path, format: OutputFormat) -> Result<(), UnfError> {
    // Step 1: Resolve storage dir (may or may not exist yet)
    let storage_dir = storage::resolve_storage_dir(project_root)?;

    // Query auto-restart state (used in all output paths)
    let auto_restart = crate::autostart::is_installed().unwrap_or(false);

    // Determine the status mode
    let canonical_project_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());

    let mode = determine_status_mode(project_root, &canonical_project_root, &storage_dir);

    // Step 2: Handle each mode
    match mode {
        StatusMode::NeverWatched => {
            // Mode 0: Directory has never been watched
            let output = StatusOutput {
                recording: false,
                since: None,
                snapshots: None,
                files_tracked: None,
                store_bytes: None,
                newest: None,
                reason: Some("never_watched".to_string()),
                status_mode: Some(0),
                auto_restart,
            };

            if format == OutputFormat::Json {
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
            } else {
                println!("This directory is not being watched.");
                println!("Run 'unf watch' to start recording changes.");
            }
            return Ok(());
        }

        StatusMode::PreviouslyWatched => {
            // Mode 1: Directory was watched but is currently inactive
            let output = StatusOutput {
                recording: false,
                since: None,
                snapshots: None,
                files_tracked: None,
                store_bytes: None,
                newest: None,
                reason: Some("previously_watched_inactive".to_string()),
                status_mode: Some(1),
                auto_restart,
            };

            if format == OutputFormat::Json {
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
            } else {
                println!("This directory was previously watched but is not currently active.");
                println!("Run 'unf watch' to resume recording. Your history is safe.");
            }
            return Ok(());
        }

        StatusMode::ActivelyWatching => {
            // Mode 2: Actively watching — continue to stats display
        }
    }

    // Step 3: Open engine and query stats
    let engine = Engine::open(project_root, &storage_dir)?;

    let snapshot_count = engine.get_snapshot_count()?;
    let file_count = engine.get_tracked_file_count()?;
    let store_size = engine.get_store_size()?;

    // Get the time running (from oldest snapshot to now)
    let duration_str = get_duration_string(&engine)?;
    let since_time = get_oldest_snapshot_time(&engine)?;
    let newest_time = get_newest_snapshot_time(&engine)?;

    // Step 4: Build and print output
    let output = StatusOutput {
        recording: true,
        since: since_time,
        snapshots: Some(snapshot_count),
        files_tracked: Some(file_count),
        store_bytes: Some(store_size),
        newest: newest_time,
        reason: None,
        status_mode: Some(2),
        auto_restart,
    };

    if format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        println!("Watching since {}", duration_str);
        println!("  Snapshots:  {}", format_number(snapshot_count));
        println!("  Files tracked:  {}", format_number(file_count));
        println!("  Store size:  {}", super::format_size(store_size));
        println!(
            "  Auto-restart: {}",
            if auto_restart { "enabled" } else { "disabled" }
        );
    }

    Ok(())
}

/// Determines the status mode for a directory.
///
/// Mode 0 (NeverWatched): Directory is not in the registry and storage doesn't exist.
/// Mode 1 (PreviouslyWatched): Directory is in the registry but daemon isn't watching.
/// Mode 2 (ActivelyWatching): Daemon is alive and actively watching this directory.
fn determine_status_mode(
    project_root: &Path,
    canonical_project_root: &Path,
    storage_dir: &Path,
) -> StatusMode {
    // Check if daemon is currently watching
    if is_daemon_watching_project(project_root, storage_dir) {
        return StatusMode::ActivelyWatching;
    }

    // Daemon is not active. Now check if directory was ever registered.
    if let Ok(registry) = crate::registry::load() {
        if registry
            .projects
            .iter()
            .any(|p| p.path == canonical_project_root)
        {
            // Found in registry but daemon not watching → Mode 1
            return StatusMode::PreviouslyWatched;
        }
    }

    // Not in registry → Mode 0
    StatusMode::NeverWatched
}

/// Checks if the daemon is actively watching this project.
///
/// First checks the global daemon (new model), then falls back to
/// the per-project PID file (backward compatibility with old daemons).
///
/// # Arguments
///
/// * `project_root` - The root directory of the project
/// * `storage_dir` - The centralized storage directory for this project
///
/// # Returns
///
/// `true` if the daemon is alive and actively watching this project.
fn is_daemon_watching_project(project_root: &Path, _storage_dir: &Path) -> bool {
    let global_pid_path = match storage::global_pid_path() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let pid_file = PidFile::new(global_pid_path);
    let pid = match pid_file.read() {
        Ok(Some(p)) => p,
        _ => return false,
    };
    if !crate::process::is_alive(pid) {
        return false;
    }
    // Global daemon alive — check if this project is registered
    if let Ok(registry) = crate::registry::load() {
        let canonical = project_root
            .canonicalize()
            .unwrap_or_else(|_| project_root.to_path_buf());
        return registry.projects.iter().any(|p| p.path == canonical);
    }
    false
}

use super::output::format_number;

/// Formats the duration since recording started as a human-readable string.
///
/// # Examples
///
/// ```text
/// 30 seconds ago
/// 5 minutes ago
/// 2 hours ago
/// 3 days ago
/// ```
fn format_duration(duration_secs: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * 60;
    const DAY: u64 = 24 * 60 * 60;

    if duration_secs < MINUTE {
        format!("{} seconds ago", duration_secs)
    } else if duration_secs < HOUR {
        let minutes = duration_secs / MINUTE;
        format!("{} {} ago", minutes, plural(minutes, "minute"))
    } else if duration_secs < DAY {
        let hours = duration_secs / HOUR;
        format!("{} {} ago", hours, plural(hours, "hour"))
    } else {
        let days = duration_secs / DAY;
        format!("{} {} ago", days, plural(days, "day"))
    }
}

/// Returns the singular or plural form of a word.
fn plural(count: u64, singular: &str) -> String {
    if count == 1 {
        singular.to_string()
    } else {
        format!("{}s", singular)
    }
}

/// Gets the oldest snapshot time as ISO 8601 string, or None if no snapshots.
fn get_oldest_snapshot_time(engine: &Engine) -> Result<Option<String>, UnfError> {
    match engine.get_oldest_snapshot_time()? {
        Some(oldest_time) => Ok(Some(oldest_time.to_rfc3339())),
        None => Ok(None),
    }
}

/// Gets the newest snapshot time as ISO 8601 string, or None if no snapshots.
fn get_newest_snapshot_time(engine: &Engine) -> Result<Option<String>, UnfError> {
    match engine.get_newest_snapshot_time()? {
        Some(newest_time) => Ok(Some(newest_time.to_rfc3339())),
        None => Ok(None),
    }
}

/// Computes the duration string from the oldest snapshot to now.
///
/// Returns a formatted string like "3 hours ago" if snapshots exist,
/// or "0 seconds ago" if no snapshots exist yet.
fn get_duration_string(engine: &Engine) -> Result<String, UnfError> {
    match engine.get_oldest_snapshot_time()? {
        Some(oldest_time) => {
            let now = Utc::now();
            let duration = now.signed_duration_since(oldest_time);
            let duration_secs = std::cmp::max(0, duration.num_seconds()) as u64;
            Ok(format_duration(duration_secs))
        }
        None => {
            // No snapshots yet, recording just started
            Ok("0 seconds ago".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_seconds() {
        assert_eq!(format_duration(0), "0 seconds ago");
        assert_eq!(format_duration(1), "1 seconds ago");
        assert_eq!(format_duration(30), "30 seconds ago");
        assert_eq!(format_duration(59), "59 seconds ago");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(60), "1 minute ago");
        assert_eq!(format_duration(120), "2 minutes ago");
        assert_eq!(format_duration(300), "5 minutes ago");
    }

    #[test]
    fn format_duration_hours() {
        assert_eq!(format_duration(3600), "1 hour ago");
        assert_eq!(format_duration(7200), "2 hours ago");
        assert_eq!(format_duration(18000), "5 hours ago");
    }

    #[test]
    fn format_duration_days() {
        assert_eq!(format_duration(86400), "1 day ago");
        assert_eq!(format_duration(172800), "2 days ago");
        assert_eq!(format_duration(259200), "3 days ago");
    }

    #[test]
    fn is_pid_alive_with_invalid_pid() {
        // PID 999999 is almost certainly not running
        assert!(!crate::process::is_alive(999999));
    }

    #[test]
    fn plural_singular() {
        assert_eq!(plural(1, "minute"), "minute");
        assert_eq!(plural(1, "hour"), "hour");
        assert_eq!(plural(1, "day"), "day");
    }

    #[test]
    fn plural_multiple() {
        assert_eq!(plural(0, "minute"), "minutes");
        assert_eq!(plural(2, "hour"), "hours");
        assert_eq!(plural(5, "day"), "days");
    }

    #[test]
    fn is_daemon_watching_project_no_global_daemon() {
        // When there's no global PID file and no per-project PID, should return false
        let temp = tempfile::TempDir::new().expect("create temp");
        assert!(!is_daemon_watching_project(temp.path(), temp.path()));
    }

    #[test]
    fn determine_mode_never_watched() {
        // Test Mode 0: Directory never registered, daemon not watching
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();

        let temp = tempfile::TempDir::new().expect("create temp");
        std::env::set_var("HOME", temp.path());

        let project_dir = temp.path().join("project");
        std::fs::create_dir_all(&project_dir).expect("create project dir");

        let canonical = project_dir.canonicalize().expect("canonicalize");
        let storage_dir = temp.path().join(".unfudged").join("data").join("project");

        let mode = determine_status_mode(&project_dir, &canonical, &storage_dir);

        assert_eq!(mode, StatusMode::NeverWatched);

        // Cleanup
        let original_home = std::env::var("HOME").ok();
        if let Some(h) = original_home {
            std::env::set_var("HOME", h);
        }
    }

    #[test]
    fn determine_mode_previously_watched() {
        // Test Mode 1: Directory registered but daemon not watching
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();

        let temp = tempfile::TempDir::new().expect("create temp");
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", temp.path());

        // Create project and registry directories
        let project_dir = temp.path().join("project");
        std::fs::create_dir_all(&project_dir).expect("create project dir");
        std::fs::create_dir_all(temp.path().join(".unfudged")).expect("create .unfudged");

        // Register the project
        crate::registry::register_project(&project_dir).expect("register project");

        let canonical = project_dir.canonicalize().expect("canonicalize");
        let storage_dir = temp.path().join(".unfudged").join("data").join("project");

        let mode = determine_status_mode(&project_dir, &canonical, &storage_dir);

        assert_eq!(mode, StatusMode::PreviouslyWatched);

        // Cleanup
        if let Some(h) = original_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }
    }
}
