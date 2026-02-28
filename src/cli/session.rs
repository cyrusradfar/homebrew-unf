//! Session detection for the UNFUDGED CLI.
//!
//! Sessions are contiguous periods of file-editing activity separated by
//! gaps of inactivity. They are computed on the fly from snapshot timestamps
//! and are never persisted.

use chrono::{DateTime, Utc};
use std::collections::BTreeSet;

use crate::error::UnfError;
use crate::types::Snapshot;

/// A detected work session boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    /// 1-based session number (oldest = 1).
    pub number: usize,
    /// Timestamp of the first snapshot in this session.
    pub start: DateTime<Utc>,
    /// Timestamp of the last snapshot in this session.
    pub end: DateTime<Utc>,
    /// Number of snapshots in this session.
    pub edit_count: usize,
    /// Distinct file paths edited in this session.
    pub files: Vec<String>,
}

/// JSON-serializable session output.
#[derive(serde::Serialize)]
pub struct SessionOutput {
    pub number: usize,
    pub start: String,
    pub end: String,
    pub duration_seconds: i64,
    pub edit_count: usize,
    pub file_count: usize,
    pub files: Vec<String>,
}

/// JSON wrapper for session list output.
#[derive(serde::Serialize)]
pub struct SessionsOutput {
    pub sessions: Vec<SessionOutput>,
    pub total_edits: usize,
    pub total_files: usize,
}

/// Computes an adaptive gap threshold for session boundaries.
///
/// The threshold scales with the total time span of the history:
/// - span < 6h: 5 minutes (300_000 ms)
/// - 6h <= span < 48h: linear ramp from 5 to 30 minutes
/// - span >= 48h: 30 minutes (1_800_000 ms)
///
/// # Arguments
/// * `total_span_ms` - Total time span in milliseconds from first to last timestamp
///
/// # Returns
/// Gap threshold in milliseconds
pub fn compute_gap_threshold_ms(total_span_ms: i64) -> i64 {
    const MS_PER_HOUR: i64 = 60 * 60 * 1000;
    const MIN_THRESHOLD_MS: i64 = 5 * 60 * 1000; // 5 minutes
    const MAX_THRESHOLD_MS: i64 = 30 * 60 * 1000; // 30 minutes
    const RAMP_START_HOURS: i64 = 6;
    const RAMP_END_HOURS: i64 = 48;

    let span_hours = total_span_ms / MS_PER_HOUR;

    if span_hours < RAMP_START_HOURS {
        // < 6h: constant 5 minutes
        MIN_THRESHOLD_MS
    } else if span_hours < RAMP_END_HOURS {
        // 6h to 48h: linear ramp
        let ramp_range = RAMP_END_HOURS - RAMP_START_HOURS; // 42 hours
        let hours_into_ramp = span_hours - RAMP_START_HOURS;
        let ramp_fraction = hours_into_ramp as f64 / ramp_range as f64;
        let threshold_ms =
            MIN_THRESHOLD_MS as f64 + (MAX_THRESHOLD_MS - MIN_THRESHOLD_MS) as f64 * ramp_fraction;
        threshold_ms as i64
    } else {
        // >= 48h: constant 30 minutes
        MAX_THRESHOLD_MS
    }
}

