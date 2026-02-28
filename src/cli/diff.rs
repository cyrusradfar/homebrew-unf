//! Implementation of `unf diff` command.
//!
//! Supports two modes:
//! 1. Single-point: `unf diff --at <time>` - shows changes since a point in time
//! 2. Two-point: `unf diff --from <time> [--to <time>]` - compares state at two points in time

use std::collections::HashMap;
use std::path::Path;

use chrono::Utc;
use similar::{ChangeTag, TextDiff};

use crate::cli::output::{colors, use_color};
use crate::cli::OutputFormat;
use crate::engine::Engine;
use crate::error::UnfError;
use crate::storage;
use crate::types::{EventType, Snapshot};

pub use crate::diff::{compute_diff_stats, DiffStats};

/// JSON output for the diff command.
#[derive(serde::Serialize)]
struct DiffOutput {
    from: String,
    to: String,
    changes: Vec<DiffChange>,
}

/// A single file change in diff output.
#[derive(serde::Serialize)]
struct DiffChange {
    file: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<Vec<DiffLine>>, // OLD: kept for backward compat
    #[serde(skip_serializing_if = "Option::is_none")]
    hunks: Option<Vec<DiffHunk>>, // NEW: contextual format
}

/// A single line in a diff.
#[derive(serde::Serialize)]
struct DiffLine {
    op: String,
    content: String,
}

/// A single hunk in contextual diff format.
#[derive(serde::Serialize)]
struct DiffHunk {
    old_start: u32,
    old_count: u32,
    new_start: u32,
    new_count: u32,
    lines: Vec<DiffHunkLine>,
}

/// A single line in a hunk with line numbers.
#[derive(serde::Serialize)]
struct DiffHunkLine {
    op: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    old_num: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_num: Option<u32>,
}

/// Result of comparing two file states.
#[derive(Debug, PartialEq, Clone, Copy)]
enum FileChange {
    Created,
    Deleted,
    Modified,
}

/// Generates hunks with line numbers from a TextDiff (pure function).
///
/// # Arguments
/// * `diff` - The TextDiff to process
/// * `context_radius` - Number of context lines around changes
///
/// # Returns
/// A vector of DiffHunk structs with line numbers.
fn generate_hunks<'a>(diff: &TextDiff<'a, 'a, 'a, str>, context_radius: usize) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let mut binding = diff.unified_diff();
    let udiff = binding.context_radius(context_radius);

    for hunk in udiff.iter_hunks() {
        let mut lines = Vec::new();
        let mut old_start = 0u32;
        let mut old_count = 0u32;
        let mut new_start = 0u32;
        let mut new_count = 0u32;
        let mut first_change = true;

        for change in hunk.iter_changes() {
            let op = match change.tag() {
                ChangeTag::Delete => "delete",
                ChangeTag::Insert => "insert",
                ChangeTag::Equal => "equal",
            };

            // Extract line numbers (0-based, so add 1)
            let old_num = change.old_index().map(|idx| (idx + 1) as u32);
            let new_num = change.new_index().map(|idx| (idx + 1) as u32);

            // Track hunk range
            if first_change {
                old_start = old_num.unwrap_or(1);
                new_start = new_num.unwrap_or(1);
                first_change = false;
            }

            if old_num.is_some() {
                old_count += 1;
            }
            if new_num.is_some() {
                new_count += 1;
            }

            lines.push(DiffHunkLine {
                op: op.to_string(),
                content: change.to_string(),
                old_num,
                new_num,
            });
        }

        hunks.push(DiffHunk {
            old_start,
            old_count,
            new_start,
            new_count,
            lines,
        });
    }

    hunks
}

