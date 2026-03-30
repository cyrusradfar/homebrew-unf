//! Schema initialization and migrations.

use crate::error::DbError;
use rusqlite::Connection;

/// Current schema version.
///
/// Increment this when making schema changes, and add migration logic
/// to `init_schema()`.
const SCHEMA_VERSION: i64 = 2;

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
pub fn init_schema(conn: &Connection) -> Result<(), DbError> {
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
            [SCHEMA_VERSION],
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
