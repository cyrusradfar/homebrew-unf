//! Command-line interface modules for the UNFUDGED flight recorder.
//!
//! Each module implements a single CLI command (status, log, diff, etc.)
//! following the SUPER principle: pure logic at the core, side effects
//! at the boundaries.

pub mod boot;
pub mod cat;
pub mod diff;
pub mod filter;
pub mod init;
pub mod list;
pub mod log;
pub mod output;
pub mod prune;
pub mod recap;
pub mod restart;
pub mod restore;
pub mod session;
pub mod status;
pub mod stop;
pub mod unwatch;
pub mod watch;

// Shared utility functions for CLI modules
use chrono::{DateTime, Local, Utc};

/// Controls output format for CLI commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
}

/// Formats a UTC timestamp as a local time string for display.
///
/// # Examples
///
/// ```text
/// 2025-02-09 14:30:45
/// ```
pub fn format_local_time(utc_time: DateTime<Utc>) -> String {
    let local_time = utc_time.with_timezone(&Local);
    local_time.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Parses a time specification into a DateTime in UTC.
///
/// Accepts two formats:
/// 1. Relative durations: "5m" (minutes), "2h" (hours), "1d" (days) — returns (now - duration)
/// 2. Absolute ISO 8601/RFC 3339 timestamps: "2026-02-09T20:17:00Z" or "2026-02-09T15:17:00-05:00"
///
/// # Arguments
/// * `spec` - Time specification (e.g., "5m", "2h", "2026-02-09T20:17:00Z")
///
/// # Returns
/// A DateTime in UTC
///
/// # Errors
/// Returns `UnfError::InvalidArgument` if the spec is malformed.
pub fn parse_time_spec(spec: &str) -> Result<DateTime<Utc>, crate::error::UnfError> {
    // Try absolute ISO 8601 / RFC 3339 first if it looks like a timestamp
    // Check for 'T' (ISO format) or starts with 4 digits followed by '-' (YYYY-MM-DD...)
    let looks_like_timestamp = spec.contains('T')
        || (spec.len() >= 5
            && spec[..4].chars().all(|c| c.is_ascii_digit())
            && spec.as_bytes().get(4) == Some(&b'-'));

    if looks_like_timestamp {
        match DateTime::parse_from_rfc3339(spec) {
            Ok(dt) => return Ok(dt.with_timezone(&Utc)),
            Err(_) => {
                return Err(crate::error::UnfError::InvalidArgument(format!(
                    "Invalid timestamp: \"{}\". Expected format: 2026-02-09T20:17:00Z",
                    spec
                )));
            }
        }
    }

    // Fall through to relative duration parsing
    let (value_str, unit) = if let Some(value_str) = spec.strip_suffix('s') {
        (value_str, 's')
    } else if let Some(value_str) = spec.strip_suffix('m') {
        (value_str, 'm')
    } else if let Some(value_str) = spec.strip_suffix('h') {
        (value_str, 'h')
    } else if let Some(value_str) = spec.strip_suffix('d') {
        (value_str, 'd')
    } else {
        return Err(crate::error::UnfError::InvalidArgument(
            "Time spec must be a relative duration (e.g., '30s', '5m', '2h', '1d') or an ISO 8601 timestamp (e.g., '2026-02-09T20:17:00Z')".to_string(),
        ));
    };

    let value: i64 = value_str.parse().map_err(|_| {
        crate::error::UnfError::InvalidArgument(format!("Invalid time value: {}", value_str))
    })?;

    let secs = match unit {
        's' => value,
        'm' => value * 60,
        'h' => value * 60 * 60,
        'd' => value * 24 * 60 * 60,
        _ => unreachable!(),
    };

    let now = Utc::now();
    let target_time = now - chrono::Duration::seconds(secs);
    Ok(target_time)
}

/// Formats a chrono::TimeDelta as a human-readable duration string (without "ago").
///
/// # Examples
///
/// ```text
/// 30 seconds
/// 5 minutes
/// 2 hours
/// 3 days
/// ```
pub fn format_duration(duration: chrono::TimeDelta) -> String {
    let total_secs = duration.num_seconds().max(0) as u64;
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * 60;
    const DAY: u64 = 24 * 60 * 60;

    if total_secs < MINUTE {
        format!("{} seconds", total_secs)
    } else if total_secs < HOUR {
        let minutes = total_secs / MINUTE;
        let plural = if minutes == 1 { "" } else { "s" };
        format!("{} minute{}", minutes, plural)
    } else if total_secs < DAY {
        let hours = total_secs / HOUR;
        let plural = if hours == 1 { "" } else { "s" };
        format!("{} hour{}", hours, plural)
    } else {
        let days = total_secs / DAY;
        let plural = if days == 1 { "" } else { "s" };
        format!("{} day{}", days, plural)
    }
}

/// Formats a chrono::TimeDelta as a human-readable "ago" string.
///
/// # Examples
///
/// ```text
/// 30 seconds ago
/// 5 minutes ago
/// 2 hours ago
/// 3 days ago
/// ```
pub fn format_duration_ago(duration: chrono::TimeDelta) -> String {
    let total_secs = duration.num_seconds().max(0) as u64;
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * 60;
    const DAY: u64 = 24 * 60 * 60;

    if total_secs < MINUTE {
        format!("{} seconds ago", total_secs)
    } else if total_secs < HOUR {
        let minutes = total_secs / MINUTE;
        let plural = if minutes == 1 { "" } else { "s" };
        format!("{} minute{} ago", minutes, plural)
    } else if total_secs < DAY {
        let hours = total_secs / HOUR;
        let plural = if hours == 1 { "" } else { "s" };
        format!("{} hour{} ago", hours, plural)
    } else {
        let days = total_secs / DAY;
        let plural = if days == 1 { "" } else { "s" };
        format!("{} day{} ago", days, plural)
    }
}

/// Formats a byte count as a human-readable size string.
///
/// Converts bytes to KB, MB, or GB with one decimal place.
/// Values below 1024 are shown as raw bytes.
///
/// # Examples
///
/// ```text
/// 512 -> "512 B"
/// 2048 -> "2.0 KB"
/// 5242880 -> "5.0 MB"
/// 1073741824 -> "1.0 GB"
/// ```
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;

    if bytes < KB {
        format!("{} B", bytes)
    } else if bytes < MB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else if bytes < GB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;
    use chrono::Timelike;

    #[test]
    fn parse_time_spec_iso8601_utc() {
        let result = parse_time_spec("2026-02-09T20:17:00Z");
        assert!(result.is_ok());
        let dt = result.unwrap();
        assert_eq!(dt.year(), 2026);
        assert_eq!(dt.month(), 2);
        assert_eq!(dt.day(), 9);
        assert_eq!(dt.hour(), 20);
        assert_eq!(dt.minute(), 17);
    }

    #[test]
    fn parse_time_spec_iso8601_with_offset() {
        let result = parse_time_spec("2026-02-09T15:17:00-05:00");
        assert!(result.is_ok());
        let dt = result.unwrap();
        // -05:00 offset means 20:17 UTC
        assert_eq!(dt.hour(), 20);
        assert_eq!(dt.minute(), 17);
    }

    #[test]
    fn parse_time_spec_invalid_iso8601() {
        let result = parse_time_spec("2026-02-09T99:99:99Z");
        assert!(result.is_err());
    }

    #[test]
    fn parse_time_spec_relative_still_works() {
        // Ensure existing relative specs still work
        assert!(parse_time_spec("30s").is_ok());
        assert!(parse_time_spec("5m").is_ok());
        assert!(parse_time_spec("1h").is_ok());
        assert!(parse_time_spec("2d").is_ok());
    }

    #[test]
    fn parse_time_spec_seconds() {
        let result = parse_time_spec("30s");
        assert!(result.is_ok());
        let dt = result.unwrap();
        let now = Utc::now();
        let diff = now.signed_duration_since(dt).num_seconds();
        // Should be approximately 30 seconds ago (allow 2s tolerance)
        assert!(diff >= 28 && diff <= 32, "Expected ~30s ago, got {}s", diff);
    }

    #[test]
    fn parse_time_spec_invalid_relative() {
        assert!(parse_time_spec("abc").is_err());
        assert!(parse_time_spec("5x").is_err());
    }

    #[test]
    fn format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn format_size_kilobytes() {
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(2048), "2.0 KB");
    }

    #[test]
    fn format_size_megabytes() {
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn format_size_gigabytes() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
    }
}
