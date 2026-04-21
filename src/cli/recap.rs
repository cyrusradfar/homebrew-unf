//! Implementation of `unf recap` command.
//!
//! Aggregates session data, git state, and latest activity into a single response
//! for AI agents rebuilding context after a crash or context overflow.

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use chrono::{DateTime, Utc};

use crate::cli::OutputFormat;
use crate::engine::Engine;
use crate::error::UnfError;
use crate::storage;
use crate::types::Snapshot;

use super::session::{detect_sessions, Session};

// ============================================================================
// Data Types
// ============================================================================

/// Git repository state (collected via subprocess).
#[derive(Debug, Clone, serde::Serialize)]
pub struct GitInfo {
    /// Current branch name.
    pub branch: String,
    /// List of files with uncommitted changes (modified + untracked).
    pub uncommitted_files: Vec<String>,
    /// Recent commit summaries.
    pub recent_commits: Vec<GitCommit>,
}

/// A single git commit summary.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GitCommit {
    /// Short hash (7 chars).
    pub hash: String,
    /// First line of commit message.
    pub message: String,
}

/// Per-file edit summary within a session.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FileActivity {
    /// Relative file path.
    pub path: String,
    /// Number of snapshots (edits) in this session.
    pub edits: u64,
    /// Timestamp of most recent edit.
    pub last_edit: String,
    /// Total lines added across all edits in this session.
    pub lines_added: u64,
    /// Total lines removed across all edits in this session.
    pub lines_removed: u64,
}

/// A session recap with per-file breakdown.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionRecap {
    pub number: usize,
    pub start: String,
    pub end: String,
    pub duration_seconds: i64,
    pub edit_count: usize,
    pub file_count: usize,
    /// Files sorted by edit count descending (most active first).
    pub files: Vec<FileActivity>,
}

/// Information about the most recent editing activity.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LatestActivity {
    /// Timestamp of the most recent snapshot.
    pub last_edit: String,
    /// File path of the most recent snapshot.
    pub last_file: String,
    /// Minutes since the last edit (at time of query).
    pub minutes_ago: u64,
}

/// Top-level recap output for a single project.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecapOutput {
    /// Canonical project path.
    pub project: String,
    /// Detected sessions in the time window.
    pub sessions: Vec<SessionRecap>,
    /// Git state (null if git unavailable or not a git repo).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git: Option<GitInfo>,
    /// Most recent editing activity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_activity: Option<LatestActivity>,
}

/// Top-level recap output for global (cross-project) mode.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GlobalRecapOutput {
    pub projects: Vec<RecapOutput>,
    pub total_sessions: usize,
    pub total_edits: usize,
}

// ============================================================================
// Pure Functions
// ============================================================================

/// Computes per-file edit counts from a session's snapshots.
///
/// Takes the full snapshot list and a session's time boundaries.
/// Returns FileActivity entries sorted by edit count descending.
///
/// Pure function: no I/O.
fn compute_file_edits(
    snapshots: &[Snapshot],
    session_start: DateTime<Utc>,
    session_end: DateTime<Utc>,
) -> Vec<FileActivity> {
    let mut file_stats: BTreeMap<String, (u64, DateTime<Utc>, u64, u64)> = BTreeMap::new();

    for snapshot in snapshots {
        if snapshot.timestamp < session_start || snapshot.timestamp > session_end {
            continue;
        }

        let entry =
            file_stats
                .entry(snapshot.file_path.clone())
                .or_insert((0, snapshot.timestamp, 0, 0));

        entry.0 += 1; // edit count
        if snapshot.timestamp > entry.1 {
            entry.1 = snapshot.timestamp; // last edit time
        }
        entry.2 += snapshot.lines_added; // lines added
        entry.3 += snapshot.lines_removed; // lines removed
    }

    let mut file_activities: Vec<FileActivity> = file_stats
        .into_iter()
        .map(
            |(path, (edits, last_timestamp, lines_added, lines_removed))| FileActivity {
                path,
                edits,
                last_edit: last_timestamp.to_rfc3339(),
                lines_added,
                lines_removed,
            },
        )
        .collect();

    // Sort by edit count descending
    file_activities.sort_by_key(|a| Reverse(a.edits));

    file_activities
}

