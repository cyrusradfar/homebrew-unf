//! Storage engine for the UNFUDGED flight recorder.
//!
//! This module contains the core storage subsystems:
//! - Content-Addressable Store (CAS) for immutable object storage
//! - SQLite metadata layer for snapshot tracking
//! - Engine facade that coordinates CAS + DB operations
//!
//! The [`Engine`] struct provides a unified interface for all storage operations,
//! hiding the complexity of coordinating between the CAS layer and SQLite metadata.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::Connection;

use crate::error::{CasError, UnfError};
use crate::types::{ContentHash, EventType, Snapshot};

/// Snapshot metadata: (content_hash, size_bytes, line_count, lines_added, lines_removed)
type SnapshotData = (ContentHash, u64, u64, u64, u64);

/// Statistics from a prune operation.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct PruneStats {
    /// Number of snapshot rows deleted from the database.
    pub snapshots_removed: u64,
    /// Number of CAS objects removed by garbage collection.
    pub objects_removed: u64,
    /// Total bytes freed from CAS object store.
    pub bytes_freed: u64,
}

pub mod cas;
pub mod db;

/// SQLite database filename.
const DB_FILENAME: &str = "db.sqlite3";

/// Object store directory name.
const OBJECTS_DIR: &str = "objects";

/// Sentinel hash value for deleted files (all zeros).
const EMPTY_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Recursively walks a directory tree and sums the size of all files.
///
/// Used by [`Engine::get_store_size`] to calculate total object store size.
/// Accumulates file sizes into the provided mutable reference.
fn walk_dir_sum(path: &Path, total: &mut u64) -> Result<(), UnfError> {
    if path.is_file() {
        let metadata = fs::metadata(path).map_err(CasError::Io)?;
        *total += metadata.len();
    } else if path.is_dir() {
        for entry in fs::read_dir(path).map_err(CasError::Io)? {
            let entry = entry.map_err(CasError::Io)?;
            walk_dir_sum(&entry.path(), total)?;
        }
    }
    Ok(())
}

/// The main storage engine for UNFUDGED.
///
/// Coordinates operations between the Content-Addressable Store (CAS) and
/// the SQLite metadata database. Owns the database connection and stores
/// paths to the project root and objects directory.
///
/// # SUPER Compliance
///
/// This struct follows the SUPER principle by isolating side effects:
/// - Pure logic: hash computation, path manipulation
/// - Side effects at edge: file I/O, database writes, filesystem reads
///
/// All public methods use `Result` for error handling - no panics.
pub struct Engine {
    /// SQLite connection for metadata storage.
    conn: Connection,
    /// Path to the objects directory (CAS store).
    objects_path: PathBuf,
    /// Root directory of the project being tracked.
    project_root: PathBuf,
}

impl Engine {
    /// Initializes a new UNFUDGED storage engine.
    ///
    /// Creates the storage directory structure if it doesn't exist,
    /// initializes the SQLite database, and returns an `Engine` ready for use.
    ///
    /// # Arguments
    ///
    /// * `project_root` - The root directory of the project to track
    /// * `storage_dir` - The centralized storage directory for this project
    ///
    /// # Returns
    ///
    /// An initialized `Engine` connected to the database.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Db` if database initialization fails or
    /// `UnfError::Cas` if directory creation fails.
    pub fn init(project_root: &Path, storage_dir: &Path) -> Result<Engine, UnfError> {
        let db_path = storage_dir.join(DB_FILENAME);
        let objects_path = storage_dir.join(OBJECTS_DIR);

        // Create storage and objects/ directories
        fs::create_dir_all(&objects_path).map_err(CasError::Io)?;

        // Open and initialize the database
        let conn = db::open_db(&db_path)?;

        Ok(Engine {
            conn,
            objects_path,
            project_root: project_root.to_path_buf(),
        })
    }

    /// Opens an existing UNFUDGED storage engine.
    ///
    /// Verifies that the storage directory exists and opens the database.
    /// Used by read-only commands like `unf log` and `unf diff`.
    ///
    /// # Arguments
    ///
    /// * `project_root` - The root directory of the tracked project
    /// * `storage_dir` - The centralized storage directory for this project
    ///
    /// # Returns
    ///
    /// An `Engine` connected to the existing database.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::NotInitialized` if the storage directory does not exist,
    /// or `UnfError::Db` if the database cannot be opened.
    pub fn open(project_root: &Path, storage_dir: &Path) -> Result<Engine, UnfError> {
        if !storage_dir.exists() {
            return Err(UnfError::NotInitialized);
        }

        let db_path = storage_dir.join(DB_FILENAME);
        let objects_path = storage_dir.join(OBJECTS_DIR);

        let conn = db::open_db(&db_path)?;

        Ok(Engine {
            conn,
            objects_path,
            project_root: project_root.to_path_buf(),
        })
    }

