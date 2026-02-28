//! Implementation of `unf log` command.
//!
//! Streams file change history with cursor-based pagination. Supports filtering
//! by file, directory, or all changes. Supports interactive pagination when
//! connected to a TTY.

use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

use crate::cli::filter::GlobFilter;
use crate::cli::OutputFormat;
use crate::engine::db::{HistoryCursor, HistoryScope};
use crate::engine::Engine;
use crate::error::UnfError;
use crate::storage;
use crate::types::{EventType, Snapshot};

/// JSON output for a single log entry.
#[derive(serde::Serialize)]
struct LogEntry {
    id: i64,
    file: String,
    event: String,
    bytes: u64,
    size_human: String,
    timestamp: String,
    hash: String,
    lines: u64,
    lines_added: u64,
    lines_removed: u64,
}

/// JSON output wrapping log entries with cursor pagination.
#[derive(serde::Serialize)]
struct PaginatedLogOutput {
    entries: Vec<LogEntry>,
    next_cursor: Option<String>,
}

/// JSON output for density histogram.
#[derive(serde::Serialize)]
struct DensityOutput {
    buckets: Vec<DensityBucket>,
    total: u64,
    from: String,
    to: String,
}

/// A single bucket in the density histogram.
#[derive(serde::Serialize, Debug, PartialEq)]
struct DensityBucket {
    start: String,
    end: String,
    count: u64,
}

/// JSON output for grouped log view.
#[derive(serde::Serialize)]
struct GroupedLogOutput {
    files: Vec<GroupedFileEntry>,
    summary: GroupedSummary,
}

/// A single file's history in grouped JSON output.
#[derive(serde::Serialize)]
struct GroupedFileEntry {
    path: String,
    change_count: usize,
    entries: Vec<LogEntry>,
}

/// Summary statistics for grouped JSON output.
#[derive(serde::Serialize)]
struct GroupedSummary {
    total_files: usize,
    total_changes: usize,
}

/// Page size for history pagination.
const PAGE_SIZE: u32 = 50;

/// Page size for grouped output (number of complete file groups per page).
const GROUPED_PAGE_SIZE: usize = 20;

use super::output::colors;
use super::output::use_color;

/// Extracts a cursor from the last snapshot in a page.
///
/// Returns `None` if the page is empty.
fn cursor_from_page(page: &[Snapshot]) -> Option<HistoryCursor> {
    page.last().map(|s| HistoryCursor {
        timestamp: s.timestamp,
        id: s.id,
    })
}

/// Computes density histogram buckets from a list of timestamps.
///
/// Divides the time range `[from, to]` into `num_buckets` equal-width buckets
/// and counts how many timestamps fall into each bucket.
///
/// # Arguments
/// * `timestamps` - Sorted or unsorted list of timestamps to bucket
/// * `from` - Start of the time range (inclusive)
/// * `to` - End of the time range (inclusive)
/// * `num_buckets` - Number of buckets to divide the range into
///
/// # Returns
/// A vector of `DensityBucket` structs, one per bucket.
/// Empty input or zero buckets returns an empty vector.
fn compute_density_buckets(
    timestamps: &[chrono::DateTime<chrono::Utc>],
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    num_buckets: u32,
) -> Vec<DensityBucket> {
    if timestamps.is_empty() || num_buckets == 0 {
        return Vec::new();
    }

    let total_duration_ms = (to - from).num_milliseconds().max(1);
    let bucket_width_ms = total_duration_ms / num_buckets as i64;
    let bucket_width_ms = bucket_width_ms.max(1); // Avoid division by zero

    let mut counts = vec![0u64; num_buckets as usize];
    for ts in timestamps {
        let offset_ms = (*ts - from).num_milliseconds();
        let idx = (offset_ms / bucket_width_ms) as usize;
        let idx = idx.min(num_buckets as usize - 1);
        counts[idx] += 1;
    }

    counts
        .into_iter()
        .enumerate()
        .map(|(i, count)| {
            let start = from + chrono::Duration::milliseconds(i as i64 * bucket_width_ms);
            let end = if i == num_buckets as usize - 1 {
                to
            } else {
                from + chrono::Duration::milliseconds((i as i64 + 1) * bucket_width_ms)
            };
            DensityBucket {
                start: start.to_rfc3339(),
                end: end.to_rfc3339(),
                count,
            }
        })
        .collect()
}

/// Parses a cursor string in `RFC3339:SnapshotId` format.
///
/// Uses `rsplit_once(':')` to separate the timestamp from the ID, which works
/// because RFC3339 timestamps never end with a colon and the snapshot ID is
/// always the rightmost segment.
fn parse_cursor(s: &str) -> Result<HistoryCursor, UnfError> {
    let (timestamp_str, id_str) = s.rsplit_once(':').ok_or_else(|| {
        UnfError::InvalidArgument(
            "Invalid cursor format. Expected RFC3339:SnapshotId (e.g., 2026-02-12T14:32:07+00:00:42)".to_string(),
        )
    })?;

    let timestamp = chrono::DateTime::parse_from_rfc3339(timestamp_str)
        .map_err(|e| UnfError::InvalidArgument(format!("Invalid cursor timestamp: {}", e)))?
        .with_timezone(&chrono::Utc);

    let id = id_str
        .parse::<i64>()
        .map_err(|e| UnfError::InvalidArgument(format!("Invalid cursor snapshot ID: {}", e)))?;

    Ok(HistoryCursor {
        timestamp,
        id: crate::types::SnapshotId(id),
    })
}

/// Formats a `HistoryCursor` as a string in `RFC3339:SnapshotId` format.
fn format_cursor(cursor: &HistoryCursor) -> String {
    format!("{}:{}", cursor.timestamp.to_rfc3339(), cursor.id.0)
}

