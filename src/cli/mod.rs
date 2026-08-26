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
pub mod unwatch;
pub mod watch;

// Shared utility functions for CLI modules
use chrono::{DateTime, FixedOffset, Local, LocalResult, NaiveDateTime, TimeZone, Utc};

/// Controls output format for CLI commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
}

/// Renders an instant that already carries its display offset.
///
/// Pure: the offset is supplied by the caller, so no ambient process state is
/// read. `%z` always emits exactly five characters (`+0000`, `-0600`, `+0545`),
/// never `Z`, so the result is always 25 characters wide.
///
/// # Examples
///
/// ```text
/// 2025-02-09 14:30:45 -0600
/// ```
fn format_offset_time(t: DateTime<FixedOffset>) -> String {
    t.format("%Y-%m-%d %H:%M:%S %z").to_string()
}

/// Formats a UTC timestamp as a local time string for display.
///
/// Edge: reads the process timezone via `Local`. The rendering itself lives in
/// the pure [`format_offset_time`].
///
/// # Examples
///
/// ```text
/// 2025-02-09 14:30:45 -0600
/// ```
pub fn format_local_time(utc_time: DateTime<Utc>) -> String {
    format_offset_time(utc_time.with_timezone(&Local).fixed_offset())
}

/// Parses a time specification that carries its own UTC offset.
///
/// Pure: no clock and no timezone are consulted — the offset is in the input.
///
/// Two forms are accepted, in this order:
/// 1. RFC 3339 (`2026-02-09T20:17:00Z`, `2026-02-09T15:17:00-05:00`, fractional
///    seconds included).
/// 2. The display form that `unf log` prints: `2026-02-09 14:17:03 -0600`.
///
/// Chrono's `%z` is more permissive on parse than on format: it accepts
/// `-0600`, `-06:00` and `-06 00` alike, so a user who retypes the offset with
/// a colon is still understood.
///
/// Returns `None` when neither pattern matches the whole input.
fn parse_offset_time(spec: &str) -> Option<DateTime<FixedOffset>> {
    const OFFSET_DISPLAY_FORMAT: &str = "%Y-%m-%d %H:%M:%S %z";

    DateTime::parse_from_rfc3339(spec)
        .or_else(|_| DateTime::parse_from_str(spec, OFFSET_DISPLAY_FORMAT))
        .ok()
}

/// Parses a time specification that names a wall-clock time with no offset.
///
/// Pure: no timezone is consulted. The caller decides what zone the result
/// belongs to.
///
/// Both separators are accepted at second precision:
/// `2026-02-09 14:17:03` and `2026-02-09T14:17:03`. Anything coarser
/// (date-only, `HH:MM`) is deliberately rejected — a restore target must name
/// the second it means.
///
/// Returns `None` when no pattern matches the whole input.
fn parse_naive_local(spec: &str) -> Option<NaiveDateTime> {
    const NAIVE_FORMATS: [&str; 2] = ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S"];

    NAIVE_FORMATS
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(spec, format).ok())
}

/// Resolves a local wall-clock time against the answer a timezone gave for it.
///
/// Pure: it takes the timezone's *answer*, not the timezone, so every
/// daylight-saving outcome is reachable in a test without a zone database.
///
/// * `Single` — one instant exists; use it.
/// * `Ambiguous` — the clock fell back and the wall time happened twice. Use
///   the earlier instant: for a recovery tool, reaching further back is the
///   safe direction, since the later instant's content is still ahead of it.
/// * `None` — the clock sprang forward and the wall time never existed. There
///   is no instant to pick, so this is an error that names two nearby times
///   that do exist.
fn resolve_local(
    spec: &str,
    mapped: LocalResult<DateTime<FixedOffset>>,
) -> Result<DateTime<Utc>, crate::error::UnfError> {
    match mapped {
        LocalResult::Single(t) => Ok(t.with_timezone(&Utc)),
        // Earliest of the two. Never `.single()` or `.unwrap()`: a panic on a
        // clock edge in a recovery tool would recur twice a year, per user.
        LocalResult::Ambiguous(earlier, _later) => Ok(earlier.with_timezone(&Utc)),
        LocalResult::None => Err(crate::error::UnfError::InvalidArgument(gap_message(spec))),
    }
}

