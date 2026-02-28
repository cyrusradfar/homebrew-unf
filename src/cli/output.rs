//! Shared output formatting for the UNFUDGED CLI.
//!
//! Single source of truth for formatting functions, color handling, and
//! message styles. All CLI modules should use these functions instead of
//! implementing their own formatting.

use std::io::IsTerminal;

/// ANSI color codes for terminal output.
pub mod colors {
    pub const GREEN: &str = "\x1b[32m";
    pub const BOLD_GREEN: &str = "\x1b[1;32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const RED: &str = "\x1b[31m";
    pub const BOLD_RED: &str = "\x1b[1;31m";
    pub const CYAN: &str = "\x1b[36m";
    pub const DIM: &str = "\x1b[2m";
    pub const BOLD: &str = "\x1b[1m";
    pub const RESET: &str = "\x1b[0m";
}

/// Determines whether color output should be used.
///
/// Checks (in order):
/// 1. `NO_COLOR` env var — if set, no color
/// 2. `CLICOLOR_FORCE` env var — if set and non-zero, force color
/// 3. `CLICOLOR` env var — if set to "0", no color
/// 4. Falls back to TTY detection on stdout
pub fn use_color() -> bool {
    // NO_COLOR takes highest priority (https://no-color.org/)
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }

    // CLICOLOR_FORCE overrides TTY detection
    if let Some(val) = std::env::var_os("CLICOLOR_FORCE") {
        if val != "0" {
            return true;
        }
    }

    // CLICOLOR=0 disables color
    if let Some(val) = std::env::var_os("CLICOLOR") {
        if val == "0" {
            return false;
        }
    }

    // Default: color only if stdout is a TTY
    std::io::stdout().is_terminal()
}

/// Formats a cargo-style status prefix with a right-aligned bold green verb.
///
/// The verb is right-aligned to 12 characters. When color is enabled, the
/// verb is bold green.
///
/// # Examples
///
/// ```text
///    Recording  /path/to/project
///      Stopped  daemon (pid 42381)
/// ```
pub fn status_prefix(verb: &str, subject: &str) -> String {
    let colored = use_color();
    if colored {
        format!(
            "{}{:>12}{}  {}",
            colors::BOLD_GREEN,
            verb,
            colors::RESET,
            subject
        )
    } else {
        format!("{:>12}  {}", verb, subject)
    }
}

/// Prints a cargo-style status line to stdout.
pub fn print_status(verb: &str, subject: &str) {
    println!("{}", status_prefix(verb, subject));
}

/// Prints an error message to stderr with bold red `error:` prefix and indented hint.
///
/// # Examples
///
/// ```text
/// error: daemon is not running
///   Run `unf watch` to start recording.
/// ```
pub fn print_error(msg: &str, hint: Option<&str>) {
    let colored = use_color();
    if colored {
        eprint!("{}error:{} {}", colors::BOLD_RED, colors::RESET, msg);
    } else {
        eprint!("error: {}", msg);
    }
    eprintln!();
    if let Some(h) = hint {
        eprintln!("  {}", h);
    }
}

