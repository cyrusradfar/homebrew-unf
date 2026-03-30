//! Write and mutation functions for the database.

use super::helpers::event_type_to_str;
use crate::error::DbError;
use crate::types::{ContentHash, EventType, SnapshotId};
use chrono::DateTime;
use rusqlite::{params, Connection};

/// Inserts a new snapshot into the database.
///
/// # Arguments
/// * `conn` - SQLite connection
/// * `file_path` - Absolute path to the file
/// * `content_hash` - BLAKE3 hash of the file content
/// * `size_bytes` - File size in bytes
/// * `timestamp` - UTC timestamp when the snapshot was taken
/// * `event_type` - The filesystem event that triggered this snapshot
/// * `line_count` - Total number of lines in the file
/// * `lines_added` - Number of lines added since previous snapshot
/// * `lines_removed` - Number of lines removed since previous snapshot
///
/// # Returns
/// The newly created `SnapshotId` (SQLite rowid).
///
/// # Errors
/// Returns `DbError::Sqlite` if the insert fails.
#[allow(clippy::too_many_arguments)]
pub fn insert_snapshot(
    conn: &Connection,
    file_path: &str,
    content_hash: &ContentHash,
    size_bytes: u64,
    timestamp: DateTime<chrono::Utc>,
    event_type: &EventType,
    line_count: u64,
    lines_added: u64,
    lines_removed: u64,
) -> Result<SnapshotId, DbError> {
    let timestamp_str = timestamp.to_rfc3339();
    let event_type_str = event_type_to_str(event_type);

    conn.execute(
        "INSERT INTO snapshots (file_path, content_hash, size_bytes, timestamp, event_type, line_count, lines_added, lines_removed) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            file_path,
            content_hash.0,
            size_bytes as i64,
            timestamp_str,
            event_type_str,
            line_count as i64,
            lines_added as i64,
            lines_removed as i64
        ],
    )?;

    let id = conn.last_insert_rowid();
    Ok(SnapshotId(id))
}

/// Deletes all snapshots with timestamps strictly before the cutoff.
///
/// Returns the number of rows deleted. This is the primary operation for
/// implementing retention policies.
///
/// # Arguments
/// * `conn` - SQLite connection
/// * `cutoff` - UTC timestamp (exclusive upper bound)
///
/// # Returns
/// The number of snapshot rows deleted.
///
/// # Errors
/// Returns `DbError::Sqlite` if the delete operation fails.
pub fn delete_snapshots_before(
    conn: &Connection,
    cutoff: DateTime<chrono::Utc>,
) -> Result<u64, DbError> {
    let cutoff_str = cutoff.to_rfc3339();
    let deleted = conn.execute(
        "DELETE FROM snapshots WHERE timestamp < ?1",
        params![cutoff_str],
    )?;
    Ok(deleted as u64)
}
