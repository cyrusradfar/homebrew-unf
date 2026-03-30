//! Formatting helpers for timestamps, cursors, and display output.

use super::super::output::colors;
use crate::engine::db::HistoryCursor;
use crate::error::UnfError;
use crate::types::{EventType, Snapshot};

/// Extracts a cursor from the last snapshot in a page.
///
/// Returns `None` if the page is empty.
pub(super) fn cursor_from_page(page: &[Snapshot]) -> Option<HistoryCursor> {
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
pub fn compute_density_buckets(
    timestamps: &[chrono::DateTime<chrono::Utc>],
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    num_buckets: u32,
) -> Vec<super::DensityBucket> {
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
            super::DensityBucket {
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
pub fn parse_cursor(s: &str) -> Result<HistoryCursor, UnfError> {
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
pub fn format_cursor(cursor: &HistoryCursor) -> String {
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
pub(super) fn format_snapshot_line(snapshot: &Snapshot, use_color: bool) -> String {
    let local_time = super::super::format_local_time(snapshot.timestamp);
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
        format!("{:>8}", super::super::format_size(snapshot.size_bytes))
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
pub(super) fn spans_multiple_days(group: &super::FileGroup) -> bool {
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
pub(super) fn format_grouped_timestamp(
    utc_time: chrono::DateTime<chrono::Utc>,
    multi_day: bool,
) -> String {
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
pub(super) fn format_grouped_entry(
    snapshot: &Snapshot,
    use_color: bool,
    multi_day: bool,
) -> String {
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
        format!("{:>8}", super::super::format_size(snapshot.size_bytes))
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

/// Formats a chrono::TimeDelta as a compact duration string.
///
/// # Format
/// - < 60s: "Xs"
/// - < 60m: "Xm"
/// - < 24h: "Xh Xm"
/// - >= 24h: "Xd Xh"
pub fn format_session_duration(duration: chrono::TimeDelta) -> String {
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