    /// Creates a snapshot of a file.
    ///
    /// For `Create` and `Modify` events, reads the file from disk, hashes its content,
    /// stores it in the CAS, and inserts a snapshot record in the database.
    /// Returns `None` if the file content is detected to be binary (defense-in-depth check).
    ///
    /// For `Delete` events, inserts a snapshot record with an empty hash sentinel
    /// and zero size.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Path to the file (relative to project root)
    /// * `event_type` - The type of filesystem event
    ///
    /// # Returns
    ///
    /// `Ok(Some(snapshot))` for successfully created snapshots.
    /// `Ok(None)` if the file content is detected as binary.
    /// `Err` for filesystem or database errors.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Cas` if file reading or CAS storage fails,
    /// or `UnfError::Db` if database insertion fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use unfudged::engine::Engine;
    /// use unfudged::types::EventType;
    /// use std::path::Path;
    ///
    /// let engine = Engine::open(Path::new("/my/project"), Path::new("/storage/dir")).unwrap();
    /// if let Ok(Some(snapshot)) = engine.create_snapshot("src/main.rs", EventType::Modify) {
    ///     println!("Created snapshot: {}", snapshot.id);
    /// }
    /// ```
    pub fn create_snapshot(
        &self,
        file_path: &str,
        event_type: EventType,
    ) -> Result<Option<Snapshot>, UnfError> {
        let timestamp = Utc::now();

        let snapshot_data = match event_type {
            EventType::Create | EventType::Modify => {
                self.snapshot_create_or_modify(file_path, timestamp)?
            }
            EventType::Delete => self.snapshot_delete(file_path, timestamp)?,
        };

        // Return None if snapshot_data is None (binary file or no actual change)
        let (content_hash, size_bytes, line_count, lines_added, lines_removed) = match snapshot_data
        {
            Some(data) => data,
            None => return Ok(None),
        };

        // Insert snapshot record in database
        let id = db::insert_snapshot(
            &self.conn,
            file_path,
            &content_hash,
            size_bytes,
            timestamp,
            &event_type,
            line_count,
            lines_added,
            lines_removed,
        )?;

        Ok(Some(Snapshot {
            id,
            file_path: file_path.to_string(),
            content_hash,
            size_bytes,
            timestamp,
            event_type,
            line_count,
            lines_added,
            lines_removed,
        }))
    }

    /// Handles snapshot creation for Create/Modify events.
    ///
    /// Reads file content, validates against binary detection, hashes and stores in CAS,
    /// and computes diff statistics against previous version.
    ///
    /// Returns `Ok(None)` if content is binary (defense-in-depth check) or
    /// if hash matches previous snapshot (no actual change).
    fn snapshot_create_or_modify(
        &self,
        file_path: &str,
        timestamp: DateTime<Utc>,
    ) -> Result<Option<SnapshotData>, UnfError> {
        // Read file content from disk
        let full_path = self.project_root.join(file_path);
        let content = fs::read(&full_path).map_err(CasError::Io)?;

        // Defense-in-depth: skip binary content even if extension wasn't caught
        if crate::watcher::filter::is_likely_binary(&content) {
            return Ok(None);
        }

        let size = content.len() as u64;

        // Hash and store in CAS
        let hash = cas::hash_content(&content);
        cas::store_object(&self.objects_path, &hash, &content)?;

        // Compute line count for this snapshot
        let line_count = cas::count_lines(&content);

        // Look up the previous snapshot to compute diff stats.
        // Returns None if content hash is identical (dedup — no actual change).
        let (lines_added, lines_removed) =
            match self.compute_diff_stats(file_path, timestamp, &hash, &content, line_count)? {
                Some(stats) => stats,
                None => return Ok(None), // Hash matched previous — skip
            };

        Ok(Some((hash, size, line_count, lines_added, lines_removed)))
    }

    /// Handles snapshot creation for Delete events.
    ///
    /// Retrieves the previous snapshot to determine how many lines were removed.
    fn snapshot_delete(
        &self,
        file_path: &str,
        timestamp: DateTime<Utc>,
    ) -> Result<Option<SnapshotData>, UnfError> {
        // For delete events, get the previous snapshot to find how many lines were removed
        let lines_removed = match self.get_latest_snapshot_for_file(file_path, timestamp) {
            Ok(Some(prev)) => prev.line_count,
            _ => 0,
        };

        // Use empty hash sentinel for deleted files
        Ok(Some((
            ContentHash(EMPTY_HASH.to_string()),
            0,
            0,
            0,
            lines_removed,
        )))
    }

    /// Computes diff statistics between previous and current file content.
    ///
    /// Returns `Ok(None)` if content hash matches previous snapshot (dedup).
    /// Returns `Ok(Some((added, removed)))` otherwise.
    fn compute_diff_stats(
        &self,
        file_path: &str,
        timestamp: DateTime<Utc>,
        hash: &ContentHash,
        content: &[u8],
        line_count: u64,
    ) -> Result<Option<(u64, u64)>, UnfError> {
        match self.get_latest_snapshot_for_file(file_path, timestamp) {
            Ok(Some(prev)) => {
                // Content hash identical to previous — no actual change
                if prev.content_hash == *hash {
                    return Ok(None);
                }

                // Load previous content and compute diff
                match self.load_content(&prev.content_hash) {
                    Ok(old_content) => {
                        let stats = crate::diff::compute_diff_stats(&old_content, content);
                        Ok(Some((stats.lines_added as u64, stats.lines_removed as u64)))
                    }
                    Err(_) => {
                        // If we can't load previous content, fall back to zeros
                        Ok(Some((0, 0)))
                    }
                }
            }
            Ok(None) => {
                // First snapshot for this file: all lines are "added"
                Ok(Some((line_count, 0)))
            }
            Err(_) => {
                // If lookup fails, fall back to zeros
                Ok(Some((0, 0)))
            }
        }
    }

    /// Retrieves all snapshots for a specific file.
    ///
    /// Returns snapshots in descending chronological order (newest first).
    /// Used by `unf log <file>` to show the file's history.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Path to the file (relative to project root)
    ///
    /// # Returns
    ///
    /// A vector of snapshots, newest first. Empty if the file has no history.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Db` if the database query fails.
    pub fn get_file_history(&self, file_path: &str) -> Result<Vec<Snapshot>, UnfError> {
        let snapshots = db::get_snapshots_for_file(&self.conn, file_path)?;
        Ok(snapshots)
    }

