//! Implementation of `unf cat` command.
//!
//! Outputs the content of a file at a specific point in time or snapshot ID.

use std::io::{self, Write};
use std::path::Path;

use crate::cli::OutputFormat;
use crate::engine::Engine;
use crate::error::UnfError;
use crate::storage;
use crate::types::{EventType, SnapshotId};

/// JSON output for the cat command.
#[derive(serde::Serialize)]
struct CatOutput {
    file: String,
    snapshot_id: i64,
    timestamp: String,
    hash: String,
    bytes: u64,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    encoding: Option<String>,
}

pub fn run(
    project_root: &Path,
    file: &str,
    at: Option<&str>,
    snapshot_id: Option<i64>,
    format: OutputFormat,
) -> Result<(), UnfError> {
    let storage_dir = storage::resolve_storage_dir(project_root)?;
    let engine = Engine::open(project_root, &storage_dir)?;

    // Resolve the snapshot
    let snapshot = if let Some(id) = snapshot_id {
        // By snapshot ID
        engine
            .get_snapshot_by_id(SnapshotId(id))?
            .ok_or_else(|| UnfError::NoResults(format!("Snapshot {} not found.", id)))?
    } else if let Some(time_spec) = at {
        // By time
        let target_time = super::parse_time_spec(time_spec)?;
        engine
            .get_latest_snapshot_for_file(file, target_time)?
            .ok_or_else(|| UnfError::NoResults(format!("No history for \"{}\".", file)))?
    } else {
        return Err(UnfError::InvalidArgument(
            "Either --at or --snapshot is required.".to_string(),
        ));
    };

    // Check if snapshot is for the right file (when using --snapshot)
    if snapshot_id.is_some() && snapshot.file_path != file {
        return Err(UnfError::InvalidArgument(format!(
            "Snapshot {} is for \"{}\", not \"{}\".",
            snapshot.id, snapshot.file_path, file
        )));
    }

    // Check if file was deleted at this point
    if snapshot.event_type == EventType::Delete {
        return Err(UnfError::NoResults(format!(
            "\"{}\" was deleted at {}.",
            file,
            snapshot.timestamp.to_rfc3339()
        )));
    }

    // Load content from CAS
    let content_bytes = engine.load_content(&snapshot.content_hash)?;

    match format {
        OutputFormat::Json => {
            // Try UTF-8, fall back to lossy UTF-8
            let (content_str, encoding) = match String::from_utf8(content_bytes.clone()) {
                Ok(s) => (s, None),
                Err(_) => (
                    String::from_utf8_lossy(&content_bytes).into_owned(),
                    Some("utf8_lossy".to_string()),
                ),
            };

            let output = CatOutput {
                file: snapshot.file_path.clone(),
                snapshot_id: snapshot.id.0,
                timestamp: snapshot.timestamp.to_rfc3339(),
                hash: snapshot.content_hash.0.clone(),
                bytes: snapshot.size_bytes,
                content: content_str,
                encoding,
            };
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
        OutputFormat::Human => {
            // Raw content to stdout, no decoration
            io::stdout()
                .write_all(&content_bytes)
                .map_err(|e| UnfError::InvalidArgument(format!("Failed to write output: {}", e)))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_engine() -> (TempDir, TempDir, Engine) {
        let project = TempDir::new().expect("create project dir");
        let storage = TempDir::new().expect("create storage dir");
        let engine = Engine::init(project.path(), storage.path()).expect("Init should succeed");
        (project, storage, engine)
    }

    #[test]
    fn cat_load_content_by_snapshot_id() {
        let (project, _storage, engine) = setup_test_engine();

        let file_path = "test.txt";
        let full_path = project.path().join(file_path);
        let content = b"hello world";

        fs::write(&full_path, content).expect("Failed to write test file");
        let snapshot = engine
            .create_snapshot(file_path, crate::types::EventType::Create)
            .expect("Snapshot creation should succeed")
            .expect("Should be Some for text file");

        // Verify we can retrieve the snapshot by ID and load its content
        let retrieved = engine
            .get_snapshot_by_id(snapshot.id)
            .expect("Query should succeed")
            .expect("Snapshot should exist");
        assert_eq!(retrieved.file_path, file_path);

        let loaded = engine
            .load_content(&retrieved.content_hash)
            .expect("Load should succeed");
        assert_eq!(loaded, content);
    }

    #[test]
    fn cat_load_content_by_time() {
        let (project, _storage, engine) = setup_test_engine();

        let file_path = "test.txt";
        let full_path = project.path().join(file_path);

        fs::write(&full_path, b"version 1").expect("Failed to write");
        engine
            .create_snapshot(file_path, crate::types::EventType::Create)
            .expect("Snapshot creation should succeed");

        // Use a time in the future to find the snapshot
        let future = chrono::Utc::now() + chrono::Duration::hours(1);
        let retrieved = engine
            .get_latest_snapshot_for_file(file_path, future)
            .expect("Query should succeed")
            .expect("Snapshot should exist");
        assert_eq!(retrieved.file_path, file_path);

        let loaded = engine
            .load_content(&retrieved.content_hash)
            .expect("Load should succeed");
        assert_eq!(loaded, b"version 1");
    }

    #[test]
    fn cat_nonexistent_snapshot() {
        let (_project, _storage, engine) = setup_test_engine();

        let result = engine
            .get_snapshot_by_id(crate::types::SnapshotId(9999))
            .expect("Query should succeed");
        assert!(result.is_none());
    }

    #[test]
    fn cat_deleted_file_snapshot() {
        let (_project, _storage, engine) = setup_test_engine();

        let file_path = "deleted.txt";

        let snapshot = engine
            .create_snapshot(file_path, crate::types::EventType::Delete)
            .expect("Snapshot creation should succeed")
            .expect("Should be Some for delete event");

        let retrieved = engine
            .get_snapshot_by_id(snapshot.id)
            .expect("Query should succeed")
            .expect("Snapshot should exist");

        assert_eq!(retrieved.event_type, crate::types::EventType::Delete);
    }

    #[test]
    fn cat_snapshot_for_wrong_file() {
        let (project, _storage, engine) = setup_test_engine();

        let file1 = "file1.txt";
        let full_path1 = project.path().join(file1);

        fs::write(&full_path1, b"content1").expect("Failed to write");
        let snapshot = engine
            .create_snapshot(file1, crate::types::EventType::Create)
            .expect("Snapshot creation should succeed")
            .expect("Should be Some for text file");

        // Retrieve the snapshot and verify it's for file1, not file2
        let retrieved = engine
            .get_snapshot_by_id(snapshot.id)
            .expect("Query should succeed")
            .expect("Snapshot should exist");
        assert_eq!(retrieved.file_path, file1);
        assert_ne!(retrieved.file_path, "file2.txt");
    }
}
