//! Append-only audit log for UNFUDGED.
//!
//! Records significant lifecycle events (WATCH, UNWATCH, STOP, etc.) to
//! `~/.unfudged/audit.log` for debugging "how did this project get unwatched?"
//! situations. This module is best-effort: write failures are logged to stderr
//! but never propagated.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use chrono::Utc;

use crate::error::UnfError;

/// Maximum audit log size before rotation (1 MB).
const MAX_LOG_SIZE: u64 = 1_048_576;

/// Returns the path to the audit log (`~/.unfudged/audit.log`).
pub fn audit_path() -> Result<PathBuf, UnfError> {
    Ok(crate::registry::global_dir()?.join("audit.log"))
}

/// Logs an event to the audit log. Best-effort: failures go to stderr.
///
/// Format: `{ISO8601}\t{EVENT}\t{DETAIL}\n`
///
/// If the log exceeds 1 MB, it is rotated (renamed to `audit.log.1`)
/// before writing.
pub fn log_event(event: &str, detail: &str) {
    if let Err(e) = log_event_inner(event, detail) {
        eprintln!("audit: failed to log event: {}", e);
    }
}

fn log_event_inner(event: &str, detail: &str) -> Result<(), UnfError> {
    let path = audit_path()?;

    // Rotate if needed
    if let Ok(metadata) = fs::metadata(&path) {
        if metadata.len() >= MAX_LOG_SIZE {
            let backup = path.with_extension("log.1");
            let _ = fs::rename(&path, &backup);
        }
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to open audit log: {}", e)))?;

    let timestamp = Utc::now().to_rfc3339();
    writeln!(file, "{}\t{}\t{}", timestamp, event, detail)
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to write audit log: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::TempDir;

    fn with_test_home<F: FnOnce(&std::path::Path)>(f: F) {
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();
        let temp = TempDir::new().expect("create temp dir");
        let original = env::var("UNF_HOME").ok();
        env::set_var("UNF_HOME", temp.path());

        f(temp.path());

        if let Some(val) = original {
            env::set_var("UNF_HOME", val);
        } else {
            env::remove_var("UNF_HOME");
        }
    }

    #[test]
    fn log_event_creates_file() {
        with_test_home(|home| {
            log_event("TEST", "hello world");
            let path = home.join("audit.log");
            assert!(path.exists());
            let content = fs::read_to_string(&path).expect("read audit log");
            assert!(content.contains("TEST"));
            assert!(content.contains("hello world"));
        });
    }

    #[test]
    fn log_event_appends() {
        with_test_home(|home| {
            log_event("EVENT1", "first");
            log_event("EVENT2", "second");
            let path = home.join("audit.log");
            let content = fs::read_to_string(&path).expect("read audit log");
            let lines: Vec<&str> = content.lines().collect();
            assert_eq!(lines.len(), 2);
            assert!(lines[0].contains("EVENT1"));
            assert!(lines[1].contains("EVENT2"));
        });
    }

    #[test]
    fn log_event_format_has_three_tab_separated_fields() {
        with_test_home(|home| {
            log_event("WATCH", "/some/path");
            let path = home.join("audit.log");
            let content = fs::read_to_string(&path).expect("read audit log");
            let line = content.lines().next().unwrap();
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), 3);
            // First field is ISO8601 timestamp
            assert!(fields[0].contains('T'));
            assert_eq!(fields[1], "WATCH");
            assert_eq!(fields[2], "/some/path");
        });
    }

    #[test]
    fn log_rotation_at_1mb() {
        with_test_home(|home| {
            let path = home.join("audit.log");
            // Create a file just over 1MB
            let big_content = "x".repeat(MAX_LOG_SIZE as usize + 1);
            fs::write(&path, &big_content).expect("write big file");

            log_event("AFTER_ROTATION", "new entry");

            // Original should be rotated
            let backup = home.join("audit.log.1");
            assert!(backup.exists());
            let backup_content = fs::read_to_string(&backup).expect("read backup");
            assert_eq!(backup_content.len(), MAX_LOG_SIZE as usize + 1);

            // New file should have only the new entry
            let content = fs::read_to_string(&path).expect("read new log");
            assert!(content.contains("AFTER_ROTATION"));
            assert!(content.len() < 200); // Just one line
        });
    }
}
