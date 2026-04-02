//! Query and read functions for the database.

use super::helpers::row_to_snapshot;
use super::types::{HistoryCursor, HistoryScope, QueryBuilder};
use crate::error::DbError;
use crate::types::{Snapshot, SnapshotId};
use chrono::DateTime;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;

/// Retrieves all snapshots for a specific file, ordered by timestamp descending.
///
/// # Arguments
/// * `conn` - SQLite connection
/// * `file_path` - Absolute path to the file
///
/// # Returns
/// A vector of snapshots, newest first. Empty if no snapshots exist.
///
/// # Errors
/// Returns `DbError::Sqlite` if the query fails.
pub fn get_snapshots_for_file(
    conn: &Connection,
    file_path: &str,
) -> Result<Vec<Snapshot>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, file_path, content_hash, size_bytes, timestamp, event_type, line_count, lines_added, lines_removed
         FROM snapshots
         WHERE file_path = ?1
         ORDER BY timestamp DESC",
    )?;

    let rows = stmt.query_map(params![file_path], row_to_snapshot)?;

    let mut snapshots = Vec::new();
    for row_result in rows {
        snapshots.push(row_result?);
    }

    Ok(snapshots)
}

/// Retrieves all snapshots since a given timestamp, ordered by timestamp descending.
///
/// # Arguments
/// * `conn` - SQLite connection
/// * `since` - UTC timestamp (inclusive lower bound)
///
/// # Returns
/// A vector of snapshots at or after the given time, newest first.
///
/// # Errors
/// Returns `DbError::Sqlite` if the query fails.
pub fn get_snapshots_since(
    conn: &Connection,
    since: DateTime<chrono::Utc>,
) -> Result<Vec<Snapshot>, DbError> {
    let since_str = since.to_rfc3339();

    let mut stmt = conn.prepare(
        "SELECT id, file_path, content_hash, size_bytes, timestamp, event_type, line_count, lines_added, lines_removed
         FROM snapshots
         WHERE timestamp >= ?1
         ORDER BY timestamp DESC",
    )?;

    let rows = stmt.query_map(params![since_str], row_to_snapshot)?;

    let mut snapshots = Vec::new();
    for row_result in rows {
        snapshots.push(row_result?);
    }

    Ok(snapshots)
}

/// Retrieves the latest snapshot for a file at or before a given timestamp.
///
/// # Arguments
/// * `conn` - SQLite connection
/// * `file_path` - Absolute path to the file
/// * `at_time` - UTC timestamp (inclusive upper bound)
///
/// # Returns
/// The most recent snapshot at or before `at_time`, or `None` if no snapshots exist.
///
/// # Errors
/// Returns `DbError::Sqlite` if the query fails.
pub fn get_latest_snapshot_at(
    conn: &Connection,
    file_path: &str,
    at_time: DateTime<chrono::Utc>,
) -> Result<Option<Snapshot>, DbError> {
    let at_time_str = at_time.to_rfc3339();

    let mut stmt = conn.prepare(
        "SELECT id, file_path, content_hash, size_bytes, timestamp, event_type, line_count, lines_added, lines_removed
         FROM snapshots
         WHERE file_path = ?1 AND timestamp <= ?2
         ORDER BY timestamp DESC
         LIMIT 1",
    )?;

    let snapshot = stmt
        .query_row(params![file_path, at_time_str], row_to_snapshot)
        .optional()?;

    Ok(snapshot)
}

/// Retrieves a snapshot by its ID.
///
/// # Arguments
/// * `conn` - SQLite connection
/// * `id` - The snapshot ID to look up
///
/// # Returns
/// The snapshot if found, or `None` if no snapshot with that ID exists.
///
/// # Errors
/// Returns `DbError::Sqlite` if the query fails.
pub fn get_snapshot_by_id(conn: &Connection, id: SnapshotId) -> Result<Option<Snapshot>, DbError> {
    let mut stmt = conn.prepare(
        "SELECT id, file_path, content_hash, size_bytes, timestamp, event_type, line_count, lines_added, lines_removed
         FROM snapshots
         WHERE id = ?1",
    )?;

    let snapshot = stmt.query_row(params![id.0], row_to_snapshot).optional()?;

    Ok(snapshot)
}

/// Retrieves all distinct file paths that have at least one snapshot.
///
/// # Arguments
/// * `conn` - SQLite connection
///
/// # Returns
/// A vector of file paths (absolute paths as strings), in arbitrary order.
///
/// # Errors
/// Returns `DbError::Sqlite` if the query fails.
pub fn get_all_tracked_files(conn: &Connection) -> Result<Vec<String>, DbError> {
    let mut stmt = conn.prepare("SELECT DISTINCT file_path FROM snapshots")?;

    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;

    let mut files = Vec::new();
    for row_result in rows {
        files.push(row_result?);
    }

    Ok(files)
}