/// Converts a Session + file edits into a SessionRecap.
///
/// Pure function: no I/O.
fn build_session_recap(session: &Session, file_edits: Vec<FileActivity>) -> SessionRecap {
    let duration_seconds = session
        .end
        .signed_duration_since(session.start)
        .num_seconds();
    let edit_count = session.edit_count;
    let file_count = file_edits.len();

    SessionRecap {
        number: session.number,
        start: session.start.to_rfc3339(),
        end: session.end.to_rfc3339(),
        duration_seconds,
        edit_count,
        file_count,
        files: file_edits,
    }
}

/// Determines the default --since time from detected sessions.
///
/// Strategy:
/// 1. If sessions exist, use the start of the most recent session
///    (minus a 5-minute buffer to catch pre-session context).
/// 2. If no sessions, fall back to 24 hours ago.
///
/// Pure function: takes `now` as parameter for testability.
fn infer_since_time(sessions: &[Session], now: DateTime<Utc>) -> DateTime<Utc> {
    if let Some(latest_session) = sessions.last() {
        // Most recent session minus 5 minutes
        latest_session.start - chrono::Duration::minutes(5)
    } else {
        // Fall back to 24 hours ago
        now - chrono::Duration::hours(24)
    }
}

/// Assembles the final RecapOutput from collected data.
///
/// Pure function: no I/O.
fn build_recap_output(
    project: &str,
    sessions: Vec<SessionRecap>,
    git_info: Option<GitInfo>,
    latest_activity: Option<LatestActivity>,
) -> RecapOutput {
    RecapOutput {
        project: project.to_string(),
        sessions,
        git: git_info,
        latest_activity,
    }
}

// ============================================================================
// Side-Effect Functions
// ============================================================================

/// Collects git repository state by running git subprocesses.
///
/// Runs three commands:
///   git rev-parse --abbrev-ref HEAD          -> branch name
///   git status --porcelain                    -> uncommitted files
///   git log --oneline -5 --no-decorate       -> recent commits
///
/// Returns None if any fundamental command fails (not a git repo, git not installed).
/// Individual command failures (e.g., empty log) produce empty fields, not None.
fn collect_git_info(project_root: &Path) -> Option<GitInfo> {
    // Get branch name
    let branch_output = Command::new("git")
        .arg("rev-parse")
        .arg("--abbrev-ref")
        .arg("HEAD")
        .current_dir(project_root)
        .output()
        .ok()?;

    if !branch_output.status.success() {
        return None;
    }

    let branch = String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .to_string();

    // Get uncommitted files
    let status_output = Command::new("git")
        .arg("status")
        .arg("--porcelain")
        .current_dir(project_root)
        .output()
        .ok()?;

    let mut uncommitted_files = Vec::new();
    if status_output.status.success() {
        for line in String::from_utf8_lossy(&status_output.stdout).lines() {
            if line.len() > 3 {
                uncommitted_files.push(line[3..].to_string());
            }
        }
    }

    // Get recent commits
    let log_output = Command::new("git")
        .arg("log")
        .arg("--oneline")
        .arg("-5")
        .arg("--no-decorate")
        .current_dir(project_root)
        .output()
        .ok()?;

    let mut recent_commits = Vec::new();
    if log_output.status.success() {
        for line in String::from_utf8_lossy(&log_output.stdout).lines() {
            if let Some(space_pos) = line.find(' ') {
                let hash = line[..space_pos].to_string();
                let message = line[space_pos + 1..].to_string();
                recent_commits.push(GitCommit { hash, message });
            }
        }
    }

    Some(GitInfo {
        branch,
        uncommitted_files,
        recent_commits,
    })
}

