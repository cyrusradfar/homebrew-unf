//! `unf list` command implementation.
//!
//! Shows all registered UNFUDGED projects on the machine with their status.

use std::path::Path;

use crate::cli::OutputFormat;
use crate::engine::Engine;
use crate::error::UnfError;
use crate::registry;
use crate::storage;

/// JSON output for a single project in the list.
#[derive(serde::Serialize)]
struct ProjectInfo {
    path: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshots: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    store_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tracked_files: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recording_since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_activity: Option<String>,
    // Private fields for human output formatting (not serialized)
    #[serde(skip)]
    oldest_snapshot_time: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(skip)]
    newest_snapshot_time: Option<chrono::DateTime<chrono::Utc>>,
}

/// JSON output for the list command.
#[derive(serde::Serialize)]
struct ListOutput {
    projects: Vec<ProjectInfo>,
}

/// Runs the `unf list` command.
///
/// Loads the global project registry and displays status for each project.
///
/// # Arguments
///
/// * `format` - Output format (human or JSON)
/// * `verbose` - If true, include additional project details
pub fn run(format: OutputFormat, verbose: bool) -> Result<(), UnfError> {
    let reg = registry::load()?;

    if reg.projects.is_empty() {
        if format == OutputFormat::Json {
            let output = ListOutput { projects: vec![] };
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        } else {
            println!("No projects registered.");
        }
        return Ok(());
    }

    let mut infos = Vec::new();

    for entry in &reg.projects {
        let info = gather_project_info(&entry.path, verbose);
        infos.push(info);
    }

    if format == OutputFormat::Json {
        let output = ListOutput { projects: infos };
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        // Two-pass formatting: collect display strings, compute widths, then print
        use super::output::{colors, format_recency, format_short_date, shorten_home, use_color};

        #[derive(Debug)]
        struct DisplayRow {
            path: String,
            status: String,
            snapshots: String,
            size: String,
            files: String, // only used in verbose
            range: String,
        }

        let mut rows = Vec::new();

        for info in &infos {
            let path_display = shorten_home(&info.path);

            let snapshots_str = match info.snapshots {
                Some(n) => format_number(n),
                None => "--".to_string(),
            };

            let size_str = match info.store_bytes {
                Some(b) => format_size(b),
                None => "--".to_string(),
            };

            let files_str = match info.tracked_files {
                Some(n) => format_number(n),
                None => "--".to_string(),
            };

            let range_str = match (info.oldest_snapshot_time, info.newest_snapshot_time) {
                (Some(oldest), Some(newest)) => {
                    format!("{} – {}", format_short_date(oldest), format_recency(newest))
                }
                (Some(oldest), None) => {
                    // Only oldest exists (shouldn't happen normally)
                    format!("{} – {}", format_short_date(oldest), "?")
                }
                (None, Some(newest)) => {
                    // Only newest exists (shouldn't happen normally)
                    format!("? – {}", format_recency(newest))
                }
                (None, None) => "--".to_string(),
            };

            rows.push(DisplayRow {
                path: path_display,
                status: info.status.clone(),
                snapshots: snapshots_str,
                size: size_str,
                files: files_str,
                range: range_str,
            });
        }

        // Compute column widths
        let mut col_path_width = 7; // "PROJECT"
        let mut col_status_width = 6; // "STATUS"
        let mut col_snapshots_width = 9; // "SNAPSHOTS"
        let mut col_size_width = 4; // "SIZE"
        let mut col_files_width = 5; // "FILES"
        let mut col_range_width = 5; // "RANGE"

        for row in &rows {
            col_path_width = col_path_width.max(row.path.len());
            col_status_width = col_status_width.max(row.status.len());
            col_snapshots_width = col_snapshots_width.max(row.snapshots.len());
            col_size_width = col_size_width.max(row.size.len());
            col_files_width = col_files_width.max(row.files.len());
            col_range_width = col_range_width.max(row.range.len());
        }

        // Print header (dimmed)
        let use_color_output = use_color();
        if use_color_output {
            print!("{}", colors::DIM);
        }

        print!(
            "{:<width_path$}  {:<width_status$}  {:>width_snapshots$}  {:>width_size$}",
            "PROJECT",
            "STATUS",
            "SNAPSHOTS",
            "SIZE",
            width_path = col_path_width,
            width_status = col_status_width,
            width_snapshots = col_snapshots_width,
            width_size = col_size_width,
        );

        if verbose {
            print!("  {:>width_files$}", "FILES", width_files = col_files_width);
        }

        println!("  RANGE");

        if use_color_output {
            print!("{}", colors::RESET);
        }

        // Print rows
        for row in rows {
            // Pad status to column width FIRST, then wrap with color
            // (ANSI codes are invisible but count in format width)
            let status_padded = format!("{:<width$}", row.status, width = col_status_width);
            let status_display = if use_color_output {
                match row.status.as_str() {
                    "watching" => format!("{}{}{}", colors::GREEN, status_padded, colors::RESET),
                    "stopped" => format!("{}{}{}", colors::YELLOW, status_padded, colors::RESET),
                    "crashed" | "orphaned" | "error" => {
                        format!("{}{}{}", colors::RED, status_padded, colors::RESET)
                    }
                    _ => status_padded,
                }
            } else {
                status_padded
            };

            print!(
                "{:<width_path$}  {}  {:>width_snapshots$}  {:>width_size$}",
                row.path,
                status_display,
                row.snapshots,
                row.size,
                width_path = col_path_width,
                width_snapshots = col_snapshots_width,
                width_size = col_size_width,
            );

            if verbose {
                print!(
                    "  {:>width_files$}",
                    row.files,
                    width_files = col_files_width
                );
            }

            println!("  {}", row.range);
        }
    }

    Ok(())
}

