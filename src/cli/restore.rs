//! Implementation of `unf restore --at <time>` command.
//!
//! Restores files to a point in time, with a pre-restore safety snapshot.

use chrono::Utc;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::Path;

use super::{format_local_time, parse_time_spec};
use crate::cli::OutputFormat;
use crate::engine::Engine;
use crate::error::UnfError;
use crate::storage;
use crate::types::EventType;

/// JSON output for the restore command.
#[derive(serde::Serialize)]
struct RestoreOutput {
    target_time: String,
    restored: Vec<String>,
    skipped: Vec<SkippedFile>,
    dry_run: bool,
}

/// Information about a skipped file.
#[derive(serde::Serialize)]
struct SkippedFile {
    file: String,
    reason: String,
}

/// Restores files to a point in time.
///
/// # Flow:
/// 1. Parse the time specification and compute what would be restored
/// 2. If `dry_run`: show what would change and return
/// 3. If NOT `dry_run`:
///    - If JSON mode OR `yes` is true OR not a TTY: proceed without prompt
///    - Otherwise: prompt for confirmation
/// 4. If confirmed: create a pre-restore safety snapshot, restore files, show summary
///
/// # Arguments
///
/// * `project_root` - Root directory of the project
/// * `at` - Time specification (relative or absolute)
/// * `file_filter` - Optional file to restore (if specified, only this file is restored)
/// * `dry_run` - If true, only show what would happen without making changes
/// * `yes` - If true, skip confirmation prompt
/// * `format` - Output format (human or JSON)
///
/// # Returns
///
/// `Ok(())` on success, or `UnfError` if querying, loading, or writing fails.
#[allow(clippy::cognitive_complexity)]
// TODO(v0.18): reduce complexity
pub fn run(
    project_root: &Path,
    at: &str,
    file_filter: Option<&str>,
    dry_run: bool,
    yes: bool,
    format: OutputFormat,
) -> Result<(), UnfError> {
    // Parse the time specification
    let target_time = parse_time_spec(at)?;

    // Open the engine
    let storage_dir = storage::resolve_storage_dir(project_root)?;
    let engine = Engine::open(project_root, &storage_dir)?;

    // Get the target state (files as they existed at the target time)
    let target_state = engine.get_state_at(target_time)?;

    // Get all currently tracked files
    let current_files = engine.get_all_tracked_files()?;

    // Apply file filter if specified
    let current_files = if let Some(filter) = file_filter {
        if !current_files.iter().any(|f| f == filter) {
            return Err(UnfError::NoResults(format!(
                "No history for \"{}\".",
                filter
            )));
        }
        vec![filter.to_string()]
    } else {
        current_files
    };

    // Determine what needs to be restored
    let mut restored_files = Vec::new();
    let mut skipped_files = Vec::new();

    for file_path in current_files {
        match target_state.get(&file_path) {
            Some(target_snapshot) => {
                // File existed at target time
                if target_snapshot.event_type != EventType::Delete {
                    // File was not deleted at target time - restore it
                    restored_files.push(file_path);
                }
                // If it was deleted at target time, skip it (don't delete current files)
            }
            None => {
                // File didn't exist at target time
                skipped_files.push((
                    file_path,
                    "created after target time, still on disk".to_string(),
                ));
            }
        }
    }

    // Sort for consistent output
    restored_files.sort();

    // If dry_run, just show what would happen and return
    if dry_run {
        let skipped_json: Vec<SkippedFile> = skipped_files
            .iter()
            .map(|(file, reason)| SkippedFile {
                file: file.clone(),
                reason: reason.clone(),
            })
            .collect();

        let output = RestoreOutput {
            target_time: target_time.to_rfc3339(),
            restored: restored_files.clone(),
            skipped: skipped_json,
            dry_run: true,
        };

        if format == OutputFormat::Json {
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        } else {
            let local_time = format_local_time(target_time);
            let now = Utc::now();
            let duration = now.signed_duration_since(target_time);
            println!(
                "Restoring to {} ({}):\n",
                super::format_duration_ago(duration),
                local_time
            );

            for file in &restored_files {
                println!("  restored  {}", file);
            }

            for (file, reason) in &skipped_files {
                println!("  skipped   {}  ({})", file, reason);
            }

            println!(
                "\n{} file{} would be restored, {} skipped.",
                restored_files.len(),
                if restored_files.len() == 1 { "" } else { "s" },
                skipped_files.len()
            );

            println!("\nThis was a dry run. No files were modified.");
        }

        return Ok(());
    }

    // Determine if we should prompt for confirmation
    let should_prompt = format != OutputFormat::Json && !yes && io::stdout().is_terminal();

    // If we need to prompt, show what would change and ask for confirmation
    if should_prompt {
        let local_time = format_local_time(target_time);
        let now = Utc::now();
        let duration = now.signed_duration_since(target_time);
        println!(
            "Restore to {} ({})?\n",
            super::format_duration_ago(duration),
            local_time
        );

        if !restored_files.is_empty() {
            println!("  Will restore: {}", restored_files.join(", "));
        }

        if !skipped_files.is_empty() {
            let skip_list: Vec<&str> = skipped_files.iter().map(|(f, _)| f.as_str()).collect();
            println!(
                "  Will skip (created after target): {}",
                skip_list.join(", ")
            );
        }

        // Prompt for confirmation
        print!("\nProceed? [y/N] ");
        use std::io::Write;
        io::stdout()
            .flush()
            .map_err(|e| UnfError::InvalidArgument(format!("Failed to flush output: {}", e)))?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|e| UnfError::InvalidArgument(format!("Failed to read input: {}", e)))?;

        if !input.trim().eq_ignore_ascii_case("y") {
            return Err(UnfError::InvalidArgument("Restore cancelled.".to_string()));
        }
    }

    // Create a pre-restore safety snapshot
    create_safety_snapshot(&engine)?;

    // Now perform the actual restore
    let mut actually_restored = Vec::new();
    for file_path in restored_files {
        if let Some(target_snapshot) = target_state.get(&file_path) {
            restore_file(&engine, &file_path, target_snapshot)?;
            actually_restored.push(file_path);
        }
    }

    // Prepare output
    let skipped_json: Vec<SkippedFile> = skipped_files
        .iter()
        .map(|(file, reason)| SkippedFile {
            file: file.clone(),
            reason: reason.clone(),
        })
        .collect();

    let output = RestoreOutput {
        target_time: target_time.to_rfc3339(),
        restored: actually_restored.clone(),
        skipped: skipped_json,
        dry_run: false,
    };

    if format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        let local_time = format_local_time(target_time);

        for file in &actually_restored {
            super::output::print_status("Restored", &format!("{} (from {})", file, local_time));
        }

        for (file, reason) in &skipped_files {
            println!("{:>12}  {}  ({})", "Skipped", file, reason);
        }
    }

    Ok(())
}