/// Main entry point for single-project recap.
pub fn run(project_root: &Path, since: Option<&str>, format: OutputFormat) -> Result<(), UnfError> {
    // Resolve project path to canonical form
    let project_canonical = project_root
        .canonicalize()
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to resolve project path: {}", e)))?;

    // Resolve storage directory
    let storage_dir = storage::resolve_storage_dir(&project_canonical)?;

    // Open engine
    let engine = Engine::open(&project_canonical, &storage_dir)?;

    // Determine the time window
    let since_time = if let Some(spec) = since {
        super::parse_time_spec(spec)?
    } else {
        // Smart inference: scan 48h, detect sessions, narrow if needed
        let scan_start = Utc::now() - chrono::Duration::hours(48);
        let initial_snapshots = engine.get_snapshots_since(scan_start)?;

        if initial_snapshots.is_empty() {
            // No snapshots in 48h, use 24h fallback
            Utc::now() - chrono::Duration::hours(24)
        } else {
            // Detect sessions to infer proper window
            let sessions = detect_sessions(&initial_snapshots);
            infer_since_time(&sessions, Utc::now())
        }
    };

    // Get snapshots for the determined window
    let snapshots = engine.get_snapshots_since(since_time)?;

    if snapshots.is_empty() {
        // No snapshots found
        let git_info = collect_git_info(&project_canonical);
        let recap = build_recap_output(
            &project_canonical.to_string_lossy(),
            Vec::new(),
            git_info,
            None,
        );

        output_recap(&recap, format)?;
        return Ok(());
    }

    // Detect sessions
    let sessions = detect_sessions(&snapshots);

    // Build recaps for each session
    let mut session_recaps = Vec::new();
    for session in &sessions {
        let file_edits = compute_file_edits(&snapshots, session.start, session.end);
        let recap = build_session_recap(session, file_edits);
        session_recaps.push(recap);
    }

    // Get latest activity (most recent snapshot)
    let latest_activity = snapshots.last().map(|snap| {
        let now = Utc::now();
        let diff = now.signed_duration_since(snap.timestamp);
        let minutes_ago = diff.num_seconds().max(0) as u64 / 60;

        LatestActivity {
            last_edit: snap.timestamp.to_rfc3339(),
            last_file: snap.file_path.clone(),
            minutes_ago,
        }
    });

    // Collect git info
    let git_info = collect_git_info(&project_canonical);

    // Build and output recap
    let recap = build_recap_output(
        &project_canonical.to_string_lossy(),
        session_recaps,
        git_info,
        latest_activity,
    );

    output_recap(&recap, format)?;
    Ok(())
}

/// Cross-project recap.
pub fn run_global(
    include_project: &[String],
    exclude_project: &[String],
    since: Option<&str>,
    format: OutputFormat,
) -> Result<(), UnfError> {
    // Use resolve_global_projects from log module
    let projects = super::log::resolve_global_projects(include_project, exclude_project)?;

    let mut all_recaps = Vec::new();
    let mut total_sessions = 0;
    let mut total_edits = 0;

    for (project_path, storage_dir) in projects {
        // Open engine for this project
        match Engine::open(&project_path, &storage_dir) {
            Ok(engine) => {
                // Determine the time window
                let since_time = if let Some(spec) = since {
                    match super::parse_time_spec(spec) {
                        Ok(t) => t,
                        Err(_) => {
                            // Skip this project on invalid time spec
                            continue;
                        }
                    }
                } else {
                    // Smart inference: scan 48h
                    let scan_start = Utc::now() - chrono::Duration::hours(48);
                    match engine.get_snapshots_since(scan_start) {
                        Ok(initial_snapshots) => {
                            if initial_snapshots.is_empty() {
                                Utc::now() - chrono::Duration::hours(24)
                            } else {
                                let sessions = detect_sessions(&initial_snapshots);
                                infer_since_time(&sessions, Utc::now())
                            }
                        }
                        Err(_) => {
                            // Skip this project on error
                            continue;
                        }
                    }
                };

                // Get snapshots
                match engine.get_snapshots_since(since_time) {
                    Ok(snapshots) => {
                        let mut session_recaps = Vec::new();

                        if !snapshots.is_empty() {
                            let sessions = detect_sessions(&snapshots);
                            total_sessions += sessions.len();

                            for session in &sessions {
                                let file_edits =
                                    compute_file_edits(&snapshots, session.start, session.end);
                                total_edits += session.edit_count;
                                let recap = build_session_recap(session, file_edits);
                                session_recaps.push(recap);
                            }
                        }

                        let latest_activity = snapshots.last().map(|snap| {
                            let now = Utc::now();
                            let diff = now.signed_duration_since(snap.timestamp);
                            let minutes_ago = diff.num_seconds().max(0) as u64 / 60;

                            LatestActivity {
                                last_edit: snap.timestamp.to_rfc3339(),
                                last_file: snap.file_path.clone(),
                                minutes_ago,
                            }
                        });

                        let git_info = collect_git_info(&project_path);

                        let recap = build_recap_output(
                            &project_path.to_string_lossy(),
                            session_recaps,
                            git_info,
                            latest_activity,
                        );

                        all_recaps.push(recap);
                    }
                    Err(_) => {
                        // Skip this project on error
                        continue;
                    }
                }
            }
            Err(_) => {
                // Skip this project if engine fails to open
                continue;
            }
        }
    }

    if all_recaps.is_empty() {
        return Err(UnfError::NoResults(
            "No projects with snapshots found.".to_string(),
        ));
    }

    let global_output = GlobalRecapOutput {
        projects: all_recaps,
        total_sessions,
        total_edits,
    };

    output_global_recap(&global_output, format)?;
    Ok(())
}

