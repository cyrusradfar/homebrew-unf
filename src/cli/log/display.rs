//! Display and rendering logic for log output.

use std::io;

use crate::engine::Engine;
use crate::error::UnfError;
use crate::types::Snapshot;

use super::super::output::colors;
use super::format::{format_grouped_entry, format_snapshot_line, spans_multiple_days};
use super::{
    FileGroup, GlobalGroupedOutput, GlobalGroupedProject, GlobalGroupedSummary, GlobalLogEntry,
    GlobalLogOutput, GroupedFileEntry, GroupedLogOutput, GroupedSummary, LogEntry,
    GROUPED_PAGE_SIZE,
};

/// Renders grouped file history as JSON output.
///
/// Takes file groups (already sorted by most-recent-activity) and converts
/// them to the GroupedLogOutput structure. Always includes stats from snapshot fields.
///
/// # Arguments
/// * `_engine` - Engine instance (currently unused but kept for consistency)
/// * `groups` - FileGroup entries sorted by most-recent activity (newest first)
///
/// # Returns
/// A GroupedLogOutput struct with files and summary statistics
pub(super) fn render_grouped_json(_engine: &Engine, groups: Vec<FileGroup>) -> GroupedLogOutput {
    let mut total_changes = 0;
    let files: Vec<GroupedFileEntry> = groups
        .into_iter()
        .map(|group| {
            let change_count = group.entries.len();
            total_changes += change_count;

            // Convert each snapshot to a LogEntry
            let entries: Vec<LogEntry> = group
                .entries
                .iter()
                .map(|snapshot| LogEntry {
                    id: snapshot.id.0,
                    file: snapshot.file_path.clone(),
                    event: snapshot.event_type.to_string(),
                    bytes: snapshot.size_bytes,
                    size_human: super::super::format_size(snapshot.size_bytes),
                    timestamp: snapshot.timestamp.to_rfc3339(),
                    hash: snapshot.content_hash.0.clone(),
                    lines: snapshot.line_count,
                    lines_added: snapshot.lines_added,
                    lines_removed: snapshot.lines_removed,
                })
                .collect();

            GroupedFileEntry {
                path: group.path,
                change_count,
                entries,
            }
        })
        .collect();

    let total_files = files.len();
    GroupedLogOutput {
        files,
        summary: GroupedSummary {
            total_files,
            total_changes,
        },
    }
}

/// Renders grouped file history as human-readable tree view.
///
/// Outputs each file group with a header line showing the path and change count,
/// followed by indented entries. Handles multi-day groups specially to show dates.
/// Always shows stats from snapshot fields.
pub(super) fn render_grouped_human(
    _engine: &Engine,
    groups: Vec<FileGroup>,
    use_color: bool,
    is_tty: bool,
) -> Result<(), UnfError> {
    for (displayed_groups, group) in groups.iter().enumerate() {
        // Check pagination: 20 groups per page in TTY
        if is_tty && displayed_groups > 0 && displayed_groups % GROUPED_PAGE_SIZE == 0 {
            println!(
                "-- more ({} groups displayed, Enter to continue, q to quit) --",
                displayed_groups
            );
            let mut input = String::new();
            io::stdin()
                .read_line(&mut input)
                .map_err(|e| UnfError::InvalidArgument(format!("Failed to read input: {}", e)))?;

            if input.trim().eq_ignore_ascii_case("q") {
                println!("-- {} groups shown --", displayed_groups);
                return Ok(());
            }
        }

        let multi_day = spans_multiple_days(group);
        let change_count = group.entries.len();
        let change_word = if change_count == 1 {
            "change"
        } else {
            "changes"
        };

        // Format file header
        let path_display = if use_color {
            format!("{}{}{}", colors::BOLD, group.path, colors::RESET)
        } else {
            group.path.clone()
        };

        let header = if multi_day {
            let first = &group.entries[0];
            let last = &group.entries[group.entries.len() - 1];
            let first_local = first.timestamp.with_timezone(&chrono::Local);
            let last_local = last.timestamp.with_timezone(&chrono::Local);
            let first_date_str = first_local.format("%b %d").to_string();
            let last_date_str = last_local.format("%b %d").to_string();
            format!(
                "{}  ({} {}, {} - {})",
                path_display, change_count, change_word, first_date_str, last_date_str
            )
        } else {
            format!("{}  ({} {})", path_display, change_count, change_word)
        };

        println!("{}", header);

        // Print each entry in the group
        for snapshot in &group.entries {
            let line = format_grouped_entry(snapshot, use_color, multi_day);
            println!("{}", line);
        }

        // Blank line between groups
        println!();
    }

    // Footer
    let total_changes: usize = groups.iter().map(|g| g.entries.len()).sum();
    let file_word = if groups.len() == 1 { "file" } else { "files" };
    let change_word = if total_changes == 1 {
        "change"
    } else {
        "changes"
    };
    println!(
        "-- {} {}, {} {} --",
        groups.len(),
        file_word,
        total_changes,
        change_word
    );

    Ok(())
}

