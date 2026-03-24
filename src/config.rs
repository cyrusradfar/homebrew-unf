//! User configuration for UNFUDGED.
//!
//! Persists a small JSON file at the OS-appropriate config path so users can
//! override the default storage location without setting env vars.
//!
//! All state is in the pure [`Config`] struct. [`load`] and [`save`] are the
//! only I/O boundary functions, following the SUPER principle of isolating
//! side effects at the edge.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::UnfError;

/// Application name used as the config subdirectory.
const APP_NAME: &str = "unfudged";

/// Config file name within the application config directory.
const CONFIG_FILE: &str = "config.json";

/// Subdirectory under storage root that contains per-project data.
///
/// Each immediate child of this directory is counted as one project.
const DATA_SUBDIR: &str = "data";

/// User configuration for UNFUDGED.
///
/// Pure data struct — no behaviour, no I/O. Serializes to/from JSON via serde.
/// All fields have serde defaults so missing keys in older config files are
/// handled gracefully.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    /// Override for the storage directory.
    ///
    /// `None` means use the default (`~/.unfudged`). When present this path
    /// is used instead of `global_dir()` for all storage operations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_dir: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// I/O boundary functions
// ---------------------------------------------------------------------------

/// Returns the OS-appropriate path for the config file.
///
/// | Platform | Path |
/// |---|---|
/// | macOS   | `~/Library/Application Support/unfudged/config.json` |
/// | Linux   | `~/.config/unfudged/config.json` |
///
/// # Errors
///
/// Returns [`UnfError::Config`] if the OS config directory cannot be
/// determined.
pub fn config_path() -> Result<PathBuf, UnfError> {
    let base = dirs::config_dir().ok_or_else(|| {
        UnfError::Config("Cannot determine OS config directory".to_string())
    })?;
    Ok(base.join(APP_NAME).join(CONFIG_FILE))
}

/// Loads the config from disk.
///
/// | File state | Behaviour |
/// |---|---|
/// | Missing    | Returns [`Config::default()`] silently |
/// | Corrupted  | Prints a warning to stderr, returns [`Config::default()`] |
/// | Valid JSON | Returns the parsed [`Config`] |
///
/// This function never propagates a JSON parse error — callers always get a
/// usable config, even if the file is damaged.
///
/// # Errors
///
/// Returns [`UnfError::Config`] only if the config path itself cannot be
/// determined (i.e. the OS has no config dir).
pub fn load() -> Result<Config, UnfError> {
    let path = config_path()?;

    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Config::default());
        }
        Err(e) => {
            return Err(UnfError::Config(format!(
                "Failed to read config file {}: {}",
                path.display(),
                e
            )));
        }
    };

    match serde_json::from_slice::<Config>(&bytes) {
        Ok(cfg) => Ok(cfg),
        Err(e) => {
            eprintln!(
                "warning: config file {} is corrupted ({}); using defaults",
                path.display(),
                e
            );
            Ok(Config::default())
        }
    }
}

/// Writes the config to disk atomically.
///
/// Creates parent directories if they don't exist. Writes to a sibling temp
/// file first and then renames it into place so readers always see a complete
/// file even if the process crashes mid-write.
///
/// # Errors
///
/// Returns [`UnfError::Config`] on any I/O failure.
pub fn save(config: &Config) -> Result<(), UnfError> {
    let path = config_path()?;

    // Ensure the parent directory exists.
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            UnfError::Config(format!(
                "Failed to create config directory {}: {}",
                parent.display(),
                e
            ))
        })?;
    }

    let json = serde_json::to_string_pretty(config).map_err(|e| {
        UnfError::Config(format!("Failed to serialize config: {}", e))
    })?;

    // Atomic write: write to temp file in same directory, then rename.
    let tmp_path = path.with_extension("json.tmp");
    fs::write(&tmp_path, &json).map_err(|e| {
        UnfError::Config(format!(
            "Failed to write temp config file {}: {}",
            tmp_path.display(),
            e
        ))
    })?;
    fs::rename(&tmp_path, &path).map_err(|e| {
        UnfError::Config(format!(
            "Failed to rename temp config to {}: {}",
            path.display(),
            e
        ))
    })?;

    Ok(())
}