    /// Retrieves all snapshots since a specific time.
    ///
    /// Returns snapshots at or after the given timestamp, in descending
    /// chronological order. Used by `unf diff --since <time>`.
    ///
    /// # Arguments
    ///
    /// * `since` - UTC timestamp (inclusive lower bound)
    ///
    /// # Returns
    ///
    /// A vector of snapshots, newest first.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Db` if the database query fails.
    pub fn get_snapshots_since(&self, since: DateTime<Utc>) -> Result<Vec<Snapshot>, UnfError> {
        let snapshots = db::get_snapshots_since(&self.conn, since)?;
        Ok(snapshots)
    }

    /// Reconstructs the state of all files at a specific point in time.
    ///
    /// For each tracked file, returns the latest snapshot at or before the
    /// target time. Used by `unf restore --at <time>` to reconstruct historical
    /// project state.
    ///
    /// # Arguments
    ///
    /// * `at_time` - UTC timestamp to reconstruct state at
    ///
    /// # Returns
    ///
    /// A map from file path to its snapshot at the target time. Files with no
    /// snapshot before `at_time` are not included.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Db` if database queries fail.
    pub fn get_state_at(
        &self,
        at_time: DateTime<Utc>,
    ) -> Result<HashMap<String, Snapshot>, UnfError> {
        let files = db::get_all_tracked_files(&self.conn)?;
        let mut state = HashMap::new();

        for file_path in files {
            if let Some(snapshot) = db::get_latest_snapshot_at(&self.conn, &file_path, at_time)? {
                state.insert(file_path, snapshot);
            }
        }

        Ok(state)
    }

    /// Retrieves a snapshot by its ID.
    ///
    /// Used by `unf cat --snapshot <id>`.
    ///
    /// # Arguments
    ///
    /// * `id` - The snapshot ID to look up
    ///
    /// # Returns
    ///
    /// The snapshot if found, or `None`.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Db` if the database query fails.
    pub fn get_snapshot_by_id(
        &self,
        id: crate::types::SnapshotId,
    ) -> Result<Option<Snapshot>, UnfError> {
        let snapshot = db::get_snapshot_by_id(&self.conn, id)?;
        Ok(snapshot)
    }

    /// Retrieves the latest snapshot for a file at or before a given time.
    ///
    /// Used by `unf cat --at <time>`.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Path to the file (relative to project root)
    /// * `at_time` - UTC timestamp (inclusive upper bound)
    ///
    /// # Returns
    ///
    /// The most recent snapshot at or before `at_time`, or `None`.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Db` if the database query fails.
    pub fn get_latest_snapshot_for_file(
        &self,
        file_path: &str,
        at_time: DateTime<Utc>,
    ) -> Result<Option<Snapshot>, UnfError> {
        let snapshot = db::get_latest_snapshot_at(&self.conn, file_path, at_time)?;
        Ok(snapshot)
    }

    /// Loads file content from the CAS by hash.
    ///
    /// Used by `unf diff` and `unf restore` to retrieve historical file content.
    ///
    /// # Arguments
    ///
    /// * `hash` - The content hash to load
    ///
    /// # Returns
    ///
    /// The file content as bytes.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Cas` if the object is not found or cannot be read.
    pub fn load_content(&self, hash: &ContentHash) -> Result<Vec<u8>, UnfError> {
        let content = cas::load_object(&self.objects_path, hash)?;
        Ok(content)
    }

    /// Retrieves all files that have been tracked (have at least one snapshot).
    ///
    /// Used by `unf restore` to determine which files need restoring.
    ///
    /// # Returns
    ///
    /// A vector of file paths in arbitrary order.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Db` if the database query fails.
    pub fn get_all_tracked_files(&self) -> Result<Vec<String>, UnfError> {
        let files = db::get_all_tracked_files(&self.conn)?;
        Ok(files)
    }

    /// Returns the total number of snapshots in the database.
    ///
    /// Used by `unf status` to show storage statistics.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Db` if the database query fails.
    pub fn get_snapshot_count(&self) -> Result<u64, UnfError> {
        let count = db::get_snapshot_count(&self.conn)?;
        Ok(count)
    }

    /// Returns the number of unique files being tracked.
    ///
    /// Used by `unf status` to show storage statistics.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Db` if the database query fails.
    pub fn get_tracked_file_count(&self) -> Result<u64, UnfError> {
        let count = db::get_tracked_file_count(&self.conn)?;
        Ok(count)
    }

