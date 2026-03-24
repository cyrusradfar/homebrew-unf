//! Storage migration engine for `unf config --move-storage`.
//!
//! Moves all UNFUDGED data from the current storage location to a new one
//! using copy-then-swap semantics: source is NEVER modified during copy.
//! Ctrl+C at any point leaves source intact.
//!
//! Migration phases:
//! 1. Pre-flight: validate destination path, check disk space
//! 2. Stop daemon: send SIGTERM, wait, SIGKILL if needed
//! 3. Copy data: copy project files with per-project progress output
//! 4. Swap config: update config.json with new storage_dir
//! 5. Verify: confirm destination is readable
//! 6. Restart daemon: spawn via `__boot` subcommand
//! 7. Cleanup: rename old dir to .migrated

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::cli::OutputFormat;
use crate::error::UnfError;

/// Files that should NOT be copied during migration (runtime state).
const SKIP_FILES: &[&str] = &["daemon.pid", "sentinel.pid", "stopped"];

/// Extensions to skip (runtime SQLite journal files).
const SKIP_EXTENSIONS: &[&str] = &["sqlite3-wal", "sqlite3-shm"];

/// Timeout waiting for daemon to stop gracefully (ms).
const DAEMON_STOP_TIMEOUT_MS: u64 = 2000;

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Executes `unf config --move-storage <DEST>`.
///
/// Validates the destination, stops the daemon, copies data, swaps the
/// config, verifies, restarts the daemon, and renames the old directory
/// to `.migrated`.
///
/// # Arguments
///
/// * `dest_arg` - Destination path string (may be "default" for ~/.unfudged)
/// * `format`   - Output format (human or JSON)
///
/// # Errors
///
/// Returns a descriptive error. All error messages include either
/// "No changes made" (if nothing was modified) or "Your data is safe at
/// [original path]".
pub fn run(dest_arg: &str, format: OutputFormat) -> Result<(), UnfError> {
    let start = Instant::now();

    // 1. Resolve source and destination.
    let source = crate::registry::global_dir()?;
    let (dest, is_default) = resolve_destination(dest_arg)?;

    // 2. Pre-flight checks (before any side effects).
    preflight_checks(&source, &dest)?;

    // 3. Get size / project info for progress output.
    let (total_bytes, project_count) =
        crate::config::storage_usage(&source).unwrap_or((0, 0));

    emit_progress(
        format,
        &serde_json::json!({
            "event": "started",
            "total_bytes": total_bytes,
            "project_count": project_count,
        }),
        &format!(
            "Stopping daemon... (moving {} across {} project{})",
            crate::cli::format_size(total_bytes),
            project_count,
            if project_count == 1 { "" } else { "s" }
        ),
    );

    // 4. Stop daemon.
    stop_daemon();

    emit_progress(
        format,
        &serde_json::json!({"event": "daemon_stopped"}),
        "Copying data...",
    );

    // 5. Copy data to destination.
    copy_storage(&source, &dest, format)?;

    // 6. Swap config.
    swap_config(&dest, is_default)?;

    emit_progress(
        format,
        &serde_json::json!({"event": "config_swapped"}),
        "Updating configuration...",
    );

    // 7. Verify destination.
    verify_destination(&dest)?;

    emit_progress(
        format,
        &serde_json::json!({"event": "verified"}),
        "Verifying new location...",
    );

    // 8. Restart daemon.
    restart_daemon()?;

    emit_progress(
        format,
        &serde_json::json!({"event": "daemon_restarted"}),
        "Restarting daemon...",
    );

    // 9. Rename old directory to .migrated.
    let backup_path = cleanup_old(&source)?;

    let elapsed = start.elapsed().as_secs_f64();
    emit_progress(
        format,
        &serde_json::json!({
            "event": "done",
            "elapsed_secs": elapsed,
            "backup_path": backup_path.display().to_string(),
        }),
        &format!(
            "Done. Previous data saved at {}",
            backup_path.display()
        ),
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Phase 1: resolve destination
// ---------------------------------------------------------------------------

/// Resolves the destination argument to a canonical path.
///
/// The special value `"default"` resolves to `~/.unfudged` (None config).
/// All other values must be absolute paths.
///
/// Returns `(dest_path, is_default)`.
///
/// # Errors
///
/// Returns `UnfError::InvalidArgument` for relative paths.
pub fn resolve_destination(dest_arg: &str) -> Result<(PathBuf, bool), UnfError> {
    if dest_arg == "default" {
        let home = dirs::home_dir().ok_or_else(|| {
            UnfError::Config("Cannot determine home directory. No changes made.".to_string())
        })?;
        return Ok((home.join(".unfudged"), true));
    }

    let path = PathBuf::from(dest_arg);
    if !path.is_absolute() {
        return Err(UnfError::InvalidArgument(
            "Path must be absolute. No changes made.".to_string(),
        ));
    }

    Ok((path, false))
}

// ---------------------------------------------------------------------------
// Phase 2: pre-flight checks
// ---------------------------------------------------------------------------

/// Validates that the migration can proceed before any side effects.
///
/// Checks:
/// - Destination is not inside source (would cause recursive copy)
/// - Destination does not already contain data
/// - Sufficient disk space is available
///
/// # Errors
///
/// Returns `UnfError::InvalidArgument` with a "No changes made" message.
pub fn preflight_checks(source: &Path, dest: &Path) -> Result<(), UnfError> {
    // Dest must not be the same as source.
    if dest == source {
        return Err(UnfError::InvalidArgument(
            "Destination is the same as the current storage location. No changes made.".to_string(),
        ));
    }

    // Dest must not be inside source (would cause infinite recursive copy).
    if let Ok(dest_canonical) = dest.canonicalize() {
        if let Ok(source_canonical) = source.canonicalize() {
            if dest_canonical.starts_with(&source_canonical) {
                return Err(UnfError::InvalidArgument(format!(
                    "{} is inside the current storage directory. No changes made.",
                    dest.display()
                )));
            }
        }
    } else if dest.starts_with(source) {
        // Destination doesn't exist yet but its non-existent path is a child of source.
        return Err(UnfError::InvalidArgument(format!(
            "{} is inside the current storage directory. No changes made.",
            dest.display()
        )));
    }

    // Destination must be empty or not exist.
    if dest.exists() {
        // Allow the path to exist if it is an empty directory.
        let is_empty = fs::read_dir(dest)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
        if !is_empty {
            return Err(UnfError::InvalidArgument(format!(
                "{} already contains data. No changes made.",
                dest.display()
            )));
        }
    }

    // Check parent directory is writable (we will create dest inside it).
    let parent = dest.parent().unwrap_or(Path::new("/"));
    let parent_to_check = if parent.exists() { parent } else { Path::new("/tmp") };

    // Disk space check using fs2.
    let source_size = crate::config::storage_usage(source)
        .map(|(bytes, _)| bytes)
        .unwrap_or(0);

    if source_size > 0 {
        match available_space(parent_to_check) {
            Ok(avail) if avail < source_size => {
                return Err(UnfError::InvalidArgument(format!(
                    "Not enough space at {}. Need {}, have {} available. No changes made.",
                    dest.display(),
                    crate::cli::format_size(source_size),
                    crate::cli::format_size(avail),
                )));
            }
            _ => {} // Either enough space or could not determine — proceed.
        }
    }

    Ok(())
}

/// Returns available disk space in bytes for the filesystem containing `path`.
///
/// Uses `fs2::available_space` (fs2 is already a dependency).
fn available_space(path: &Path) -> std::io::Result<u64> {
    fs2::available_space(path)
}

// ---------------------------------------------------------------------------
// Phase 3: stop daemon
// ---------------------------------------------------------------------------

/// Stops the global daemon gracefully (best-effort, no error propagated).
///
/// Kills sentinel first, then sends SIGTERM to daemon, waits up to 2s,
/// escalates to SIGKILL if still alive.
fn stop_daemon() {
    // Kill sentinel first so it won't respawn the daemon.
    let _ = crate::sentinel::kill_sentinel();

    let global_pid_path = match crate::storage::global_pid_path() {
        Ok(p) => p,
        Err(_) => return,
    };

    let pid = match read_live_pid(&global_pid_path) {
        Some(p) => p,
        None => return,
    };

    // SIGTERM + wait.
    let _ = crate::process::terminate(pid);
    wait_for_exit(pid, DAEMON_STOP_TIMEOUT_MS);

    // SIGKILL if still alive.
    if crate::process::is_alive(pid) {
        let _ = crate::process::force_terminate(pid, 500);
    }

    // Clean up PID file.
    let _ = fs::remove_file(&global_pid_path);
}

/// Reads a PID from a file and returns it if the process is alive.
fn read_live_pid(pid_path: &Path) -> Option<u32> {
    let content = fs::read_to_string(pid_path).ok()?;
    let pid: u32 = content.trim().parse().ok()?;
    if crate::process::is_alive(pid) { Some(pid) } else { None }
}

/// Polls until the process exits or timeout is reached.
fn wait_for_exit(pid: u32, timeout_ms: u64) {
    let poll = Duration::from_millis(100);
    let iters = timeout_ms / 100;
    for _ in 0..iters {
        if !crate::process::is_alive(pid) {
            return;
        }
        thread::sleep(poll);
    }
}

// ---------------------------------------------------------------------------
// Phase 4: copy storage
// ---------------------------------------------------------------------------

/// Copies the source storage directory to the destination.
///
/// Attempts same-filesystem rename first (atomic, instant). Falls back to
/// file-by-file copy on EXDEV (cross-device) or other rename failure.
///
/// Emits per-project progress events.
fn copy_storage(source: &Path, dest: &Path, format: OutputFormat) -> Result<(), UnfError> {
    // Ensure destination parent exists.
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            UnfError::Config(format!(
                "Cannot write to {}. No changes made. ({})",
                dest.display(),
                e
            ))
        })?;
    }

    // Try a same-filesystem rename for speed (only works before dest exists).
    // We do NOT attempt rename here because we need the source preserved
    // for safety: copy-then-swap is the contract.
    // Fall straight through to file-by-file copy.
    copy_dir_recursive(source, dest, SKIP_FILES, SKIP_EXTENSIONS).map_err(|e| {
        // If copy fails, destination may be partially written. Source is intact.
        UnfError::Config(format!(
            "Copy failed: {}. Your data is safe at {}",
            e,
            source.display()
        ))
    })?;

    // Emit per-project progress after the copy (registry read post-copy).
    if let Ok(registry) = crate::registry::load() {
        for entry in &registry.projects {
            let project_name = entry
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| entry.path.display().to_string());

            // Size of this project's data in the new location.
            let project_size =
                project_size_at_dest(&entry.path, dest).unwrap_or(0);

            emit_progress(
                format,
                &serde_json::json!({
                    "event": "project_start",
                    "project": project_name,
                    "bytes": project_size,
                }),
                &format!(
                    "  {} ({})",
                    project_name,
                    crate::cli::format_size(project_size)
                ),
            );

            emit_progress(
                format,
                &serde_json::json!({
                    "event": "project_done",
                    "project": project_name,
                }),
                "",
            );
        }
    }

    Ok(())
}

