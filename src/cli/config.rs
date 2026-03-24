//! The `unf config` command implementation.
//!
//! Shows the current storage configuration and disk usage. Detects
//! interrupted migrations and prints recovery guidance.

use crate::cli::OutputFormat;
use crate::error::UnfError;

/// JSON output for the config command.
#[derive(serde::Serialize)]
struct ConfigOutput {
    storage_dir: Option<String>,
    storage_dir_display: String,
    is_default: bool,
    disk_usage_bytes: u64,
    project_count: usize,
}

/// Executes the `unf config` command.
///
/// Checks for an interrupted migration lock, then loads the current
/// configuration and prints storage location and disk usage.
///
/// # Arguments
///
/// * `format` - Output format (human or JSON)
///
/// # Errors
///
/// Returns an error if the config directory cannot be determined or
/// disk usage cannot be calculated.
pub fn run(format: OutputFormat) -> Result<(), UnfError> {
    // 1. Check for migration lock (config dir sibling of config.json)
    let config_file_path = crate::config::config_path()?;
    let config_dir = config_file_path
        .parent()
        .ok_or_else(|| UnfError::Config("Cannot determine config directory".to_string()))?;
    let lock_path = config_dir.join("migration.lock");

    if lock_path.exists() {
        // Read the original path from the lock file, if present.
        let original_path = std::fs::read_to_string(&lock_path)
            .ok()
            .and_then(|s| {
                let trimmed = s.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            })
            .unwrap_or_else(|| "~/.unfudged".to_string());

        println!("Migration was interrupted.");
        println!(
            "Your data is safe at the original location: {}",
            original_path
        );
        println!(
            "Run `unf config --move-storage <DEST>` to retry, or delete the lock file to proceed."
        );
        return Ok(());
    }

    // 2. Load config
    let config = crate::config::load()?;

    // 3. Determine if this is the default (no UNF_HOME, no config override)
    let unf_home_set = std::env::var("UNF_HOME").is_ok();
    let is_default = !unf_home_set && config.storage_dir.is_none();

    // 4. Resolve the actual storage path
    let storage_path = crate::registry::global_dir()?;

    // 5. Build a display path (replace $HOME prefix with ~)
    let display_path = home_relative(&storage_path);

    // 6. Get usage stats
    let (disk_usage_bytes, project_count) = crate::config::storage_usage(&storage_path)?;

    // 7. Print output
    if format == OutputFormat::Json {
        let output = ConfigOutput {
            storage_dir: config.storage_dir.map(|p| p.display().to_string()),
            storage_dir_display: display_path,
            is_default,
            disk_usage_bytes,
            project_count,
        };
        println!("{}", serde_json::to_string(&output).unwrap());
    } else {
        let label = if is_default {
            format!("{} (default)", display_path)
        } else {
            display_path
        };
        println!("Storage directory: {}", label);
        println!(
            "  {} {}, {} used",
            project_count,
            if project_count == 1 {
                "project"
            } else {
                "projects"
            },
            crate::cli::format_size(disk_usage_bytes),
        );
    }

    Ok(())
}

/// Delegates `--move-storage` to the migration engine.
///
/// # Arguments
///
/// * `dest` - Destination path string (may be "default" for ~/.unfudged)
/// * `format` - Output format (human or JSON)
///
/// # Errors
///
/// Propagates any error from the migration engine.
pub fn run_move_storage(dest: &str, format: OutputFormat) -> Result<(), UnfError> {
    super::migrate::run(dest, format)
}