/// Detects sessions from a chronologically-sorted list of snapshots.
///
/// Sessions are contiguous periods of activity separated by gaps exceeding
/// an adaptive threshold. The threshold is computed from the total time span.
///
/// # Arguments
/// * `snapshots` - Snapshots **must be sorted ascending by timestamp**
///
/// # Returns
/// A list of detected sessions, numbered 1..N from oldest to newest.
/// Returns an empty vec for empty input.
pub fn detect_sessions(snapshots: &[Snapshot]) -> Vec<Session> {
    if snapshots.is_empty() {
        return vec![];
    }

    if snapshots.len() == 1 {
        return vec![Session {
            number: 1,
            start: snapshots[0].timestamp,
            end: snapshots[0].timestamp,
            edit_count: 1,
            files: vec![snapshots[0].file_path.clone()],
        }];
    }

    // Compute total span and gap threshold
    let first_timestamp = snapshots[0].timestamp;
    let last_timestamp = snapshots[snapshots.len() - 1].timestamp;
    let total_span = last_timestamp.signed_duration_since(first_timestamp);
    let total_span_ms = total_span.num_milliseconds();

    let gap_threshold_ms = compute_gap_threshold_ms(total_span_ms);

    // Walk through snapshots, splitting on large gaps
    let mut sessions = vec![];
    let mut current_session_start = first_timestamp;
    let mut current_session_end = first_timestamp;
    let mut current_files: BTreeSet<String> = BTreeSet::new();
    let mut current_edit_count = 1;

    current_files.insert(snapshots[0].file_path.clone());

    for snapshot in &snapshots[1..] {
        let gap = snapshot
            .timestamp
            .signed_duration_since(current_session_end)
            .num_milliseconds();

        if gap > gap_threshold_ms {
            // Large gap: end current session and start a new one
            let session_number = sessions.len() + 1;
            sessions.push(Session {
                number: session_number,
                start: current_session_start,
                end: current_session_end,
                edit_count: current_edit_count,
                files: current_files.iter().cloned().collect(),
            });

            // Start new session
            current_session_start = snapshot.timestamp;
            current_session_end = snapshot.timestamp;
            current_files = BTreeSet::new();
            current_files.insert(snapshot.file_path.clone());
            current_edit_count = 1;
        } else {
            // Continue current session
            current_session_end = snapshot.timestamp;
            current_files.insert(snapshot.file_path.clone());
            current_edit_count += 1;
        }
    }

    // Add the final session
    let session_number = sessions.len() + 1;
    sessions.push(Session {
        number: session_number,
        start: current_session_start,
        end: current_session_end,
        edit_count: current_edit_count,
        files: current_files.iter().cloned().collect(),
    });

    sessions
}

/// Resolves a session number to an actual session.
///
/// # Arguments
/// * `sessions` - List of detected sessions
/// * `number` - Optional session number (1-based). `None` resolves to the most recent session.
///
/// # Returns
/// A reference to the resolved session, or an error if not found.
pub fn resolve_session(sessions: &[Session], number: Option<usize>) -> Result<&Session, UnfError> {
    if sessions.is_empty() {
        return Err(UnfError::InvalidArgument(
            "No sessions detected.".to_string(),
        ));
    }

    match number {
        None => {
            // Return the last (most recent) session
            Ok(&sessions[sessions.len() - 1])
        }
        Some(n) => sessions.iter().find(|s| s.number == n).ok_or_else(|| {
            UnfError::InvalidArgument(format!(
                "Session {} not found. Available sessions: 1..{}",
                n,
                sessions.len()
            ))
        }),
    }
}

impl Session {
    /// Converts a session to JSON-serializable output.
    pub fn to_output(&self) -> SessionOutput {
        let duration = self.end.signed_duration_since(self.start);
        let duration_seconds = duration.num_seconds();

        SessionOutput {
            number: self.number,
            start: self.start.to_rfc3339(),
            end: self.end.to_rfc3339(),
            duration_seconds,
            edit_count: self.edit_count,
            file_count: self.files.len(),
            files: self.files.clone(),
        }
    }
}

impl SessionsOutput {
    /// Creates a JSON output wrapper from a list of sessions.
    pub fn from_sessions(sessions: &[Session]) -> Self {
        let total_edits = sessions.iter().map(|s| s.edit_count).sum();
        let total_files: std::collections::HashSet<String> = sessions
            .iter()
            .flat_map(|s| s.files.iter().cloned())
            .collect();

        SessionsOutput {
            sessions: sessions.iter().map(|s| s.to_output()).collect(),
            total_edits,
            total_files: total_files.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_snapshot(ts: DateTime<Utc>, file: &str) -> Snapshot {
        Snapshot {
            id: crate::types::SnapshotId(0),
            file_path: file.to_string(),
            content_hash: crate::types::ContentHash("abc".to_string()),
            size_bytes: 100,
            timestamp: ts,
            event_type: crate::types::EventType::Modify,
            line_count: 10,
            lines_added: 5,
            lines_removed: 2,
        }
    }

    #[test]
    fn empty_input_returns_empty() {
        let sessions = detect_sessions(&[]);
        assert_eq!(sessions, vec![]);
    }

    #[test]
    fn single_snapshot_returns_one_session() {
        let ts = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let snap = make_snapshot(ts, "test.rs");
        let sessions = detect_sessions(&[snap]);

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].number, 1);
        assert_eq!(sessions[0].start, ts);
        assert_eq!(sessions[0].end, ts);
        assert_eq!(sessions[0].edit_count, 1);
        assert_eq!(sessions[0].files, vec!["test.rs"]);
    }