/// Returns the total number of snapshots in the database.
///
/// # Arguments
/// * `conn` - SQLite connection
///
/// # Returns
/// The total count of snapshot rows.
///
/// # Errors
/// Returns `DbError::Sqlite` if the query fails.
pub fn get_snapshot_count(conn: &Connection) -> Result<u64, DbError> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM snapshots", [], |row| row.get(0))?;
    Ok(count as u64)
}

/// Returns the number of unique files being tracked.
///
/// # Arguments
/// * `conn` - SQLite connection
///
/// # Returns
/// The count of distinct file paths in the snapshots table.
///
/// # Errors
/// Returns `DbError::Sqlite` if the query fails.
pub fn get_tracked_file_count(conn: &Connection) -> Result<u64, DbError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT file_path) FROM snapshots",
        [],
        |row| row.get(0),
    )?;
    Ok(count as u64)
}

/// Returns the timestamp of the oldest snapshot in the database.
///
/// # Arguments
/// * `conn` - SQLite connection
///
/// # Returns
/// The timestamp of the first snapshot, or `None` if no snapshots exist.
///
/// # Errors
/// Returns `DbError::Sqlite` if the query fails.
pub fn get_oldest_snapshot_time(
    conn: &Connection,
) -> Result<Option<DateTime<chrono::Utc>>, DbError> {
    let result: Option<String> = conn
        .query_row(
            "SELECT timestamp FROM snapshots ORDER BY timestamp ASC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;

    match result {
        Some(timestamp_str) => {
            let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                .map_err(|e| {
                    DbError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    ))
                })?
                .to_utc();
            Ok(Some(timestamp))
        }
        None => Ok(None),
    }
}

/// Returns the timestamp of the most recent snapshot in the database.
///
/// # Arguments
/// * `conn` - SQLite connection
///
/// # Returns
/// The timestamp of the most recent snapshot, or `None` if no snapshots exist.
///
/// # Errors
/// Returns `DbError::Sqlite` if the query fails.
pub fn get_newest_snapshot_time(
    conn: &Connection,
) -> Result<Option<DateTime<chrono::Utc>>, DbError> {
    let result: Option<String> = conn
        .query_row(
            "SELECT timestamp FROM snapshots ORDER BY timestamp DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;

    match result {
        Some(timestamp_str) => {
            let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                .map_err(|e| {
                    DbError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    ))
                })?
                .to_utc();
            Ok(Some(timestamp))
        }
        None => Ok(None),
    }
}

/// Returns all snapshot timestamps and file paths, optionally filtered by since time.
///
/// Used by density histogram computation. Returns only the fields needed for
/// bucketing and glob filtering, avoiding the overhead of loading full snapshots.
///
/// # Arguments
/// * `conn` - SQLite connection
/// * `since` - Optional lower bound (inclusive) for timestamps
///
/// # Returns
/// A vector of `(timestamp, file_path)` pairs, ordered by timestamp descending.
///
/// # Errors
/// Returns `DbError::Sqlite` if the query fails.
pub fn get_all_snapshot_timestamps(
    conn: &Connection,
    since: Option<DateTime<chrono::Utc>>,
) -> Result<Vec<(DateTime<chrono::Utc>, String)>, DbError> {
    let mut qb = QueryBuilder::new("SELECT timestamp, file_path FROM snapshots");

    if let Some(since_time) = since {
        qb.add_condition("timestamp >= ?", since_time.to_rfc3339());
    }

    qb.order_by("timestamp DESC");

    let (query, params) = qb.build();

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map(params.as_slice(), |row| {
        let timestamp_str: String = row.get(0)?;
        let file_path: String = row.get(1)?;
        Ok((timestamp_str, file_path))
    })?;

    let mut results = Vec::new();
    for row in rows {
        let (ts_str, path) = row?;
        let timestamp = DateTime::parse_from_rfc3339(&ts_str)
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?
            .to_utc();
        results.push((timestamp, path));
    }

    Ok(results)
}

/// Counts snapshots with timestamps strictly before the cutoff.
///
/// This is used for dry-run preview of pruning operations.
///
/// # Arguments
/// * `conn` - SQLite connection
/// * `cutoff` - UTC timestamp (exclusive upper bound)
///
/// # Returns
/// The number of snapshots with timestamp < cutoff.
///
/// # Errors
/// Returns `DbError::Sqlite` if the query fails.
pub fn count_snapshots_before(
    conn: &Connection,
    cutoff: DateTime<chrono::Utc>,
) -> Result<u64, DbError> {
    let cutoff_str = cutoff.to_rfc3339();
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM snapshots WHERE timestamp < ?1",
        params![cutoff_str],
        |row| row.get(0),
    )?;
    Ok(count as u64)
}

