//! `unf list` command implementation.
//!
//! Shows all registered UNFUDGED projects on the machine with their status.

use std::path::Path;

use crate::cli::OutputFormat;
use crate::engine::Engine;
use crate::error::UnfError;
use crate::process::PidFile;
use crate::registry;
use crate::storage;
use crate::types::WatchSettings;

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
    /// True when this project runs with `--force-watch-gitignore`.
    /// Always serialized; the `status` string never carries the marker.
    force_watch_gitignore: bool,
    /// Excluded directories this project records, from `--unignore-dir`.
    /// Always serialized as an array, empty when none. The default table
    /// carries no marker for this state — only the verbose column shows it.
    unignored_dirs: Vec<String>,
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
#[allow(clippy::cognitive_complexity)]
// TODO(v0.18): reduce complexity
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

    // Auto-start sentinel if projects are registered but daemon isn't running.
    // This handles the post-install case: clear the stopped marker (left by
    // a previous `unf stop` or uninstall) and start the sentinel.
    if !crate::sentinel::is_sentinel_alive() {
        if let Ok(stopped_path) = storage::global_stopped_path() {
            let _ = std::fs::remove_file(&stopped_path);
        }
        let _ = crate::sentinel::ensure_sentinel_running();
        // Give sentinel time to start the daemon before we query status
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    let mut infos = Vec::new();

    for entry in &reg.projects {
        let info = gather_project_info(&entry.path, verbose, &entry.settings);
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
            /// Bare status word. Never carries the marker — the color match
            /// below keys off this exact string.
            status: String,
            /// `"*"` when the project forces gitignore, `""` otherwise.
            marker: &'static str,
            snapshots: String,
            size: String,
            files: String,     // only used in verbose
            gitignore: String, // only used in verbose
            unignored: String, // only used in verbose
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
                marker: status_marker(info.force_watch_gitignore),
                snapshots: snapshots_str,
                size: size_str,
                files: files_str,
                gitignore: gitignore_label(info.force_watch_gitignore).to_string(),
                unignored: unignored_label(&info.unignored_dirs),
                range: range_str,
            });
        }

        // Compute column widths
        let mut col_path_width = 7; // "PROJECT"
        let mut col_status_width = 6; // "STATUS"
        let mut col_snapshots_width = 9; // "SNAPSHOTS"
        let mut col_size_width = 4; // "SIZE"
        let mut col_files_width = 5; // "FILES"
        let mut col_gitignore_width = GITIGNORE_HEADER.len();
        let mut col_unignored_width = UNIGNORED_HEADER.len();
        let mut col_range_width = 5; // "RANGE"

        for row in &rows {
            col_path_width = col_path_width.max(row.path.len());
            // The marker prints inside the status cell, so it counts toward the width.
            col_status_width = col_status_width.max(row.status.len() + row.marker.len());
            col_snapshots_width = col_snapshots_width.max(row.snapshots.len());
            col_size_width = col_size_width.max(row.size.len());
            col_files_width = col_files_width.max(row.files.len());
            col_gitignore_width = col_gitignore_width.max(row.gitignore.len());
            col_unignored_width = col_unignored_width.max(row.unignored.len());
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
            print!(
                "  {:<width_gitignore$}",
                GITIGNORE_HEADER,
                width_gitignore = col_gitignore_width
            );
            print!(
                "  {:<width_unignored$}",
                UNIGNORED_HEADER,
                width_unignored = col_unignored_width
            );
        }

        println!("  RANGE");

        if use_color_output {
            print!("{}", colors::RESET);
        }

        let any_marked = rows.iter().any(|row| !row.marker.is_empty());

        // Print rows
        for row in &rows {
            let status_display =
                render_status_cell(&row.status, row.marker, col_status_width, use_color_output);

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
                print!(
                    "  {:<width_gitignore$}",
                    row.gitignore,
                    width_gitignore = col_gitignore_width
                );
                print!(
                    "  {:<width_unignored$}",
                    row.unignored,
                    width_unignored = col_unignored_width
                );
            }

            println!("  {}", row.range);
        }

        // Footnote only when the table actually shows a marker.
        if any_marked {
            println!();
            println!("{}", MARKER_FOOTNOTE);
        }
    }

    Ok(())
}

/// Header for the verbose-only gitignore column.
const GITIGNORE_HEADER: &str = "GITIGNORE";

