//! Helper functions for type conversions and row mapping.

use crate::error::DbError;
use crate::types::{ContentHash, EventType, Snapshot, SnapshotId};
use chrono::DateTime;
use rusqlite::{Error as RusqliteError, Row};

/// Converts an `EventType` to its string representation for database storage.
pub(crate) fn event_type_to_str(et: &EventType) -> &'static str {
    match et {
        EventType::Create => "create",
        EventType::Modify => "modify",
        EventType::Delete => "delete",
    }
}

/// Converts a string from the database to an `EventType`.
///
/// # Errors
/// Returns `DbError::Migration` if the string is not a recognized event type.
pub(crate) fn event_type_from_str(s: &str) -> Result<EventType, DbError> {
    match s {
        "create" => Ok(EventType::Create),
        "modify" => Ok(EventType::Modify),
        "delete" => Ok(EventType::Delete),
        _ => Err(DbError::Migration(format!("Unknown event type: {}", s))),
    }
}

/// Maps a SQLite row to a `Snapshot` struct.
///
/// # Errors
/// Returns `rusqlite::Error` if column access fails or data is malformed.
pub(crate) fn row_to_snapshot(row: &Row) -> Result<Snapshot, RusqliteError> {
    let id: i64 = row.get(0)?;
    let file_path: String = row.get(1)?;
    let content_hash: String = row.get(2)?;
    let size_bytes: i64 = row.get(3)?;
    let timestamp_str: String = row.get(4)?;
    let event_type_str: String = row.get(5)?;
    let line_count: i64 = row.get(6)?;
    let lines_added: i64 = row.get(7)?;
    let lines_removed: i64 = row.get(8)?;

    // Parse timestamp from RFC3339 string
    let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
        .map_err(|e| {
            RusqliteError::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
        })?
        .to_utc();

    // Parse event type
    let event_type = event_type_from_str(&event_type_str).map_err(|e| {
        RusqliteError::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
    })?;

    Ok(Snapshot {
        id: SnapshotId(id),
        file_path,
        content_hash: ContentHash(content_hash),
        size_bytes: size_bytes as u64,
        timestamp,
        event_type,
        line_count: line_count as u64,
        lines_added: lines_added as u64,
        lines_removed: lines_removed as u64,
    })
}
