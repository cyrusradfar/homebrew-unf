//! SQLite metadata layer for the UNFUDGED flight recorder.
//!
//! This module manages the relational metadata for all snapshots. The schema
//! is designed for high-velocity writes with minimal indexing overhead, and
//! efficient queries by file path and time range.
//!
//! Schema version tracking allows future migrations. WAL mode is enabled for
//! concurrent read/write access.

use crate::error::DbError;
use rusqlite::Connection;
use std::path::Path;

// Private submodules
mod helpers;
mod schema;

// Public submodules (re-exported below)
pub mod query;
pub mod types;
pub mod write;

// Test module
#[cfg(test)]
mod tests;

// Re-export public types and functions for cleaner API
pub use query::{
    count_snapshots_before, get_all_snapshot_timestamps, get_all_tracked_files, get_history_page,
    get_latest_snapshot_at, get_newest_snapshot_time, get_oldest_snapshot_time,
    get_previous_snapshot, get_referenced_hashes, get_snapshot_by_id, get_snapshot_count,
    get_snapshots_for_file, get_snapshots_since, get_tracked_file_count,
};
pub use types::{HistoryCursor, HistoryScope, QueryBuilder};
pub use write::{delete_snapshots_before, insert_snapshot};

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

    schema::init_schema(&conn)?;

    Ok(conn)
}