/// Estimates the size of one project's data at the destination.
fn project_size_at_dest(project_path: &Path, dest_root: &Path) -> Option<u64> {
    // Mirror the storage path structure under dest.
    let relative = project_path
        .to_string_lossy()
        .trim_start_matches('/')
        .to_owned();
    let project_storage = dest_root.join("data").join(relative);
    crate::config::storage_usage(&project_storage)
        .map(|(b, _)| b)
        .ok()
}

/// Recursively copies `src` into `dst`, skipping runtime files.
///
/// - Creates `dst` if it does not exist.
/// - Skips any filename found in `skip_files`.
/// - Skips files whose extension is in `skip_extensions`.
/// - Copies all other files preserving the directory structure.
///
/// # Errors
///
/// Returns `std::io::Error` on any I/O failure during the copy.
pub fn copy_dir_recursive(
    src: &Path,
    dst: &Path,
    skip_files: &[&str],
    skip_extensions: &[&str],
) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let src_path = entry.path();
        let dst_path = dst.join(&name);

        // Skip runtime files by exact name.
        if skip_files.iter().any(|&s| s == name_str.as_ref()) {
            continue;
        }

        // Skip by extension.
        if let Some(ext) = src_path.extension() {
            let ext_str = ext.to_string_lossy();
            if skip_extensions.iter().any(|&s| s == ext_str.as_ref()) {
                continue;
            }
        }

        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            copy_dir_recursive(&src_path, &dst_path, skip_files, skip_extensions)?;
        } else if metadata.is_file() {
            fs::copy(&src_path, &dst_path)?;
        }
        // Symlinks are intentionally skipped (UNFUDGED stores no symlinks).
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Phase 5: swap config
// ---------------------------------------------------------------------------

