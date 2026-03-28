//! Centralized storage path resolution for UNFUDGED.
//!
//! All per-project storage lives under `~/.unfudged/data/{mirrored_canonical_path}/`.
//! This module is the single source of truth for deriving storage paths from
//! project roots. Every module that needs a storage path calls these functions
//! instead of constructing paths independently.

use std::path::{Path, PathBuf};

use crate::error::UnfError;

/// Directory name under `~/.unfudged/` for per-project data.
const DATA_DIR: &str = "data";

/// Resolves the centralized storage directory for a project.
///
/// Given a project root like `/Users/cy/code/myapp`, returns
/// `~/.unfudged/data/Users/cy/code/myapp/`.
///
/// The path is canonicalized to resolve symlinks and normalize `.`/`..`.
///
/// # Arguments
///
/// * `project_root` - The project directory (will be canonicalized)
///
/// # Errors
///
/// Returns error if the home directory cannot be determined, or if
/// the project root cannot be canonicalized.
pub fn resolve_storage_dir(project_root: &Path) -> Result<PathBuf, UnfError> {
    let global_dir = crate::registry::global_dir()?;
    let canonical = project_root.canonicalize().map_err(|e| {
        UnfError::InvalidArgument(format!("Failed to canonicalize project path: {}", e))
    })?;
    let relative = strip_root_prefix(&canonical);
    Ok(global_dir.join(DATA_DIR).join(relative))
}

/// Resolves the storage directory using a custom global root.
///
/// Same as [`resolve_storage_dir`] but allows overriding the global
/// root for testing purposes.
///
/// # Arguments
///
/// * `project_root` - The project directory (will be canonicalized)
/// * `global_root` - Override for `~/.unfudged/` (e.g., a temp dir in tests)
pub fn resolve_storage_dir_with_root(
    project_root: &Path,
    global_root: &Path,
) -> Result<PathBuf, UnfError> {
    let canonical = project_root.canonicalize().map_err(|e| {
        UnfError::InvalidArgument(format!("Failed to canonicalize project path: {}", e))
    })?;
    let relative = strip_root_prefix(&canonical);
    Ok(global_root.join(DATA_DIR).join(relative))
}

/// Resolves the storage directory for an already-canonical project path.
///
/// Unlike [`resolve_storage_dir`], this does NOT call `canonicalize()`.
/// Use this when the path is already known to be canonical (e.g., from
/// the project registry) and the original directory may no longer exist.
///
/// # Arguments
///
/// * `canonical_project_root` - The canonical project directory path
///
/// # Errors
///
/// Returns error if the home directory cannot be determined.
pub fn resolve_storage_dir_canonical(canonical_project_root: &Path) -> Result<PathBuf, UnfError> {
    let global_dir = crate::registry::global_dir()?;
    let relative = strip_root_prefix(canonical_project_root);
    Ok(global_dir.join(DATA_DIR).join(relative))
}

/// Test variant of [`resolve_storage_dir_canonical`] with custom global root.
///
/// # Arguments
///
/// * `canonical_project_root` - The canonical project directory path
/// * `global_root` - Override for `~/.unfudged/` (e.g., a temp dir in tests)
pub fn resolve_storage_dir_canonical_with_root(
    canonical_project_root: &Path,
    global_root: &Path,
) -> PathBuf {
    let relative = strip_root_prefix(canonical_project_root);
    global_root.join(DATA_DIR).join(relative)
}

/// Strips the root prefix from a canonicalized path.
///
/// Unix: strips leading `/` -> `Users/cy/code/myapp`
/// Windows: strips `\\?\`, removes `:` from drive letter, strips leading `\`
///
/// Pure function, no I/O.
fn strip_root_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();

    #[cfg(windows)]
    {
        let cleaned = s.strip_prefix(r"\\?\").unwrap_or(&s).replace(":", "");
        let cleaned = cleaned.strip_prefix('\\').unwrap_or(&cleaned);
        PathBuf::from(cleaned)
    }

    #[cfg(not(windows))]
    {
        let stripped = s.strip_prefix('/').unwrap_or(&s);
        PathBuf::from(stripped)
    }
}

/// Returns the path to the SQLite database file.
pub fn db_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join("db.sqlite3")
}

/// Returns the path to the CAS objects directory.
pub fn objects_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join("objects")
}

/// Returns the path to the daemon PID file.
pub fn pid_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join("daemon.pid")
}

/// Returns the path to the stopped sentinel file.
pub fn stopped_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join("stopped")
}

/// Returns the path to the global daemon PID file.
///
/// The global daemon manages all watched projects. Its PID file
/// lives at `~/.unfudged/daemon.pid`.
pub fn global_pid_path() -> Result<PathBuf, UnfError> {
    let global_dir = crate::registry::global_dir()?;
    Ok(global_dir.join("daemon.pid"))
}

/// Returns the path to the sentinel PID file.
///
/// The sentinel watchdog monitors the daemon. Its PID file
/// lives at `~/.unfudged/sentinel.pid`.
pub fn sentinel_pid_path() -> Result<PathBuf, UnfError> {
    let global_dir = crate::registry::global_dir()?;
    Ok(global_dir.join("sentinel.pid"))
}

/// Returns the path to the global stopped marker.
///
/// When `unf stop` is run, this file is created to signal the sentinel
/// to exit. Removed by `unf restart`.
pub fn global_stopped_path() -> Result<PathBuf, UnfError> {
    let global_dir = crate::registry::global_dir()?;
    Ok(global_dir.join("stopped"))
}