/// Formats a single snapshot as a display line.
///
/// Output format with stats always shown:
/// ```text
///   2026-02-09 14:32:07  modified  src/engine/cas.rs       1.2 KB  +12/-3
///   2026-02-09 14:31:44  created   src/engine/compress.rs  0.9 KB  +0/-0
///   2026-02-09 14:30:12  deleted   src/old_engine.rs
///   2026-02-09 14:30:11  created   src/legacy.rs           5.3 KB  -/-
/// ```
///
/// Snapshots without recorded stats (pre-v0.5) show `-/-` instead of counts.
fn format_snapshot_line(snapshot: &Snapshot, use_color: bool) -> String {
    let local_time = super::format_local_time(snapshot.timestamp);
    let event_type_str = snapshot.event_type.to_string();

    // Pad the raw string first, then wrap with color to avoid ANSI codes
    // breaking column alignment.
    let padded_event = format!("{:<9}", event_type_str);
    let colored_event_type = if use_color {
        let color = match snapshot.event_type {
            EventType::Create => colors::GREEN,
            EventType::Modify => colors::YELLOW,
            EventType::Delete => colors::RED,
        };
        format!("{}{}{}", color, padded_event, colors::RESET)
    } else {
        padded_event
    };

    let size_str = if snapshot.event_type == EventType::Delete {
        String::new()
    } else {
        format!("{:>8}", super::format_size(snapshot.size_bytes))
    };

    let stats_str = if snapshot.event_type == EventType::Delete
        || (snapshot.lines_added == 0 && snapshot.lines_removed == 0 && snapshot.size_bytes > 0)
    {
        String::new()
    } else {
        let added_str = if use_color {
            format!(
                "{}+{}{}",
                colors::GREEN,
                snapshot.lines_added,
                colors::RESET
            )
        } else {
            format!("+{}", snapshot.lines_added)
        };
        let removed_str = if use_color {
            format!(
                "{}-{}{}",
                colors::RED,
                snapshot.lines_removed,
                colors::RESET
            )
        } else {
            format!("-{}", snapshot.lines_removed)
        };
        format!("  {}/{}", added_str, removed_str)
    };

    format!(
        "  {}  {}  {}  {}{}",
        local_time, colored_event_type, snapshot.file_path, size_str, stats_str
    )
}

/// Checks if a file group spans multiple calendar days.
///
/// Compares the date (not time) of the first and last entries in the group.
fn spans_multiple_days(group: &FileGroup) -> bool {
    if group.entries.len() < 2 {
        return false;
    }

    let first = &group.entries[0];
    let last = &group.entries[group.entries.len() - 1];

    let first_local = first.timestamp.with_timezone(&chrono::Local);
    let last_local = last.timestamp.with_timezone(&chrono::Local);

    first_local.format("%Y-%m-%d").to_string() != last_local.format("%Y-%m-%d").to_string()
}

/// Formats a UTC timestamp for grouped output.
///
/// When `multi_day` is false: shows time only (HH:MM:SS)
/// When `multi_day` is true: shows "Mon DD HH:MM:SS"
fn format_grouped_timestamp(utc_time: chrono::DateTime<chrono::Utc>, multi_day: bool) -> String {
    let local_time = utc_time.with_timezone(&chrono::Local);
    if multi_day {
        local_time.format("%a %d %H:%M:%S").to_string()
    } else {
        local_time.format("%H:%M:%S").to_string()
    }
}

/// Formats a single snapshot entry within a file group.
///
/// - Uses 4-space indent instead of 2-space
/// - Omits the file_path (shown in group header)
/// - Varies timestamp format based on multi_day flag
/// - Always shows stats (unless pre-migration or delete)
fn format_grouped_entry(snapshot: &Snapshot, use_color: bool, multi_day: bool) -> String {
    let timestamp_str = format_grouped_timestamp(snapshot.timestamp, multi_day);
    let event_type_str = snapshot.event_type.to_string();

    // Pad the raw string first, then wrap with color
    let padded_event = format!("{:<9}", event_type_str);
    let colored_event_type = if use_color {
        let color = match snapshot.event_type {
            EventType::Create => colors::GREEN,
            EventType::Modify => colors::YELLOW,
            EventType::Delete => colors::RED,
        };
        format!("{}{}{}", color, padded_event, colors::RESET)
    } else {
        padded_event
    };

    let size_str = if snapshot.event_type == EventType::Delete {
        String::new()
    } else {
        format!("{:>8}", super::format_size(snapshot.size_bytes))
    };

    let stats_str = if snapshot.event_type == EventType::Delete
        || (snapshot.lines_added == 0 && snapshot.lines_removed == 0 && snapshot.size_bytes > 0)
    {
        String::new()
    } else {
        let added_str = if use_color {
            format!(
                "{}+{}{}",
                colors::GREEN,
                snapshot.lines_added,
                colors::RESET
            )
        } else {
            format!("+{}", snapshot.lines_added)
        };
        let removed_str = if use_color {
            format!(
                "{}-{}{}",
                colors::RED,
                snapshot.lines_removed,
                colors::RESET
            )
        } else {
            format!("-{}", snapshot.lines_removed)
        };
        format!("  {}/{}", added_str, removed_str)
    };

    format!(
        "    {}  {}  {}{}",
        timestamp_str, colored_event_type, size_str, stats_str
    )
}

/// Groups snapshots by file path.
///
/// Contains all snapshots for a single file, sorted chronologically (oldest first).
#[derive(Debug, Clone)]
pub struct FileGroup {
    /// The file path for all snapshots in this group.
    pub path: String,
    /// Snapshots for this file, sorted oldest-first (chronological order).
    pub entries: Vec<Snapshot>,
}

/// Groups a flat list of snapshots by file path.
///
/// Snapshots are organized into `FileGroup` structs, one per unique file.
/// The output is sorted by most-recent activity (newest file first), while
/// entries within each group are sorted chronologically (oldest first).
///
/// # Arguments
/// * `snapshots` - A vector of snapshots to group
///
/// # Returns
/// A vector of `FileGroup` sorted by newest activity descending.
/// Each group's entries are sorted oldest-first.
/// Empty input returns empty output.
///
/// # Examples
/// ```ignore
/// let snaps = vec![snap1, snap2, snap3];
/// let groups = group_by_file(snaps);
/// // groups[0] contains the file with the most recent change
/// // groups[0].entries is sorted chronologically
/// ```
pub fn group_by_file(snapshots: Vec<Snapshot>) -> Vec<FileGroup> {
    use std::collections::BTreeMap;

    if snapshots.is_empty() {
        return Vec::new();
    }

    // Group snapshots by file path
    let mut groups: BTreeMap<String, Vec<Snapshot>> = BTreeMap::new();
    for snapshot in snapshots {
        groups
            .entry(snapshot.file_path.clone())
            .or_default()
            .push(snapshot);
    }

    // Sort entries within each group chronologically (oldest first),
    // then convert to FileGroup structs
    let mut result: Vec<FileGroup> = groups
        .into_iter()
        .map(|(path, mut entries)| {
            entries.sort_by_key(|snap| snap.timestamp);
            FileGroup { path, entries }
        })
        .collect();

    // Sort groups by most recent activity (newest file first)
    result.sort_by(|a, b| {
        let a_newest = a.entries.last().map(|snap| snap.timestamp);
        let b_newest = b.entries.last().map(|snap| snap.timestamp);
        b_newest.cmp(&a_newest) // Descending (newest first)
    });

    result
}

/// Kind of history scope, determined from target argument.
enum ScopeKind {
    All,
    File,
    Directory,
}