/// Gathers status information for a single project.
fn gather_project_info(project_path: &Path, _verbose: bool) -> ProjectInfo {
    let path_str = project_path.display().to_string();

    // Resolve the centralized storage directory
    let storage_dir = match storage::resolve_storage_dir_canonical(project_path) {
        Ok(d) => d,
        Err(_) => {
            return ProjectInfo {
                path: path_str,
                status: "error".to_string(),
                snapshots: None,
                store_bytes: None,
                tracked_files: None,
                recording_since: None,
                last_activity: None,
                oldest_snapshot_time: None,
                newest_snapshot_time: None,
            };
        }
    };

    if !storage_dir.exists() {
        return ProjectInfo {
            path: path_str,
            status: "error".to_string(),
            snapshots: None,
            store_bytes: None,
            tracked_files: None,
            recording_since: None,
            last_activity: None,
            oldest_snapshot_time: None,
            newest_snapshot_time: None,
        };
    }

    // Check if project directory still exists (orphan detection)
    let project_exists = project_path.exists();

    // Determine daemon status
    let status = if !project_exists {
        "orphaned"
    } else {
        let stopped_file = storage::stopped_path(&storage_dir);
        let is_recording = is_project_being_watched(project_path, &storage_dir);

        if is_recording {
            "watching"
        } else if stopped_file.exists() {
            "stopped"
        } else {
            "crashed"
        }
    };

    // Always try to query engine stats (even for stopped/orphaned projects)
    match Engine::open(project_path, &storage_dir) {
        Ok(engine) => {
            let snapshots = engine.get_snapshot_count().ok();
            let store_bytes = engine.get_store_size().ok();

            // Always populate these fields for JSON output (Tauri GUI needs them)
            let tracked = engine.get_tracked_file_count().ok();
            let oldest = engine.get_oldest_snapshot_time().ok().flatten();
            let newest = engine.get_newest_snapshot_time().ok().flatten();

            let recording_str = oldest.map(crate::cli::format_local_time);
            let activity_str = newest.map(|t| {
                let now = chrono::Utc::now();
                let duration = now.signed_duration_since(t);
                crate::cli::format_duration_ago(duration)
            });

            ProjectInfo {
                path: path_str,
                status: status.to_string(),
                snapshots,
                store_bytes,
                tracked_files: tracked,
                recording_since: recording_str,
                last_activity: activity_str,
                oldest_snapshot_time: oldest,
                newest_snapshot_time: newest,
            }
        }
        Err(_) => ProjectInfo {
            path: path_str,
            status: status.to_string(),
            snapshots: None,
            store_bytes: None,
            tracked_files: None,
            recording_since: None,
            last_activity: None,
            oldest_snapshot_time: None,
            newest_snapshot_time: None,
        },
    }
}

/// Checks if a project is actively being watched by the global daemon.
fn is_project_being_watched(project_path: &Path, _storage_dir: &Path) -> bool {
    let global_pid_path = match storage::global_pid_path() {
        Ok(p) => p,
        Err(_) => return false,
    };
    let pid_str = match std::fs::read_to_string(&global_pid_path) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let pid = match pid_str.trim().parse::<u32>() {
        Ok(p) => p,
        Err(_) => return false,
    };
    if !crate::process::is_alive(pid) {
        return false;
    }
    // Global daemon alive — check if this project is registered
    if let Ok(registry) = crate::registry::load() {
        let canonical = project_path
            .canonicalize()
            .unwrap_or_else(|_| project_path.to_path_buf());
        return registry.projects.iter().any(|p| p.path == canonical);
    }
    false
}

use super::output::{format_number, format_size};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gather_info_missing_directory() {
        let info = gather_project_info(Path::new("/nonexistent/path"), false);
        assert_eq!(info.status, "error");
        assert!(info.snapshots.is_none());
        assert!(info.store_bytes.is_none());
    }
}