/// Returns the path to the daemon freshness sidecar file.
///
/// The daemon writes the current UTC timestamp (RFC 3339) to this file
/// after each non-empty batch flush. The sentinel reads this file to
/// verify that snapshots are actively being recorded without opening SQLite.
///
/// Pure function — no I/O.
pub fn last_snapshot_time_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join("last_snapshot_time")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn strip_root_prefix_unix_absolute() {
        let path = PathBuf::from("/Users/cy/code/myapp");
        let result = strip_root_prefix(&path);
        assert_eq!(result, PathBuf::from("Users/cy/code/myapp"));
    }

    #[test]
    fn strip_root_prefix_unix_root() {
        let path = PathBuf::from("/");
        let result = strip_root_prefix(&path);
        assert_eq!(result, PathBuf::from(""));
    }

    #[test]
    fn strip_root_prefix_unix_single_component() {
        let path = PathBuf::from("/tmp");
        let result = strip_root_prefix(&path);
        assert_eq!(result, PathBuf::from("tmp"));
    }

    #[test]
    fn resolve_storage_dir_with_root_produces_correct_path() {
        let project = TempDir::new().expect("create project dir");
        let global = TempDir::new().expect("create global dir");

        let result =
            resolve_storage_dir_with_root(project.path(), global.path()).expect("should resolve");

        // Should be under global/data/...
        assert!(result.starts_with(global.path().join("data")));
        // Should not contain the leading /
        let relative = result
            .strip_prefix(global.path().join("data"))
            .expect("should strip prefix");
        assert!(!relative.to_string_lossy().starts_with('/'));
    }

    #[test]
    fn resolve_storage_dir_with_root_is_deterministic() {
        let project = TempDir::new().expect("create project dir");
        let global = TempDir::new().expect("create global dir");

        let result1 =
            resolve_storage_dir_with_root(project.path(), global.path()).expect("resolve 1");
        let result2 =
            resolve_storage_dir_with_root(project.path(), global.path()).expect("resolve 2");

        assert_eq!(result1, result2);
    }

    #[test]
    fn resolve_storage_dir_with_root_fails_for_nonexistent_path() {
        let global = TempDir::new().expect("create global dir");
        let result =
            resolve_storage_dir_with_root(Path::new("/nonexistent/path/12345"), global.path());
        assert!(result.is_err());
    }

    #[test]
    fn accessor_db_path() {
        let storage = PathBuf::from("/home/.unfudged/data/project");
        assert_eq!(
            db_path(&storage),
            PathBuf::from("/home/.unfudged/data/project/db.sqlite3")
        );
    }

    #[test]
    fn accessor_objects_path() {
        let storage = PathBuf::from("/home/.unfudged/data/project");
        assert_eq!(
            objects_path(&storage),
            PathBuf::from("/home/.unfudged/data/project/objects")
        );
    }

    #[test]
    fn accessor_pid_path() {
        let storage = PathBuf::from("/home/.unfudged/data/project");
        assert_eq!(
            pid_path(&storage),
            PathBuf::from("/home/.unfudged/data/project/daemon.pid")
        );
    }

    #[test]
    fn accessor_stopped_path() {
        let storage = PathBuf::from("/home/.unfudged/data/project");
        assert_eq!(
            stopped_path(&storage),
            PathBuf::from("/home/.unfudged/data/project/stopped")
        );
    }

    #[test]
    fn resolve_storage_dir_canonical_with_root_produces_correct_path() {
        let global = TempDir::new().expect("create global dir");
        let canonical_project = PathBuf::from("/Users/cy/code/myapp");

        let result = resolve_storage_dir_canonical_with_root(&canonical_project, global.path());

        // Should be under global/data/...
        assert!(result.starts_with(global.path().join("data")));
        // Should contain the project path without the leading /
        assert!(result.ends_with("Users/cy/code/myapp"));
        // Should not contain the leading /
        let relative = result
            .strip_prefix(global.path().join("data"))
            .expect("should strip prefix");
        assert!(!relative.to_string_lossy().starts_with('/'));
    }

    #[test]
    fn resolve_storage_dir_canonical_with_root_works_for_nonexistent_path() {
        let global = TempDir::new().expect("create global dir");
        let canonical_project = PathBuf::from("/nonexistent/deleted/project");

        // Should not panic or error - it doesn't check if the path exists
        let result = resolve_storage_dir_canonical_with_root(&canonical_project, global.path());

        // Should produce a valid storage path even though the project doesn't exist
        assert!(result.starts_with(global.path().join("data")));
        assert!(result.ends_with("nonexistent/deleted/project"));
    }

    #[test]
    fn resolve_storage_dir_canonical_with_root_matches_canonical_variant() {
        let project = TempDir::new().expect("create project dir");
        let global = TempDir::new().expect("create global dir");

        // Get canonical path first
        let canonical = project.path().canonicalize().expect("canonicalize");

        // Compare the two approaches
        let result_with_canonicalization =
            resolve_storage_dir_with_root(project.path(), global.path())
                .expect("resolve with canonicalization");
        let result_already_canonical =
            resolve_storage_dir_canonical_with_root(&canonical, global.path());

        // They should produce the same result
        assert_eq!(result_with_canonicalization, result_already_canonical);
    }

    #[test]
    fn global_pid_path_ends_with_daemon_pid() {
        let path = global_pid_path().expect("should resolve global pid path");
        assert!(path.ends_with("daemon.pid"));
    }

    #[test]
    fn last_snapshot_time_path_correct() {
        let storage = PathBuf::from("/tmp/store");
        assert_eq!(
            last_snapshot_time_path(&storage),
            PathBuf::from("/tmp/store/last_snapshot_time")
        );
    }
}