    #[test]
    fn continuous_burst_is_one_session() {
        let base = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let mut snapshots = vec![];

        // 10 snapshots, 30 seconds apart (span = 4.5 minutes, well below threshold)
        for i in 0..10 {
            let ts = base + chrono::Duration::seconds(i * 30);
            snapshots.push(make_snapshot(ts, &format!("file{}.rs", i)));
        }

        let sessions = detect_sessions(&snapshots);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].number, 1);
        assert_eq!(sessions[0].edit_count, 10);
        assert_eq!(sessions[0].start, base);
        assert_eq!(sessions[0].end, base + chrono::Duration::seconds(9 * 30));
    }

    #[test]
    fn gap_splits_into_two_sessions() {
        let base = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let mut snapshots = vec![];

        // 5 snapshots in first session (30s apart, 2m span)
        for i in 0..5 {
            let ts = base + chrono::Duration::seconds(i * 30);
            snapshots.push(make_snapshot(ts, "file1.rs"));
        }

        // 40-minute gap
        let gap_time = base + chrono::Duration::minutes(42);

        // 5 more snapshots in second session (30s apart)
        for i in 0..5 {
            let ts = gap_time + chrono::Duration::seconds(i * 30);
            snapshots.push(make_snapshot(ts, "file2.rs"));
        }

        let sessions = detect_sessions(&snapshots);
        assert_eq!(sessions.len(), 2);

        // First session
        assert_eq!(sessions[0].number, 1);
        assert_eq!(sessions[0].edit_count, 5);
        assert_eq!(sessions[0].files, vec!["file1.rs"]);

        // Second session
        assert_eq!(sessions[1].number, 2);
        assert_eq!(sessions[1].edit_count, 5);
        assert_eq!(sessions[1].files, vec!["file2.rs"]);
    }

    #[test]
    fn gap_below_threshold_stays_one_session() {
        let base = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let mut snapshots = vec![];

        // 5 snapshots (30s apart)
        for i in 0..5 {
            let ts = base + chrono::Duration::seconds(i * 30);
            snapshots.push(make_snapshot(ts, "file1.rs"));
        }

        // 3-minute gap (below the 5-minute threshold for short spans)
        let gap_time = base + chrono::Duration::minutes(4);

        // 5 more snapshots
        for i in 0..5 {
            let ts = gap_time + chrono::Duration::seconds(i * 30);
            snapshots.push(make_snapshot(ts, "file2.rs"));
        }

        let sessions = detect_sessions(&snapshots);
        assert_eq!(
            sessions.len(),
            1,
            "Should be one session since gap is below threshold"
        );
        assert_eq!(sessions[0].edit_count, 10);
    }

    #[test]
    fn threshold_scales_with_span() {
        // Test compute_gap_threshold_ms directly

        // 1 hour span: should be 5 minutes
        let one_hour_ms = 60 * 60 * 1000;
        assert_eq!(compute_gap_threshold_ms(one_hour_ms), 5 * 60 * 1000);

        // 24 hours span: should ramp (between 5 and 30 minutes)
        let twenty_four_hours_ms = 24 * 60 * 60 * 1000;
        let threshold_24h = compute_gap_threshold_ms(twenty_four_hours_ms);
        assert!(threshold_24h > 5 * 60 * 1000);
        assert!(threshold_24h < 30 * 60 * 1000);

        // 72 hours span: should be 30 minutes
        let seventy_two_hours_ms = 72 * 60 * 60 * 1000;
        assert_eq!(
            compute_gap_threshold_ms(seventy_two_hours_ms),
            30 * 60 * 1000
        );
    }

    #[test]
    fn sessions_numbered_ascending() {
        let base = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let mut snapshots = vec![];

        // Create 3 sessions with large gaps
        for session_idx in 0..3 {
            let session_start = base + chrono::Duration::hours(session_idx as i64 * 6);
            for i in 0..2 {
                let ts = session_start + chrono::Duration::seconds(i * 30);
                snapshots.push(make_snapshot(ts, &format!("file{}.rs", session_idx)));
            }
        }

        let sessions = detect_sessions(&snapshots);
        assert_eq!(sessions.len(), 3);
        assert_eq!(sessions[0].number, 1);
        assert_eq!(sessions[1].number, 2);
        assert_eq!(sessions[2].number, 3);
    }

    #[test]
    fn resolve_none_returns_latest() {
        let base = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let mut snapshots = vec![];

        // Two sessions
        for session_idx in 0..2 {
            let session_start = base + chrono::Duration::hours(session_idx as i64 * 6);
            snapshots.push(make_snapshot(session_start, "file.rs"));
        }

        let sessions = detect_sessions(&snapshots);
        let resolved = resolve_session(&sessions, None);

        assert!(resolved.is_ok());
        assert_eq!(resolved.unwrap().number, 2);
    }

    #[test]
    fn resolve_specific_number() {
        let base = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let mut snapshots = vec![];

        // Three sessions
        for session_idx in 0..3 {
            let session_start = base + chrono::Duration::hours(session_idx as i64 * 6);
            snapshots.push(make_snapshot(session_start, "file.rs"));
        }

        let sessions = detect_sessions(&snapshots);

        let resolved1 = resolve_session(&sessions, Some(1));
        assert!(resolved1.is_ok());
        assert_eq!(resolved1.unwrap().number, 1);

        let resolved2 = resolve_session(&sessions, Some(2));
        assert!(resolved2.is_ok());
        assert_eq!(resolved2.unwrap().number, 2);
    }

    #[test]
    fn resolve_out_of_range_errors() {
        let base = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let snap = make_snapshot(base, "file.rs");
        let sessions = detect_sessions(&[snap]);

        let resolved = resolve_session(&sessions, Some(999));
        assert!(resolved.is_err());
        assert!(resolved.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn resolve_empty_errors() {
        let resolved = resolve_session(&[], None);
        assert!(resolved.is_err());
        assert!(resolved.unwrap_err().to_string().contains("No sessions"));
    }

    #[test]
    fn files_are_deduplicated() {
        let base = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let mut snapshots = vec![];

        // Same file edited 5 times
        for i in 0..5 {
            let ts = base + chrono::Duration::seconds(i * 30);
            snapshots.push(make_snapshot(ts, "shared.rs"));
        }

        let sessions = detect_sessions(&snapshots);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].files.len(), 1, "File should be deduplicated");
        assert_eq!(sessions[0].files[0], "shared.rs");
    }

    #[test]
    fn session_to_output() {
        let base = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let session = Session {
            number: 1,
            start: base,
            end: base + chrono::Duration::minutes(10),
            edit_count: 5,
            files: vec!["a.rs".to_string(), "b.rs".to_string()],
        };

        let output = session.to_output();
        assert_eq!(output.number, 1);
        assert_eq!(output.edit_count, 5);
        assert_eq!(output.file_count, 2);
        assert_eq!(output.duration_seconds, 600);
        assert_eq!(output.files.len(), 2);
    }

    #[test]
    fn sessions_output_aggregates_totals() {
        let base = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let sessions = vec![
            Session {
                number: 1,
                start: base,
                end: base + chrono::Duration::minutes(5),
                edit_count: 3,
                files: vec!["a.rs".to_string(), "b.rs".to_string()],
            },
            Session {
                number: 2,
                start: base + chrono::Duration::hours(1),
                end: base + chrono::Duration::hours(1) + chrono::Duration::minutes(5),
                edit_count: 2,
                files: vec!["b.rs".to_string(), "c.rs".to_string()],
            },
        ];

        let output = SessionsOutput::from_sessions(&sessions);
        assert_eq!(output.total_edits, 5);
        assert_eq!(output.total_files, 3); // a.rs, b.rs, c.rs (deduplicated)
        assert_eq!(output.sessions.len(), 2);
    }
}