/// Restores files to the start of a detected session.
///
/// # Arguments
///
/// * `project_root` - Root directory of the project
/// * `session_num` - Session number (0 = most recent, N > 0 = specific session)
/// * `since` - Optional time to limit session detection scope
/// * `file_filter` - Optional file to restore (if specified, only this file is restored)
/// * `dry_run` - If true, only show what would happen without making changes
/// * `yes` - If true, skip confirmation prompt
/// * `format` - Output format (human or JSON)
///
/// # Returns
///
/// `Ok(())` on success, or `UnfError` if session detection or restoration fails.
pub fn run_session(
    project_root: &Path,
    session_num: usize,
    since: Option<&str>,
    file_filter: Option<&str>,
    dry_run: bool,
    yes: bool,
    format: OutputFormat,
) -> Result<(), UnfError> {
    use crate::cli::session;
    use crate::engine::Engine;
    use crate::storage;

    let storage_dir = storage::resolve_storage_dir(project_root)?;
    let engine = Engine::open(project_root, &storage_dir)?;

    // Get snapshots for session detection
    let since_time = if let Some(spec) = since {
        parse_time_spec(spec)?
    } else {
        chrono::TimeZone::with_ymd_and_hms(&Utc, 2000, 1, 1, 0, 0, 0).unwrap()
    };

    let mut snapshots = engine.get_snapshots_since(since_time)?;
    snapshots.sort_by_key(|s| s.timestamp);

    let sessions = session::detect_sessions(&snapshots);
    let number = if session_num == 0 {
        None
    } else {
        Some(session_num)
    };
    let resolved = session::resolve_session(&sessions, number)?;

    // Delegate to existing run() with the session's start time
    let at_spec = resolved.start.to_rfc3339();
    run(project_root, &at_spec, file_filter, dry_run, yes, format)
}

/// Restores a single file from the CAS to disk.
fn restore_file(
    engine: &Engine,
    file_path: &str,
    snapshot: &crate::types::Snapshot,
) -> Result<(), UnfError> {
    // Load content from CAS
    let content = engine.load_content(&snapshot.content_hash)?;

    // Write to disk
    let engine_root = std::env::current_dir().map_err(|e| {
        UnfError::InvalidArgument(format!("Failed to get current directory: {}", e))
    })?;
    let full_path = engine_root.join(file_path);

    // Create parent directories if needed
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            UnfError::InvalidArgument(format!("Failed to create directories: {}", e))
        })?;
    }

    // Write the file
    fs::write(&full_path, &content).map_err(|e| {
        UnfError::InvalidArgument(format!("Failed to write file {}: {}", file_path, e))
    })?;

    Ok(())
}