/// Calculates disk usage under a storage root.
///
/// Walks the entire directory tree under `storage_dir`, summing file sizes.
/// Counts immediate child directories of `storage_dir/data/` as projects
/// (each one mirrors a watched project root).
///
/// Returns `(total_bytes, project_count)`.
///
/// # Errors
///
/// Returns [`UnfError::Config`] if the directory cannot be read.
pub fn storage_usage(storage_dir: &Path) -> Result<(u64, usize), UnfError> {
    let mut total_bytes: u64 = 0;
    let mut project_count: usize = 0;

    // Count projects: each subdirectory under data/ is one project.
    let data_dir = storage_dir.join(DATA_SUBDIR);
    if data_dir.exists() {
        let entries = fs::read_dir(&data_dir).map_err(|e| {
            UnfError::Config(format!(
                "Failed to read data directory {}: {}",
                data_dir.display(),
                e
            ))
        })?;
        for entry in entries {
            let entry = entry.map_err(|e| {
                UnfError::Config(format!("Failed to read directory entry: {}", e))
            })?;
            if entry.path().is_dir() {
                project_count += 1;
            }
        }
    }

    // Sum all file sizes recursively under storage_dir.
    if storage_dir.exists() {
        total_bytes = sum_dir_bytes(storage_dir).map_err(|e| {
            UnfError::Config(format!(
                "Failed to calculate size of {}: {}",
                storage_dir.display(),
                e
            ))
        })?;
    }

    Ok((total_bytes, project_count))
}

// ---------------------------------------------------------------------------
// Pure helpers
// ---------------------------------------------------------------------------