/// Returns the set of all content hashes still referenced by at least one snapshot.
///
/// This is used after deletion to determine which CAS objects can be safely
/// garbage collected. Any hash not in this set can be removed from storage.
///
/// # Arguments
/// * `conn` - SQLite connection
///
/// # Returns
/// A HashSet of all distinct content_hash values in the snapshots table.
///
/// # Errors
/// Returns `DbError::Sqlite` if the query fails.
pub fn get_referenced_hashes(conn: &Connection) -> Result<HashSet<String>, DbError> {
    let mut stmt = conn.prepare("SELECT DISTINCT content_hash FROM snapshots")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;

    let mut hashes = HashSet::new();
    for row_result in rows {
        hashes.insert(row_result?);
    }

    Ok(hashes)
}

/// Retrieves the snapshot immediately preceding a given snapshot for the
/// same file, ordered by (timestamp DESC, id DESC).
///
/// Used by `unf log --stats` to find the "before" state for comparison.
///
/// Returns `None` if the given snapshot is the first for that file.
///
/// # Arguments
/// * `conn` - SQLite connection
/// * `file_path` - Absolute path to the file
/// * `before_timestamp` - UTC timestamp to search before
/// * `before_id` - Snapshot ID to search before
///
/// # Returns
/// The snapshot immediately preceding the given snapshot, or `None` if it's the first.
///
/// # Errors
/// Returns `DbError::Sqlite` if the query fails.
pub fn get_previous_snapshot(
    conn: &Connection,
    file_path: &str,
    before_timestamp: DateTime<chrono::Utc>,
    before_id: SnapshotId,
) -> Result<Option<Snapshot>, DbError> {
    let timestamp_str = before_timestamp.to_rfc3339();

    let mut stmt = conn.prepare(
        "SELECT id, file_path, content_hash, size_bytes, timestamp, event_type, line_count, lines_added, lines_removed
         FROM snapshots
         WHERE file_path = ?1
           AND (timestamp < ?2 OR (timestamp = ?2 AND id < ?3))
         ORDER BY timestamp DESC, id DESC
         LIMIT 1",
    )?;

    let snapshot = stmt
        .query_row(
            params![file_path, timestamp_str, before_id.0],
            row_to_snapshot,
        )
        .optional()?;

    Ok(snapshot)
}

/// Fetches the next page of snapshot history.
///
/// Returns up to `page_size` snapshots ordered newest-first (timestamp DESC, id DESC).
/// Pass cursor from last snapshot of previous page to get next page.
/// Returns empty vec when history is exhausted.
///
/// Optional `since` parameter filters to snapshots at or after the given time.
///
/// # Arguments
/// * `conn` - SQLite connection
/// * `scope` - Scope for history queries (File, Directory, or All)
/// * `cursor` - Optional cursor from last snapshot of previous page
/// * `page_size` - Maximum number of snapshots to return
/// * `since` - Optional lower bound for timestamp filtering
///
/// # Returns
/// A vector of up to `page_size` snapshots, newest first.
///
/// # Errors
/// Returns `DbError::Sqlite` if the query fails.
pub fn get_history_page(
    conn: &Connection,
    scope: HistoryScope<'_>,
    cursor: Option<&HistoryCursor>,
    page_size: u32,
    since: Option<DateTime<chrono::Utc>>,
    until: Option<DateTime<chrono::Utc>>,
) -> Result<Vec<Snapshot>, DbError> {
    let mut qb = QueryBuilder::new(
        "SELECT id, file_path, content_hash, size_bytes, timestamp, event_type, line_count, lines_added, lines_removed FROM snapshots",
    );

    // Add scope filter
    match scope {
        HistoryScope::File(file_path) => {
            qb.add_condition("file_path = ?", file_path.to_string());
        }
        HistoryScope::Directory(dir_prefix) => {
            qb.add_condition("file_path LIKE ?", format!("{}%", dir_prefix));
        }
        HistoryScope::All => {
            // No filter
        }
    }

    // Add cursor filter (keyset pagination)
    if let Some(cursor) = cursor {
        let cursor_timestamp = cursor.timestamp.to_rfc3339();
        let cursor_params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
            Box::new(cursor_timestamp.clone()),
            Box::new(cursor_timestamp),
            Box::new(cursor.id.0),
        ];
        qb.add_condition_with_params(
            "(timestamp < ? OR (timestamp = ? AND id < ?))",
            cursor_params,
        );
    }

    // Add time range filters
    if let Some(since_time) = since {
        qb.add_condition("timestamp >= ?", since_time.to_rfc3339());
    }
    if let Some(until_time) = until {
        qb.add_condition("timestamp <= ?", until_time.to_rfc3339());
    }

    qb.order_by("timestamp DESC, id DESC");
    qb.limit(page_size as i64);

    let (query, params) = qb.build();

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map(params.as_slice(), row_to_snapshot)?;

    let mut snapshots = Vec::new();
    for row_result in rows {
        snapshots.push(row_result?);
    }

    Ok(snapshots)
}