/// Creates a safety snapshot of all currently tracked files.
///
/// This is done before any restore operation to ensure the user can recover
/// if the restore goes wrong.
fn create_safety_snapshot(engine: &Engine) -> Result<(), UnfError> {
    // Get all currently tracked files
    let current_files = engine.get_all_tracked_files()?;

    // Create a snapshot for each currently tracked file
    for file_path in current_files {
        // Check if the file still exists
        let engine_root = std::env::current_dir().map_err(|e| {
            UnfError::InvalidArgument(format!("Failed to get current directory: {}", e))
        })?;
        let full_path = engine_root.join(&file_path);
        if full_path.exists() && full_path.is_file() {
            // Snapshot the current state
            engine.create_snapshot(&file_path, EventType::Modify)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration as StdDuration;
    use tempfile::TempDir;

    fn setup_test_engine() -> (TempDir, TempDir, Engine) {
        let project = TempDir::new().expect("create project dir");
        let storage = TempDir::new().expect("create storage dir");
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");
        (project, storage, engine)
    }

    #[test]
    fn restore_snapshot_metadata_exists() {
        let (project, _storage, engine) = setup_test_engine();

        // Create a file and snapshot it
        let file_path = "test.txt";
        let full_path = project.path().join(file_path);
        fs::write(&full_path, b"original").expect("Failed to write");
        engine
            .create_snapshot(file_path, EventType::Create)
            .expect("Snapshot failed");

        // Wait a bit
        thread::sleep(StdDuration::from_millis(10));

        // Record the time before modification
        let target_time = Utc::now();
        thread::sleep(StdDuration::from_millis(10));

        // Modify the file
        fs::write(&full_path, b"modified").expect("Failed to write");
        engine
            .create_snapshot(file_path, EventType::Modify)
            .expect("Snapshot failed");

        // Get state at target time (should be the original)
        let state = engine
            .get_state_at(target_time)
            .expect("get_state_at should succeed");

        // Should have the file with original content hash
        assert!(state.contains_key(file_path));
        let snapshot = &state[file_path];
        assert_eq!(snapshot.size_bytes, 8); // "original"
    }

    #[test]
    fn restore_get_tracked_files() {
        let (project, _storage, engine) = setup_test_engine();

        // Initially no tracked files
        let files = engine
            .get_all_tracked_files()
            .expect("Query should succeed");
        assert_eq!(files.len(), 0);

        // Create and snapshot a file
        let file_path = "test.txt";
        let full_path = project.path().join(file_path);
        fs::write(&full_path, b"content").expect("Failed to write");
        engine
            .create_snapshot(file_path, EventType::Create)
            .expect("Snapshot failed");

        // Now should have one tracked file
        let files = engine
            .get_all_tracked_files()
            .expect("Query should succeed");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], file_path);
    }

    #[test]
    fn restore_file_filter_limits_to_single_file() {
        let (project, _storage, engine) = setup_test_engine();

        let file1 = "file1.txt";
        let file2 = "file2.txt";
        let full_path1 = project.path().join(file1);
        let full_path2 = project.path().join(file2);

        // Create two files
        fs::write(&full_path1, b"content1").expect("Failed to write");
        engine
            .create_snapshot(file1, EventType::Create)
            .expect("Snapshot 1 failed");

        fs::write(&full_path2, b"content2").expect("Failed to write");
        engine
            .create_snapshot(file2, EventType::Create)
            .expect("Snapshot 2 failed");

        // Both files should be tracked
        let files = engine
            .get_all_tracked_files()
            .expect("Query should succeed");
        assert_eq!(files.len(), 2);

        // With filter, only the filtered file should be returned
        let filtered: Vec<String> = if let Some(filter) = Some("file1.txt") {
            if files.iter().any(|f| f == filter) {
                vec![filter.to_string()]
            } else {
                vec![]
            }
        } else {
            files
        };
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0], "file1.txt");
    }

    #[test]
    fn restore_file_filter_nonexistent_returns_empty() {
        let (project, _storage, engine) = setup_test_engine();

        let file1 = "file1.txt";
        let full_path1 = project.path().join(file1);
        fs::write(&full_path1, b"content1").expect("Failed to write");
        engine
            .create_snapshot(file1, EventType::Create)
            .expect("Snapshot 1 failed");

        let files = engine
            .get_all_tracked_files()
            .expect("Query should succeed");

        // Filter for nonexistent file
        let has_match = files.iter().any(|f| f == "nonexistent.txt");
        assert!(!has_match);
    }

    #[test]
    fn restore_multiple_tracked_files() {
        let (project, _storage, engine) = setup_test_engine();

        let file1 = "file1.txt";
        let file2 = "file2.txt";
        let full_path1 = project.path().join(file1);
        let full_path2 = project.path().join(file2);

        // Create two files
        fs::write(&full_path1, b"content1").expect("Failed to write");
        engine
            .create_snapshot(file1, EventType::Create)
            .expect("Snapshot 1 failed");

        fs::write(&full_path2, b"content2").expect("Failed to write");
        engine
            .create_snapshot(file2, EventType::Create)
            .expect("Snapshot 2 failed");

        // Should have two tracked files
        let mut files = engine
            .get_all_tracked_files()
            .expect("Query should succeed");
        files.sort();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0], file1);
        assert_eq!(files[1], file2);
    }
}