/// Updates `config.json` to point at the new storage directory.
///
/// If `is_default` is true, clears `storage_dir` (None = use ~/.unfudged).
///
/// # Errors
///
/// Returns `UnfError::Config` if the config cannot be saved. Source data is
/// already copied at this point; the error message reflects that.
pub fn swap_config(dest: &Path, is_default: bool) -> Result<(), UnfError> {
    let mut config = crate::config::load().unwrap_or_default();

    if is_default {
        config.storage_dir = None;
    } else {
        config.storage_dir = Some(dest.to_path_buf());
    }

    crate::config::save(&config).map_err(|e| {
        UnfError::Config(format!(
            "Failed to update config: {}. Your data is safe at {}",
            e,
            dest.display()
        ))
    })
}

// ---------------------------------------------------------------------------
// Phase 6: verify destination
// ---------------------------------------------------------------------------

/// Verifies the destination is functional after the copy.
///
/// Checks that `projects.json` exists and is readable at the destination.
/// Also attempts to open one SQLite database if any exist.
///
/// # Errors
///
/// Returns `UnfError::Config` if verification fails.
pub fn verify_destination(dest: &Path) -> Result<(), UnfError> {
    // Check projects.json is readable.
    let projects_json = dest.join("projects.json");
    if projects_json.exists() {
        fs::read(&projects_json).map_err(|e| {
            UnfError::Config(format!(
                "Verification failed: cannot read projects.json at {}: {}. Your data is safe at {}",
                dest.display(),
                e,
                dest.display()
            ))
        })?;
    }

    // Attempt to open one SQLite database.
    if let Ok(registry) = crate::registry::load() {
        for entry in &registry.projects {
            let relative = entry
                .path
                .to_string_lossy()
                .trim_start_matches('/')
                .to_owned();
            let db_path = dest.join("data").join(&relative).join("db.sqlite3");
            if db_path.exists() {
                rusqlite::Connection::open(&db_path).map_err(|e| {
                    UnfError::Config(format!(
                        "Verification failed: cannot open database at {}: {}. Your data is safe at {}",
                        db_path.display(),
                        e,
                        dest.display()
                    ))
                })?;
                break; // One successful open is enough.
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Phase 7: restart daemon
// ---------------------------------------------------------------------------

/// Spawns the daemon via the `__boot` subcommand.
///
/// Fire-and-forget: the new daemon process detaches immediately.
///
/// # Errors
///
/// Returns `UnfError::Config` if the current executable cannot be found or
/// the spawn fails.
pub fn restart_daemon() -> Result<(), UnfError> {
    let exe = std::env::current_exe().map_err(|e| {
        UnfError::Config(format!("Cannot determine executable path: {}", e))
    })?;

    std::process::Command::new(&exe)
        .arg("__boot")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| {
            UnfError::Config(format!(
                "Failed to restart daemon: {}. Your data is safe at the new location.",
                e
            ))
        })?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Phase 8: cleanup old directory
// ---------------------------------------------------------------------------

/// Renames the source directory to `<source>.migrated`.
///
/// If `<source>.migrated` already exists, appends a Unix timestamp to make
/// the name unique: `<source>.migrated.<timestamp>`.
///
/// Returns the path the source was renamed to.
///
/// # Errors
///
/// Returns `UnfError::Config` if the rename fails.
pub fn cleanup_old(source: &Path) -> Result<PathBuf, UnfError> {
    let migrated_base = {
        let name = source.to_string_lossy();
        PathBuf::from(format!("{}.migrated", name))
    };

    let migrated_path = if migrated_base.exists() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        PathBuf::from(format!("{}.{}", migrated_base.display(), ts))
    } else {
        migrated_base
    };

    fs::rename(source, &migrated_path).map_err(|e| {
        UnfError::Config(format!(
            "Failed to rename old storage directory: {}. Your data is safe at {}",
            e,
            source.display()
        ))
    })?;

    Ok(migrated_path)
}

// ---------------------------------------------------------------------------
// Progress output helpers
// ---------------------------------------------------------------------------

/// Emits a progress event in the appropriate format.
///
/// JSON mode: prints one JSON object per line on stdout.
/// Human mode: prints the human message (skips empty strings).
fn emit_progress(format: OutputFormat, json_value: &serde_json::Value, human_msg: &str) {
    if format == OutputFormat::Json {
        println!("{}", json_value);
    } else if !human_msg.is_empty() {
        println!("{}", human_msg);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // resolve_destination
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_destination_default() {
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        // "default" should resolve to a path ending in .unfudged and is_default=true.
        let result = resolve_destination("default");
        // If home dir is available this should succeed.
        if let Ok((path, is_default)) = result {
            assert!(is_default, "should be default");
            assert!(
                path.ends_with(".unfudged"),
                "expected path ending in .unfudged, got: {}",
                path.display()
            );
        }
        // If home dir unavailable (CI edge case), we just accept the error.
    }

    #[test]
    fn resolve_destination_absolute_path() {
        let (path, is_default) =
            resolve_destination("/some/absolute/path").expect("absolute path should resolve");
        assert_eq!(path, PathBuf::from("/some/absolute/path"));
        assert!(!is_default, "custom path should not be default");
    }

    #[test]
    fn resolve_destination_relative_path_rejected() {
        let result = resolve_destination("relative/path");
        assert!(result.is_err(), "relative paths must be rejected");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("No changes made"), "error must mention No changes made: {}", msg);
    }

    // -----------------------------------------------------------------------
    // preflight_checks
    // -----------------------------------------------------------------------

    #[test]
    fn preflight_dest_inside_source_rejected() {
        let source = TempDir::new().expect("source dir");
        let dest = source.path().join("subdir");

        let result = preflight_checks(source.path(), &dest);
        assert!(result.is_err(), "dest inside source must be rejected");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("No changes made"), "must mention No changes made: {}", msg);
    }

    #[test]
    fn preflight_dest_has_data_rejected() {
        let source = TempDir::new().expect("source dir");
        let dest = TempDir::new().expect("dest dir");

        // Write a file to make dest non-empty.
        fs::write(dest.path().join("existing.txt"), b"data").expect("write file");

        let result = preflight_checks(source.path(), dest.path());
        assert!(result.is_err(), "non-empty dest must be rejected");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("No changes made"), "must mention No changes made: {}", msg);
    }

    #[test]
    fn preflight_dest_empty_ok() {
        let source = TempDir::new().expect("source dir");
        // Write something to source so space check has a reference.
        fs::write(source.path().join("data.txt"), b"hello").expect("write");
        let dest = TempDir::new().expect("dest dir (empty)");

        // Both dirs are on the same filesystem in /tmp — space should be fine.
        // dest is empty so the check should pass.
        let result = preflight_checks(source.path(), dest.path());
        assert!(result.is_ok(), "empty dest should pass: {:?}", result);
    }

    #[test]
    fn preflight_same_path_rejected() {
        let source = TempDir::new().expect("dir");
        let result = preflight_checks(source.path(), source.path());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("No changes made"), "{}", msg);
    }

    // -----------------------------------------------------------------------
    // copy_dir_recursive
    // -----------------------------------------------------------------------

    #[test]
    fn copy_dir_recursive_basic() {
        let src = TempDir::new().expect("src");
        let dst = TempDir::new().expect("dst");

        fs::write(src.path().join("hello.txt"), b"hello").expect("write");
        fs::create_dir(src.path().join("sub")).expect("mkdir");
        fs::write(src.path().join("sub").join("world.txt"), b"world").expect("write sub");

        copy_dir_recursive(src.path(), dst.path(), SKIP_FILES, SKIP_EXTENSIONS)
            .expect("copy should succeed");

        assert!(dst.path().join("hello.txt").exists(), "hello.txt must be copied");
        assert!(dst.path().join("sub").join("world.txt").exists(), "nested file must be copied");

        let content = fs::read(dst.path().join("hello.txt")).expect("read");
        assert_eq!(content, b"hello");
    }

    #[test]
    fn copy_dir_recursive_skips_runtime_files() {
        let src = TempDir::new().expect("src");
        let dst = TempDir::new().expect("dst");

        // Create files that should be skipped.
        fs::write(src.path().join("daemon.pid"), b"12345").expect("write daemon.pid");
        fs::write(src.path().join("sentinel.pid"), b"67890").expect("write sentinel.pid");
        fs::write(src.path().join("stopped"), b"").expect("write stopped");
        fs::write(src.path().join("db.sqlite3-wal"), b"wal").expect("write wal");
        fs::write(src.path().join("db.sqlite3-shm"), b"shm").expect("write shm");

        // Create a normal file that should be copied.
        fs::write(src.path().join("projects.json"), b"{}").expect("write projects.json");

        copy_dir_recursive(src.path(), dst.path(), SKIP_FILES, SKIP_EXTENSIONS)
            .expect("copy should succeed");

        assert!(!dst.path().join("daemon.pid").exists(), "daemon.pid must be skipped");
        assert!(!dst.path().join("sentinel.pid").exists(), "sentinel.pid must be skipped");
        assert!(!dst.path().join("stopped").exists(), "stopped must be skipped");
        assert!(!dst.path().join("db.sqlite3-wal").exists(), "wal file must be skipped");
        assert!(!dst.path().join("db.sqlite3-shm").exists(), "shm file must be skipped");
        assert!(dst.path().join("projects.json").exists(), "projects.json must be copied");
    }

    // -----------------------------------------------------------------------
    // cleanup_old
    // -----------------------------------------------------------------------

    #[test]
    fn cleanup_old_renames_to_migrated() {
        let base = TempDir::new().expect("base");
        let source = base.path().join("storage");
        fs::create_dir_all(&source).expect("create source");
        fs::write(source.join("data.txt"), b"data").expect("write");

        let backup = cleanup_old(&source).expect("cleanup should succeed");

        assert!(!source.exists(), "source must not exist after cleanup");
        assert!(backup.exists(), "backup must exist");
        let backup_name = backup.file_name().unwrap().to_string_lossy();
        assert!(
            backup_name.ends_with(".migrated"),
            "backup name must end with .migrated, got: {}",
            backup_name
        );
    }

    #[test]
    fn cleanup_old_appends_timestamp_if_migrated_exists() {
        let base = TempDir::new().expect("base");

        // Create both source and a pre-existing .migrated directory.
        let source = base.path().join("storage");
        let migrated = base.path().join("storage.migrated");

        fs::create_dir_all(&source).expect("create source");
        fs::create_dir_all(&migrated).expect("create pre-existing .migrated");
        fs::write(source.join("data.txt"), b"data").expect("write");

        let backup = cleanup_old(&source).expect("cleanup should succeed");

        assert!(!source.exists(), "source must not exist after cleanup");
        // .migrated already existed, so the new backup gets a timestamp suffix.
        let backup_name = backup.file_name().unwrap().to_string_lossy();
        // It should be something like "storage.migrated.1234567890"
        assert!(
            backup_name.contains(".migrated."),
            "backup name must contain .migrated. suffix, got: {}",
            backup_name
        );
        assert!(migrated.exists(), "pre-existing .migrated must still exist");
    }
}