/// Renders global log as flat JSON output.
pub(super) fn render_global_flat_json(snapshots: &[super::ProjectSnapshot]) {
    let entries: Vec<GlobalLogEntry> = snapshots
        .iter()
        .map(|ps| GlobalLogEntry {
            project: ps.project_path.clone(),
            entry: LogEntry {
                id: ps.snapshot.id.0,
                file: ps.snapshot.file_path.clone(),
                event: ps.snapshot.event_type.to_string(),
                bytes: ps.snapshot.size_bytes,
                size_human: super::super::format_size(ps.snapshot.size_bytes),
                timestamp: ps.snapshot.timestamp.to_rfc3339(),
                hash: ps.snapshot.content_hash.0.clone(),
                lines: ps.snapshot.line_count,
                lines_added: ps.snapshot.lines_added,
                lines_removed: ps.snapshot.lines_removed,
            },
        })
        .collect();
    let output = GlobalLogOutput { entries };
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

/// Renders global log as grouped JSON output (grouped by project, then by file).
pub(super) fn render_global_grouped_json(snapshots: &[super::ProjectSnapshot]) {
    use std::collections::BTreeMap;

    // Group by project
    let mut by_project: BTreeMap<String, Vec<Snapshot>> = BTreeMap::new();
    for ps in snapshots {
        by_project
            .entry(ps.project_path.clone())
            .or_default()
            .push(ps.snapshot.clone());
    }

    let mut total_files = 0;
    let mut total_changes = 0;

    let projects_output: Vec<GlobalGroupedProject> = by_project
        .into_iter()
        .map(|(project, snaps)| {
            let groups = super::filters::group_by_file(snaps);
            let files: Vec<GroupedFileEntry> = groups
                .into_iter()
                .map(|group| {
                    let change_count = group.entries.len();
                    total_changes += change_count;
                    let entries = group
                        .entries
                        .iter()
                        .map(|s| LogEntry {
                            id: s.id.0,
                            file: s.file_path.clone(),
                            event: s.event_type.to_string(),
                            bytes: s.size_bytes,
                            size_human: super::super::format_size(s.size_bytes),
                            timestamp: s.timestamp.to_rfc3339(),
                            hash: s.content_hash.0.clone(),
                            lines: s.line_count,
                            lines_added: s.lines_added,
                            lines_removed: s.lines_removed,
                        })
                        .collect();
                    GroupedFileEntry {
                        path: group.path,
                        change_count,
                        entries,
                    }
                })
                .collect();
            total_files += files.len();
            GlobalGroupedProject { project, files }
        })
        .collect();

    let total_projects = projects_output.len();
    let output = GlobalGroupedOutput {
        projects: projects_output,
        summary: GlobalGroupedSummary {
            total_projects,
            total_files,
            total_changes,
        },
    };
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

/// Renders global log as flat human output with project headers.
pub(super) fn render_global_flat_human(snapshots: &[super::ProjectSnapshot], colored: bool) {
    let mut current_project: Option<&str> = None;
    let mut project_count = 0;

    for ps in snapshots {
        if current_project != Some(&ps.project_path) {
            if current_project.is_some() {
                println!(); // Blank line between projects
            }
            let header = if colored {
                format!("{}-- {} --{}", colors::BOLD, ps.project_path, colors::RESET)
            } else {
                format!("-- {} --", ps.project_path)
            };
            println!("{}", header);
            current_project = Some(&ps.project_path);
            project_count += 1;
        }

        let line = format_snapshot_line(&ps.snapshot, colored);
        println!("{}", line);
    }

    // Footer
    let change_word = if snapshots.len() == 1 {
        "change"
    } else {
        "changes"
    };
    let project_word = if project_count == 1 {
        "project"
    } else {
        "projects"
    };
    println!(
        "\n-- {} {} across {} {} --",
        snapshots.len(),
        change_word,
        project_count,
        project_word
    );
}

/// Renders global log as grouped human output (project headers, then file groups).
pub(super) fn render_global_grouped_human(
    snapshots: &[super::ProjectSnapshot],
    colored: bool,
) -> Result<(), UnfError> {
    use std::collections::BTreeMap;

    // Group by project
    let mut by_project: BTreeMap<String, Vec<Snapshot>> = BTreeMap::new();
    for ps in snapshots {
        by_project
            .entry(ps.project_path.clone())
            .or_default()
            .push(ps.snapshot.clone());
    }

    let mut total_changes = 0;
    let mut total_files = 0;
    let project_count = by_project.len();

    for (project, snaps) in &by_project {
        let header = if colored {
            format!("{}=== {} ==={}", colors::BOLD, project, colors::RESET)
        } else {
            format!("=== {} ===", project)
        };
        println!("{}", header);

        let groups = super::filters::group_by_file(snaps.clone());
        total_files += groups.len();

        for group in &groups {
            total_changes += group.entries.len();
            let multi_day = spans_multiple_days(group);
            let change_count = group.entries.len();
            let change_word = if change_count == 1 {
                "change"
            } else {
                "changes"
            };

            let path_display = if colored {
                format!("{}{}{}", colors::BOLD, group.path, colors::RESET)
            } else {
                group.path.clone()
            };

            println!("{}  ({} {})", path_display, change_count, change_word);

            for snapshot in &group.entries {
                let line = format_grouped_entry(snapshot, colored, multi_day);
                println!("{}", line);
            }
            println!();
        }
    }

    // Footer
    let project_word = if project_count == 1 {
        "project"
    } else {
        "projects"
    };
    let file_word = if total_files == 1 { "file" } else { "files" };
    let change_word = if total_changes == 1 {
        "change"
    } else {
        "changes"
    };
    println!(
        "-- {} {}, {} {}, {} {} --",
        project_count, project_word, total_files, file_word, total_changes, change_word
    );

    Ok(())
}