/// Displays changes to files comparing two points in time or since a specified time.
///
/// Supports two modes:
/// 1. Single-point mode (`--at`): Shows snapshots since a point in time
/// 2. Two-point mode (`--from`/`--to`): Compares file state at two specific points
///
/// # Arguments
///
/// * `project_root` - Root directory of the project
/// * `at` - Time specification for single-point mode (exclusive with `from`/`to`)
/// * `from` - Start time for two-point mode
/// * `to` - End time for two-point mode (defaults to now if not provided)
/// * `file_filter` - Optional file to limit diff to
/// * `format` - Output format (human or JSON)
/// * `context_radius` - Number of context lines around changes (default: 3)
///
/// # Returns
///
/// `Ok(())` on success, or `UnfError` if validation, querying, or processing fails.
#[allow(clippy::too_many_arguments)]
pub fn run(
    project_root: &Path,
    at: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
    snapshot: Option<i64>,
    file_filter: Option<&str>,
    format: OutputFormat,
    context_radius: usize,
) -> Result<(), UnfError> {
    let storage_dir = storage::resolve_storage_dir(project_root)?;
    let engine = Engine::open(project_root, &storage_dir)?;

    // Snapshot mode: --snapshot <id> (diff against predecessor)
    if let Some(snap_id) = snapshot {
        return run_snapshot_diff(&engine, snap_id, format, context_radius);
    }

    // Determine diff mode and validate arguments
    if at.is_some() && from.is_some() {
        return Err(UnfError::InvalidArgument(
            "Cannot use --at with --from/--to. Use --at for single-point diff or --from/--to for two-point diff.".to_string(),
        ));
    }

    if at.is_none() && from.is_none() {
        return Err(UnfError::InvalidArgument(
            "Either --at, --from, or --snapshot is required.".to_string(),
        ));
    }

    // Single-point mode: --at
    if let Some(at_spec) = at {
        run_single_point(&engine, at_spec, file_filter, format, context_radius)
    } else {
        // Two-point mode: --from / --to
        let from_spec = from.expect("from should be Some at this point");
        run_two_point(&engine, from_spec, to, file_filter, format, context_radius)
    }
}

/// Single-point diff: shows changes since a point in time.
///
/// Delegates to the two-point path: `from_time = parse(at_spec)`, `to_time = now`.
fn run_single_point(
    engine: &Engine,
    at_spec: &str,
    file_filter: Option<&str>,
    format: OutputFormat,
    context_radius: usize,
) -> Result<(), UnfError> {
    run_two_point(engine, at_spec, None, file_filter, format, context_radius)
}

