//! Command-line interface modules for the UNFUDGED flight recorder.
//!
//! Each module implements a single CLI command (status, log, diff, etc.)
//! following the SUPER principle: pure logic at the core, side effects
//! at the boundaries.

pub mod boot;
pub mod cat;
pub mod config;
pub mod diff;
pub mod filter;
pub mod init;
pub mod list;
pub mod log;
pub mod migrate;
pub mod output;
pub mod prune;
pub mod recap;
pub mod restart;
pub mod restore;
pub mod session;
pub mod status;
pub mod stop;
mod time;
pub mod unwatch;
pub mod watch;

// Re-exported so every call site stays `cli::parse_time_spec` and
// `cli::format_local_time`.
pub use time::{format_local_time, parse_time_spec};

/// Controls output format for CLI commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
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