/// Builds the error text for a local time that a daylight-saving jump skipped.
///
/// Pure. The two suggestions are the requested time shifted one hour each way.
/// Every real spring-forward gap is one hour or shorter, so both suggestions
/// land outside it.
fn gap_message(spec: &str) -> String {
    const NEARBY: &str = "%Y-%m-%d %H:%M:%S";

    let nearby = parse_naive_local(spec).map(|naive| {
        let step = chrono::Duration::hours(1);
        (
            (naive - step).format(NEARBY).to_string(),
            (naive + step).format(NEARBY).to_string(),
        )
    });

    match nearby {
        Some((before, after)) => format!(
            "\"{spec}\" is not a valid local time — a daylight-saving change skips it. \
             Use \"{before}\" or \"{after}\", or add an explicit offset."
        ),
        None => format!(
            "\"{spec}\" is not a valid local time — a daylight-saving change skips it. \
             Use a time an hour earlier or an hour later, or add an explicit offset."
        ),
    }
}

/// Parses a relative duration spec such as `30s`, `5m`, `2h` or `1d`.
///
/// Pure: `now` is supplied by the caller, so the result is exact and testable.
///
/// # Errors
/// Returns `UnfError::InvalidArgument` if the suffix or the value is malformed.
fn parse_relative(spec: &str, now: DateTime<Utc>) -> Result<DateTime<Utc>, crate::error::UnfError> {
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

    Ok(now - chrono::Duration::seconds(secs))
}