// ============================================================================
// Output Formatting
// ============================================================================

/// Outputs a single-project recap in the requested format.
fn output_recap(recap: &RecapOutput, format: OutputFormat) -> Result<(), UnfError> {
    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(recap).map_err(|e| {
                UnfError::InvalidArgument(format!("JSON serialization failed: {}", e))
            })?;
            println!("{}", json);
        }
        OutputFormat::Human => {
            output_recap_human(recap);
        }
    }
    Ok(())
}

/// Outputs a single-project recap in human-readable format.
fn output_recap_human(recap: &RecapOutput) {
    println!("-- {} --\n", recap.project);

    if recap.sessions.is_empty() {
        println!("No recent sessions detected.\n");
    } else {
        for (i, session) in recap.sessions.iter().enumerate() {
            let is_latest = i == recap.sessions.len() - 1 && recap.sessions.len() > 1;
            let latest_marker = if is_latest { " <-- latest" } else { "" };

            let start_local = session
                .start
                .parse::<DateTime<Utc>>()
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Local))
                .map(|dt| dt.format("%H:%M").to_string())
                .unwrap_or_else(|| "??:??".to_string());

            let end_local = session
                .end
                .parse::<DateTime<Utc>>()
                .ok()
                .map(|dt| dt.with_timezone(&chrono::Local))
                .map(|dt| dt.format("%H:%M").to_string())
                .unwrap_or_else(|| "??:??".to_string());

            let duration_mins = session.duration_seconds / 60;
            println!(
                "Session {}  {} - {}  ({}m, {} edits, {} files){}",
                session.number,
                start_local,
                end_local,
                duration_mins,
                session.edit_count,
                session.file_count,
                latest_marker
            );

            // Show top 5 files
            let shown_files = session.files.iter().take(5).collect::<Vec<_>>();
            for file in shown_files.iter() {
                println!(
                    "  {}     {} edits  (+{} -{})",
                    file.path, file.edits, file.lines_added, file.lines_removed
                );
            }

            if session.files.len() > 5 {
                println!("  ... and {} more files", session.files.len() - 5);
            }
            println!();
        }
    }

    // Git info
    if let Some(git) = &recap.git {
        print!(
            "git: {}  {} uncommitted files",
            git.branch,
            git.uncommitted_files.len()
        );
        if !git.uncommitted_files.is_empty() {
            println!();
            for file in git.uncommitted_files.iter().take(10) {
                println!("  {}", file);
            }
            if git.uncommitted_files.len() > 10 {
                println!("  ... and {} more files", git.uncommitted_files.len() - 10);
            }
        } else {
            println!();
        }
        println!();
    }

    // Latest activity
    if let Some(activity) = &recap.latest_activity {
        println!(
            "Last edit: {}  {} minutes ago",
            activity.last_file, activity.minutes_ago
        );
    }
}

/// Outputs a global (cross-project) recap in the requested format.
fn output_global_recap(recap: &GlobalRecapOutput, format: OutputFormat) -> Result<(), UnfError> {
    match format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(recap).map_err(|e| {
                UnfError::InvalidArgument(format!("JSON serialization failed: {}", e))
            })?;
            println!("{}", json);
        }
        OutputFormat::Human => {
            output_global_recap_human(recap);
        }
    }
    Ok(())
}