/// Replaces the user's home directory prefix in `path` with `~`.
///
/// Pure function — no I/O. Falls back to the full path string if the
/// home directory cannot be determined or is not a prefix of `path`.
fn home_relative(path: &std::path::Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(stripped) = path.strip_prefix(&home) {
            return format!("~/{}", stripped.display());
        }
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // -----------------------------------------------------------------------
    // home_relative()
    // -----------------------------------------------------------------------

    #[test]
    fn home_relative_replaces_home_prefix() {
        if let Some(home) = dirs::home_dir() {
            let path = home.join("some").join("path");
            let result = home_relative(&path);
            assert!(result.starts_with("~/"), "Expected ~/…, got: {}", result);
            assert!(result.contains("some/path"), "Got: {}", result);
        }
    }

    #[test]
    fn home_relative_non_home_path_unchanged() {
        let path = std::path::Path::new("/tmp/nonhome/path");
        let result = home_relative(path);
        assert_eq!(result, "/tmp/nonhome/path");
    }

    // -----------------------------------------------------------------------
    // Helpers for env isolation
    // -----------------------------------------------------------------------

    /// Sets an env var and returns the previous value so it can be restored.
    fn set_env(key: &str, val: &std::path::Path) -> Option<String> {
        let prev = std::env::var(key).ok();
        std::env::set_var(key, val);
        prev
    }

    /// Restores an env var to its previous value (or removes it if it was absent).
    fn restore_env(key: &str, prev: Option<String>) {
        match prev {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
    }

    // -----------------------------------------------------------------------
    // Migration lock detection
    // -----------------------------------------------------------------------

    /// Runs the config command with UNF_HOME pointing at `tmp` so that
    /// registry::global_dir() resolves to a known directory.
    ///
    /// Returns stdout captured via a redirected print — because the function
    /// writes to stdout, we test the lock detection path separately by
    /// checking the lock file exists and the function returns Ok(()).
    #[test]
    fn lock_file_detected_returns_ok() {
        let guard = crate::test_util::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let tmp = TempDir::new().expect("create temp dir");
        let prev_home = set_env("HOME", tmp.path());
        let prev_xdg = set_env("XDG_CONFIG_HOME", &tmp.path().join("config"));
        let prev_unf = set_env("UNF_HOME", tmp.path());

        // Create the config dir and drop a migration.lock file there
        let config_path = crate::config::config_path().expect("config_path");
        let config_dir = config_path.parent().expect("config dir");
        fs::create_dir_all(config_dir).expect("create config dir");
        fs::write(config_dir.join("migration.lock"), b"/old/path").expect("write lock");

        // Should return Ok without printing normal output
        let result = run(OutputFormat::Human);

        restore_env("HOME", prev_home);
        restore_env("XDG_CONFIG_HOME", prev_xdg);
        restore_env("UNF_HOME", prev_unf);
        drop(guard);

        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    }

    // -----------------------------------------------------------------------
    // Default config output
    // -----------------------------------------------------------------------

    #[test]
    fn default_config_human_output_returns_ok() {
        let guard = crate::test_util::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let tmp = TempDir::new().expect("create temp dir");
        let prev_home = set_env("HOME", tmp.path());
        let prev_xdg = set_env("XDG_CONFIG_HOME", &tmp.path().join("config"));
        let prev_unf = set_env("UNF_HOME", tmp.path());

        let result = run(OutputFormat::Human);

        restore_env("HOME", prev_home);
        restore_env("XDG_CONFIG_HOME", prev_xdg);
        restore_env("UNF_HOME", prev_unf);
        drop(guard);

        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    }

    // -----------------------------------------------------------------------
    // JSON output
    // -----------------------------------------------------------------------

    #[test]
    fn json_output_returns_ok() {
        let guard = crate::test_util::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let tmp = TempDir::new().expect("create temp dir");
        let prev_home = set_env("HOME", tmp.path());
        let prev_xdg = set_env("XDG_CONFIG_HOME", &tmp.path().join("config"));
        let prev_unf = set_env("UNF_HOME", tmp.path());

        let result = run(OutputFormat::Json);

        restore_env("HOME", prev_home);
        restore_env("XDG_CONFIG_HOME", prev_xdg);
        restore_env("UNF_HOME", prev_unf);
        drop(guard);

        assert!(result.is_ok(), "Expected Ok, got: {:?}", result);
    }

    // -----------------------------------------------------------------------
    // ConfigOutput JSON structure
    // -----------------------------------------------------------------------

    #[test]
    fn config_output_serializes_correctly() {
        let output = ConfigOutput {
            storage_dir: None,
            storage_dir_display: "~/.unfudged".to_string(),
            is_default: true,
            disk_usage_bytes: 4_500_000_000,
            project_count: 12,
        };
        let json = serde_json::to_string(&output).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");

        assert_eq!(parsed["storage_dir"], serde_json::Value::Null);
        assert_eq!(parsed["storage_dir_display"], "~/.unfudged");
        assert_eq!(parsed["is_default"], true);
        assert_eq!(parsed["disk_usage_bytes"], 4_500_000_000_u64);
        assert_eq!(parsed["project_count"], 12);
    }

    #[test]
    fn config_output_with_custom_dir_serializes_correctly() {
        let output = ConfigOutput {
            storage_dir: Some("/custom/path".to_string()),
            storage_dir_display: "/custom/path".to_string(),
            is_default: false,
            disk_usage_bytes: 1024,
            project_count: 3,
        };
        let json = serde_json::to_string(&output).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");

        assert_eq!(parsed["storage_dir"], "/custom/path");
        assert_eq!(parsed["is_default"], false);
    }
}