/// Recursively sums the sizes of all regular files under `dir`.
///
/// Pure function — the only I/O is reading directory metadata.
fn sum_dir_bytes(dir: &Path) -> std::io::Result<u64> {
    let mut total: u64 = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = entry.metadata()?;
        if metadata.is_file() {
            total += metadata.len();
        } else if metadata.is_dir() {
            total += sum_dir_bytes(&path)?;
        }
    }
    Ok(total)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::MutexGuard;
    use tempfile::TempDir;

    /// Redirect config_path() to a temp directory by overriding HOME (via a
    /// throwaway env setup). Because config_path() uses dirs::config_dir()
    /// which itself reads the process env, we need the ENV_LOCK from
    /// test_util.
    ///
    /// Returns (TempDir, guard) — keep the guard alive for the test's scope.
    fn with_temp_config_dir() -> (TempDir, MutexGuard<'static, ()>) {
        let guard = crate::test_util::ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new().expect("create temp dir");
        // dirs::config_dir() on macOS reads HOME; on Linux reads XDG_CONFIG_HOME.
        // Setting both covers all platforms under test.
        std::env::set_var("HOME", tmp.path());
        std::env::set_var("XDG_CONFIG_HOME", tmp.path().join("config"));
        (tmp, guard)
    }

    fn clear_config_env() {
        std::env::remove_var("HOME");
        std::env::remove_var("XDG_CONFIG_HOME");
    }

    // -----------------------------------------------------------------------
    // config_path()
    // -----------------------------------------------------------------------

    #[test]
    fn config_path_returns_absolute_path() {
        let (_tmp, _guard) = with_temp_config_dir();
        let path = config_path().expect("config_path should succeed");
        assert!(path.is_absolute(), "config path must be absolute: {:?}", path);
        clear_config_env();
    }

    #[test]
    fn config_path_ends_with_config_json() {
        let (_tmp, _guard) = with_temp_config_dir();
        let path = config_path().expect("config_path should succeed");
        assert!(
            path.ends_with("unfudged/config.json"),
            "expected ...unfudged/config.json, got {:?}",
            path
        );
        clear_config_env();
    }

    // -----------------------------------------------------------------------
    // load()
    // -----------------------------------------------------------------------

    #[test]
    fn load_missing_file_returns_default() {
        let (_tmp, _guard) = with_temp_config_dir();
        let cfg = load().expect("load should succeed even without a file");
        assert!(cfg.storage_dir.is_none());
        clear_config_env();
    }

    #[test]
    fn load_corrupted_json_returns_default_no_panic() {
        let (tmp, _guard) = with_temp_config_dir();

        // Write garbage into the config file location.
        let path = config_path().expect("config_path");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"this is not json {{{{").unwrap();

        let cfg = load().expect("load should not error on corrupted JSON");
        assert!(
            cfg.storage_dir.is_none(),
            "corrupted JSON should yield default config"
        );
        drop(tmp);
        clear_config_env();
    }

    #[test]
    fn load_valid_config_with_storage_dir() {
        let (tmp, _guard) = with_temp_config_dir();

        let path = config_path().expect("config_path");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let json = r#"{"storage_dir": "/custom/storage"}"#;
        fs::write(&path, json).unwrap();

        let cfg = load().expect("load");
        assert_eq!(cfg.storage_dir, Some(PathBuf::from("/custom/storage")));
        drop(tmp);
        clear_config_env();
    }

    #[test]
    fn load_valid_config_with_null_storage_dir() {
        let (tmp, _guard) = with_temp_config_dir();

        let path = config_path().expect("config_path");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // storage_dir explicitly null should deserialize as None.
        let json = r#"{"storage_dir": null}"#;
        fs::write(&path, json).unwrap();

        // serde_json deserializes null into Option::None.
        // However our field uses `#[serde(default)]` — null is also valid.
        let cfg = load().expect("load");
        assert!(cfg.storage_dir.is_none());
        drop(tmp);
        clear_config_env();
    }

    // -----------------------------------------------------------------------
    // save()
    // -----------------------------------------------------------------------

    #[test]
    fn save_creates_parent_directories() {
        let (_tmp, _guard) = with_temp_config_dir();

        let cfg = Config {
            storage_dir: Some(PathBuf::from("/some/path")),
        };
        save(&cfg).expect("save should succeed");

        let path = config_path().expect("config_path");
        assert!(path.exists(), "config file should exist after save");
        clear_config_env();
    }

    #[test]
    fn save_is_atomic_via_temp_then_rename() {
        // Verify there is no leftover .json.tmp file after a successful save.
        let (_tmp, _guard) = with_temp_config_dir();

        let cfg = Config::default();
        save(&cfg).expect("save");

        let path = config_path().expect("config_path");
        let tmp_path = path.with_extension("json.tmp");
        assert!(
            !tmp_path.exists(),
            "temp file should be cleaned up after atomic rename"
        );
        clear_config_env();
    }

    // -----------------------------------------------------------------------
    // Round-trip
    // -----------------------------------------------------------------------

    #[test]
    fn round_trip_save_then_load_preserves_storage_dir() {
        let (tmp, _guard) = with_temp_config_dir();

        let original = Config {
            storage_dir: Some(tmp.path().join("my_storage")),
        };
        save(&original).expect("save");
        let loaded = load().expect("load");

        assert_eq!(original.storage_dir, loaded.storage_dir);
        clear_config_env();
    }

    #[test]
    fn round_trip_none_storage_dir() {
        let (_tmp, _guard) = with_temp_config_dir();

        let original = Config { storage_dir: None };
        save(&original).expect("save");
        let loaded = load().expect("load");

        assert!(loaded.storage_dir.is_none());
        clear_config_env();
    }

    // -----------------------------------------------------------------------
    // storage_usage()
    // -----------------------------------------------------------------------

    #[test]
    fn storage_usage_empty_dir_returns_zeros() {
        let tmp = TempDir::new().expect("create temp dir");
        let (bytes, projects) = storage_usage(tmp.path()).expect("storage_usage");
        assert_eq!(bytes, 0);
        assert_eq!(projects, 0);
    }

    #[test]
    fn storage_usage_nonexistent_dir_returns_zeros() {
        let tmp = TempDir::new().expect("create temp dir");
        let nonexistent = tmp.path().join("does_not_exist");
        let (bytes, projects) = storage_usage(&nonexistent).expect("storage_usage");
        assert_eq!(bytes, 0);
        assert_eq!(projects, 0);
    }

    #[test]
    fn storage_usage_counts_files_correctly() {
        let tmp = TempDir::new().expect("create temp dir");

        // Write two files of known size.
        fs::write(tmp.path().join("a.txt"), b"hello").unwrap(); // 5 bytes
        fs::write(tmp.path().join("b.txt"), b"world!!").unwrap(); // 7 bytes

        let (bytes, projects) = storage_usage(tmp.path()).expect("storage_usage");
        assert_eq!(bytes, 12);
        assert_eq!(projects, 0); // no data/ subdir yet
    }

    #[test]
    fn storage_usage_counts_project_subdirs() {
        let tmp = TempDir::new().expect("create temp dir");
        let data = tmp.path().join("data");

        // Create two project directories under data/.
        fs::create_dir_all(data.join("proj_a")).unwrap();
        fs::create_dir_all(data.join("proj_b")).unwrap();

        let (bytes, projects) = storage_usage(tmp.path()).expect("storage_usage");
        assert_eq!(projects, 2);
        // No files, so bytes can be 0 (directories themselves have no counted size).
        let _ = bytes; // we don't assert exact value; OS may vary
    }

    #[test]
    fn storage_usage_files_only_count_under_project_dirs() {
        let tmp = TempDir::new().expect("create temp dir");
        let data = tmp.path().join("data");

        // One project, with one object file inside.
        let proj = data.join("my_project");
        fs::create_dir_all(&proj).unwrap();
        fs::write(proj.join("object.blob"), b"12345678").unwrap(); // 8 bytes

        let (bytes, projects) = storage_usage(tmp.path()).expect("storage_usage");
        assert_eq!(projects, 1);
        assert_eq!(bytes, 8);
    }

    #[test]
    fn storage_usage_recursive_sum() {
        let tmp = TempDir::new().expect("create temp dir");

        // Nested file layout.
        let sub = tmp.path().join("level1").join("level2");
        fs::create_dir_all(&sub).unwrap();
        fs::write(tmp.path().join("root.txt"), b"aa").unwrap(); // 2
        fs::write(tmp.path().join("level1").join("mid.txt"), b"bbbb").unwrap(); // 4
        fs::write(sub.join("deep.txt"), b"cccccccc").unwrap(); // 8

        let (bytes, _) = storage_usage(tmp.path()).expect("storage_usage");
        assert_eq!(bytes, 14);
    }
}