/// Determines the scope kind and canonical target string from a target path.
///
/// - `None` → All
/// - Path ending with `/` → Directory
/// - Path where `project_root.join(path).is_dir()` → Directory (appends `/`)
/// - Otherwise → File
fn determine_scope_kind(project_root: &Path, target: Option<&str>) -> (ScopeKind, String) {
    match target {
        None => (ScopeKind::All, String::new()),
        Some(path) => {
            if path.ends_with('/') {
                (ScopeKind::Directory, path.to_string())
            } else if project_root.join(path).is_dir() {
                (ScopeKind::Directory, format!("{}/", path))
            } else {
                (ScopeKind::File, path.to_string())
            }
        }
    }
}

/// Computes diff statistics for a snapshot by comparing it to its previous version.
///
/// Returns `(lines_added, lines_removed)` or an error if content cannot be loaded.
/// Handles all event types:
/// - Create: compares empty to current content
/// - Modify: compares previous to current content
/// - Delete: compares previous to empty
///
/// If hashes are identical, returns (0, 0).
///
/// Note: This function is currently unused as stats are now stored in snapshots directly.
/// Kept for potential future use.
#[allow(dead_code)]
fn compute_snapshot_stats(engine: &Engine, snapshot: &Snapshot) -> Result<(u32, u32), UnfError> {
    // Try to get the previous snapshot
    let previous =
        engine.get_previous_snapshot(&snapshot.file_path, snapshot.timestamp, snapshot.id)?;

    // Determine old and new content based on event type and previous snapshot
    let old_content = match previous {
        Some(prev_snap) => {
            if prev_snap.event_type == EventType::Delete {
                Vec::new()
            } else if prev_snap.content_hash.0 == snapshot.content_hash.0 {
                // Same hash, skip loading
                return Ok((0, 0));
            } else {
                engine.load_content(&prev_snap.content_hash)?
            }
        }
        None => Vec::new(), // No previous snapshot = first snapshot for this file
    };

    let new_content = match snapshot.event_type {
        EventType::Delete => Vec::new(),
        _ => engine.load_content(&snapshot.content_hash)?,
    };

    // Compute diff stats
    let stats = super::diff::compute_diff_stats(&old_content, &new_content);
    Ok((stats.lines_added, stats.lines_removed))
}

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
fn render_grouped_json(_engine: &Engine, groups: Vec<FileGroup>) -> GroupedLogOutput {
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
                    size_human: super::format_size(snapshot.size_bytes),
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
fn render_grouped_human(
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

/// Streams file change history with interactive pagination.
///
/// Stats are always shown from snapshot fields (no computation needed).
///
/// # Arguments
/// * `project_root` - Root directory of the project
/// * `target` - Optional file or directory path to filter by
/// * `since` - Optional time specification (e.g., "5m", "1h", "2d")
/// * `limit` - Maximum entries to return (only used in JSON mode)
/// * `include` - Glob patterns to include (repeatable, OR'd)
/// * `exclude` - Glob patterns to exclude (repeatable, OR'd)
/// * `ignore_case` - Case-insensitive glob matching
/// * `grouped` - Group output by file path (tree view)
/// * `format` - Output format (human or JSON)
/// * `density` - If true, return density histogram instead of entries (JSON only)
/// * `num_buckets` - Number of buckets for density histogram
/// * `cursor_str` - Optional cursor string for pagination (JSON only)
///
/// # Returns
/// `Ok(())` on success, or `UnfError` if querying history fails.
#[allow(clippy::too_many_arguments)]
pub fn run(
    project_root: &Path,
    target: Option<&str>,
    since: Option<&str>,
    limit: u32,
    include: &[String],
    exclude: &[String],
    ignore_case: bool,
    grouped: bool,
    format: OutputFormat,
    density: bool,
    num_buckets: u32,
    cursor_str: Option<&str>,
) -> Result<(), UnfError> {
    // Validate JSON-only flags
    if density && format != OutputFormat::Json {
        return Err(UnfError::InvalidArgument(
            "--density requires --json".to_string(),
        ));
    }
    if cursor_str.is_some() && format != OutputFormat::Json {
        return Err(UnfError::InvalidArgument(
            "--cursor requires --json".to_string(),
        ));
    }

    let storage_dir = storage::resolve_storage_dir(project_root)?;
    let engine = Engine::open(project_root, &storage_dir)?;

    // Create glob filter from include/exclude patterns
    let filter = GlobFilter::new(include, exclude, ignore_case)?;

    // Parse since parameter if provided
    let since_time = if let Some(spec) = since {
        Some(super::parse_time_spec(spec)?)
    } else {
        None
    };

    // Density mode: compute histogram and return early
    if density {
        return run_density(&engine, since_time, &filter, num_buckets);
    }

    // Determine the scope kind and canonical target string.
    // The owned String lives here so HistoryScope can borrow it each iteration.
    let (scope_kind, scope_target) = determine_scope_kind(project_root, target);

    // Parse cursor if provided
    let initial_cursor = match cursor_str {
        Some(s) => Some(parse_cursor(s)?),
        None => None,
    };

    // JSON mode: collect all results up to limit
    if format == OutputFormat::Json {
        let mut all_snapshots = Vec::new();
        let mut cursor: Option<HistoryCursor> = initial_cursor;
        let mut remaining = limit;
        let mut has_more = false;

        loop {
            let scope = match scope_kind {
                ScopeKind::All => HistoryScope::All,
                ScopeKind::File => HistoryScope::File(&scope_target),
                ScopeKind::Directory => HistoryScope::Directory(&scope_target),
            };

            let page_size = std::cmp::min(PAGE_SIZE, remaining);
            let mut page =
                engine.get_history_page(scope, cursor.as_ref(), page_size, since_time)?;

            // Capture raw page info before filtering for cursor advancement
            let raw_page_len = page.len();
            cursor = cursor_from_page(&page);

            // Apply glob filter to the page
            page.retain(|s| filter.matches(&s.file_path));

            if page.is_empty() {
                // If DB returned no results (or fewer than requested), no more data exists
                if raw_page_len == 0 || raw_page_len < page_size as usize {
                    break;
                }
                // Filter removed all results but more pages may have matches
                remaining = limit - all_snapshots.len() as u32;
                if remaining == 0 {
                    has_more = true;
                    break;
                }
                continue;
            }

            for snapshot in page {
                if all_snapshots.len() >= limit as usize {
                    has_more = true;
                    break;
                }
                all_snapshots.push(snapshot);
            }

            if all_snapshots.len() >= limit as usize {
                has_more = true;
                break;
            }

            remaining = limit - all_snapshots.len() as u32;

            if raw_page_len < PAGE_SIZE as usize {
                break;
            }
        }

        let is_empty = all_snapshots.is_empty();

        // Compute next_cursor from the last collected snapshot
        let next_cursor = if has_more {
            all_snapshots.last().map(|s| {
                format_cursor(&HistoryCursor {
                    timestamp: s.timestamp,
                    id: s.id,
                })
            })
        } else {
            None
        };

        if grouped {
            // Grouped JSON output (no cursor support)
            let groups = group_by_file(all_snapshots);
            let output = render_grouped_json(&engine, groups);
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        } else {
            // Paginated flat JSON output
            let entries: Vec<LogEntry> = all_snapshots
                .iter()
                .map(|snapshot| LogEntry {
                    id: snapshot.id.0,
                    file: snapshot.file_path.clone(),
                    event: snapshot.event_type.to_string(),
                    bytes: snapshot.size_bytes,
                    size_human: super::format_size(snapshot.size_bytes),
                    timestamp: snapshot.timestamp.to_rfc3339(),
                    hash: snapshot.content_hash.0.clone(),
                    lines: snapshot.line_count,
                    lines_added: snapshot.lines_added,
                    lines_removed: snapshot.lines_removed,
                })
                .collect();
            let output = PaginatedLogOutput {
                entries,
                next_cursor,
            };
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }

        if is_empty {
            return Err(UnfError::NoResults(String::new()));
        }

        return Ok(());
    }

    // Human mode with grouping: collect all results and render grouped view
    if grouped {
        let mut all_snapshots = Vec::new();
        let mut cursor: Option<HistoryCursor> = None;

        // Collect all snapshots (with a reasonable limit to prevent unbounded memory)
        let max_snapshots = 10000;

        loop {
            let scope = match scope_kind {
                ScopeKind::All => HistoryScope::All,
                ScopeKind::File => HistoryScope::File(&scope_target),
                ScopeKind::Directory => HistoryScope::Directory(&scope_target),
            };

            let page_size = PAGE_SIZE;
            let mut page =
                engine.get_history_page(scope, cursor.as_ref(), page_size, since_time)?;

            // Capture raw page info before filtering for cursor advancement
            let raw_page_len = page.len();
            cursor = cursor_from_page(&page);

            // Apply glob filter to the page
            page.retain(|s| filter.matches(&s.file_path));

            if page.is_empty() {
                // If DB returned no results or fewer than requested, no more data
                if raw_page_len == 0 || raw_page_len < page_size as usize {
                    if all_snapshots.is_empty() {
                        let target_display = if scope_target.is_empty() {
                            "all files".to_string()
                        } else {
                            format!("\"{}\"", scope_target)
                        };
                        return Err(UnfError::NoResults(format!(
                            "No history for {}.",
                            target_display
                        )));
                    }
                    break;
                }
                // Filter removed all results but more pages may have matches
                continue;
            }

            for snapshot in page {
                if all_snapshots.len() >= max_snapshots {
                    break;
                }
                all_snapshots.push(snapshot);
            }

            if all_snapshots.len() >= max_snapshots {
                break;
            }

            // If raw DB page was not full, we've reached the end
            if raw_page_len < PAGE_SIZE as usize {
                break;
            }
        }

        // Group and render
        let groups = group_by_file(all_snapshots);
        let is_tty = io::stdout().is_terminal();
        render_grouped_human(&engine, groups, use_color(), is_tty)?;

        return Ok(());
    }

    // Human mode: interactive pagination (flat view)
    let is_tty = io::stdout().is_terminal();
    let colored = use_color();

    // Streaming pagination loop
    let mut cursor: Option<HistoryCursor> = None;
    let mut displayed_any = false;

    loop {
        let scope = match scope_kind {
            ScopeKind::All => HistoryScope::All,
            ScopeKind::File => HistoryScope::File(&scope_target),
            ScopeKind::Directory => HistoryScope::Directory(&scope_target),
        };
        let mut page = engine.get_history_page(scope, cursor.as_ref(), PAGE_SIZE, since_time)?;

        // Capture raw page info before filtering for cursor advancement
        let raw_page_len = page.len();
        cursor = cursor_from_page(&page);

        // Apply glob filter to the page
        page.retain(|s| filter.matches(&s.file_path));

        if page.is_empty() {
            // If DB returned no results or fewer than requested, no more data
            if raw_page_len == 0 || raw_page_len < PAGE_SIZE as usize {
                if !displayed_any {
                    let target_display = if scope_target.is_empty() {
                        "all files".to_string()
                    } else {
                        format!("\"{}\"", scope_target)
                    };
                    return Err(UnfError::NoResults(format!(
                        "No history for {}.",
                        target_display
                    )));
                }
                println!("-- end --");
                break;
            }
            // Filter removed all results but more pages may have matches
            continue;
        }

        displayed_any = true;

        // Print each snapshot
        for snapshot in &page {
            let line = format_snapshot_line(snapshot, colored);
            println!("{}", line);
        }

        // Check if DB had more pages
        if raw_page_len < PAGE_SIZE as usize {
            println!("-- end --");
            break;
        }

        // If connected to a TTY, prompt for continuation
        if is_tty {
            println!("-- press Enter for more, q to quit --");
            let mut input = String::new();
            io::stdin()
                .read_line(&mut input)
                .map_err(|e| UnfError::InvalidArgument(format!("Failed to read input: {}", e)))?;

            if input.trim().eq_ignore_ascii_case("q") {
                break;
            }
        }
    }

    Ok(())
}

/// Runs density histogram mode.
///
/// Collects all snapshot timestamps, applies glob filtering, computes buckets,
/// and outputs the DensityOutput JSON.
fn run_density(
    engine: &Engine,
    since_time: Option<chrono::DateTime<chrono::Utc>>,
    filter: &GlobFilter,
    num_buckets: u32,
) -> Result<(), UnfError> {
    let all_timestamps = engine.get_all_snapshot_timestamps(since_time)?;

    // Apply glob filter
    let filtered_timestamps: Vec<chrono::DateTime<chrono::Utc>> = all_timestamps
        .into_iter()
        .filter(|(_, path)| filter.matches(path))
        .map(|(ts, _)| ts)
        .collect();

    if filtered_timestamps.is_empty() {
        let output = DensityOutput {
            buckets: vec![],
            total: 0,
            from: String::new(),
            to: String::new(),
        };
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
        return Err(UnfError::NoResults(String::new()));
    }

    let from = *filtered_timestamps.iter().min().unwrap();
    let to = *filtered_timestamps.iter().max().unwrap();
    let buckets = compute_density_buckets(&filtered_timestamps, from, to, num_buckets);

    let output = DensityOutput {
        buckets,
        total: filtered_timestamps.len() as u64,
        from: from.to_rfc3339(),
        to: to.to_rfc3339(),
    };
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
    Ok(())
}

/// Runs density histogram mode across all registered projects.
///
/// Opens engines for all matching projects, collects snapshot timestamps,
/// applies glob filtering, merges them, and passes the unified list to
/// `compute_density_buckets`.
pub fn run_global_density(
    include_project: &[String],
    exclude_project: &[String],
    since: Option<&str>,
    include: &[String],
    exclude: &[String],
    ignore_case: bool,
    num_buckets: u32,
) -> Result<(), UnfError> {
    let filter = GlobFilter::new(include, exclude, ignore_case)?;
    let since_time = if let Some(spec) = since {
        Some(super::parse_time_spec(spec)?)
    } else {
        None
    };

    let projects = resolve_global_projects(include_project, exclude_project)?;

    let mut all_timestamps: Vec<chrono::DateTime<chrono::Utc>> = Vec::new();
    for (project_path, storage_dir) in &projects {
        match Engine::open(project_path, storage_dir) {
            Ok(engine) => match engine.get_all_snapshot_timestamps(since_time) {
                Ok(timestamps) => {
                    let filtered = timestamps
                        .into_iter()
                        .filter(|(_, path)| filter.matches(path))
                        .map(|(ts, _)| ts);
                    all_timestamps.extend(filtered);
                }
                Err(_) => continue,
            },
            Err(_) => continue,
        }
    }

    if all_timestamps.is_empty() {
        let output = DensityOutput {
            buckets: vec![],
            total: 0,
            from: String::new(),
            to: String::new(),
        };
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
        return Err(UnfError::NoResults(String::new()));
    }

    let from = *all_timestamps.iter().min().unwrap();
    let to = *all_timestamps.iter().max().unwrap();
    let buckets = compute_density_buckets(&all_timestamps, from, to, num_buckets);

    let output = DensityOutput {
        buckets,
        total: all_timestamps.len() as u64,
        from: from.to_rfc3339(),
        to: to.to_rfc3339(),
    };
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
    Ok(())
}

// --- Global (cross-project) log types and functions ---

/// A snapshot annotated with its project path for cross-project output.
struct ProjectSnapshot {
    project_path: String,
    snapshot: Snapshot,
}

/// A lazy stream of snapshots from a single project, fetched page-by-page.
struct ProjectStream {
    project_path: String,
    engine: Engine,
    buffer: Vec<Snapshot>,
    index: usize,
    cursor: Option<HistoryCursor>,
    exhausted: bool,
    since: Option<chrono::DateTime<chrono::Utc>>,
    filter: GlobFilter,
}

impl ProjectStream {
    /// Creates a new stream for the given project.
    fn new(
        project_path: String,
        engine: Engine,
        since: Option<chrono::DateTime<chrono::Utc>>,
        filter: GlobFilter,
    ) -> Self {
        ProjectStream {
            project_path,
            engine,
            buffer: Vec::new(),
            index: 0,
            cursor: None,
            exhausted: false,
            since,
            filter,
        }
    }

    /// Peeks at the current snapshot without advancing.
    fn peek(&mut self) -> Result<Option<&Snapshot>, UnfError> {
        // If we have buffered data, return next item
        if self.index < self.buffer.len() {
            return Ok(Some(&self.buffer[self.index]));
        }

        // If exhausted, nothing left
        if self.exhausted {
            return Ok(None);
        }

        // Fetch next page
        self.fetch_next_page()?;

        if self.index < self.buffer.len() {
            Ok(Some(&self.buffer[self.index]))
        } else {
            Ok(None)
        }
    }

    /// Advances past the current snapshot.
    fn advance(&mut self) {
        if self.index < self.buffer.len() {
            self.index += 1;
        }
    }

    /// Fetches the next page of results, applying glob filter.
    fn fetch_next_page(&mut self) -> Result<(), UnfError> {
        loop {
            let page = self.engine.get_history_page(
                HistoryScope::All,
                self.cursor.as_ref(),
                PAGE_SIZE,
                self.since,
            )?;

            let raw_len = page.len();
            self.cursor = cursor_from_page(&page);

            let filtered: Vec<Snapshot> = page
                .into_iter()
                .filter(|s| self.filter.matches(&s.file_path))
                .collect();

            if !filtered.is_empty() {
                self.buffer = filtered;
                self.index = 0;
                return Ok(());
            }

            // No matches in this page
            if raw_len < PAGE_SIZE as usize {
                self.exhausted = true;
                self.buffer.clear();
                self.index = 0;
                return Ok(());
            }
            // Try next page
        }
    }
}

/// JSON output for a single global log entry.
#[derive(serde::Serialize)]
struct GlobalLogEntry {
    project: String,
    #[serde(flatten)]
    entry: LogEntry,
}

/// JSON flat output for global log.
#[derive(serde::Serialize)]
struct GlobalLogOutput {
    entries: Vec<GlobalLogEntry>,
}

/// JSON grouped output for global log (grouped by project, then by file).
#[derive(serde::Serialize)]
struct GlobalGroupedProject {
    project: String,
    files: Vec<GroupedFileEntry>,
}

/// JSON grouped output wrapper with summary.
#[derive(serde::Serialize)]
struct GlobalGroupedOutput {
    projects: Vec<GlobalGroupedProject>,
    summary: GlobalGroupedSummary,
}

/// Summary statistics for global grouped output.
#[derive(serde::Serialize)]
struct GlobalGroupedSummary {
    total_projects: usize,
    total_files: usize,
    total_changes: usize,
}

/// Expands `~/` to `$HOME/` and canonicalizes if the path exists.
fn resolve_filter_path(path: &str) -> String {
    let expanded = if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            format!("{}/{}", home, rest)
        } else {
            path.to_string()
        }
    } else {
        path.to_string()
    };

    // Try to canonicalize, fall back to expanded
    match std::fs::canonicalize(&expanded) {
        Ok(canonical) => canonical.to_string_lossy().to_string(),
        Err(_) => expanded,
    }
}