/// Header for the verbose-only un-ignored-directories column.
const UNIGNORED_HEADER: &str = "UNIGNORED";

/// Value shown in the `UNIGNORED` column when nothing is opted in.
///
/// A dash, not an empty cell: an empty cell reads as missing data.
const UNIGNORED_EMPTY: &str = "-";

/// How many directory names the `UNIGNORED` column spells out before it
/// switches to a `+N` remainder. Two keeps the column narrow enough that
/// `RANGE` still fits on an 80-column terminal.
const UNIGNORED_NAMES_SHOWN: usize = 2;

/// Suffix shown on the status cell of a project that forces gitignore.
const FORCE_GITIGNORE_MARKER: &str = "*";

/// Footnote printed under the table when any row carries the marker.
const MARKER_FOOTNOTE: &str =
    "* --force-watch-gitignore is on. This project records gitignored files.";

/// Returns the status-cell suffix for a project.
///
/// Empty for the default (gitignore respected), so unmarked tables look
/// exactly as they do today.
fn status_marker(force_watch_gitignore: bool) -> &'static str {
    if force_watch_gitignore {
        FORCE_GITIGNORE_MARKER
    } else {
        ""
    }
}

/// Returns the verbose `GITIGNORE` column value for a project.
fn gitignore_label(force_watch_gitignore: bool) -> &'static str {
    if force_watch_gitignore {
        "forced"
    } else {
        "respected"
    }
}

/// Returns the verbose `UNIGNORED` column value for a project.
///
/// Names are already sorted and deduplicated — they arrive from a
/// `BTreeSet` — so this only has to decide how many of them fit.
///
/// * empty -> `-`
/// * one or two -> `target` / `target,dist`
/// * more -> `target,dist+2`, the first two names plus a remainder count
///
/// The count is what matters once the list is long: a user who sees `+2`
/// runs `unf status` in that project for the full list. Comma without a
/// space keeps the cell a single word, so the column stays scannable.
fn unignored_label(dirs: &[String]) -> String {
    if dirs.is_empty() {
        return UNIGNORED_EMPTY.to_string();
    }

    let shown = dirs
        .iter()
        .take(UNIGNORED_NAMES_SHOWN)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(",");

    match dirs.len().saturating_sub(UNIGNORED_NAMES_SHOWN) {
        0 => shown,
        hidden => format!("{shown}+{hidden}"),
    }
}

/// Renders one padded, colored status cell.
///
/// The color keys off `status` alone. `marker` is appended after the color is
/// chosen, so a marked row keeps the color of its bare status word — appending
/// the marker to `status` first would make every marked row fall through to the
/// uncolored arm.
///
/// Padding is applied before the ANSI codes are wrapped around the cell:
/// escape codes are invisible but would otherwise count toward the width.
///
/// # Arguments
///
/// * `status` - Bare status word (`"watching"`, `"stopped"`, ...)
/// * `marker` - `"*"` or `""`
/// * `width` - Target column width, marker included
/// * `use_color` - Whether to emit ANSI color codes
fn render_status_cell(status: &str, marker: &str, width: usize, use_color: bool) -> String {
    use super::output::colors;

    let padded = format!("{:<width$}", format!("{}{}", status, marker), width = width);

    if !use_color {
        return padded;
    }

    let color = match status {
        "watching" => colors::GREEN,
        "stopped" => colors::YELLOW,
        "crashed" | "orphaned" | "error" => colors::RED,
        _ => return padded,
    };

    format!("{}{}{}", color, padded, colors::RESET)
}