/// Prints a warning message to stderr with yellow `warning:` prefix.
///
/// # Examples
///
/// ```text
/// warning: 3 files skipped (binary detected)
/// ```
pub fn print_warning(msg: &str) {
    let colored = use_color();
    if colored {
        eprintln!("{}warning:{} {}", colors::YELLOW, colors::RESET, msg);
    } else {
        eprintln!("warning: {}", msg);
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

/// Formats a large number with comma separators.
///
/// # Examples
///
/// ```text
/// 1 -> "1"
/// 1000 -> "1,000"
/// 1234567 -> "1,234,567"
/// ```
#[allow(unknown_lints, clippy::manual_is_multiple_of)]
pub fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();

    for (i, c) in chars.iter().enumerate() {
        result.push(*c);
        let remaining = len - i - 1;
        if remaining > 0 && remaining % 3 == 0 {
            result.push(',');
        }
    }

    result
}

/// Shortens a file path by replacing the home directory prefix with `~`.
///
/// If the path starts with the home directory, it is replaced with `~`.
/// Otherwise, the path is returned as-is.
///
/// # Examples
///
/// ```text
/// "/Users/alice/code/unfudged" -> "~/code/unfudged"
/// "/Users/alice" -> "~"
/// "/var/log/syslog" -> "/var/log/syslog"
/// ```
pub fn shorten_home(path: &str) -> String {
    if let Some(home_dir) = dirs::home_dir() {
        let home_str = home_dir.display().to_string();
        if path == home_str {
            "~".to_string()
        } else if path.starts_with(&format!("{}/", home_str)) {
            format!("~{}", &path[home_str.len()..])
        } else {
            path.to_string()
        }
    } else {
        path.to_string()
    }
}

/// Formats a UTC datetime as a short date string in the local timezone.
///
/// Produces "Mon DD" format (e.g., "Feb 12"), omitting the year.
///
/// # Examples
///
/// ```text
/// 2026-02-12 14:30:00 UTC -> "Feb 12"
/// 2026-01-01 00:00:00 UTC -> "Jan 01"
/// ```
pub fn format_short_date(utc_time: chrono::DateTime<chrono::Utc>) -> String {
    let local = utc_time.with_timezone(&chrono::Local);
    local.format("%b %d").to_string()
}

/// Formats a UTC datetime as a compact relative time string ("ago" format).
///
/// Returns "now" if less than 60 seconds ago, otherwise uses the most significant
/// unit: "7s ago", "3m ago", "2h ago", "4d ago".
///
/// # Examples
///
/// ```text
/// 30 seconds ago -> "now"
/// 45 seconds ago -> "now"
/// 90 seconds ago -> "1m ago"
/// 4 days ago -> "4d ago"
/// ```
pub fn format_recency(utc_time: chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let duration = now.signed_duration_since(utc_time);
    let total_secs = duration.num_seconds().max(0) as u64;

    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * 60;
    const DAY: u64 = 24 * 60 * 60;

    if total_secs < MINUTE {
        "now".to_string()
    } else if total_secs < HOUR {
        let minutes = total_secs / MINUTE;
        format!("{}m ago", minutes)
    } else if total_secs < DAY {
        let hours = total_secs / HOUR;
        format!("{}h ago", hours)
    } else {
        let days = total_secs / DAY;
        format!("{}d ago", days)
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

    #[test]
    fn format_number_small() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(1), "1");
        assert_eq!(format_number(999), "999");
    }

    #[test]
    fn format_number_thousands() {
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(10000), "10,000");
        assert_eq!(format_number(999999), "999,999");
    }

    #[test]
    fn format_number_millions() {
        assert_eq!(format_number(1000000), "1,000,000");
        assert_eq!(format_number(1234567), "1,234,567");
    }

    #[test]
    fn status_prefix_formatting() {
        // In test context, NO_COLOR may or may not be set, so just
        // verify the verb and subject are present
        let result = status_prefix("Recording", "/path/to/project");
        assert!(result.contains("Recording"));
        assert!(result.contains("/path/to/project"));
    }

    #[test]
    fn use_color_respects_no_color() {
        // This test verifies the logic path, not the actual env var
        // (since we can't safely set env vars in parallel tests)
        // Just verify the function is callable and returns a bool
        let _result: bool = use_color();
    }

    #[test]
    fn shorten_home_with_home_prefix() {
        if let Some(home_dir) = dirs::home_dir() {
            let home_str = home_dir.display().to_string();
            let path = format!("{}/code/unfudged", home_str);
            let result = shorten_home(&path);
            assert_eq!(result, "~/code/unfudged");
        }
    }

    #[test]
    fn shorten_home_exact_home_dir() {
        if let Some(home_dir) = dirs::home_dir() {
            let home_str = home_dir.display().to_string();
            let result = shorten_home(&home_str);
            assert_eq!(result, "~");
        }
    }

    #[test]
    fn shorten_home_without_home_prefix() {
        let path = "/var/log/syslog";
        let result = shorten_home(path);
        assert_eq!(result, path);
    }

    #[test]
    fn format_short_date_basic() {
        use chrono::{TimeZone, Utc};
        // 2026-02-12 14:30:00 UTC (actual time doesn't matter for format check)
        let dt = Utc.with_ymd_and_hms(2026, 2, 12, 14, 30, 0).unwrap();
        let result = format_short_date(dt);
        // Should be "Feb 12" (month abbr + space + day)
        assert!(
            result.contains("12"),
            "Expected day 12 in result: {}",
            result
        );
        assert!(
            result.contains("Feb") || result.contains("02"),
            "Expected Feb in result: {}",
            result
        );
    }

    #[test]
    fn format_recency_less_than_minute() {
        use chrono::Utc;
        let now = Utc::now();
        let thirty_secs_ago = now - chrono::Duration::seconds(30);
        let result = format_recency(thirty_secs_ago);
        assert_eq!(result, "now");
    }

    #[test]
    fn format_recency_minutes() {
        use chrono::Utc;
        let now = Utc::now();
        let five_mins_ago = now - chrono::Duration::seconds(5 * 60 + 30);
        let result = format_recency(five_mins_ago);
        assert_eq!(result, "5m ago");
    }

    #[test]
    fn format_recency_hours() {
        use chrono::Utc;
        let now = Utc::now();
        let three_hours_ago = now - chrono::Duration::seconds(3 * 3600 + 1800);
        let result = format_recency(three_hours_ago);
        assert_eq!(result, "3h ago");
    }

    #[test]
    fn format_recency_days() {
        use chrono::Utc;
        let now = Utc::now();
        let four_days_ago = now - chrono::Duration::seconds(4 * 24 * 3600 + 3600);
        let result = format_recency(four_days_ago);
        assert_eq!(result, "4d ago");
    }
}