/// Resolves which projects to include in a global log query.
///
/// Loads the registry, applies include/exclude prefix matching, and returns
/// `(project_path, storage_dir)` pairs for accessible projects.
pub(super) fn resolve_global_projects(
    include: &[String],
    exclude: &[String],
) -> Result<Vec<(PathBuf, PathBuf)>, UnfError> {
    let registry = crate::registry::load()?;

    if registry.projects.is_empty() {
        return Err(UnfError::InvalidArgument(
            "No projects registered. Run `unf watch` in a project directory first.".to_string(),
        ));
    }

    // Resolve filter paths
    let include_resolved: Vec<String> = include.iter().map(|p| resolve_filter_path(p)).collect();
    let exclude_resolved: Vec<String> = exclude.iter().map(|p| resolve_filter_path(p)).collect();

    let mut projects = Vec::new();

    for entry in &registry.projects {
        let path_str = entry.path.to_string_lossy().to_string();

        // Apply include filter (prefix match)
        if !include_resolved.is_empty()
            && !include_resolved.iter().any(|inc| path_str.starts_with(inc))
        {
            continue;
        }

        // Apply exclude filter (prefix match)
        if exclude_resolved.iter().any(|exc| path_str.starts_with(exc)) {
            continue;
        }

        // Resolve storage dir (skip projects with missing storage)
        match storage::resolve_storage_dir_canonical(&entry.path) {
            Ok(storage_dir) if storage_dir.exists() => {
                projects.push((entry.path.clone(), storage_dir));
            }
            _ => continue,
        }
    }

    if projects.is_empty() {
        return Err(UnfError::InvalidArgument(
            "No matching projects found.".to_string(),
        ));
    }

    Ok(projects)
}