/// Outputs a global recap in human-readable format.
fn output_global_recap_human(recap: &GlobalRecapOutput) {
    for project_recap in &recap.projects {
        println!("=== {} ===\n", project_recap.project);

        if project_recap.sessions.is_empty() {
            println!("No recent sessions detected.\n");
        } else {
            for session in &project_recap.sessions {
                let start_local = session
                    .start
                    .parse::<DateTime<Utc>>()
                    .ok()
                    .map(|dt| dt.with_timezone(&chrono::Local))
                    .map(|dt| dt.format("%H:%M").to_string())
                    .unwrap_or_else(|| "??:??".to_string());

                let end_local = session
                    .end
                    .parse::<DateTime<Utc>>()
                    .ok()
                    .map(|dt| dt.with_timezone(&chrono::Local))
                    .map(|dt| dt.format("%H:%M").to_string())
                    .unwrap_or_else(|| "??:??".to_string());

                let duration_mins = session.duration_seconds / 60;
                println!(
                    "Session {}  {} - {}  ({}m, {} edits, {} files)",
                    session.number,
                    start_local,
                    end_local,
                    duration_mins,
                    session.edit_count,
                    session.file_count
                );

                let shown_files = session.files.iter().take(5).collect::<Vec<_>>();
                for file in shown_files.iter() {
                    println!("  {}     {} edits", file.path, file.edits);
                }

                if session.files.len() > 5 {
                    println!("  ... and {} more files", session.files.len() - 5);
                }
            }
            println!();
        }

        // Git info
        if let Some(git) = &project_recap.git {
            println!(
                "git: {}  {} uncommitted files\n",
                git.branch,
                git.uncommitted_files.len()
            );
        }

        // Latest activity
        if let Some(activity) = &project_recap.latest_activity {
            println!(
                "Last edit: {}  {} minutes ago\n",
                activity.last_file, activity.minutes_ago
            );
        }
    }

    println!(
        "-- {} projects, {} sessions, {} edits --",
        recap.projects.len(),
        recap.total_sessions,
        recap.total_edits
    );
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_file_edits_counts_correctly() {
        let now = Utc::now();
        let session_start = now;
        let session_end = now + chrono::Duration::minutes(10);

        let snapshots = vec![
            Snapshot {
                id: crate::types::SnapshotId(1),
                file_path: "file1.rs".to_string(),
                event_type: crate::types::EventType::Create,
                timestamp: now + chrono::Duration::seconds(0),
                size_bytes: 100,
                content_hash: crate::types::ContentHash("abc123".to_string()),
                line_count: 10,
                lines_added: 10,
                lines_removed: 0,
            },
            Snapshot {
                id: crate::types::SnapshotId(2),
                file_path: "file1.rs".to_string(),
                event_type: crate::types::EventType::Modify,
                timestamp: now + chrono::Duration::seconds(30),
                size_bytes: 110,
                content_hash: crate::types::ContentHash("def456".to_string()),
                line_count: 15,
                lines_added: 5,
                lines_removed: 2,
            },
            Snapshot {
                id: crate::types::SnapshotId(3),
                file_path: "file2.rs".to_string(),
                event_type: crate::types::EventType::Create,
                timestamp: now + chrono::Duration::seconds(60),
                size_bytes: 50,
                content_hash: crate::types::ContentHash("ghi789".to_string()),
                line_count: 8,
                lines_added: 8,
                lines_removed: 0,
            },
        ];

        let file_edits = compute_file_edits(&snapshots, session_start, session_end);

        assert_eq!(file_edits.len(), 2);
        assert_eq!(file_edits[0].path, "file1.rs");
        assert_eq!(file_edits[0].edits, 2);
        assert_eq!(file_edits[1].path, "file2.rs");
        assert_eq!(file_edits[1].edits, 1);
    }

    #[test]
    fn test_compute_file_edits_sums_lines() {
        let now = Utc::now();
        let session_start = now;
        let session_end = now + chrono::Duration::minutes(10);

        let snapshots = vec![
            Snapshot {
                id: crate::types::SnapshotId(1),
                file_path: "file1.rs".to_string(),
                event_type: crate::types::EventType::Create,
                timestamp: now + chrono::Duration::seconds(0),
                size_bytes: 100,
                content_hash: crate::types::ContentHash("abc123".to_string()),
                line_count: 10,
                lines_added: 10,
                lines_removed: 0,
            },
            Snapshot {
                id: crate::types::SnapshotId(2),
                file_path: "file1.rs".to_string(),
                event_type: crate::types::EventType::Modify,
                timestamp: now + chrono::Duration::seconds(30),
                size_bytes: 110,
                content_hash: crate::types::ContentHash("def456".to_string()),
                line_count: 15,
                lines_added: 5,
                lines_removed: 2,
            },
        ];

        let file_edits = compute_file_edits(&snapshots, session_start, session_end);

        assert_eq!(file_edits.len(), 1);
        assert_eq!(file_edits[0].lines_added, 15);
        assert_eq!(file_edits[0].lines_removed, 2);
    }

    #[test]
    fn test_compute_file_edits_sorts_by_edit_count() {
        let now = Utc::now();
        let session_start = now;
        let session_end = now + chrono::Duration::minutes(10);

        let snapshots = vec![
            Snapshot {
                id: crate::types::SnapshotId(1),
                file_path: "file1.rs".to_string(),
                event_type: crate::types::EventType::Create,
                timestamp: now + chrono::Duration::seconds(0),
                size_bytes: 100,
                content_hash: crate::types::ContentHash("abc123".to_string()),
                line_count: 10,
                lines_added: 10,
                lines_removed: 0,
            },
            Snapshot {
                id: crate::types::SnapshotId(2),
                file_path: "file2.rs".to_string(),
                event_type: crate::types::EventType::Create,
                timestamp: now + chrono::Duration::seconds(30),
                size_bytes: 50,
                content_hash: crate::types::ContentHash("def456".to_string()),
                line_count: 8,
                lines_added: 8,
                lines_removed: 0,
            },
            Snapshot {
                id: crate::types::SnapshotId(3),
                file_path: "file2.rs".to_string(),
                event_type: crate::types::EventType::Modify,
                timestamp: now + chrono::Duration::seconds(60),
                size_bytes: 55,
                content_hash: crate::types::ContentHash("ghi789".to_string()),
                line_count: 10,
                lines_added: 2,
                lines_removed: 1,
            },
            Snapshot {
                id: crate::types::SnapshotId(4),
                file_path: "file2.rs".to_string(),
                event_type: crate::types::EventType::Modify,
                timestamp: now + chrono::Duration::seconds(90),
                size_bytes: 58,
                content_hash: crate::types::ContentHash("jkl012".to_string()),
                line_count: 13,
                lines_added: 3,
                lines_removed: 0,
            },
        ];

        let file_edits = compute_file_edits(&snapshots, session_start, session_end);

        assert_eq!(file_edits.len(), 2);
        // file2 has 3 edits, file1 has 1
        assert_eq!(file_edits[0].path, "file2.rs");
        assert_eq!(file_edits[0].edits, 3);
        assert_eq!(file_edits[1].path, "file1.rs");
        assert_eq!(file_edits[1].edits, 1);
    }

    #[test]
    fn test_infer_since_time_with_sessions() {
        let now = Utc::now();
        let session_start = now - chrono::Duration::hours(2);
        let session_end = now - chrono::Duration::hours(1);

        let session = Session {
            number: 1,
            start: session_start,
            end: session_end,
            edit_count: 10,
            files: vec![],
        };

        let inferred = infer_since_time(&[session], now);

        // Should be session start minus 5 minutes
        let expected = session_start - chrono::Duration::minutes(5);
        assert_eq!(inferred, expected);
    }

    #[test]
    fn test_infer_since_time_without_sessions() {
        let now = Utc::now();

        let inferred = infer_since_time(&[], now);

        // Should be 24 hours ago
        let expected = now - chrono::Duration::hours(24);
        assert_eq!(inferred, expected);
    }

    #[test]
    fn test_build_session_recap() {
        let now = Utc::now();
        let session = Session {
            number: 1,
            start: now,
            end: now + chrono::Duration::minutes(30),
            edit_count: 5,
            files: vec!["file1.rs".to_string(), "file2.rs".to_string()],
        };

        let file_edits = vec![
            FileActivity {
                path: "file1.rs".to_string(),
                edits: 3,
                last_edit: (now + chrono::Duration::minutes(25)).to_rfc3339(),
                lines_added: 10,
                lines_removed: 2,
            },
            FileActivity {
                path: "file2.rs".to_string(),
                edits: 2,
                last_edit: (now + chrono::Duration::minutes(20)).to_rfc3339(),
                lines_added: 5,
                lines_removed: 1,
            },
        ];

        let recap = build_session_recap(&session, file_edits);

        assert_eq!(recap.number, 1);
        assert_eq!(recap.duration_seconds, 30 * 60);
        assert_eq!(recap.edit_count, 5);
        assert_eq!(recap.file_count, 2);
        assert_eq!(recap.files.len(), 2);
    }

    #[test]
    fn test_build_recap_output_with_git_info() {
        let git_info = GitInfo {
            branch: "main".to_string(),
            uncommitted_files: vec!["file.rs".to_string()],
            recent_commits: vec![GitCommit {
                hash: "abc1234".to_string(),
                message: "Fix bug".to_string(),
            }],
        };

        let recap = build_recap_output("/path/to/project", vec![], Some(git_info.clone()), None);

        assert_eq!(recap.project, "/path/to/project");
        assert!(recap.git.is_some());
        assert_eq!(recap.git.unwrap().branch, "main");
    }

    #[test]
    fn test_build_recap_output_without_git_info() {
        let recap = build_recap_output("/path/to/project", vec![], None, None);

        assert_eq!(recap.project, "/path/to/project");
        assert!(recap.git.is_none());
    }
}