/// Snapshot diff: diffs a specific snapshot against its predecessor.
fn run_snapshot_diff(
    engine: &Engine,
    snapshot_id: i64,
    format: OutputFormat,
    context_radius: usize,
) -> Result<(), UnfError> {
    use crate::types::SnapshotId;

    let snap = engine
        .get_snapshot_by_id(SnapshotId(snapshot_id))
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to load snapshot: {}", e)))?
        .ok_or_else(|| UnfError::InvalidArgument(format!("Snapshot {} not found", snapshot_id)))?;

    let prev = engine.get_previous_snapshot(&snap.file_path, snap.timestamp, snap.id)?;

    // If content hash matches predecessor, there's no actual change
    if let Some(ref p) = prev {
        if p.content_hash == snap.content_hash {
            if format == OutputFormat::Json {
                let output = DiffOutput {
                    from: p.timestamp.to_rfc3339(),
                    to: snap.timestamp.to_rfc3339(),
                    changes: vec![],
                };
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
            } else {
                println!(
                    "No changes in snapshot {} for {}",
                    snapshot_id, snap.file_path
                );
            }
            return Ok(());
        }
    }

    let old_content = match &prev {
        Some(p) if p.event_type != EventType::Delete => {
            String::from_utf8_lossy(&engine.load_content(&p.content_hash)?).to_string()
        }
        _ => String::new(),
    };

    let new_content = if snap.event_type != EventType::Delete {
        String::from_utf8_lossy(&engine.load_content(&snap.content_hash)?).to_string()
    } else {
        String::new()
    };

    let status = match snap.event_type {
        EventType::Create => "created",
        EventType::Delete => "deleted",
        EventType::Modify => "modified",
    };

    let diff = TextDiff::from_lines(&old_content, &new_content);

    // OLD format: diff lines without equal
    let mut diff_lines = Vec::new();
    for change in diff.iter_all_changes() {
        let op = match change.tag() {
            ChangeTag::Delete => "delete",
            ChangeTag::Insert => "insert",
            ChangeTag::Equal => continue,
        };
        diff_lines.push(DiffLine {
            op: op.to_string(),
            content: change.to_string(),
        });
    }

    // NEW format: hunks with line numbers
    let hunks = generate_hunks(&diff, context_radius);

    if format == OutputFormat::Json {
        let output = DiffOutput {
            from: prev
                .as_ref()
                .map(|p| p.timestamp.to_rfc3339())
                .unwrap_or_default(),
            to: snap.timestamp.to_rfc3339(),
            changes: vec![DiffChange {
                file: snap.file_path.clone(),
                status: status.to_string(),
                diff: if diff_lines.is_empty() {
                    None
                } else {
                    Some(diff_lines)
                },
                hunks: if hunks.is_empty() { None } else { Some(hunks) },
            }],
        };
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else if diff_lines.is_empty() {
        println!(
            "No changes in snapshot {} for {}",
            snapshot_id, snap.file_path
        );
    } else {
        let colored = use_color();
        if colored {
            println!("{}--- a/{}{}", colors::BOLD, snap.file_path, colors::RESET);
            println!("{}+++ b/{}{}", colors::BOLD, snap.file_path, colors::RESET);
        } else {
            println!("--- a/{}", snap.file_path);
            println!("+++ b/{}", snap.file_path);
        }
        let mut binding = diff.unified_diff();
        let udiff = binding.context_radius(context_radius);
        for hunk in udiff.iter_hunks() {
            if colored {
                print!("{}{}{}", colors::CYAN, hunk.header(), colors::RESET);
                for change in hunk.iter_changes() {
                    match change.tag() {
                        ChangeTag::Delete => {
                            print!("{}-{}{}", colors::RED, change, colors::RESET)
                        }
                        ChangeTag::Insert => {
                            print!("{}+{}{}", colors::GREEN, change, colors::RESET)
                        }
                        ChangeTag::Equal => print!(" {}", change),
                    }
                    if change.missing_newline() {
                        println!();
                    }
                }
            } else {
                print!("{}", hunk);
            }
        }
    }

    Ok(())
}

/// Two-point diff: compares file state at two specific points in time.
fn run_two_point(
    engine: &Engine,
    from_spec: &str,
    to_spec: Option<&str>,
    file_filter: Option<&str>,
    format: OutputFormat,
    context_radius: usize,
) -> Result<(), UnfError> {
    let from_time = super::parse_time_spec(from_spec)?;
    let to_time = match to_spec {
        Some(spec) => super::parse_time_spec(spec)?,
        None => Utc::now(),
    };

    // Get the state at both points in time
    let from_state = engine.get_state_at(from_time)?;
    let to_state = engine.get_state_at(to_time)?;

    // Compute the diff between the two states
    let mut changes = compute_state_diff(&from_state, &to_state);

    // Apply file filter if specified
    if let Some(filter) = file_filter {
        changes.retain(|(path, _)| path == filter);
    }

    // Render the output
    render_state_diff(
        engine,
        from_time,
        to_time,
        &from_state,
        &to_state,
        &changes,
        format,
        context_radius,
    )
}

/// Computes the diff between two file state snapshots (pure function).
///
/// Compares the file states at two points in time and categorizes each
/// file as created, deleted, or modified.
///
/// # Arguments
/// * `from_state` - File states at the earlier time
/// * `to_state` - File states at the later time
///
/// # Returns
/// A sorted vector of (file_path, change_type) tuples.
fn compute_state_diff(
    from_state: &HashMap<String, Snapshot>,
    to_state: &HashMap<String, Snapshot>,
) -> Vec<(String, FileChange)> {
    let mut changes = Vec::new();

    // Files in to_state but not in from_state -> Created
    // Files in both but with different content_hash -> Modified
    for (path, to_snap) in to_state {
        match from_state.get(path) {
            None => {
                // File didn't exist at from_time
                if to_snap.event_type != EventType::Delete {
                    changes.push((path.clone(), FileChange::Created));
                }
            }
            Some(from_snap) => {
                // Both exist - check for changes
                if from_snap.event_type == EventType::Delete
                    && to_snap.event_type != EventType::Delete
                {
                    changes.push((path.clone(), FileChange::Created));
                } else if from_snap.event_type != EventType::Delete
                    && to_snap.event_type == EventType::Delete
                {
                    changes.push((path.clone(), FileChange::Deleted));
                } else if from_snap.content_hash != to_snap.content_hash {
                    changes.push((path.clone(), FileChange::Modified));
                }
            }
        }
    }

    // Files in from_state but not in to_state -> they weren't tracked yet at to_time
    // This is unusual but handle it: if file was in from but not to, it might have been deleted
    for (path, from_snap) in from_state {
        if !to_state.contains_key(path) && from_snap.event_type != EventType::Delete {
            // File existed at from_time but has no state at to_time
            // This shouldn't normally happen since get_state_at considers all tracked files
            // But handle gracefully
            changes.push((path.clone(), FileChange::Deleted));
        }
    }

    changes.sort_by(|a, b| a.0.cmp(&b.0));
    changes
}

/// Diff for a detected session. Resolves session boundaries, then delegates to two-point diff.
///
/// # Arguments
///
/// * `project_root` - Root directory of the project
/// * `session_num` - Session number (0 = most recent, N > 0 = specific session)
/// * `since` - Optional time to limit session detection scope
/// * `file_filter` - Optional file to limit diff to
/// * `format` - Output format (human or JSON)
/// * `context_radius` - Number of context lines around changes
///
/// # Returns
///
/// `Ok(())` on success, or `UnfError` if session detection or diffing fails.
pub fn run_session(
    project_root: &Path,
    session_num: usize,
    since: Option<&str>,
    file_filter: Option<&str>,
    format: OutputFormat,
    context_radius: usize,
) -> Result<(), UnfError> {
    use crate::cli::session;

    let storage_dir = storage::resolve_storage_dir(project_root)?;
    let engine = Engine::open(project_root, &storage_dir)?;

    // Get snapshots for session detection
    let since_time = if let Some(spec) = since {
        super::parse_time_spec(spec)?
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

    // Delegate to two-point diff using session boundaries
    let from_spec = resolved.start.to_rfc3339();
    let to_spec = resolved.end.to_rfc3339();

    run_two_point(
        &engine,
        &from_spec,
        Some(&to_spec),
        file_filter,
        format,
        context_radius,
    )
}

/// Renders the two-point diff output in human or JSON format.
#[allow(clippy::too_many_arguments)]
fn render_state_diff(
    engine: &Engine,
    from_time: chrono::DateTime<Utc>,
    to_time: chrono::DateTime<Utc>,
    from_state: &HashMap<String, Snapshot>,
    to_state: &HashMap<String, Snapshot>,
    changes: &[(String, FileChange)],
    format: OutputFormat,
    context_radius: usize,
) -> Result<(), UnfError> {
    if changes.is_empty() {
        if format == OutputFormat::Json {
            let output = DiffOutput {
                from: from_time.to_rfc3339(),
                to: to_time.to_rfc3339(),
                changes: vec![],
            };
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        } else {
            println!("No changes between the specified times.");
        }
        return Ok(());
    }

    if format == OutputFormat::Json {
        let mut json_changes = Vec::new();
        for (path, change) in changes {
            let status = match change {
                FileChange::Created => "created",
                FileChange::Deleted => "deleted",
                FileChange::Modified => "modified",
            };
            if *change == FileChange::Modified {
                let (diff_lines, hunks) =
                    get_state_diff_with_hunks(engine, path, from_state, to_state, context_radius)?;
                json_changes.push(DiffChange {
                    file: path.clone(),
                    status: status.to_string(),
                    diff: Some(diff_lines),
                    hunks: Some(hunks),
                });
            } else {
                json_changes.push(DiffChange {
                    file: path.clone(),
                    status: status.to_string(),
                    diff: None,
                    hunks: None,
                });
            }
        }
        let output = DiffOutput {
            from: from_time.to_rfc3339(),
            to: to_time.to_rfc3339(),
            changes: json_changes,
        };
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        println!(
            "Changes between {} and {}:\n",
            super::format_local_time(from_time),
            super::format_local_time(to_time),
        );
        for (path, change) in changes {
            match change {
                FileChange::Deleted => println!("  deleted   {}", path),
                FileChange::Modified => println!("  modified  {}", path),
                FileChange::Created => println!("  created   {}", path),
            }
        }
        // Print diffs for modified files
        let modified: Vec<&str> = changes
            .iter()
            .filter(|(_, c)| *c == FileChange::Modified)
            .map(|(p, _)| p.as_str())
            .collect();
        let colored = use_color();
        if !modified.is_empty() {
            println!();
            for path in modified {
                print_state_file_diff(engine, path, from_state, to_state, colored, context_radius)?;
            }
        }
    }
    Ok(())
}

/// Gets diff lines and hunks for JSON output in two-point mode.
fn get_state_diff_with_hunks(
    engine: &Engine,
    file_path: &str,
    from_state: &HashMap<String, Snapshot>,
    to_state: &HashMap<String, Snapshot>,
    context_radius: usize,
) -> Result<(Vec<DiffLine>, Vec<DiffHunk>), UnfError> {
    let old_content = match from_state.get(file_path) {
        Some(snap) if snap.event_type != EventType::Delete => {
            let bytes = engine.load_content(&snap.content_hash)?;
            String::from_utf8_lossy(&bytes).to_string()
        }
        _ => String::new(),
    };
    let new_content = match to_state.get(file_path) {
        Some(snap) if snap.event_type != EventType::Delete => {
            let bytes = engine.load_content(&snap.content_hash)?;
            String::from_utf8_lossy(&bytes).to_string()
        }
        _ => String::new(),
    };

    let diff = TextDiff::from_lines(&old_content, &new_content);

    // OLD format: lines without equal
    let mut lines = Vec::new();
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Delete => lines.push(DiffLine {
                op: "delete".to_string(),
                content: change.to_string(),
            }),
            ChangeTag::Insert => lines.push(DiffLine {
                op: "insert".to_string(),
                content: change.to_string(),
            }),
            ChangeTag::Equal => {}
        }
    }

    // NEW format: hunks with line numbers
    let hunks = generate_hunks(&diff, context_radius);

    Ok((lines, hunks))
}