/// Runs the global (cross-project) log command.
///
/// Opens engines for all matching projects, performs a k-way merge across
/// their history streams (sorted by timestamp descending), and renders
/// output in the requested format.
#[allow(clippy::too_many_arguments)]
pub fn run_global(
    include_project: &[String],
    exclude_project: &[String],
    since: Option<&str>,
    limit: u32,
    include: &[String],
    exclude: &[String],
    ignore_case: bool,
    grouped: bool,
    format: OutputFormat,
) -> Result<(), UnfError> {
    let filter = GlobFilter::new(include, exclude, ignore_case)?;
    let since_time = if let Some(spec) = since {
        Some(super::parse_time_spec(spec)?)
    } else {
        None
    };

    let projects = resolve_global_projects(include_project, exclude_project)?;

    // Open engines and create streams
    let mut streams: Vec<ProjectStream> = Vec::new();
    for (project_path, storage_dir) in &projects {
        match Engine::open(project_path, storage_dir) {
            Ok(engine) => {
                streams.push(ProjectStream::new(
                    project_path.to_string_lossy().to_string(),
                    engine,
                    since_time,
                    filter.clone(),
                ));
            }
            Err(_) => continue, // Skip projects that can't be opened
        }
    }

    if streams.is_empty() {
        return Err(UnfError::NoResults(
            "No accessible projects found.".to_string(),
        ));
    }

    // K-way merge: collect up to `limit` snapshots, newest first
    let mut collected: Vec<ProjectSnapshot> = Vec::new();
    let effective_limit = if format == OutputFormat::Json {
        limit as usize
    } else {
        10000 // Reasonable cap for human mode
    };

    loop {
        if collected.len() >= effective_limit {
            break;
        }

        // Find the stream with the newest next snapshot
        let mut best_idx: Option<usize> = None;
        let mut best_key: Option<(chrono::DateTime<chrono::Utc>, i64)> = None;

        for (i, stream) in streams.iter_mut().enumerate() {
            if let Some(snap) = stream.peek()? {
                let key = (snap.timestamp, snap.id.0);
                if best_key.is_none() || key > best_key.unwrap() {
                    best_key = Some(key);
                    best_idx = Some(i);
                }
            }
        }

        match best_idx {
            Some(idx) => {
                // We already peeked, so we know there's a snapshot
                let stream = &mut streams[idx];
                let snap = stream.peek()?.unwrap().clone();
                let project_path = stream.project_path.clone();
                stream.advance();

                collected.push(ProjectSnapshot {
                    project_path,
                    snapshot: snap,
                });
            }
            None => break, // All streams exhausted
        }
    }

    if collected.is_empty() {
        return Err(UnfError::NoResults(
            "No changes found across projects.".to_string(),
        ));
    }

    // Render output
    match format {
        OutputFormat::Json => {
            if grouped {
                render_global_grouped_json(&collected);
            } else {
                render_global_flat_json(&collected);
            }
        }
        OutputFormat::Human => {
            let colored = use_color();
            if grouped {
                render_global_grouped_human(&collected, colored)?;
            } else {
                render_global_flat_human(&collected, colored);
            }
        }
    }

    Ok(())
}