    /// Returns a page of history for the given scope.
    ///
    /// This is the primary method for the streaming history navigator.
    /// Callers iterate by passing back the cursor from the last snapshot
    /// in each page.
    ///
    /// # Arguments
    ///
    /// * `scope` - File, directory, or all
    /// * `cursor` - `None` for first page, `Some` for continuation
    /// * `page_size` - Number of snapshots per page
    /// * `since` - Optional lower bound on timestamp
    ///
    /// # Returns
    ///
    /// A page of snapshots, newest first. Empty when history is exhausted.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Db` if the database query fails.
    pub fn get_history_page(
        &self,
        scope: db::HistoryScope<'_>,
        cursor: Option<&db::HistoryCursor>,
        page_size: u32,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> Result<Vec<Snapshot>, UnfError> {
        let snapshots =
            db::get_history_page(&self.conn, scope, cursor, page_size, since, until)?;
        Ok(snapshots)
    }

    /// Returns all snapshot timestamps and file paths for density histogram computation.
    ///
    /// # Arguments
    ///
    /// * `since` - Optional lower bound (inclusive) for timestamps
    ///
    /// # Returns
    ///
    /// A vector of `(timestamp, file_path)` pairs.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Db` if the database query fails.
    pub fn get_all_snapshot_timestamps(
        &self,
        since: Option<DateTime<Utc>>,
    ) -> Result<Vec<(DateTime<Utc>, String)>, UnfError> {
        let timestamps = db::get_all_snapshot_timestamps(&self.conn, since)?;
        Ok(timestamps)
    }

    /// Returns the timestamp of the oldest snapshot in the database.
    ///
    /// Used by `unf status` to compute "Recording since X ago".
    ///
    /// # Returns
    ///
    /// `Some(timestamp)` if any snapshots exist, or `None` if the database is empty.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Db` if the database query fails.
    pub fn get_oldest_snapshot_time(&self) -> Result<Option<DateTime<Utc>>, UnfError> {
        let time = db::get_oldest_snapshot_time(&self.conn)?;
        Ok(time)
    }

    /// Returns the timestamp of the most recent snapshot in the database.
    ///
    /// # Returns
    ///
    /// `Some(timestamp)` if any snapshots exist, or `None` if the database is empty.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Db` if the database query fails.
    pub fn get_newest_snapshot_time(&self) -> Result<Option<DateTime<Utc>>, UnfError> {
        let time = db::get_newest_snapshot_time(&self.conn)?;
        Ok(time)
    }

    /// Retrieves the snapshot immediately before the given one for the same file.
    ///
    /// Used by `unf log --stats` to find the comparison baseline.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Path to the file (relative to project root)
    /// * `before_timestamp` - UTC timestamp to search before
    /// * `before_id` - Snapshot ID to search before
    ///
    /// # Returns
    ///
    /// The snapshot immediately preceding the given snapshot, or `None` if it's the first.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Db` if the database query fails.
    pub fn get_previous_snapshot(
        &self,
        file_path: &str,
        before_timestamp: DateTime<Utc>,
        before_id: crate::types::SnapshotId,
    ) -> Result<Option<Snapshot>, UnfError> {
        let snapshot =
            db::get_previous_snapshot(&self.conn, file_path, before_timestamp, before_id)?;
        Ok(snapshot)
    }

    /// Calculates the total size of the object store in bytes.
    ///
    /// Walks the `objects/` directory tree and sums file sizes.
    /// Used by `unf status` to show storage usage.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Cas` if directory traversal fails.
    pub fn get_store_size(&self) -> Result<u64, UnfError> {
        let mut total_size = 0u64;
        walk_dir_sum(&self.objects_path, &mut total_size)?;
        Ok(total_size)
    }

    /// Prunes old snapshots and garbage-collects unreferenced CAS objects.
    ///
    /// Three-step pipeline:
    /// 1. Delete (or count) snapshots older than `cutoff`
    /// 2. Get the set of all still-referenced content hashes
    /// 3. GC any CAS objects not in the referenced set
    ///
    /// When `dry_run` is true, nothing is deleted — only counts are returned.
    ///
    /// # Arguments
    ///
    /// * `cutoff` - UTC timestamp (snapshots before this are removed)
    /// * `dry_run` - If true, count but don't delete
    ///
    /// # Returns
    ///
    /// Statistics about what was (or would be) removed.
    ///
    /// # Errors
    ///
    /// Returns `UnfError::Db` if database operations fail, or
    /// `UnfError::Cas` if CAS operations fail.
    pub fn prune(&self, cutoff: DateTime<Utc>, dry_run: bool) -> Result<PruneStats, UnfError> {
        // Count or delete snapshots depending on dry_run mode
        let snapshots_removed = if dry_run {
            db::count_snapshots_before(&self.conn, cutoff)?
        } else {
            db::delete_snapshots_before(&self.conn, cutoff)?
        };

        // Get current referenced hashes and garbage-collect unreferenced objects
        let referenced = db::get_referenced_hashes(&self.conn)?;
        let gc_stats = cas::gc_unreferenced(&self.objects_path, &referenced, dry_run)?;

        Ok(PruneStats {
            snapshots_removed,
            objects_removed: gc_stats.objects_removed,
            bytes_freed: gc_stats.bytes_freed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Creates a temporary project and storage directory for testing.
    fn setup_test_project() -> (TempDir, TempDir) {
        let project = TempDir::new().expect("Failed to create project dir");
        let storage = TempDir::new().expect("Failed to create storage dir");
        (project, storage)
    }

    #[test]
    fn init_creates_directory_structure() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        // Verify db.sqlite3 exists in storage dir
        let db_path = storage.path().join(DB_FILENAME);
        assert!(db_path.exists());
        assert!(db_path.is_file());

        // Verify objects/ directory exists in storage dir
        let objects_path = storage.path().join(OBJECTS_DIR);
        assert!(objects_path.exists());
        assert!(objects_path.is_dir());

        // Verify engine fields are set correctly
        assert_eq!(engine.objects_path, objects_path);
        assert_eq!(engine.project_root, project.path());
    }

    #[test]
    fn init_is_idempotent() {
        let (project, storage) = setup_test_project();

        Engine::init(project.path(), storage.path()).expect("First init should succeed");
        Engine::init(project.path(), storage.path()).expect("Second init should succeed");

        assert!(storage.path().join(DB_FILENAME).exists());
    }

    #[test]
    fn open_fails_on_uninitialized_directory() {
        let project = TempDir::new().expect("create project dir");
        let storage_dir = PathBuf::from("/tmp/nonexistent_storage_dir_12345");

        let result = Engine::open(project.path(), &storage_dir);
        assert!(result.is_err());

        match result {
            Err(UnfError::NotInitialized) => (),
            _ => panic!("Expected NotInitialized error"),
        }
    }

    #[test]
    fn open_succeeds_after_init() {
        let (project, storage) = setup_test_project();
        Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let engine = Engine::open(project.path(), storage.path()).expect("Open should succeed");
        assert_eq!(engine.project_root, project.path());
    }

    #[test]
    fn create_snapshot_for_new_file() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        // Create a test file
        let file_path = "test.txt";
        let full_path = project.path().join(file_path);
        fs::write(&full_path, b"hello world").expect("Failed to write test file");

        // Create snapshot
        let snapshot = engine
            .create_snapshot(file_path, EventType::Create)
            .expect("Snapshot creation should succeed")
            .expect("Should be Some");

        assert_eq!(snapshot.file_path, file_path);
        assert_eq!(snapshot.size_bytes, 11);
        assert_eq!(snapshot.event_type, EventType::Create);
        assert_ne!(snapshot.content_hash.0, EMPTY_HASH);
    }

    #[test]
    fn create_snapshot_for_modified_file() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let file_path = "test.txt";
        let full_path = project.path().join(file_path);

        // Create and snapshot initial version
        fs::write(&full_path, b"version 1").expect("Failed to write");
        let snap1 = engine
            .create_snapshot(file_path, EventType::Create)
            .expect("First snapshot failed")
            .expect("Should be Some");

        // Modify and snapshot again
        fs::write(&full_path, b"version 2").expect("Failed to write");
        let snap2 = engine
            .create_snapshot(file_path, EventType::Modify)
            .expect("Second snapshot failed")
            .expect("Should be Some");

        assert_eq!(snap2.file_path, file_path);
        assert_eq!(snap2.size_bytes, 9);
        assert_eq!(snap2.event_type, EventType::Modify);
        assert_ne!(snap1.content_hash, snap2.content_hash);
    }

    #[test]
    fn create_snapshot_for_deleted_file() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let file_path = "deleted.txt";
        let snapshot = engine
            .create_snapshot(file_path, EventType::Delete)
            .expect("Delete snapshot should succeed")
            .expect("Should be Some");

        assert_eq!(snapshot.file_path, file_path);
        assert_eq!(snapshot.size_bytes, 0);
        assert_eq!(snapshot.event_type, EventType::Delete);
        assert_eq!(snapshot.content_hash.0, EMPTY_HASH);
    }