/// Gathers status information for a single project.
///
/// # Arguments
///
/// * `project_path` - Absolute path to the project root
/// * `settings` - The registry entry's watch settings. Passed in by the caller,
///   which already holds the registry entry, so this function never reloads
///   `projects.json`. Taking the whole value rather than one parameter per
///   setting keeps the signature from growing with every new knob.
fn gather_project_info(
    project_path: &Path,
    _verbose: bool,
    settings: &WatchSettings,
) -> ProjectInfo {
    let path_str = project_path.display().to_string();
    let force_watch_gitignore = settings.force_watch_gitignore;
    // Built once per project: at most nine short names, cloned into each of
    // the early-return payloads below.
    let unignored_dirs: Vec<String> = settings.unignored_dirs.iter().cloned().collect();

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
                force_watch_gitignore,
                unignored_dirs: unignored_dirs.clone(),
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
            force_watch_gitignore,
            unignored_dirs: unignored_dirs.clone(),
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
                force_watch_gitignore,
                unignored_dirs: unignored_dirs.clone(),
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
            force_watch_gitignore,
            unignored_dirs: unignored_dirs.clone(),
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
    let pid_file = PidFile::new(global_pid_path);
    let pid = match pid_file.read() {
        Ok(Some(p)) => p,
        _ => return false,
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

    use super::super::output::colors;

    /// Builds the settings a registry entry would carry.
    fn settings(force_watch_gitignore: bool, dirs: &[&str]) -> WatchSettings {
        WatchSettings {
            force_watch_gitignore,
            unignored_dirs: dirs.iter().map(|d| (*d).to_string()).collect(),
        }
    }

    /// Builds an owned name list the way `ProjectInfo` carries it.
    fn names(dirs: &[&str]) -> Vec<String> {
        dirs.iter().map(|d| (*d).to_string()).collect()
    }

    #[test]
    fn gather_info_missing_directory() {
        let info =
            gather_project_info(Path::new("/nonexistent/path"), false, &settings(false, &[]));
        assert_eq!(info.status, "error");
        assert!(info.snapshots.is_none());
        assert!(info.store_bytes.is_none());
        assert!(!info.force_watch_gitignore);
        assert!(info.unignored_dirs.is_empty());
    }

    #[test]
    fn gather_info_early_return_keeps_the_flag() {
        // The error path must carry the flag too, or a broken project would
        // silently report "respected".
        let info = gather_project_info(Path::new("/nonexistent/path"), false, &settings(true, &[]));
        assert_eq!(info.status, "error");
        assert!(info.force_watch_gitignore);
    }

    #[test]
    fn gather_info_early_return_keeps_the_unignore_list() {
        // Same reasoning as the flag: a broken project must not report an
        // empty list it does not have.
        let info = gather_project_info(
            Path::new("/nonexistent/path"),
            false,
            &settings(false, &["target"]),
        );
        assert_eq!(info.status, "error");
        assert_eq!(info.unignored_dirs, names(&["target"]));
    }

    #[test]
    fn marker_does_not_change_status_color() {
        // The trap: appending "*" to the status string before the color match
        // drops a marked row into the uncolored arm.
        let plain = render_status_cell("watching", "", 10, true);
        let marked = render_status_cell("watching", FORCE_GITIGNORE_MARKER, 10, true);

        assert!(plain.starts_with(colors::GREEN), "plain: {:?}", plain);
        assert!(marked.starts_with(colors::GREEN), "marked: {:?}", marked);
        assert!(marked.ends_with(colors::RESET));
        assert!(marked.contains("watching*"));
    }

    #[test]
    fn marked_and_unmarked_cells_share_a_width() {
        // Column alignment survives the marker: padding happens before the
        // (zero-width) ANSI codes are wrapped on.
        let plain = render_status_cell("watching", "", 10, false);
        let marked = render_status_cell("watching", FORCE_GITIGNORE_MARKER, 10, false);

        assert_eq!(plain.len(), 10);
        assert_eq!(marked.len(), 10);
        assert_eq!(marked.trim_end(), "watching*");
    }

    #[test]
    fn every_colored_status_survives_the_marker() {
        for (status, color) in [
            ("watching", colors::GREEN),
            ("stopped", colors::YELLOW),
            ("crashed", colors::RED),
            ("orphaned", colors::RED),
            ("error", colors::RED),
        ] {
            let marked = render_status_cell(status, FORCE_GITIGNORE_MARKER, 12, true);
            assert!(marked.starts_with(color), "{} lost its color", status);
        }
    }

    #[test]
    fn unknown_status_stays_uncolored() {
        let cell = render_status_cell("weird", FORCE_GITIGNORE_MARKER, 8, true);
        assert_eq!(cell, "weird*  ");
    }

    #[test]
    fn no_color_mode_emits_no_escape_codes() {
        let cell = render_status_cell("watching", FORCE_GITIGNORE_MARKER, 12, false);
        assert!(!cell.contains('\x1b'));
    }

    #[test]
    fn marker_and_label_track_the_flag() {
        assert_eq!(status_marker(true), "*");
        assert_eq!(status_marker(false), "");
        assert_eq!(gitignore_label(true), "forced");
        assert_eq!(gitignore_label(false), "respected");
    }

    #[test]
    fn unignore_list_adds_no_marker_to_the_default_table() {
        // The default table stays visually unchanged: only
        // --force-watch-gitignore earns a symbol, so readers never need a
        // two-symbol legend.
        assert_eq!(status_marker(false), "");
    }

    #[test]
    fn unignored_label_is_a_dash_when_empty() {
        // A dash, not an empty cell — an empty cell reads as missing data.
        assert_eq!(unignored_label(&[]), "-");
    }

    #[test]
    fn unignored_label_spells_out_short_lists() {
        assert_eq!(unignored_label(&names(&["target"])), "target");
        assert_eq!(unignored_label(&names(&["dist", "target"])), "dist,target");
    }

    /// `.git` paths are longer than bare names but stay inside the widest
    /// cell the column already had to size for: two full names plus a
    /// remainder, e.g. `node_modules,__pycache__+2`. No format change is
    /// needed, and the cell is still one word, so the column stays
    /// scannable.
    #[test]
    fn unignored_label_renders_git_paths_within_the_existing_width() {
        assert_eq!(
            unignored_label(&names(&[".git/hooks", "target"])),
            ".git/hooks,target"
        );

        let widest = unignored_label(&names(&[
            ".git/config",
            ".git/hooks",
            ".git/info/exclude",
            "target",
        ]));
        assert_eq!(widest, ".git/config,.git/hooks+2");
        assert!(!widest.contains(' '), "the cell stays a single word");

        // The column already had to size for two long bare names plus a
        // remainder. Paths do not push it past that, so the existing
        // truncation needs no change.
        let widest_names =
            unignored_label(&names(&["node_modules", "__pycache__", "target", "build"]));
        assert!(
            widest.len() <= widest_names.len(),
            "{widest} ({}) must not exceed {widest_names} ({})",
            widest.len(),
            widest_names.len()
        );
    }

    #[test]
    fn unignored_label_truncates_long_lists_with_a_remainder() {
        assert_eq!(
            unignored_label(&names(&["target", "dist", "build", "venv"])),
            "target,dist+2"
        );
    }

    #[test]
    fn unignored_label_remainder_counts_every_hidden_name() {
        let dirs = names(&["a", "b", "c", "d", "e", "f", "g"]);
        assert_eq!(unignored_label(&dirs), "a,b+5");
    }

    #[test]
    fn json_always_carries_the_flag_and_never_the_marker() {
        let info = gather_project_info(Path::new("/nonexistent/path"), false, &settings(true, &[]));
        let value = serde_json::to_value(&info).expect("serialize");

        assert_eq!(
            value["force_watch_gitignore"],
            serde_json::Value::Bool(true)
        );

        let status = value["status"].as_str().expect("status is a string");
        assert!(
            !status.contains('*'),
            "status leaked the marker: {}",
            status
        );
    }

    #[test]
    fn json_carries_a_false_flag_explicitly() {
        let info =
            gather_project_info(Path::new("/nonexistent/path"), false, &settings(false, &[]));
        let value = serde_json::to_value(&info).expect("serialize");

        assert_eq!(
            value["force_watch_gitignore"],
            serde_json::Value::Bool(false)
        );
    }

    #[test]
    fn json_always_carries_unignored_dirs_as_an_array() {
        // Empty, never absent: scripts and the Tauri app must not treat a
        // missing field as an empty list.
        let info =
            gather_project_info(Path::new("/nonexistent/path"), false, &settings(false, &[]));
        let value = serde_json::to_value(&info).expect("serialize");

        assert_eq!(value["unignored_dirs"], serde_json::json!([]));
    }

    #[test]
    fn json_carries_unignored_dirs_sorted() {
        let info = gather_project_info(
            Path::new("/nonexistent/path"),
            false,
            &settings(false, &["target", "dist"]),
        );
        let value = serde_json::to_value(&info).expect("serialize");

        // BTreeSet ordering survives the trip to JSON.
        assert_eq!(
            value["unignored_dirs"],
            serde_json::json!(["dist", "target"])
        );

        let status = value["status"].as_str().expect("status is a string");
        assert!(
            !status.contains('*') && !status.contains('+'),
            "status leaked a marker: {}",
            status
        );
    }
}
