//! Session-related functions for log output.

use std::path::Path;

use crate::cli::filter::GlobFilter;
use crate::cli::OutputFormat;
use crate::engine::Engine;
use crate::error::UnfError;
use crate::storage;
use crate::types::Snapshot;

use super::super::output::use_color;
use super::filters::resolve_global_projects;
use super::format::format_session_duration;

/// Renders sessions as human-readable output.
pub(super) fn render_sessions_human(sessions: &[super::super::session::Session], colored: bool) {
    let latest_number = if sessions.len() > 1 {
        Some(sessions.last().unwrap().number)
    } else {
        None
    };

    for session in sessions {
        let duration = session.end.signed_duration_since(session.start);
        let duration_str = format_session_duration(duration);
        let start_time = session.start.with_timezone(&chrono::Local);
        let start_str = start_time.format("%H:%M").to_string();
        let end_time = session.end.with_timezone(&chrono::Local);
        let end_str = end_time.format("%H:%M").to_string();

        let file_word = if session.files.len() == 1 {
            "file"
        } else {
            "files"
        };

        let latest_marker = if latest_number == Some(session.number) {
            "  <-- latest"
        } else {
            ""
        };

        let line = format!(
            "Session {}  {} - {}  ({}, {} edits, {} {}){}",
            session.number,
            start_str,
            end_str,
            duration_str,
            session.edit_count,
            session.files.len(),
            file_word,
            latest_marker
        );

        let _ = colored; // colored is not used yet but may be used for formatting in future
        println!("{}", line);
    }
}

/// Renders sessions as JSON output.
pub(super) fn render_sessions_json(sessions: &[super::super::session::Session]) {
    let output = super::super::session::SessionsOutput::from_sessions(sessions);
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

/// Runs `unf log --sessions` for a single project.
pub fn run_sessions(
    project_root: &Path,
    since: Option<&str>,
    include: &[String],
    exclude: &[String],
    ignore_case: bool,
    format: OutputFormat,
) -> Result<(), UnfError> {
    let storage_dir = storage::resolve_storage_dir(project_root)?;
    let engine = Engine::open(project_root, &storage_dir)?;

    // Create glob filter
    let filter = GlobFilter::new(include, exclude, ignore_case)?;

    // Parse since parameter if provided
    let since_time = if let Some(spec) = since {
        super::super::parse_time_spec(spec)?
    } else {
        // Use far-past sentinel for "all history"
        chrono::DateTime::<chrono::Utc>::from(
            chrono::DateTime::parse_from_rfc3339("2000-01-01T00:00:00Z").unwrap(),
        )
    };

    // Get all snapshots since the specified time
    let mut snapshots = engine.get_snapshots_since(since_time)?;

    // Apply glob filter
    snapshots.retain(|s| filter.matches(&s.file_path));

    // Sort chronologically ascending (snapshots from get_snapshots_since are DESC)
    snapshots.sort_by_key(|s| s.timestamp);

    if snapshots.is_empty() {
        return Err(UnfError::NoResults("No sessions detected".to_string()));
    }

    // Detect sessions
    let sessions = super::super::session::detect_sessions(&snapshots);

    if sessions.is_empty() {
        return Err(UnfError::NoResults("No sessions detected".to_string()));
    }

    // Render output
    match format {
        OutputFormat::Json => render_sessions_json(&sessions),
        OutputFormat::Human => render_sessions_human(&sessions, use_color()),
    }

    Ok(())
}

/// Runs `unf log --sessions --global` across all registered projects.
pub fn run_global_sessions(
    include_project: &[String],
    exclude_project: &[String],
    since: Option<&str>,
    include: &[String],
    exclude: &[String],
    ignore_case: bool,
    format: OutputFormat,
) -> Result<(), UnfError> {
    let filter = GlobFilter::new(include, exclude, ignore_case)?;

    let since_time = if let Some(spec) = since {
        super::super::parse_time_spec(spec)?
    } else {
        // Use far-past sentinel for "all history"
        chrono::DateTime::<chrono::Utc>::from(
            chrono::DateTime::parse_from_rfc3339("2000-01-01T00:00:00Z").unwrap(),
        )
    };

    let projects = resolve_global_projects(include_project, exclude_project)?;

    // Get home directory for tilde notation
    let home = std::env::var("HOME").unwrap_or_default();

    // Collect snapshots from all projects
    let mut all_snapshots: Vec<Snapshot> = Vec::new();

    for (project_path, storage_dir) in &projects {
        match Engine::open(project_path, storage_dir) {
            Ok(engine) => {
                match engine.get_snapshots_since(since_time) {
                    Ok(mut snaps) => {
                        // Filter by glob patterns
                        snaps.retain(|s| filter.matches(&s.file_path));

                        // Prefix file paths with project display path
                        for snap in &mut snaps {
                            let display_path = if project_path.starts_with(&home) {
                                format!("~{}", &project_path.to_string_lossy()[home.len()..])
                            } else {
                                project_path.to_string_lossy().to_string()
                            };
                            snap.file_path = format!("{}: {}", display_path, snap.file_path);
                        }

                        all_snapshots.extend(snaps);
                    }
                    Err(_) => continue,
                }
            }
            Err(_) => continue,
        }
    }

    if all_snapshots.is_empty() {
        return Err(UnfError::NoResults("No sessions detected".to_string()));
    }

    // Sort ascending by timestamp
    all_snapshots.sort_by_key(|s| s.timestamp);

    // Detect sessions
    let sessions = super::super::session::detect_sessions(&all_snapshots);

    if sessions.is_empty() {
        return Err(UnfError::NoResults("No sessions detected".to_string()));
    }

    // Render output
    match format {
        OutputFormat::Json => render_sessions_json(&sessions),
        OutputFormat::Human => render_sessions_human(&sessions, use_color()),
    }

    Ok(())
}
