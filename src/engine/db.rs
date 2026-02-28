//! SQLite metadata layer for the UNFUDGED flight recorder.
//!
//! This module manages the relational metadata for all snapshots. The schema
//! is designed for high-velocity writes with minimal indexing overhead, and
//! efficient queries by file path and time range.
//!
//! Schema version tracking allows future migrations. WAL mode is enabled for
//! concurrent read/write access.

use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::DbError;
use crate::types::{ContentHash, EventType, Snapshot, SnapshotId};

/// Cursor for keyset pagination through snapshot history.
#[derive(Debug, Clone)]
pub struct HistoryCursor {
    pub timestamp: DateTime<Utc>,
    pub id: SnapshotId,
}

/// Scope for history queries.
#[derive(Debug)]
pub enum HistoryScope<'a> {
    /// Exact file match.
    File(&'a str),
    /// Directory prefix (recursive). The string must end with '/'.
    Directory(&'a str),
    /// All files.
    All,
}

/// Current schema version.
///
/// Increment this when making schema changes, and add migration logic
/// to `init_schema()`.
const SCHEMA_VERSION: i64 = 2;

/// Opens or creates a SQLite database at the given path.
///
/// Enables WAL mode for concurrent access, and initializes the schema
/// if the database is new. This function is idempotent: calling it
/// multiple times on the same path is safe.
///
/// # Arguments
/// * `db_path` - Absolute path to the SQLite database file
///
/// # Returns
/// A connection with WAL mode enabled and schema initialized.
///
/// # Errors
/// Returns `DbError::Sqlite` if the database cannot be opened or
/// initialized.
pub fn open_db(db_path: &Path) -> Result<Connection, DbError> {
    let conn = Connection::open(db_path)?;

    // Enable WAL mode for concurrent reads and writes
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;

    init_schema(&conn)?;

    Ok(conn)
}

/// Initializes the database schema if it does not exist.
///
/// This function is idempotent: it uses `IF NOT EXISTS` clauses to avoid
/// errors on repeated calls. Schema version tracking is included for
/// future migration support.
///
/// # Arguments
/// * `conn` - SQLite connection to initialize
///
/// # Errors
/// Returns `DbError::Sqlite` if schema creation fails.
fn init_schema(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS snapshots (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            file_path   TEXT    NOT NULL,
            content_hash TEXT   NOT NULL,
            size_bytes  INTEGER NOT NULL,
            timestamp   TEXT    NOT NULL,
            event_type  TEXT    NOT NULL,
            line_count  INTEGER NOT NULL DEFAULT 0,
            lines_added INTEGER NOT NULL DEFAULT 0,
            lines_removed INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_snapshots_file_time
            ON snapshots (file_path, timestamp DESC);

        CREATE INDEX IF NOT EXISTS idx_snapshots_time
            ON snapshots (timestamp DESC);
        "#,
    )?;

    // Insert schema version if the table is empty
    let version_exists: bool =
        conn.query_row("SELECT EXISTS(SELECT 1 FROM schema_version)", [], |row| {
            row.get(0)
        })?;

    if !version_exists {
        conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            params![SCHEMA_VERSION],
        )?;
    } else {
        // Check current version and run migrations if needed
        let current_version: i64 =
            conn.query_row("SELECT version FROM schema_version", [], |row| row.get(0))?;

        if current_version == 1 && SCHEMA_VERSION > 1 {
            // Migration from v1 to v2: Add line statistics columns
            conn.execute_batch(
                r#"
                ALTER TABLE snapshots ADD COLUMN line_count INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE snapshots ADD COLUMN lines_added INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE snapshots ADD COLUMN lines_removed INTEGER NOT NULL DEFAULT 0;
                "#,
            )?;

            // Update schema version to 2
            conn.execute("UPDATE schema_version SET version = 2", [])?;
        }
    }

    Ok(())
}

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
    timestamp: DateTime<Utc>,
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
    since: DateTime<Utc>,
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
    at_time: DateTime<Utc>,
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
pub fn get_oldest_snapshot_time(conn: &Connection) -> Result<Option<DateTime<Utc>>, DbError> {
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
pub fn get_newest_snapshot_time(conn: &Connection) -> Result<Option<DateTime<Utc>>, DbError> {
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
    since: Option<DateTime<Utc>>,
) -> Result<Vec<(DateTime<Utc>, String)>, DbError> {
    let (query, params): (String, Vec<Box<dyn rusqlite::ToSql>>) = match since {
        Some(since_time) => (
            "SELECT timestamp, file_path FROM snapshots WHERE timestamp >= ?1 ORDER BY timestamp DESC".to_string(),
            vec![Box::new(since_time.to_rfc3339())],
        ),
        None => (
            "SELECT timestamp, file_path FROM snapshots ORDER BY timestamp DESC".to_string(),
            vec![],
        ),
    };

    let mut stmt = conn.prepare(&query)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), |row| {
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
pub fn count_snapshots_before(conn: &Connection, cutoff: DateTime<Utc>) -> Result<u64, DbError> {
    let cutoff_str = cutoff.to_rfc3339();
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM snapshots WHERE timestamp < ?1",
        params![cutoff_str],
        |row| row.get(0),
    )?;
    Ok(count as u64)
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
pub fn delete_snapshots_before(conn: &Connection, cutoff: DateTime<Utc>) -> Result<u64, DbError> {
    let cutoff_str = cutoff.to_rfc3339();
    let deleted = conn.execute(
        "DELETE FROM snapshots WHERE timestamp < ?1",
        params![cutoff_str],
    )?;
    Ok(deleted as u64)
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
pub fn get_referenced_hashes(
    conn: &Connection,
) -> Result<std::collections::HashSet<String>, DbError> {
    let mut stmt = conn.prepare("SELECT DISTINCT content_hash FROM snapshots")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;

    let mut hashes = std::collections::HashSet::new();
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
    before_timestamp: DateTime<Utc>,
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
    since: Option<DateTime<Utc>>,
) -> Result<Vec<Snapshot>, DbError> {
    // Build the query dynamically based on scope, cursor, and since
    let mut query = String::from(
        "SELECT id, file_path, content_hash, size_bytes, timestamp, event_type, line_count, lines_added, lines_removed FROM snapshots",
    );
    let mut conditions = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    // Add scope filter
    match scope {
        HistoryScope::File(file_path) => {
            conditions.push("file_path = ?".to_string());
            params.push(Box::new(file_path.to_string()));
        }
        HistoryScope::Directory(dir_prefix) => {
            conditions.push("file_path LIKE ?".to_string());
            params.push(Box::new(format!("{}%", dir_prefix)));
        }
        HistoryScope::All => {
            // No filter
        }
    }

    // Add cursor filter (keyset pagination)
    if let Some(cursor) = cursor {
        let cursor_timestamp = cursor.timestamp.to_rfc3339();
        conditions.push("(timestamp < ? OR (timestamp = ? AND id < ?))".to_string());
        params.push(Box::new(cursor_timestamp.clone()));
        params.push(Box::new(cursor_timestamp));
        params.push(Box::new(cursor.id.0));
    }

    // Add since filter
    if let Some(since_time) = since {
        let since_str = since_time.to_rfc3339();
        conditions.push("timestamp >= ?".to_string());
        params.push(Box::new(since_str));
    }

    // Build WHERE clause
    if !conditions.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&conditions.join(" AND "));
    }

    // Add ordering and limit
    query.push_str(" ORDER BY timestamp DESC, id DESC LIMIT ?");
    params.push(Box::new(page_size as i64));

    // Execute query
    let mut stmt = conn.prepare(&query)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(param_refs.as_slice(), row_to_snapshot)?;

    let mut snapshots = Vec::new();
    for row_result in rows {
        snapshots.push(row_result?);
    }

    Ok(snapshots)
}

// --- Helper functions ---

/// Converts an `EventType` to its string representation for database storage.
fn event_type_to_str(et: &EventType) -> &'static str {
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
fn event_type_from_str(s: &str) -> Result<EventType, DbError> {
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
fn row_to_snapshot(row: &Row) -> Result<Snapshot, rusqlite::Error> {
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
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
        })?
        .to_utc();

    // Parse event type
    let event_type = event_type_from_str(&event_type_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Opens an in-memory SQLite database for testing.
    fn open_test_db() -> Connection {
        open_db(Path::new(":memory:")).expect("Failed to open test database")
    }

    #[test]
    fn schema_creation_is_idempotent() {
        let conn = open_test_db();
        // Call init_schema again to verify idempotency
        init_schema(&conn).expect("Second schema init should succeed");

        // Verify schema_version table exists
        let version: i64 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .expect("Schema version should be set");
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn insert_and_retrieve_snapshot() {
        let conn = open_test_db();
        let timestamp = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        let id = insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("abc123".to_string()),
            1024,
            timestamp,
            &EventType::Create,
            0,
            0,
            0,
        )
        .expect("Insert should succeed");

        assert!(id.0 > 0, "Snapshot ID should be positive");

        let snapshots =
            get_snapshots_for_file(&conn, "/tmp/test.txt").expect("Query should succeed");
        assert_eq!(snapshots.len(), 1);

        let snap = &snapshots[0];
        assert_eq!(snap.id, id);
        assert_eq!(snap.file_path, "/tmp/test.txt");
        assert_eq!(snap.content_hash, ContentHash("abc123".to_string()));
        assert_eq!(snap.size_bytes, 1024);
        assert_eq!(snap.timestamp, timestamp);
        assert_eq!(snap.event_type, EventType::Create);
    }

    #[test]
    fn query_snapshots_since_time() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 15, 14, 0, 0).unwrap();

        insert_snapshot(
            &conn,
            "/tmp/a.txt",
            &ContentHash("hash1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/b.txt",
            &ContentHash("hash2".to_string()),
            200,
            t2,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/c.txt",
            &ContentHash("hash3".to_string()),
            300,
            t3,
            &EventType::Delete,
            0,
            0,
            0,
        )
        .unwrap();

        // Query for snapshots since t2 (should include t2 and t3)
        let snapshots = get_snapshots_since(&conn, t2).expect("Query should succeed");
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].file_path, "/tmp/c.txt"); // newest first
        assert_eq!(snapshots[1].file_path, "/tmp/b.txt");
    }

    #[test]
    fn get_latest_snapshot_at_specific_time() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 15, 14, 0, 0).unwrap();

        insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v2".to_string()),
            200,
            t2,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v3".to_string()),
            300,
            t3,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .unwrap();

        // Query at t2 should return v2
        let snapshot = get_latest_snapshot_at(&conn, "/tmp/test.txt", t2)
            .expect("Query should succeed")
            .expect("Should find snapshot");
        assert_eq!(snapshot.content_hash, ContentHash("v2".to_string()));

        // Query before t1 should return None
        let t0 = Utc.with_ymd_and_hms(2025, 1, 15, 9, 0, 0).unwrap();
        let snapshot =
            get_latest_snapshot_at(&conn, "/tmp/test.txt", t0).expect("Query should succeed");
        assert!(snapshot.is_none());
    }

    #[test]
    fn get_all_tracked_files_returns_unique_paths() {
        let conn = open_test_db();
        let timestamp = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        insert_snapshot(
            &conn,
            "/tmp/a.txt",
            &ContentHash("hash1".to_string()),
            100,
            timestamp,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/a.txt",
            &ContentHash("hash2".to_string()),
            200,
            timestamp,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/b.txt",
            &ContentHash("hash3".to_string()),
            300,
            timestamp,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        let files = get_all_tracked_files(&conn).expect("Query should succeed");
        assert_eq!(files.len(), 2);
        assert!(files.contains(&"/tmp/a.txt".to_string()));
        assert!(files.contains(&"/tmp/b.txt".to_string()));
    }

    #[test]
    fn snapshot_count_is_accurate() {
        let conn = open_test_db();
        let timestamp = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        assert_eq!(get_snapshot_count(&conn).unwrap(), 0);

        insert_snapshot(
            &conn,
            "/tmp/a.txt",
            &ContentHash("hash1".to_string()),
            100,
            timestamp,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        assert_eq!(get_snapshot_count(&conn).unwrap(), 1);

        insert_snapshot(
            &conn,
            "/tmp/b.txt",
            &ContentHash("hash2".to_string()),
            200,
            timestamp,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        assert_eq!(get_snapshot_count(&conn).unwrap(), 2);
    }

    #[test]
    fn tracked_file_count_is_accurate() {
        let conn = open_test_db();
        let timestamp = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        assert_eq!(get_tracked_file_count(&conn).unwrap(), 0);

        insert_snapshot(
            &conn,
            "/tmp/a.txt",
            &ContentHash("hash1".to_string()),
            100,
            timestamp,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        assert_eq!(get_tracked_file_count(&conn).unwrap(), 1);

        // Same file, different snapshot
        insert_snapshot(
            &conn,
            "/tmp/a.txt",
            &ContentHash("hash2".to_string()),
            200,
            timestamp,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .unwrap();

        assert_eq!(get_tracked_file_count(&conn).unwrap(), 1);

        // Different file
        insert_snapshot(
            &conn,
            "/tmp/b.txt",
            &ContentHash("hash3".to_string()),
            300,
            timestamp,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        assert_eq!(get_tracked_file_count(&conn).unwrap(), 2);
    }

    #[test]
    fn empty_database_queries_return_empty_results() {
        let conn = open_test_db();

        let snapshots =
            get_snapshots_for_file(&conn, "/tmp/nonexistent.txt").expect("Query should succeed");
        assert_eq!(snapshots.len(), 0);

        let timestamp = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let snapshots = get_snapshots_since(&conn, timestamp).expect("Query should succeed");
        assert_eq!(snapshots.len(), 0);

        let snapshot = get_latest_snapshot_at(&conn, "/tmp/test.txt", timestamp)
            .expect("Query should succeed");
        assert!(snapshot.is_none());

        let files = get_all_tracked_files(&conn).expect("Query should succeed");
        assert_eq!(files.len(), 0);

        assert_eq!(get_snapshot_count(&conn).unwrap(), 0);
        assert_eq!(get_tracked_file_count(&conn).unwrap(), 0);
    }

    #[test]
    fn event_type_roundtrip() {
        assert_eq!(event_type_to_str(&EventType::Create), "create");
        assert_eq!(event_type_to_str(&EventType::Modify), "modify");
        assert_eq!(event_type_to_str(&EventType::Delete), "delete");

        assert_eq!(event_type_from_str("create").unwrap(), EventType::Create);
        assert_eq!(event_type_from_str("modify").unwrap(), EventType::Modify);
        assert_eq!(event_type_from_str("delete").unwrap(), EventType::Delete);

        assert!(event_type_from_str("invalid").is_err());
    }

    #[test]
    fn snapshots_ordered_by_time_descending() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 15, 14, 0, 0).unwrap();

        insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v3".to_string()),
            300,
            t3,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v2".to_string()),
            200,
            t2,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .unwrap();

        let snapshots = get_snapshots_for_file(&conn, "/tmp/test.txt").unwrap();
        assert_eq!(snapshots.len(), 3);
        assert_eq!(snapshots[0].timestamp, t3); // newest first
        assert_eq!(snapshots[1].timestamp, t2);
        assert_eq!(snapshots[2].timestamp, t1);
    }

    #[test]
    fn get_oldest_snapshot_time_returns_earliest() {
        let conn = open_test_db();

        // Empty database should return None
        let oldest = get_oldest_snapshot_time(&conn).expect("Query should succeed");
        assert!(oldest.is_none());

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 15, 14, 0, 0).unwrap();

        // Insert in non-chronological order
        insert_snapshot(
            &conn,
            "/tmp/a.txt",
            &ContentHash("hash1".to_string()),
            100,
            t2,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/b.txt",
            &ContentHash("hash2".to_string()),
            200,
            t3,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/c.txt",
            &ContentHash("hash3".to_string()),
            300,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        let oldest = get_oldest_snapshot_time(&conn)
            .expect("Query should succeed")
            .expect("Should find oldest timestamp");
        assert_eq!(oldest, t1);
    }

    #[test]
    fn get_newest_snapshot_time_empty_db() {
        let conn = open_test_db();

        // Empty database should return None
        let newest = get_newest_snapshot_time(&conn).expect("Query should succeed");
        assert!(newest.is_none());
    }

    #[test]
    fn get_newest_snapshot_time_returns_latest() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 15, 14, 0, 0).unwrap();

        // Insert in non-chronological order
        insert_snapshot(
            &conn,
            "/tmp/a.txt",
            &ContentHash("hash1".to_string()),
            100,
            t2,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/b.txt",
            &ContentHash("hash2".to_string()),
            200,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/c.txt",
            &ContentHash("hash3".to_string()),
            300,
            t3,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        let newest = get_newest_snapshot_time(&conn)
            .expect("Query should succeed")
            .expect("Should find newest timestamp");
        assert_eq!(newest, t3);
    }

    // --- Keyset Pagination Tests ---

    #[test]
    fn history_page_first_page_returns_correct_results() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v2".to_string()),
            200,
            t2,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v3".to_string()),
            300,
            t3,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .unwrap();

        let snapshots = get_history_page(&conn, HistoryScope::All, None, 10, None)
            .expect("Query should succeed");

        assert_eq!(snapshots.len(), 3);
        // Newest first
        assert_eq!(snapshots[0].timestamp, t3);
        assert_eq!(snapshots[1].timestamp, t2);
        assert_eq!(snapshots[2].timestamp, t1);
    }

    #[test]
    fn history_page_cursor_pagination_returns_next_page() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t4 = Utc.with_ymd_and_hms(2025, 1, 15, 13, 0, 0).unwrap();

        insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v2".to_string()),
            200,
            t2,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v3".to_string()),
            300,
            t3,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v4".to_string()),
            400,
            t4,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .unwrap();

        // Get first page (2 results)
        let page1 = get_history_page(&conn, HistoryScope::All, None, 2, None)
            .expect("Query should succeed");
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].timestamp, t4);
        assert_eq!(page1[1].timestamp, t3);

        // Get second page using cursor from last item of page1
        let cursor = HistoryCursor {
            timestamp: page1[1].timestamp,
            id: page1[1].id,
        };
        let page2 = get_history_page(&conn, HistoryScope::All, Some(&cursor), 2, None)
            .expect("Query should succeed");
        assert_eq!(page2.len(), 2);
        assert_eq!(page2[0].timestamp, t2);
        assert_eq!(page2[1].timestamp, t1);
    }

    #[test]
    fn history_page_file_scope_filters_exact_file() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();

        insert_snapshot(
            &conn,
            "/tmp/a.txt",
            &ContentHash("v1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/b.txt",
            &ContentHash("v2".to_string()),
            200,
            t2,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        let snapshots = get_history_page(&conn, HistoryScope::File("/tmp/a.txt"), None, 10, None)
            .expect("Query should succeed");

        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].file_path, "/tmp/a.txt");
    }

    #[test]
    fn history_page_directory_scope_matches_prefix() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        insert_snapshot(
            &conn,
            "/tmp/src/a.txt",
            &ContentHash("v1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/src/b.txt",
            &ContentHash("v2".to_string()),
            200,
            t2,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/other.txt",
            &ContentHash("v3".to_string()),
            300,
            t3,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        let snapshots =
            get_history_page(&conn, HistoryScope::Directory("/tmp/src/"), None, 10, None)
                .expect("Query should succeed");

        assert_eq!(snapshots.len(), 2);
        assert!(snapshots[0].file_path.starts_with("/tmp/src/"));
        assert!(snapshots[1].file_path.starts_with("/tmp/src/"));
    }

    #[test]
    fn history_page_all_scope_returns_everything() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        insert_snapshot(
            &conn,
            "/tmp/a.txt",
            &ContentHash("v1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/b.txt",
            &ContentHash("v2".to_string()),
            200,
            t2,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/var/c.txt",
            &ContentHash("v3".to_string()),
            300,
            t3,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        let snapshots = get_history_page(&conn, HistoryScope::All, None, 10, None)
            .expect("Query should succeed");

        assert_eq!(snapshots.len(), 3);
    }

    #[test]
    fn history_page_empty_history_returns_empty_vec() {
        let conn = open_test_db();

        let snapshots = get_history_page(&conn, HistoryScope::All, None, 10, None)
            .expect("Query should succeed");

        assert_eq!(snapshots.len(), 0);
    }

    #[test]
    fn history_page_partial_last_page() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();

        insert_snapshot(
            &conn,
            "/tmp/a.txt",
            &ContentHash("v1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/b.txt",
            &ContentHash("v2".to_string()),
            200,
            t2,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        // Request 10 items but only 2 exist
        let snapshots = get_history_page(&conn, HistoryScope::All, None, 10, None)
            .expect("Query should succeed");

        assert_eq!(snapshots.len(), 2);
    }

    #[test]
    fn history_page_since_parameter_filters_correctly() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        insert_snapshot(
            &conn,
            "/tmp/a.txt",
            &ContentHash("v1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/b.txt",
            &ContentHash("v2".to_string()),
            200,
            t2,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/c.txt",
            &ContentHash("v3".to_string()),
            300,
            t3,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        // Query since t2 (should include t2 and t3, not t1)
        let snapshots = get_history_page(&conn, HistoryScope::All, None, 10, Some(t2))
            .expect("Query should succeed");

        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].timestamp, t3);
        assert_eq!(snapshots[1].timestamp, t2);
    }

    #[test]
    fn history_page_cursor_at_end_returns_empty() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();

        let id = insert_snapshot(
            &conn,
            "/tmp/a.txt",
            &ContentHash("v1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        // Cursor pointing to the only snapshot
        let cursor = HistoryCursor { timestamp: t1, id };

        let snapshots = get_history_page(&conn, HistoryScope::All, Some(&cursor), 10, None)
            .expect("Query should succeed");

        assert_eq!(snapshots.len(), 0);
    }

    #[test]
    fn history_page_multiple_files_interleaved_chronologically() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t4 = Utc.with_ymd_and_hms(2025, 1, 15, 13, 0, 0).unwrap();

        // Interleave two files chronologically
        insert_snapshot(
            &conn,
            "/tmp/src/a.txt",
            &ContentHash("a1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/src/b.txt",
            &ContentHash("b1".to_string()),
            200,
            t2,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/src/a.txt",
            &ContentHash("a2".to_string()),
            150,
            t3,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/src/b.txt",
            &ContentHash("b2".to_string()),
            250,
            t4,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .unwrap();

        // Directory scope should get all 4, in chronological order (newest first)
        let snapshots =
            get_history_page(&conn, HistoryScope::Directory("/tmp/src/"), None, 10, None)
                .expect("Query should succeed");

        assert_eq!(snapshots.len(), 4);
        assert_eq!(snapshots[0].timestamp, t4);
        assert_eq!(snapshots[0].file_path, "/tmp/src/b.txt");
        assert_eq!(snapshots[1].timestamp, t3);
        assert_eq!(snapshots[1].file_path, "/tmp/src/a.txt");
        assert_eq!(snapshots[2].timestamp, t2);
        assert_eq!(snapshots[2].file_path, "/tmp/src/b.txt");
        assert_eq!(snapshots[3].timestamp, t1);
        assert_eq!(snapshots[3].file_path, "/tmp/src/a.txt");
    }

    // --- get_previous_snapshot Tests ---

    #[test]
    fn get_previous_snapshot_no_previous() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();

        let id1 = insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        // First snapshot has no previous
        let previous =
            get_previous_snapshot(&conn, "/tmp/test.txt", t1, id1).expect("Query should succeed");
        assert!(previous.is_none());
    }

    #[test]
    fn get_previous_snapshot_returns_preceding() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        let id1 = insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        let id2 = insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v2".to_string()),
            200,
            t2,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .unwrap();

        let id3 = insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v3".to_string()),
            300,
            t3,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .unwrap();

        // Ask for previous of the 3rd snapshot, should get the 2nd
        let previous = get_previous_snapshot(&conn, "/tmp/test.txt", t3, id3)
            .expect("Query should succeed")
            .expect("Should find previous snapshot");
        assert_eq!(previous.id, id2);
        assert_eq!(previous.content_hash, ContentHash("v2".to_string()));

        // Ask for previous of the 2nd snapshot, should get the 1st
        let previous = get_previous_snapshot(&conn, "/tmp/test.txt", t2, id2)
            .expect("Query should succeed")
            .expect("Should find previous snapshot");
        assert_eq!(previous.id, id1);
        assert_eq!(previous.content_hash, ContentHash("v1".to_string()));
    }

    #[test]
    fn get_previous_snapshot_same_timestamp_tiebreak() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();

        // Insert two snapshots at the same timestamp (different IDs)
        let id1 = insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        let id2 = insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v2".to_string()),
            200,
            t1,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .unwrap();

        // Ask for previous of id2 at timestamp t1, should get id1
        let previous = get_previous_snapshot(&conn, "/tmp/test.txt", t1, id2)
            .expect("Query should succeed")
            .expect("Should find previous snapshot");
        assert_eq!(previous.id, id1);
        assert_eq!(previous.content_hash, ContentHash("v1".to_string()));

        // Ask for previous of id1 at timestamp t1, should get None
        let previous =
            get_previous_snapshot(&conn, "/tmp/test.txt", t1, id1).expect("Query should succeed");
        assert!(previous.is_none());
    }

    #[test]
    fn get_previous_snapshot_different_file_ignored() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();

        // Insert snapshot for a different file at earlier time
        insert_snapshot(
            &conn,
            "/tmp/other.txt",
            &ContentHash("other".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        // Insert snapshot for our file
        let id2 = insert_snapshot(
            &conn,
            "/tmp/test.txt",
            &ContentHash("v1".to_string()),
            200,
            t2,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        // Ask for previous of test.txt, should get None (other.txt is ignored)
        let previous =
            get_previous_snapshot(&conn, "/tmp/test.txt", t2, id2).expect("Query should succeed");
        assert!(previous.is_none());
    }

    // --- Pruning Function Tests ---

    #[test]
    fn count_snapshots_before_empty_db() {
        let conn = open_test_db();
        let cutoff = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let count = count_snapshots_before(&conn, cutoff).expect("Query should succeed");
        assert_eq!(count, 0);
    }

    #[test]
    fn count_snapshots_before_with_data() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t4 = Utc.with_ymd_and_hms(2025, 1, 15, 13, 0, 0).unwrap();

        insert_snapshot(
            &conn,
            "/tmp/a.txt",
            &ContentHash("hash1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/b.txt",
            &ContentHash("hash2".to_string()),
            200,
            t2,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/c.txt",
            &ContentHash("hash3".to_string()),
            300,
            t3,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/d.txt",
            &ContentHash("hash4".to_string()),
            400,
            t4,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        // Cutoff at t3 should count t1 and t2 (2 snapshots before t3)
        let count = count_snapshots_before(&conn, t3).expect("Query should succeed");
        assert_eq!(count, 2);

        // Cutoff before all snapshots should return 0
        let t0 = Utc.with_ymd_and_hms(2025, 1, 15, 9, 0, 0).unwrap();
        let count = count_snapshots_before(&conn, t0).expect("Query should succeed");
        assert_eq!(count, 0);

        // Cutoff after all snapshots should return all 4
        let t5 = Utc.with_ymd_and_hms(2025, 1, 15, 14, 0, 0).unwrap();
        let count = count_snapshots_before(&conn, t5).expect("Query should succeed");
        assert_eq!(count, 4);
    }

    #[test]
    fn delete_snapshots_before_removes_old() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t4 = Utc.with_ymd_and_hms(2025, 1, 15, 13, 0, 0).unwrap();

        insert_snapshot(
            &conn,
            "/tmp/a.txt",
            &ContentHash("hash1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/b.txt",
            &ContentHash("hash2".to_string()),
            200,
            t2,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/c.txt",
            &ContentHash("hash3".to_string()),
            300,
            t3,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/d.txt",
            &ContentHash("hash4".to_string()),
            400,
            t4,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        // Delete snapshots before t3 (should remove t1 and t2)
        let deleted = delete_snapshots_before(&conn, t3).expect("Delete should succeed");
        assert_eq!(deleted, 2);

        // Verify only t3 and t4 remain
        let remaining = get_snapshot_count(&conn).expect("Query should succeed");
        assert_eq!(remaining, 2);

        // Verify the remaining snapshots are t3 and t4
        let snapshots = get_snapshots_since(&conn, t3).expect("Query should succeed");
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].timestamp, t4);
        assert_eq!(snapshots[1].timestamp, t3);
    }

    #[test]
    fn delete_snapshots_before_empty_db() {
        let conn = open_test_db();
        let cutoff = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let deleted = delete_snapshots_before(&conn, cutoff).expect("Delete should succeed");
        assert_eq!(deleted, 0);
    }

    #[test]
    fn get_referenced_hashes_empty() {
        let conn = open_test_db();
        let hashes = get_referenced_hashes(&conn).expect("Query should succeed");
        assert_eq!(hashes.len(), 0);
    }

    #[test]
    fn get_referenced_hashes_returns_distinct() {
        let conn = open_test_db();

        let t1 = Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        // Insert snapshots with some duplicate hashes
        insert_snapshot(
            &conn,
            "/tmp/a.txt",
            &ContentHash("hash1".to_string()),
            100,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/b.txt",
            &ContentHash("hash2".to_string()),
            200,
            t2,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        insert_snapshot(
            &conn,
            "/tmp/c.txt",
            &ContentHash("hash1".to_string()), // Duplicate hash
            100,
            t3,
            &EventType::Create,
            0,
            0,
            0,
        )
        .unwrap();

        let hashes = get_referenced_hashes(&conn).expect("Query should succeed");

        // Should have exactly 2 distinct hashes
        assert_eq!(hashes.len(), 2);
        assert!(hashes.contains("hash1"));
        assert!(hashes.contains("hash2"));
    }
}