    #[test]
    fn get_file_history_returns_snapshots_in_order() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let file_path = "history.txt";
        let full_path = project.path().join(file_path);

        // Create three versions
        fs::write(&full_path, b"v1").expect("Failed to write");
        engine
            .create_snapshot(file_path, EventType::Create)
            .expect("Snapshot 1 failed")
            .expect("Should be Some");

        fs::write(&full_path, b"v2").expect("Failed to write");
        engine
            .create_snapshot(file_path, EventType::Modify)
            .expect("Snapshot 2 failed")
            .expect("Should be Some");

        fs::write(&full_path, b"v3").expect("Failed to write");
        engine
            .create_snapshot(file_path, EventType::Modify)
            .expect("Snapshot 3 failed")
            .expect("Should be Some");

        // Retrieve history
        let history = engine
            .get_file_history(file_path)
            .expect("History query failed");

        assert_eq!(history.len(), 3);
        assert_eq!(history[0].size_bytes, 2); // v3, newest first
        assert_eq!(history[1].size_bytes, 2); // v2
        assert_eq!(history[2].size_bytes, 2); // v1
    }

    #[test]
    fn get_snapshots_since_filters_by_time() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let file_path = "time_test.txt";
        let full_path = project.path().join(file_path);

        fs::write(&full_path, b"first").expect("Failed to write");
        engine
            .create_snapshot(file_path, EventType::Create)
            .expect("Snapshot failed")
            .expect("Should be Some");

        let cutoff_time = Utc::now();

        // Wait a tiny bit to ensure timestamp difference
        std::thread::sleep(std::time::Duration::from_millis(10));

        fs::write(&full_path, b"second").expect("Failed to write");
        engine
            .create_snapshot(file_path, EventType::Modify)
            .expect("Snapshot failed")
            .expect("Should be Some");

        let recent = engine
            .get_snapshots_since(cutoff_time)
            .expect("Query failed");

        // Should only return the second snapshot
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].size_bytes, 6);
    }

    #[test]
    fn get_state_at_reconstructs_file_state() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let file1 = "file1.txt";
        let file2 = "file2.txt";
        let full_path1 = project.path().join(file1);
        let full_path2 = project.path().join(file2);

        // Create file1
        fs::write(&full_path1, b"file1 v1").expect("Failed to write");
        engine
            .create_snapshot(file1, EventType::Create)
            .expect("Snapshot failed")
            .expect("Should be Some");

        let checkpoint = Utc::now();

        std::thread::sleep(std::time::Duration::from_millis(10));

        // Create file2 after checkpoint
        fs::write(&full_path2, b"file2 v1").expect("Failed to write");
        engine
            .create_snapshot(file2, EventType::Create)
            .expect("Snapshot failed")
            .expect("Should be Some");

        // State at checkpoint should only include file1
        let state = engine.get_state_at(checkpoint).expect("Query failed");
        assert_eq!(state.len(), 1);
        assert!(state.contains_key(file1));
        assert!(!state.contains_key(file2));
    }

    #[test]
    fn load_content_retrieves_stored_data() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let file_path = "content_test.txt";
        let full_path = project.path().join(file_path);
        let original_content = b"test content for retrieval";

        fs::write(&full_path, original_content).expect("Failed to write");
        let snapshot = engine
            .create_snapshot(file_path, EventType::Create)
            .expect("Snapshot failed")
            .expect("Should be Some");

        // Load content by hash
        let loaded = engine
            .load_content(&snapshot.content_hash)
            .expect("Load failed");

        assert_eq!(loaded, original_content);
    }

    #[test]
    fn get_snapshot_count_is_accurate() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        assert_eq!(engine.get_snapshot_count().unwrap(), 0);

        let file_path = "count_test.txt";
        let full_path = project.path().join(file_path);

        fs::write(&full_path, b"v1").expect("Failed to write");
        engine
            .create_snapshot(file_path, EventType::Create)
            .expect("Snapshot failed")
            .expect("Should be Some");
        assert_eq!(engine.get_snapshot_count().unwrap(), 1);

        fs::write(&full_path, b"v2").expect("Failed to write");
        engine
            .create_snapshot(file_path, EventType::Modify)
            .expect("Snapshot failed")
            .expect("Should be Some");
        assert_eq!(engine.get_snapshot_count().unwrap(), 2);
    }

    #[test]
    fn get_tracked_file_count_is_accurate() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        assert_eq!(engine.get_tracked_file_count().unwrap(), 0);

        let file1 = "file1.txt";
        let file2 = "file2.txt";
        let full_path1 = project.path().join(file1);
        let full_path2 = project.path().join(file2);

        fs::write(&full_path1, b"content").expect("Failed to write");
        engine
            .create_snapshot(file1, EventType::Create)
            .expect("Snapshot failed")
            .expect("Should be Some");
        assert_eq!(engine.get_tracked_file_count().unwrap(), 1);

        // Same file, second snapshot
        fs::write(&full_path1, b"modified").expect("Failed to write");
        engine
            .create_snapshot(file1, EventType::Modify)
            .expect("Snapshot failed")
            .expect("Should be Some");
        assert_eq!(engine.get_tracked_file_count().unwrap(), 1);

        // Different file
        fs::write(&full_path2, b"content").expect("Failed to write");
        engine
            .create_snapshot(file2, EventType::Create)
            .expect("Snapshot failed")
            .expect("Should be Some");
        assert_eq!(engine.get_tracked_file_count().unwrap(), 2);
    }

    #[test]
    fn get_store_size_calculates_correctly() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let initial_size = engine.get_store_size().expect("Size calculation failed");
        assert_eq!(initial_size, 0);

        let file_path = "size_test.txt";
        let full_path = project.path().join(file_path);
        let content = b"test content with known size";

        fs::write(&full_path, content).expect("Failed to write");
        engine
            .create_snapshot(file_path, EventType::Create)
            .expect("Snapshot failed")
            .expect("Should be Some");

        let size_after = engine.get_store_size().expect("Size calculation failed");
        assert!(size_after > 0);
        assert!(size_after >= content.len() as u64);
    }

    #[test]
    fn empty_file_history_returns_empty_vec() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let history = engine
            .get_file_history("nonexistent.txt")
            .expect("Query should succeed");
        assert_eq!(history.len(), 0);
    }

    #[test]
    fn load_nonexistent_content_returns_error() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let fake_hash = ContentHash("ff".repeat(32));
        let result = engine.load_content(&fake_hash);

        assert!(result.is_err());
    }

    #[test]
    fn get_history_page_paginates_through_engine() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let file_path = "paginate.txt";
        let full_path = project.path().join(file_path);

        // Create 5 snapshots
        for i in 0..5 {
            fs::write(&full_path, format!("v{}", i)).expect("Failed to write");
            engine
                .create_snapshot(file_path, EventType::Modify)
                .expect("Snapshot failed")
                .expect("Should be Some");
        }

        // First page of 3
        let page1 = engine
            .get_history_page(db::HistoryScope::File(file_path), None, 3, None, None)
            .expect("Page 1 failed");
        assert_eq!(page1.len(), 3);

        // Build cursor from last snapshot
        let cursor = db::HistoryCursor {
            timestamp: page1.last().unwrap().timestamp,
            id: page1.last().unwrap().id,
        };

        // Second page of 3 (should get 2 remaining)
        let page2 = engine
            .get_history_page(db::HistoryScope::File(file_path), Some(&cursor), 3, None, None)
            .expect("Page 2 failed");
        assert_eq!(page2.len(), 2);
    }

    #[test]
    fn get_oldest_and_newest_snapshot_time() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        // Empty database should return None for both
        assert!(engine.get_oldest_snapshot_time().unwrap().is_none());
        assert!(engine.get_newest_snapshot_time().unwrap().is_none());

        let file_path = "time_test.txt";
        let full_path = project.path().join(file_path);

        // Create first snapshot
        fs::write(&full_path, b"v1").expect("Failed to write");
        let snap1 = engine
            .create_snapshot(file_path, EventType::Create)
            .expect("Snapshot failed")
            .expect("Should be Some");

        std::thread::sleep(std::time::Duration::from_millis(10));

        // Create second snapshot
        fs::write(&full_path, b"v2").expect("Failed to write");
        engine
            .create_snapshot(file_path, EventType::Modify)
            .expect("Snapshot failed")
            .expect("Should be Some");

        std::thread::sleep(std::time::Duration::from_millis(10));

        // Create third snapshot
        fs::write(&full_path, b"v3").expect("Failed to write");
        let snap3 = engine
            .create_snapshot(file_path, EventType::Modify)
            .expect("Snapshot failed")
            .expect("Should be Some");

        // Oldest should be snap1's time
        let oldest = engine
            .get_oldest_snapshot_time()
            .expect("Query should succeed")
            .expect("Should have oldest");
        assert_eq!(oldest, snap1.timestamp);

        // Newest should be snap3's time
        let newest = engine
            .get_newest_snapshot_time()
            .expect("Query should succeed")
            .expect("Should have newest");
        assert_eq!(newest, snap3.timestamp);
    }

    #[test]
    fn prune_removes_old_snapshots() {
        use chrono::TimeZone;

        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let file_path = "prune_test.txt";
        let full_path = project.path().join(file_path);

        // Create old snapshots at known times
        let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 11, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2025, 1, 1, 12, 0, 0).unwrap();

        // Manually insert snapshots with specific timestamps
        fs::write(&full_path, b"v1").expect("Failed to write");
        let content1 = fs::read(&full_path).expect("Failed to read");
        let hash1 = cas::hash_content(&content1);
        cas::store_object(&engine.objects_path, &hash1, &content1).expect("Failed to store");
        db::insert_snapshot(
            &engine.conn,
            file_path,
            &hash1,
            2,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .expect("Failed to insert");

        fs::write(&full_path, b"v2").expect("Failed to write");
        let content2 = fs::read(&full_path).expect("Failed to read");
        let hash2 = cas::hash_content(&content2);
        cas::store_object(&engine.objects_path, &hash2, &content2).expect("Failed to store");
        db::insert_snapshot(
            &engine.conn,
            file_path,
            &hash2,
            2,
            t2,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .expect("Failed to insert");

        fs::write(&full_path, b"v3").expect("Failed to write");
        let content3 = fs::read(&full_path).expect("Failed to read");
        let hash3 = cas::hash_content(&content3);
        cas::store_object(&engine.objects_path, &hash3, &content3).expect("Failed to store");
        db::insert_snapshot(
            &engine.conn,
            file_path,
            &hash3,
            2,
            t3,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .expect("Failed to insert");

        // Verify we have 3 snapshots
        assert_eq!(engine.get_snapshot_count().unwrap(), 3);

        // Prune snapshots before t2.5 (should remove t1 and t2, keep t3)
        let cutoff = Utc.with_ymd_and_hms(2025, 1, 1, 11, 30, 0).unwrap();
        let stats = engine.prune(cutoff, false).expect("Prune should succeed");

        assert_eq!(stats.snapshots_removed, 2);
        assert_eq!(engine.get_snapshot_count().unwrap(), 1);

        // Verify t3 snapshot still exists
        let history = engine.get_file_history(file_path).expect("Query failed");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].timestamp, t3);

        // Verify orphaned CAS objects were removed (hash1 and hash2)
        assert!(!cas::object_exists(&engine.objects_path, &hash1));
        assert!(!cas::object_exists(&engine.objects_path, &hash2));
        assert!(cas::object_exists(&engine.objects_path, &hash3));
    }

    #[test]
    fn prune_dry_run_deletes_nothing() {
        use chrono::TimeZone;

        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let file_path = "dry_run_test.txt";
        let full_path = project.path().join(file_path);

        // Create old snapshots at known times
        let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 11, 0, 0).unwrap();

        // Manually insert snapshots with specific timestamps
        fs::write(&full_path, b"v1").expect("Failed to write");
        let content1 = fs::read(&full_path).expect("Failed to read");
        let hash1 = cas::hash_content(&content1);
        cas::store_object(&engine.objects_path, &hash1, &content1).expect("Failed to store");
        db::insert_snapshot(
            &engine.conn,
            file_path,
            &hash1,
            2,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .expect("Failed to insert");

        fs::write(&full_path, b"v2").expect("Failed to write");
        let content2 = fs::read(&full_path).expect("Failed to read");
        let hash2 = cas::hash_content(&content2);
        cas::store_object(&engine.objects_path, &hash2, &content2).expect("Failed to store");
        db::insert_snapshot(
            &engine.conn,
            file_path,
            &hash2,
            2,
            t2,
            &EventType::Modify,
            0,
            0,
            0,
        )
        .expect("Failed to insert");

        // Verify we have 2 snapshots
        assert_eq!(engine.get_snapshot_count().unwrap(), 2);

        // Dry run prune (should count but not delete)
        let cutoff = Utc.with_ymd_and_hms(2025, 1, 1, 10, 30, 0).unwrap();
        let stats = engine.prune(cutoff, true).expect("Prune should succeed");

        // Stats should show what would be removed
        assert_eq!(stats.snapshots_removed, 1);

        // But nothing should actually be deleted
        assert_eq!(engine.get_snapshot_count().unwrap(), 2);

        // Verify both snapshots still exist
        let history = engine.get_file_history(file_path).expect("Query failed");
        assert_eq!(history.len(), 2);

        // Verify both CAS objects still exist
        assert!(cas::object_exists(&engine.objects_path, &hash1));
        assert!(cas::object_exists(&engine.objects_path, &hash2));
    }

    #[test]
    fn prune_gc_removes_orphaned_objects() {
        use chrono::TimeZone;

        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let file1 = "file1.txt";
        let file2 = "file2.txt";
        let full_path1 = project.path().join(file1);
        let full_path2 = project.path().join(file2);

        // Create snapshots for two files at different times
        let t1 = Utc.with_ymd_and_hms(2025, 1, 1, 10, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2025, 1, 1, 12, 0, 0).unwrap();

        // File1 at t1 (old)
        fs::write(&full_path1, b"file1 content").expect("Failed to write");
        let content1 = fs::read(&full_path1).expect("Failed to read");
        let hash1 = cas::hash_content(&content1);
        cas::store_object(&engine.objects_path, &hash1, &content1).expect("Failed to store");
        db::insert_snapshot(
            &engine.conn,
            file1,
            &hash1,
            content1.len() as u64,
            t1,
            &EventType::Create,
            0,
            0,
            0,
        )
        .expect("Failed to insert");

        // File2 at t2 (new)
        fs::write(&full_path2, b"file2 content").expect("Failed to write");
        let content2 = fs::read(&full_path2).expect("Failed to read");
        let hash2 = cas::hash_content(&content2);
        cas::store_object(&engine.objects_path, &hash2, &content2).expect("Failed to store");
        db::insert_snapshot(
            &engine.conn,
            file2,
            &hash2,
            content2.len() as u64,
            t2,
            &EventType::Create,
            0,
            0,
            0,
        )
        .expect("Failed to insert");

        // Verify both objects exist
        assert!(cas::object_exists(&engine.objects_path, &hash1));
        assert!(cas::object_exists(&engine.objects_path, &hash2));

        // Prune snapshots before t1.5 (should remove file1's snapshot)
        let cutoff = Utc.with_ymd_and_hms(2025, 1, 1, 11, 0, 0).unwrap();
        let stats = engine.prune(cutoff, false).expect("Prune should succeed");

        assert_eq!(stats.snapshots_removed, 1);
        assert_eq!(stats.objects_removed, 1);
        assert!(stats.bytes_freed > 0);

        // Verify only file2's snapshot remains
        assert_eq!(engine.get_snapshot_count().unwrap(), 1);

        // Verify only hash2 CAS object exists (hash1 was GC'd)
        assert!(!cas::object_exists(&engine.objects_path, &hash1));
        assert!(cas::object_exists(&engine.objects_path, &hash2));
    }

    #[test]
    fn create_snapshot_skips_binary_content() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        // Write a PNG file (binary magic number)
        let png_header = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";
        fs::write(project.path().join("image.png"), png_header).expect("Failed to write PNG");

        let result = engine
            .create_snapshot("image.png", EventType::Create)
            .expect("Snapshot should not error");
        assert!(result.is_none(), "Binary content should return None");
    }

    #[test]
    fn create_snapshot_stores_text_content() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        fs::write(project.path().join("hello.rs"), b"fn main() {}").expect("Failed to write file");

        let result = engine
            .create_snapshot("hello.rs", EventType::Create)
            .expect("Snapshot should not error");
        assert!(result.is_some(), "Text content should return Some");
    }

    #[test]
    fn create_snapshot_computes_line_count() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let file_path = "lines.txt";
        let full_path = project.path().join(file_path);

        // Create a file with 3 lines
        fs::write(&full_path, b"line1\nline2\nline3\n").expect("Failed to write");

        let snapshot = engine
            .create_snapshot(file_path, EventType::Create)
            .expect("Snapshot creation should succeed")
            .expect("Should be Some");

        assert_eq!(snapshot.line_count, 3);
    }

    #[test]
    fn create_snapshot_first_file_all_added() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let file_path = "first.txt";
        let full_path = project.path().join(file_path);

        // Create a file with 5 lines
        fs::write(&full_path, b"a\nb\nc\nd\ne\n").expect("Failed to write");

        let snapshot = engine
            .create_snapshot(file_path, EventType::Create)
            .expect("Snapshot creation should succeed")
            .expect("Should be Some");

        // First snapshot: lines_added should equal line_count, lines_removed should be 0
        assert_eq!(snapshot.line_count, 5);
        assert_eq!(snapshot.lines_added, 5);
        assert_eq!(snapshot.lines_removed, 0);
    }

    #[test]
    fn create_snapshot_modify_computes_diff() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let file_path = "modify.txt";
        let full_path = project.path().join(file_path);

        // Create initial version with 3 lines
        fs::write(&full_path, b"line1\nline2\nline3\n").expect("Failed to write");
        engine
            .create_snapshot(file_path, EventType::Create)
            .expect("Snapshot creation should succeed")
            .expect("Should be Some");

        // Wait a bit to ensure different timestamp
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Modify: remove line2, add two new lines
        fs::write(&full_path, b"line1\nline3\nnewline1\nnewline2\n").expect("Failed to write");

        let snapshot = engine
            .create_snapshot(file_path, EventType::Modify)
            .expect("Snapshot creation should succeed")
            .expect("Should be Some");

        // Should detect 2 added lines and 1 removed line
        assert_eq!(snapshot.line_count, 4);
        assert_eq!(snapshot.lines_added, 2);
        assert_eq!(snapshot.lines_removed, 1);
    }

    #[test]
    fn create_snapshot_delete_computes_removed() {
        let (project, storage) = setup_test_project();
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");

        let file_path = "todelete.txt";
        let full_path = project.path().join(file_path);

        // Create a file with 7 lines
        fs::write(&full_path, b"a\nb\nc\nd\ne\nf\ng\n").expect("Failed to write");
        let create_snap = engine
            .create_snapshot(file_path, EventType::Create)
            .expect("Snapshot creation should succeed")
            .expect("Should be Some");

        assert_eq!(create_snap.line_count, 7);

        // Wait a bit to ensure different timestamp
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Delete the file (don't actually delete from disk, just record the event)
        let delete_snap = engine
            .create_snapshot(file_path, EventType::Delete)
            .expect("Delete snapshot should succeed")
            .expect("Should be Some");

        // Delete snapshot should have: line_count=0, lines_added=0, lines_removed=7
        assert_eq!(delete_snap.line_count, 0);
        assert_eq!(delete_snap.lines_added, 0);
        assert_eq!(delete_snap.lines_removed, 7);
    }
}