/// Parses a time specification into a DateTime in UTC.
///
/// Edge: the only place that reads the clock (`Utc::now`) and the process
/// timezone (`Local`). All decisions live in the pure helpers above.
///
/// Accepted forms, first match wins:
/// 1. Relative durations: `30s`, `5m`, `2h`, `1d` — read as (now - duration).
/// 2. RFC 3339: `2026-02-09T20:17:00Z`, `2026-02-09T15:17:00-05:00`.
/// 3. The `unf log` display form: `2026-02-09 14:17:03 -0600`.
/// 4. Offset-less wall-clock time: `2026-02-09 14:17:03` or
///    `2026-02-09T14:17:03` — read as **local** time.
///
/// Offset-bearing forms are attempted before offset-less ones, so no existing
/// input can change meaning.
///
/// # Arguments
/// * `spec` - Time specification (e.g., "5m", "2026-02-09T20:17:00Z")
///
/// # Returns
/// A DateTime in UTC
///
/// # Errors
/// Returns `UnfError::InvalidArgument` if the spec is malformed, or if it names
/// a local time that a daylight-saving change skipped.
pub fn parse_time_spec(spec: &str) -> Result<DateTime<Utc>, crate::error::UnfError> {
    // Try absolute ISO 8601 / RFC 3339 first if it looks like a timestamp
    // Check for 'T' (ISO format) or starts with 4 digits followed by '-' (YYYY-MM-DD...)
    let looks_like_timestamp = spec.contains('T')
        || (spec.len() >= 5
            && spec[..4].chars().all(|c| c.is_ascii_digit())
            && spec.as_bytes().get(4) == Some(&b'-'));

    if looks_like_timestamp {
        if let Some(dt) = parse_offset_time(spec) {
            return Ok(dt.with_timezone(&Utc));
        }

        if let Some(naive) = parse_naive_local(spec) {
            let mapped = Local.from_local_datetime(&naive).map(|t| t.fixed_offset());
            return resolve_local(spec, mapped);
        }

        return Err(crate::error::UnfError::InvalidArgument(format!(
            "Invalid timestamp: \"{}\". Expected format: 2026-02-09T20:17:00Z",
            spec
        )));
    }

    parse_relative(spec, Utc::now())
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
        // `now` is injected, so the assertion is exact rather than a tolerance band.
        let now = fixed_utc("2026-02-09T20:17:30Z");
        let dt = parse_relative("30s", now).expect("30s is a valid relative spec");
        assert_eq!(dt, fixed_utc("2026-02-09T20:17:00Z"));
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

    /// Builds a UTC instant carried into `offset_secs`, then renders it.
    fn render_at_offset(offset_secs: i32) -> String {
        let utc = DateTime::parse_from_rfc3339("2026-02-09T20:17:03Z")
            .expect("static RFC 3339 literal is valid")
            .with_timezone(&Utc);
        let offset = FixedOffset::east_opt(offset_secs).expect("offset is within +/-24h");
        format_offset_time(utc.with_timezone(&offset))
    }

    #[test]
    fn format_offset_time_negative_offset() {
        // 20:17:03Z seen from -06:00 is 14:17:03 the same day.
        assert_eq!(render_at_offset(-6 * 3600), "2026-02-09 14:17:03 -0600");
    }

    #[test]
    fn format_offset_time_utc_renders_plus_zero_not_zulu() {
        assert_eq!(render_at_offset(0), "2026-02-09 20:17:03 +0000");
    }

    #[test]
    fn format_offset_time_half_hour_offset() {
        // +05:30 (India) — the minutes field must survive.
        assert_eq!(
            render_at_offset(5 * 3600 + 30 * 60),
            "2026-02-10 01:47:03 +0530"
        );
    }

    #[test]
    fn format_offset_time_quarter_hour_offset() {
        // +12:45 (Chatham Islands) — the widest offset in real use.
        assert_eq!(
            render_at_offset(12 * 3600 + 45 * 60),
            "2026-02-10 09:02:03 +1245"
        );
    }

    #[test]
    fn format_offset_time_width_is_always_25_chars() {
        for offset_secs in [
            -12 * 3600,
            -6 * 3600,
            -(3 * 3600 + 30 * 60),
            0,
            5 * 3600 + 30 * 60,
            12 * 3600 + 45 * 60,
            14 * 3600,
        ] {
            let rendered = render_at_offset(offset_secs);
            assert_eq!(
                rendered.len(),
                25,
                "offset {offset_secs}s rendered as {rendered:?}"
            );
            // The offset itself is always the trailing 5 chars: sign + HHMM.
            let offset_field = &rendered[20..];
            assert_eq!(offset_field.len(), 5);
            assert!(offset_field.starts_with(['+', '-']));
            assert!(offset_field[1..].chars().all(|c| c.is_ascii_digit()));
        }
    }

    #[test]
    fn format_local_time_matches_pure_renderer() {
        let utc = DateTime::parse_from_rfc3339("2026-02-09T20:17:03Z")
            .expect("static RFC 3339 literal is valid")
            .with_timezone(&Utc);
        // The edge wrapper must add nothing beyond carrying into the local offset.
        assert_eq!(
            format_local_time(utc),
            format_offset_time(utc.with_timezone(&Local).fixed_offset())
        );
        assert_eq!(format_local_time(utc).len(), 25);
    }
    // ---- helpers for the parse seams -------------------------------------

    /// Parses a static RFC 3339 literal into UTC. Panics only on a typo here.
    fn fixed_utc(literal: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(literal)
            .expect("static RFC 3339 literal is valid")
            .with_timezone(&Utc)
    }

    /// Builds a `DateTime<FixedOffset>` from a naive wall time and an offset.
    ///
    /// A fixed offset has no daylight-saving rules, so the instant is simply
    /// the wall time minus the offset. Computed arithmetically on purpose: no
    /// `LocalResult` is unwrapped anywhere in this module, tests included.
    fn at_offset(naive: &str, offset_hours: i32) -> DateTime<FixedOffset> {
        let naive = parse_naive_local(naive).expect("static naive literal is valid");
        let offset_secs = i64::from(offset_hours) * 3600;
        let offset = FixedOffset::east_opt(offset_hours * 3600).expect("offset is within +/-24h");
        let instant = DateTime::<Utc>::from_naive_utc_and_offset(
            naive - chrono::Duration::seconds(offset_secs),
            Utc,
        );
        instant.with_timezone(&offset)
    }

    // ---- parse_relative (pure) -------------------------------------------

    #[test]
    fn parse_relative_units_are_exact() {
        let now = fixed_utc("2026-02-09T20:00:00Z");
        assert_eq!(
            parse_relative("45s", now).unwrap(),
            fixed_utc("2026-02-09T19:59:15Z")
        );
        assert_eq!(
            parse_relative("5m", now).unwrap(),
            fixed_utc("2026-02-09T19:55:00Z")
        );
        assert_eq!(
            parse_relative("2h", now).unwrap(),
            fixed_utc("2026-02-09T18:00:00Z")
        );
        assert_eq!(
            parse_relative("1d", now).unwrap(),
            fixed_utc("2026-02-08T20:00:00Z")
        );
    }

    #[test]
    fn parse_relative_rejects_bad_input() {
        let now = fixed_utc("2026-02-09T20:00:00Z");
        assert!(parse_relative("abc", now).is_err());
        assert!(parse_relative("5x", now).is_err());
        assert!(parse_relative("xm", now).is_err());
    }

    // ---- parse_offset_time (pure) ----------------------------------------

    #[test]
    fn parse_offset_time_accepts_the_unf_log_display_form() {
        // The literal string `unf log` prints. If this ever fails, a user who
        // copies a timestamp out of the log cannot paste it into `--at`.
        let dt = parse_offset_time("2026-08-25 21:10:03 -0600")
            .expect("the display form must round-trip into the parser");
        assert_eq!(dt.with_timezone(&Utc), fixed_utc("2026-08-26T03:10:03Z"));
    }

    #[test]
    fn parse_offset_time_accepts_colon_and_space_offsets() {
        // Chrono's `%z` is looser on parse than on format: `-0600`, `-06:00`
        // and `-06 00` all name the same offset.
        let expected = fixed_utc("2026-08-26T03:10:03Z");
        for spec in [
            "2026-08-25 21:10:03 -0600",
            "2026-08-25 21:10:03 -06:00",
            "2026-08-25 21:10:03 -06 00",
        ] {
            let dt = parse_offset_time(spec).unwrap_or_else(|| panic!("{spec} must parse"));
            assert_eq!(dt.with_timezone(&Utc), expected, "spec was {spec}");
        }
    }

    #[test]
    fn parse_offset_time_accepts_rfc3339_forms() {
        assert!(parse_offset_time("2026-02-09T20:17:00Z").is_some());
        assert!(parse_offset_time("2026-02-09T15:17:00-05:00").is_some());
        assert!(parse_offset_time("2026-02-09T20:17:00.123456Z").is_some());
    }

    #[test]
    fn parse_offset_time_rejects_offset_less_input() {
        assert!(parse_offset_time("2026-02-09 20:17:00").is_none());
        assert!(parse_offset_time("2026-02-09T20:17:00").is_none());
        assert!(parse_offset_time("2026-02-09").is_none());
    }

    // ---- parse_naive_local (pure) ----------------------------------------

    #[test]
    fn parse_naive_local_accepts_both_separators() {
        let space = parse_naive_local("2026-02-09 14:17:03").expect("space form parses");
        let tee = parse_naive_local("2026-02-09T14:17:03").expect("T form parses");
        assert_eq!(space, tee);
        assert_eq!(space.hour(), 14);
        assert_eq!(space.second(), 3);
    }

    #[test]
    fn parse_naive_local_rejects_coarser_precision() {
        // Out of scope by decision: a restore target must name its second.
        assert!(parse_naive_local("2026-02-09").is_none());
        assert!(parse_naive_local("2026-02-09 14:17").is_none());
        assert!(parse_naive_local("2026-02-09T14:17").is_none());
    }

    #[test]
    fn parse_naive_local_rejects_trailing_offset() {
        // Chrono returns TOO_LONG on trailing input, so an offset-bearing
        // string can never be silently swallowed by a naive pattern.
        assert!(parse_naive_local("2026-02-09 14:17:03 -0600").is_none());
        assert!(parse_naive_local("2026-02-09T14:17:03Z").is_none());
    }

    // ---- resolve_local (pure) — the daylight-saving edges -----------------

    #[test]
    fn resolve_local_single_uses_the_only_instant() {
        let mapped = LocalResult::Single(at_offset("2026-08-25 21:10:03", -6));
        assert_eq!(
            resolve_local("2026-08-25 21:10:03", mapped).unwrap(),
            fixed_utc("2026-08-26T03:10:03Z")
        );
    }

    #[test]
    fn resolve_local_ambiguous_uses_the_earlier_instant() {
        // Fall-back: 01:30 happens once at -05:00 (06:30Z) and again at
        // -06:00 (07:30Z). We take the earlier one.
        let earlier = at_offset("2026-11-01 01:30:00", -5);
        let later = at_offset("2026-11-01 01:30:00", -6);
        let mapped = LocalResult::Ambiguous(earlier, later);
        assert_eq!(
            resolve_local("2026-11-01 01:30:00", mapped).unwrap(),
            fixed_utc("2026-11-01T06:30:00Z")
        );
    }

    #[test]
    fn resolve_local_gap_errors_and_names_two_valid_times() {
        // Spring-forward: 02:30 never happens. There is no instant to pick.
        let mapped: LocalResult<DateTime<FixedOffset>> = LocalResult::None;
        let err = resolve_local("2026-03-08 02:30:00", mapped)
            .expect_err("a skipped local time must not resolve");
        let msg = err.to_string();
        assert!(msg.contains("daylight-saving"), "message was: {msg}");
        // Both suggestions must be times that actually exist.
        assert!(msg.contains("2026-03-08 01:30:00"), "message was: {msg}");
        assert!(msg.contains("2026-03-08 03:30:00"), "message was: {msg}");
        assert!(msg.contains("offset"), "message was: {msg}");
    }

    #[test]
    fn resolve_local_never_panics_on_any_variant() {
        // Exhaustive over `LocalResult`: no variant may panic.
        let a = at_offset("2026-11-01 01:30:00", -5);
        let b = at_offset("2026-11-01 01:30:00", -6);
        let spec = "2026-11-01 01:30:00";
        assert!(resolve_local(spec, LocalResult::Single(a)).is_ok());
        assert!(resolve_local(spec, LocalResult::Ambiguous(a, b)).is_ok());
        assert!(resolve_local(spec, LocalResult::None).is_err());
    }

    // ---- parse_time_spec (edge) ------------------------------------------

    #[test]
    fn parse_time_spec_accepts_the_display_form_with_offset() {
        // Row 2 of the grammar: the exact shape `unf log` prints.
        let dt = parse_time_spec("2026-08-25 21:10:03 -0600")
            .expect("the display form must be accepted by --at");
        assert_eq!(dt, fixed_utc("2026-08-26T03:10:03Z"));
    }

    #[test]
    fn parse_time_spec_accepts_offset_less_local_forms() {
        // Rows 3 and 4: read as local time, so the two must agree.
        let space = parse_time_spec("2026-06-15 14:17:03").expect("space form is accepted");
        let tee = parse_time_spec("2026-06-15T14:17:03").expect("T form is accepted");
        assert_eq!(space, tee);
    }

    #[test]
    fn parse_time_spec_offset_wins_over_local_reading() {
        // An explicit offset must never be re-read as local time.
        let explicit = parse_time_spec("2026-06-15 14:17:03 +0000").expect("offset form accepted");
        assert_eq!(explicit, fixed_utc("2026-06-15T14:17:03Z"));
    }

    #[test]
    fn parse_time_spec_still_rejects_coarse_forms() {
        assert!(parse_time_spec("2026-02-09").is_err());
        assert!(parse_time_spec("2026-02-09 14:17").is_err());
        assert!(parse_time_spec("2026-02-09T14:17").is_err());
    }

    #[test]
    fn parse_time_spec_round_trips_format_local_time() {
        // The property the defect broke: anything `unf log` prints, `--at` reads
        // back to the same instant. Covers a spread of the year so a DST
        // boundary on the host zone cannot hide a regression.
        for literal in [
            "2026-01-15T08:00:00Z",
            "2026-03-08T09:30:00Z",
            "2026-06-15T20:17:03Z",
            "2026-11-01T06:30:00Z",
            "2026-12-31T23:59:59Z",
        ] {
            let t = fixed_utc(literal);
            let rendered = format_local_time(t);
            let parsed = parse_time_spec(&rendered)
                .unwrap_or_else(|e| panic!("{rendered:?} failed to parse back: {e}"));
            assert_eq!(parsed, t, "round-trip failed for {rendered:?}");
        }
    }
}