/// Prints a unified diff for a single file (standard `@@ ... @@` format with context).
fn print_state_file_diff(
    engine: &Engine,
    file_path: &str,
    from_state: &HashMap<String, Snapshot>,
    to_state: &HashMap<String, Snapshot>,
    colored: bool,
    context_radius: usize,
) -> Result<(), UnfError> {
    let old_content = match from_state.get(file_path) {
        Some(snap) if snap.event_type != EventType::Delete => {
            let bytes = engine.load_content(&snap.content_hash)?;
            String::from_utf8_lossy(&bytes).to_string()
        }
        _ => String::new(),
    };
    let new_content = match to_state.get(file_path) {
        Some(snap) if snap.event_type != EventType::Delete => {
            let bytes = engine.load_content(&snap.content_hash)?;
            String::from_utf8_lossy(&bytes).to_string()
        }
        _ => String::new(),
    };

    let diff = TextDiff::from_lines(&old_content, &new_content);
    let mut binding = diff.unified_diff();
    let udiff = binding.context_radius(context_radius);

    if colored {
        // Print colored header
        println!("{}--- a/{}{}", colors::BOLD, file_path, colors::RESET);
        println!("{}+++ b/{}{}", colors::BOLD, file_path, colors::RESET);
        for hunk in udiff.iter_hunks() {
            // Hunk header in cyan
            print!("{}{}{}", colors::CYAN, hunk.header(), colors::RESET);
            for change in hunk.iter_changes() {
                match change.tag() {
                    ChangeTag::Delete => {
                        print!("{}-{}{}", colors::RED, change, colors::RESET);
                    }
                    ChangeTag::Insert => {
                        print!("{}+{}{}", colors::GREEN, change, colors::RESET);
                    }
                    ChangeTag::Equal => {
                        print!(" {}", change);
                    }
                }
                if change.missing_newline() {
                    println!();
                }
            }
        }
    } else {
        let header_a = format!("a/{}", file_path);
        let header_b = format!("b/{}", file_path);
        print!("{}", udiff.header(&header_a, &header_b));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ContentHash, SnapshotId};
    use std::fs;
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
    fn diff_no_changes() {
        let (_project, _storage, engine) = setup_test_engine();

        // Query with a time in the future - should return empty
        let future = Utc::now() + chrono::Duration::hours(1);
        let snapshots = engine
            .get_snapshots_since(future)
            .expect("Query should succeed");
        assert_eq!(snapshots.len(), 0);
    }

    #[test]
    fn diff_with_created_file() {
        let (project, _storage, engine) = setup_test_engine();

        let before_snapshot = Utc::now();
        thread::sleep(StdDuration::from_millis(10));

        let file_path = "new.txt";
        let full_path = project.path().join(file_path);
        fs::write(&full_path, b"new content").expect("Failed to write");
        engine
            .create_snapshot(file_path, EventType::Create)
            .expect("Snapshot failed");

        let snapshots = engine
            .get_snapshots_since(before_snapshot)
            .expect("Query should succeed");
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].file_path, file_path);
        assert_eq!(snapshots[0].event_type, EventType::Create);
    }

    #[test]
    fn diff_with_modified_file() {
        let (project, _storage, engine) = setup_test_engine();

        let file_path = "test.txt";
        let full_path = project.path().join(file_path);

        // Create initial version
        fs::write(&full_path, b"original").expect("Failed to write");
        engine
            .create_snapshot(file_path, EventType::Create)
            .expect("Snapshot 1 failed");

        let before_mod = Utc::now();
        thread::sleep(StdDuration::from_millis(10));

        // Modify
        fs::write(&full_path, b"modified").expect("Failed to write");
        engine
            .create_snapshot(file_path, EventType::Modify)
            .expect("Snapshot 2 failed");

        // Query changes since before the modification
        let snapshots = engine
            .get_snapshots_since(before_mod)
            .expect("Query should succeed");
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].event_type, EventType::Modify);
    }

    #[test]
    fn diff_with_deleted_file() {
        let (project, _storage, engine) = setup_test_engine();

        let file_path = "old.txt";
        let full_path = project.path().join(file_path);

        // Create and delete
        fs::write(&full_path, b"content").expect("Failed to write");
        engine
            .create_snapshot(file_path, EventType::Create)
            .expect("Snapshot 1 failed");

        let before_delete = Utc::now();
        thread::sleep(StdDuration::from_millis(10));

        engine
            .create_snapshot(file_path, EventType::Delete)
            .expect("Delete snapshot failed");

        let snapshots = engine
            .get_snapshots_since(before_delete)
            .expect("Query should succeed");
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].event_type, EventType::Delete);
    }

    #[test]
    fn diff_multiple_changes_grouped() {
        let (project, _storage, engine) = setup_test_engine();

        let file1 = "file1.txt";
        let file2 = "file2.txt";
        let full_path1 = project.path().join(file1);
        let full_path2 = project.path().join(file2);

        // Create both files
        fs::write(&full_path1, b"content1").expect("Failed to write");
        engine
            .create_snapshot(file1, EventType::Create)
            .expect("Snapshot 1 failed");

        fs::write(&full_path2, b"content2").expect("Failed to write");
        engine
            .create_snapshot(file2, EventType::Create)
            .expect("Snapshot 2 failed");

        let before_mods = Utc::now();
        thread::sleep(StdDuration::from_millis(10));

        // Modify both
        fs::write(&full_path1, b"modified1").expect("Failed to write");
        engine
            .create_snapshot(file1, EventType::Modify)
            .expect("Snapshot 3 failed");

        fs::write(&full_path2, b"modified2").expect("Failed to write");
        engine
            .create_snapshot(file2, EventType::Modify)
            .expect("Snapshot 4 failed");

        let snapshots = engine
            .get_snapshots_since(before_mods)
            .expect("Query should succeed");
        assert_eq!(snapshots.len(), 2);
    }

    #[test]
    fn compute_state_diff_detects_created_file() {
        let from_state = HashMap::new();
        let mut to_state = HashMap::new();
        to_state.insert(
            "new.txt".to_string(),
            Snapshot {
                id: SnapshotId(1),
                file_path: "new.txt".to_string(),
                content_hash: ContentHash("abc".to_string()),
                size_bytes: 10,
                timestamp: Utc::now(),
                event_type: EventType::Create,
                line_count: 0,
                lines_added: 0,
                lines_removed: 0,
            },
        );

        let changes = compute_state_diff(&from_state, &to_state);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0, "new.txt");
        assert_eq!(changes[0].1, FileChange::Created);
    }

    #[test]
    fn compute_state_diff_detects_deleted_file() {
        let mut from_state = HashMap::new();
        from_state.insert(
            "old.txt".to_string(),
            Snapshot {
                id: SnapshotId(1),
                file_path: "old.txt".to_string(),
                content_hash: ContentHash("abc".to_string()),
                size_bytes: 10,
                timestamp: Utc::now(),
                event_type: EventType::Create,
                line_count: 0,
                lines_added: 0,
                lines_removed: 0,
            },
        );
        let mut to_state = HashMap::new();
        to_state.insert(
            "old.txt".to_string(),
            Snapshot {
                id: SnapshotId(2),
                file_path: "old.txt".to_string(),
                content_hash: ContentHash("000".to_string()),
                size_bytes: 0,
                timestamp: Utc::now(),
                event_type: EventType::Delete,
                line_count: 0,
                lines_added: 0,
                lines_removed: 0,
            },
        );

        let changes = compute_state_diff(&from_state, &to_state);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0, "old.txt");
        assert_eq!(changes[0].1, FileChange::Deleted);
    }

    #[test]
    fn compute_state_diff_detects_modified_file() {
        let mut from_state = HashMap::new();
        from_state.insert(
            "file.txt".to_string(),
            Snapshot {
                id: SnapshotId(1),
                file_path: "file.txt".to_string(),
                content_hash: ContentHash("hash1".to_string()),
                size_bytes: 10,
                timestamp: Utc::now(),
                event_type: EventType::Create,
                line_count: 0,
                lines_added: 0,
                lines_removed: 0,
            },
        );
        let mut to_state = HashMap::new();
        to_state.insert(
            "file.txt".to_string(),
            Snapshot {
                id: SnapshotId(2),
                file_path: "file.txt".to_string(),
                content_hash: ContentHash("hash2".to_string()),
                size_bytes: 20,
                timestamp: Utc::now(),
                event_type: EventType::Modify,
                line_count: 0,
                lines_added: 0,
                lines_removed: 0,
            },
        );

        let changes = compute_state_diff(&from_state, &to_state);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].0, "file.txt");
        assert_eq!(changes[0].1, FileChange::Modified);
    }

    #[test]
    fn compute_state_diff_no_changes() {
        let mut from_state = HashMap::new();
        from_state.insert(
            "file.txt".to_string(),
            Snapshot {
                id: SnapshotId(1),
                file_path: "file.txt".to_string(),
                content_hash: ContentHash("same".to_string()),
                size_bytes: 10,
                timestamp: Utc::now(),
                event_type: EventType::Create,
                line_count: 0,
                lines_added: 0,
                lines_removed: 0,
            },
        );
        let mut to_state = HashMap::new();
        to_state.insert(
            "file.txt".to_string(),
            Snapshot {
                id: SnapshotId(2),
                file_path: "file.txt".to_string(),
                content_hash: ContentHash("same".to_string()),
                size_bytes: 10,
                timestamp: Utc::now(),
                event_type: EventType::Modify,
                line_count: 0,
                lines_added: 0,
                lines_removed: 0,
            },
        );

        let changes = compute_state_diff(&from_state, &to_state);
        assert_eq!(changes.len(), 0);
    }

    #[test]
    fn generate_hunks_basic() {
        let old = "line1\nline2\nline3\n";
        let new = "line1\nmodified\nline3\n";
        let diff = TextDiff::from_lines(old, new);
        let hunks = generate_hunks(&diff, 3);

        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 1);
        assert_eq!(hunks[0].new_start, 1);
        assert!(hunks[0].lines.len() > 0);

        // Verify line numbers are present
        let has_line_nums = hunks[0]
            .lines
            .iter()
            .any(|l| l.old_num.is_some() || l.new_num.is_some());
        assert!(has_line_nums);
    }

    #[test]
    fn generate_hunks_with_context() {
        let old = "alpha\nbravo\ncharlie\ndelta\necho\n";
        let new = "alpha\nBRAVO\ncharlie\nDELTA\necho\n";
        let diff = TextDiff::from_lines(old, new);
        let hunks = generate_hunks(&diff, 3);

        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 1);
        assert_eq!(hunks[0].old_count, 5);
        assert_eq!(hunks[0].new_start, 1);
        assert_eq!(hunks[0].new_count, 5);

        // Verify we have equal, delete, and insert ops
        let ops: Vec<&str> = hunks[0].lines.iter().map(|l| l.op.as_str()).collect();
        assert!(ops.contains(&"equal"));
        assert!(ops.contains(&"delete"));
        assert!(ops.contains(&"insert"));

        // Verify line numbers
        for line in &hunks[0].lines {
            match line.op.as_str() {
                "equal" => {
                    assert!(line.old_num.is_some());
                    assert!(line.new_num.is_some());
                }
                "delete" => {
                    assert!(line.old_num.is_some());
                    assert!(line.new_num.is_none());
                }
                "insert" => {
                    assert!(line.old_num.is_none());
                    assert!(line.new_num.is_some());
                }
                _ => panic!("unexpected op: {}", line.op),
            }
        }
    }

    #[test]
    fn generate_hunks_only_insertions() {
        let old = "";
        let new = "new1\nnew2\nnew3\n";
        let diff = TextDiff::from_lines(old, new);
        let hunks = generate_hunks(&diff, 3);

        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_count, 0);
        assert_eq!(hunks[0].new_count, 3);

        // All lines should be inserts
        for line in &hunks[0].lines {
            assert_eq!(line.op, "insert");
            assert!(line.old_num.is_none());
            assert!(line.new_num.is_some());
        }
    }

    #[test]
    fn generate_hunks_only_deletions() {
        let old = "old1\nold2\nold3\n";
        let new = "";
        let diff = TextDiff::from_lines(old, new);
        let hunks = generate_hunks(&diff, 3);

        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_count, 3);
        assert_eq!(hunks[0].new_count, 0);

        // All lines should be deletes
        for line in &hunks[0].lines {
            assert_eq!(line.op, "delete");
            assert!(line.old_num.is_some());
            assert!(line.new_num.is_none());
        }
    }

    #[test]
    fn generate_hunks_no_changes() {
        let old = "same\nlines\n";
        let new = "same\nlines\n";
        let diff = TextDiff::from_lines(old, new);
        let hunks = generate_hunks(&diff, 3);

        // No hunks for identical content
        assert_eq!(hunks.len(), 0);
    }

    #[test]
    fn generate_hunks_line_numbers_sequential() {
        let old = "a\nb\nc\nd\ne\n";
        let new = "a\nB\nc\nD\ne\nf\n";
        let diff = TextDiff::from_lines(old, new);
        let hunks = generate_hunks(&diff, 3);

        assert_eq!(hunks.len(), 1);

        // Verify line numbers increment properly
        let mut last_old = 0u32;
        let mut last_new = 0u32;

        for line in &hunks[0].lines {
            if let Some(old_num) = line.old_num {
                assert!(old_num > last_old);
                last_old = old_num;
            }
            if let Some(new_num) = line.new_num {
                assert!(new_num > last_new);
                last_new = new_num;
            }
        }
    }
}