/// Renders global log as flat JSON output.
fn render_global_flat_json(snapshots: &[ProjectSnapshot]) {
    let entries: Vec<GlobalLogEntry> = snapshots
        .iter()
        .map(|ps| GlobalLogEntry {
            project: ps.project_path.clone(),
            entry: LogEntry {
                id: ps.snapshot.id.0,
                file: ps.snapshot.file_path.clone(),
                event: ps.snapshot.event_type.to_string(),
                bytes: ps.snapshot.size_bytes,
                size_human: super::format_size(ps.snapshot.size_bytes),
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
fn render_global_grouped_json(snapshots: &[ProjectSnapshot]) {
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
            let groups = group_by_file(snaps);
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
                            size_human: super::format_size(s.size_bytes),
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
fn render_global_flat_human(snapshots: &[ProjectSnapshot], colored: bool) {
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
fn render_global_grouped_human(
    snapshots: &[ProjectSnapshot],
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

        let groups = group_by_file(snaps.clone());
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

// --- Session output types and functions ---

/// Formats a chrono::TimeDelta as a compact duration string.
///
/// # Format
/// - < 60s: "Xs"
/// - < 60m: "Xm"
/// - < 24h: "Xh Xm"
/// - >= 24h: "Xd Xh"
fn format_session_duration(duration: chrono::TimeDelta) -> String {
    let total_secs = duration.num_seconds().max(0) as u64;
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * 60;
    const DAY: u64 = 24 * 60 * 60;

    if total_secs < MINUTE {
        format!("{}s", total_secs)
    } else if total_secs < HOUR {
        let minutes = total_secs / MINUTE;
        format!("{}m", minutes)
    } else if total_secs < DAY {
        let hours = total_secs / HOUR;
        let minutes = (total_secs % HOUR) / MINUTE;
        format!("{}h {}m", hours, minutes)
    } else {
        let days = total_secs / DAY;
        let hours = (total_secs % DAY) / HOUR;
        format!("{}d {}h", days, hours)
    }
}

/// Renders sessions as human-readable output.
fn render_sessions_human(sessions: &[super::session::Session], colored: bool) {
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
fn render_sessions_json(sessions: &[super::session::Session]) {
    let output = super::session::SessionsOutput::from_sessions(sessions);
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
        super::parse_time_spec(spec)?
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
    let sessions = super::session::detect_sessions(&snapshots);

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
        super::parse_time_spec(spec)?
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
    let sessions = super::session::detect_sessions(&all_snapshots);

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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn cursor_from_page_empty() {
        let page: Vec<Snapshot> = vec![];
        assert!(cursor_from_page(&page).is_none());
    }

    #[test]
    fn cursor_from_page_single() {
        let snap = Snapshot {
            id: crate::types::SnapshotId(42),
            file_path: "test.txt".to_string(),
            content_hash: crate::types::ContentHash("abc123".to_string()),
            size_bytes: 100,
            timestamp: Utc::now(),
            event_type: EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let page = vec![snap.clone()];

        let cursor = cursor_from_page(&page).unwrap();
        assert_eq!(cursor.id, snap.id);
        assert_eq!(cursor.timestamp, snap.timestamp);
    }

    #[test]
    fn cursor_from_page_multiple() {
        let now = Utc::now();
        let snap1 = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "test.txt".to_string(),
            content_hash: crate::types::ContentHash("abc".to_string()),
            size_bytes: 100,
            timestamp: now,
            event_type: EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let snap2 = Snapshot {
            id: crate::types::SnapshotId(2),
            file_path: "test.txt".to_string(),
            content_hash: crate::types::ContentHash("def".to_string()),
            size_bytes: 200,
            timestamp: now,
            event_type: EventType::Modify,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let page = vec![snap1, snap2.clone()];

        let cursor = cursor_from_page(&page).unwrap();
        assert_eq!(cursor.id, snap2.id);
        assert_eq!(cursor.timestamp, snap2.timestamp);
    }

    #[test]
    fn format_snapshot_line_created_no_color() {
        let snap = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "src/test.rs".to_string(),
            content_hash: crate::types::ContentHash("abc123".to_string()),
            size_bytes: 1234,
            timestamp: Utc::now(),
            event_type: EventType::Create,
            line_count: 10,
            lines_added: 10,
            lines_removed: 0,
        };

        let line = format_snapshot_line(&snap, false);
        assert!(line.contains("created"));
        assert!(line.contains("src/test.rs"));
        assert!(line.contains("1.2 KB"));
        assert!(line.contains("+10/-0"));
        assert!(!line.contains("\x1b["));
    }

    #[test]
    fn format_snapshot_line_modified_no_color() {
        let snap = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "src/engine.rs".to_string(),
            content_hash: crate::types::ContentHash("def456".to_string()),
            size_bytes: 2048,
            timestamp: Utc::now(),
            event_type: EventType::Modify,
            line_count: 30,
            lines_added: 5,
            lines_removed: 2,
        };

        let line = format_snapshot_line(&snap, false);
        assert!(line.contains("modified"));
        assert!(line.contains("src/engine.rs"));
        assert!(line.contains("2.0 KB"));
        assert!(line.contains("+5/-2"));
    }

    #[test]
    fn format_snapshot_line_deleted_no_color() {
        let snap = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "src/old.rs".to_string(),
            content_hash: crate::types::ContentHash("ghi789".to_string()),
            size_bytes: 500,
            timestamp: Utc::now(),
            event_type: EventType::Delete,
            line_count: 20,
            lines_added: 0,
            lines_removed: 20,
        };

        let line = format_snapshot_line(&snap, false);
        assert!(line.contains("deleted"));
        assert!(line.contains("src/old.rs"));
        assert!(!line.contains("KB")); // Delete events don't show size
        assert!(!line.contains("+")); // Delete events don't show stats
    }

    #[test]
    fn format_snapshot_line_pre_migration_no_stats() {
        let snap = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "src/legacy.rs".to_string(),
            content_hash: crate::types::ContentHash("oldstyle".to_string()),
            size_bytes: 5000,
            timestamp: Utc::now(),
            event_type: EventType::Create,
            line_count: 50,
            lines_added: 0,
            lines_removed: 0,
        };

        let line = format_snapshot_line(&snap, false);
        assert!(line.contains("created"));
        assert!(line.contains("src/legacy.rs"));
        assert!(line.contains("4.9 KB"));
        assert!(!line.contains("+")); // Pre-migration snapshots don't show stats
        assert!(!line.contains("-/-")); // No longer shows cryptic -/-
    }

    #[test]
    fn group_by_file_empty() {
        let snapshots: Vec<Snapshot> = vec![];
        let groups = group_by_file(snapshots);
        assert!(groups.is_empty());
    }

    #[test]
    fn group_by_file_single_file_single_entry() {
        let now = Utc::now();
        let snap = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "test.txt".to_string(),
            content_hash: crate::types::ContentHash("abc".to_string()),
            size_bytes: 100,
            timestamp: now,
            event_type: EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };

        let groups = group_by_file(vec![snap.clone()]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].path, "test.txt");
        assert_eq!(groups[0].entries.len(), 1);
        assert_eq!(groups[0].entries[0], snap);
    }

    #[test]
    fn group_by_file_single_file_multiple_entries_chronological() {
        let base_time = Utc::now();
        let snap1 = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "test.txt".to_string(),
            content_hash: crate::types::ContentHash("abc".to_string()),
            size_bytes: 100,
            timestamp: base_time,
            event_type: EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let snap2 = Snapshot {
            id: crate::types::SnapshotId(2),
            file_path: "test.txt".to_string(),
            content_hash: crate::types::ContentHash("def".to_string()),
            size_bytes: 200,
            timestamp: base_time + chrono::Duration::seconds(10),
            event_type: EventType::Modify,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let snap3 = Snapshot {
            id: crate::types::SnapshotId(3),
            file_path: "test.txt".to_string(),
            content_hash: crate::types::ContentHash("ghi".to_string()),
            size_bytes: 300,
            timestamp: base_time + chrono::Duration::seconds(5),
            event_type: EventType::Modify,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };

        // Input in non-chronological order
        let groups = group_by_file(vec![snap2.clone(), snap1.clone(), snap3.clone()]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].path, "test.txt");
        assert_eq!(groups[0].entries.len(), 3);
        // Entries should be sorted oldest-first
        assert_eq!(groups[0].entries[0], snap1);
        assert_eq!(groups[0].entries[1], snap3);
        assert_eq!(groups[0].entries[2], snap2);
    }

    #[test]
    fn group_by_file_multiple_files_newest_first() {
        let t1 = Utc::now();
        let t2 = t1 + chrono::Duration::seconds(100);
        let t3 = t1 + chrono::Duration::seconds(50);

        let snap_file_a = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "a.txt".to_string(),
            content_hash: crate::types::ContentHash("abc".to_string()),
            size_bytes: 100,
            timestamp: t1,
            event_type: EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let snap_file_b = Snapshot {
            id: crate::types::SnapshotId(2),
            file_path: "b.txt".to_string(),
            content_hash: crate::types::ContentHash("def".to_string()),
            size_bytes: 200,
            timestamp: t2,
            event_type: EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let snap_file_c = Snapshot {
            id: crate::types::SnapshotId(3),
            file_path: "c.txt".to_string(),
            content_hash: crate::types::ContentHash("ghi".to_string()),
            size_bytes: 300,
            timestamp: t3,
            event_type: EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };

        let groups = group_by_file(vec![snap_file_a, snap_file_b.clone(), snap_file_c.clone()]);
        assert_eq!(groups.len(), 3);
        // b.txt has the newest timestamp (t2), so should be first
        assert_eq!(groups[0].path, "b.txt");
        assert_eq!(groups[0].entries[0], snap_file_b);
        // c.txt is second (t3)
        assert_eq!(groups[1].path, "c.txt");
        assert_eq!(groups[1].entries[0], snap_file_c);
        // a.txt is oldest (t1)
        assert_eq!(groups[2].path, "a.txt");
    }

    #[test]
    fn group_by_file_mixed_files_and_entries() {
        let t1 = Utc::now();
        let t2 = t1 + chrono::Duration::seconds(10);
        let t3 = t1 + chrono::Duration::seconds(20);
        let t4 = t1 + chrono::Duration::seconds(30);

        let snap1 = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "a.txt".to_string(),
            content_hash: crate::types::ContentHash("h1".to_string()),
            size_bytes: 100,
            timestamp: t1,
            event_type: EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let snap2 = Snapshot {
            id: crate::types::SnapshotId(2),
            file_path: "b.txt".to_string(),
            content_hash: crate::types::ContentHash("h2".to_string()),
            size_bytes: 200,
            timestamp: t2,
            event_type: EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let snap3 = Snapshot {
            id: crate::types::SnapshotId(3),
            file_path: "a.txt".to_string(),
            content_hash: crate::types::ContentHash("h3".to_string()),
            size_bytes: 150,
            timestamp: t3,
            event_type: EventType::Modify,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let snap4 = Snapshot {
            id: crate::types::SnapshotId(4),
            file_path: "b.txt".to_string(),
            content_hash: crate::types::ContentHash("h4".to_string()),
            size_bytes: 250,
            timestamp: t4,
            event_type: EventType::Modify,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };

        let groups = group_by_file(vec![
            snap1.clone(),
            snap2.clone(),
            snap3.clone(),
            snap4.clone(),
        ]);
        assert_eq!(groups.len(), 2);

        // b.txt has newest activity (t4), so first
        assert_eq!(groups[0].path, "b.txt");
        assert_eq!(groups[0].entries.len(), 2);
        assert_eq!(groups[0].entries[0], snap2); // chronological (older)
        assert_eq!(groups[0].entries[1], snap4); // chronological (newer)

        // a.txt has older newest activity (t3), so second
        assert_eq!(groups[1].path, "a.txt");
        assert_eq!(groups[1].entries.len(), 2);
        assert_eq!(groups[1].entries[0], snap1); // chronological (older)
        assert_eq!(groups[1].entries[1], snap3); // chronological (newer)
    }

    // --- Density bucket tests ---

    #[test]
    fn density_empty_timestamps() {
        let from = Utc::now();
        let to = from + chrono::Duration::hours(1);
        let buckets = compute_density_buckets(&[], from, to, 10);
        assert!(buckets.is_empty());
    }

    #[test]
    fn density_zero_buckets() {
        let now = Utc::now();
        let buckets = compute_density_buckets(&[now], now, now, 0);
        assert!(buckets.is_empty());
    }

    #[test]
    fn density_single_timestamp_single_bucket() {
        let now = Utc::now();
        let buckets = compute_density_buckets(&[now], now, now, 1);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].count, 1);
    }

    #[test]
    fn density_even_distribution() {
        use chrono::TimeZone;
        let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        // 4 timestamps: at 0h, 1h, 2h, 3h
        let timestamps: Vec<_> = (0..4).map(|i| base + chrono::Duration::hours(i)).collect();
        let from = base;
        let to = base + chrono::Duration::hours(3);
        let buckets = compute_density_buckets(&timestamps, from, to, 3);
        assert_eq!(buckets.len(), 3);
        // Each bucket should have at least 1 timestamp
        assert_eq!(buckets[0].count, 1); // 0h
        assert_eq!(buckets[1].count, 1); // 1h
        assert_eq!(buckets[2].count, 2); // 2h and 3h (3h falls in last bucket)
    }

    #[test]
    fn density_all_in_one_bucket() {
        use chrono::TimeZone;
        let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let timestamps = vec![base, base, base];
        let buckets = compute_density_buckets(&timestamps, base, base, 5);
        assert_eq!(buckets.len(), 5);
        // All timestamps at the same instant go into first bucket
        assert_eq!(buckets[0].count, 3);
        for b in &buckets[1..] {
            assert_eq!(b.count, 0);
        }
    }

    // --- Cursor parsing/formatting tests ---

    #[test]
    fn parse_cursor_valid_utc() {
        let cursor = parse_cursor("2026-02-12T14:32:07+00:00:42").unwrap();
        assert_eq!(cursor.id, crate::types::SnapshotId(42));
        assert_eq!(
            cursor.timestamp,
            chrono::DateTime::parse_from_rfc3339("2026-02-12T14:32:07+00:00")
                .unwrap()
                .with_timezone(&Utc)
        );
    }

    #[test]
    fn parse_cursor_valid_z() {
        let cursor = parse_cursor("2026-02-12T14:32:07Z:100").unwrap();
        assert_eq!(cursor.id, crate::types::SnapshotId(100));
    }

    #[test]
    fn parse_cursor_valid_offset() {
        let cursor = parse_cursor("2026-02-12T14:32:07+05:30:7").unwrap();
        assert_eq!(cursor.id, crate::types::SnapshotId(7));
    }

    #[test]
    fn parse_cursor_invalid_no_colon() {
        assert!(parse_cursor("invalid").is_err());
    }

    #[test]
    fn parse_cursor_invalid_bad_timestamp() {
        assert!(parse_cursor("not-a-date:42").is_err());
    }

    #[test]
    fn parse_cursor_invalid_bad_id() {
        assert!(parse_cursor("2026-02-12T14:32:07Z:abc").is_err());
    }

    #[test]
    fn format_cursor_roundtrip() {
        use chrono::TimeZone;
        let cursor = HistoryCursor {
            timestamp: Utc.with_ymd_and_hms(2026, 2, 12, 14, 32, 7).unwrap(),
            id: crate::types::SnapshotId(42),
        };
        let formatted = format_cursor(&cursor);
        let parsed = parse_cursor(&formatted).unwrap();
        assert_eq!(parsed.timestamp, cursor.timestamp);
        assert_eq!(parsed.id, cursor.id);
    }

    // --- resolve_filter_path tests ---

    #[test]
    fn resolve_filter_path_tilde_expansion() {
        let home = std::env::var("HOME").unwrap();
        let result = resolve_filter_path("~/.claude");
        assert!(result.starts_with(&home));
        assert!(result.contains(".claude"));
    }

    #[test]
    fn resolve_filter_path_absolute_passthrough() {
        let result = resolve_filter_path("/tmp/nonexistent_test_path_12345");
        assert_eq!(result, "/tmp/nonexistent_test_path_12345");
    }

    #[test]
    fn resolve_filter_path_existing_canonicalized() {
        // /tmp should exist and canonicalize to /private/tmp on macOS
        let result = resolve_filter_path("/tmp");
        // On macOS, /tmp -> /private/tmp
        assert!(result == "/tmp" || result == "/private/tmp");
    }
}
