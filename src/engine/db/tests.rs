//! Comprehensive tests for the database module.

#[cfg(test)]
mod all_tests {
    use super::super::*;
    use crate::types::{ContentHash, EventType};
    use chrono::TimeZone;
    use rusqlite::Connection;

    fn open_test_db() -> Connection {
        let conn = Connection::open(":memory:").expect("Failed to open test database");
        schema::init_schema(&conn).expect("Failed to init schema");
        conn
    }

    // --- Schema Tests ---

    #[test]
    fn schema_creation_is_idempotent() {
        let conn = open_test_db();
        // Call init_schema again to verify idempotency
        schema::init_schema(&conn).expect("Second schema init should succeed");

        // Verify schema_version table exists and has correct version
        let version: i64 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .expect("Schema version should be set");
        assert_eq!(version, 2); // SCHEMA_VERSION = 2
    }

    // --- Helper Function Tests ---

    #[test]
    fn event_type_roundtrip() {
        // This test verifies that event types round-trip correctly through string conversion
        use crate::types::EventType;
        // Note: These conversions are tested implicitly through snapshot operations,
        // but we verify the basic conversions here
        assert_eq!(format!("{:?}", EventType::Create), "Create");
        assert_eq!(format!("{:?}", EventType::Modify), "Modify");
        assert_eq!(format!("{:?}", EventType::Delete), "Delete");
    }

    // --- Basic Insert and Retrieve Tests ---

    #[test]
    fn insert_and_retrieve_snapshot() {
        let conn = open_test_db();
        let timestamp = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        let id = write::insert_snapshot(
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
            query::get_snapshots_for_file(&conn, "/tmp/test.txt").expect("Query should succeed");
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
    fn get_latest_snapshot_at_specific_time() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t3 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 14, 0, 0).unwrap();

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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
        let snapshot = query::get_latest_snapshot_at(&conn, "/tmp/test.txt", t2)
            .expect("Query should succeed")
            .expect("Should find snapshot");
        assert_eq!(snapshot.content_hash, ContentHash("v2".to_string()));

        // Query before t1 should return None
        let t0 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 9, 0, 0).unwrap();
        let snapshot = query::get_latest_snapshot_at(&conn, "/tmp/test.txt", t0)
            .expect("Query should succeed");
        assert!(snapshot.is_none());
    }

    #[test]
    fn tracked_file_count_is_accurate() {
        let conn = open_test_db();
        let timestamp = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        assert_eq!(query::get_tracked_file_count(&conn).unwrap(), 0);

        write::insert_snapshot(
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

        assert_eq!(query::get_tracked_file_count(&conn).unwrap(), 1);

        // Same file, different snapshot
        write::insert_snapshot(
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

        assert_eq!(query::get_tracked_file_count(&conn).unwrap(), 1);

        // Different file
        write::insert_snapshot(
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

        assert_eq!(query::get_tracked_file_count(&conn).unwrap(), 2);
    }

    #[test]
    fn empty_database_queries_return_empty_results() {
        let conn = open_test_db();

        let snapshots = query::get_snapshots_for_file(&conn, "/tmp/nonexistent.txt")
            .expect("Query should succeed");
        assert_eq!(snapshots.len(), 0);

        let timestamp = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let snapshots = query::get_snapshots_since(&conn, timestamp).expect("Query should succeed");
        assert_eq!(snapshots.len(), 0);

        let snapshot = query::get_latest_snapshot_at(&conn, "/tmp/test.txt", timestamp)
            .expect("Query should succeed");
        assert!(snapshot.is_none());

        let files = query::get_all_tracked_files(&conn).expect("Query should succeed");
        assert_eq!(files.len(), 0);

        assert_eq!(query::get_snapshot_count(&conn).unwrap(), 0);
        assert_eq!(query::get_tracked_file_count(&conn).unwrap(), 0);
    }

    #[test]
    fn snapshots_ordered_by_time_descending() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t3 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 14, 0, 0).unwrap();

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        let snapshots = query::get_snapshots_for_file(&conn, "/tmp/test.txt").unwrap();
        assert_eq!(snapshots.len(), 3);
        assert_eq!(snapshots[0].timestamp, t3); // newest first
        assert_eq!(snapshots[1].timestamp, t2);
        assert_eq!(snapshots[2].timestamp, t1);
    }

    #[test]
    fn get_newest_snapshot_time_empty_db() {
        let conn = open_test_db();

        // Empty database should return None
        let newest = query::get_newest_snapshot_time(&conn).expect("Query should succeed");
        assert!(newest.is_none());
    }

    #[test]
    fn get_newest_snapshot_time_returns_latest() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t3 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 14, 0, 0).unwrap();

        // Insert in non-chronological order
        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        let newest = query::get_newest_snapshot_time(&conn)
            .expect("Query should succeed")
            .expect("Should find newest timestamp");
        assert_eq!(newest, t3);
    }

    // --- Keyset Pagination Tests ---

    #[test]
    fn history_page_first_page_returns_correct_results() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        let snapshots =
            query::get_history_page(&conn, types::HistoryScope::All, None, 10, None, None)
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

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t4 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 13, 0, 0).unwrap();

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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
        let page1 = query::get_history_page(&conn, types::HistoryScope::All, None, 2, None, None)
            .expect("Query should succeed");
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].timestamp, t4);
        assert_eq!(page1[1].timestamp, t3);

        // Get second page using cursor from last item of page1
        let cursor = types::HistoryCursor {
            timestamp: page1[1].timestamp,
            id: page1[1].id,
        };
        let page2 = query::get_history_page(
            &conn,
            types::HistoryScope::All,
            Some(&cursor),
            2,
            None,
            None,
        )
        .expect("Query should succeed");
        assert_eq!(page2.len(), 2);
        assert_eq!(page2[0].timestamp, t2);
        assert_eq!(page2[1].timestamp, t1);
    }

    #[test]
    fn history_page_file_scope_filters_exact_file() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        let snapshots = query::get_history_page(
            &conn,
            types::HistoryScope::File("/tmp/a.txt"),
            None,
            10,
            None,
            None,
        )
        .expect("Query should succeed");

        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].file_path, "/tmp/a.txt");
    }

    #[test]
    fn history_page_directory_scope_matches_prefix() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        let snapshots = query::get_history_page(
            &conn,
            types::HistoryScope::Directory("/tmp/src/"),
            None,
            10,
            None,
            None,
        )
        .expect("Query should succeed");

        assert_eq!(snapshots.len(), 2);
        assert!(snapshots[0].file_path.starts_with("/tmp/src/"));
        assert!(snapshots[1].file_path.starts_with("/tmp/src/"));
    }

    #[test]
    fn history_page_all_scope_returns_everything() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        let snapshots =
            query::get_history_page(&conn, types::HistoryScope::All, None, 10, None, None)
                .expect("Query should succeed");

        assert_eq!(snapshots.len(), 3);
    }

    #[test]
    fn history_page_empty_history_returns_empty_vec() {
        let conn = open_test_db();

        let snapshots =
            query::get_history_page(&conn, types::HistoryScope::All, None, 10, None, None)
                .expect("Query should succeed");

        assert_eq!(snapshots.len(), 0);
    }

    #[test]
    fn history_page_since_parameter_filters_correctly() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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
        let snapshots =
            query::get_history_page(&conn, types::HistoryScope::All, None, 10, Some(t2), None)
                .expect("Query should succeed");

        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].timestamp, t3);
        assert_eq!(snapshots[1].timestamp, t2);
    }

    #[test]
    fn get_history_page_filters_by_until() {
        let conn = open_test_db();
        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        for (t, name) in [(t1, "a.txt"), (t2, "b.txt"), (t3, "c.txt")] {
            write::insert_snapshot(
                &conn,
                &format!("/tmp/{name}"),
                &ContentHash(format!("v{name}")),
                100,
                t,
                &EventType::Create,
                0,
                0,
                0,
            )
            .unwrap();
        }

        // Query until t2 (should include t1 and t2, not t3)
        let snapshots =
            query::get_history_page(&conn, types::HistoryScope::All, None, 10, None, Some(t2))
                .expect("Query should succeed");

        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].timestamp, t2); // newest first
        assert_eq!(snapshots[1].timestamp, t1);

        // Query with since=t1 AND until=t2 (should get exactly t1 and t2)
        let snapshots = query::get_history_page(
            &conn,
            types::HistoryScope::All,
            None,
            10,
            Some(t1),
            Some(t2),
        )
        .expect("Query should succeed");

        assert_eq!(snapshots.len(), 2);
    }

    // --- get_previous_snapshot Tests ---

    #[test]
    fn get_previous_snapshot_no_previous() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();

        let id1 = write::insert_snapshot(
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
        let previous = query::get_previous_snapshot(&conn, "/tmp/test.txt", t1, id1)
            .expect("Query should succeed");
        assert!(previous.is_none());
    }

    #[test]
    fn get_previous_snapshot_returns_preceding() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        let id1 = write::insert_snapshot(
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

        let id2 = write::insert_snapshot(
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

        let id3 = write::insert_snapshot(
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
        let previous = query::get_previous_snapshot(&conn, "/tmp/test.txt", t3, id3)
            .expect("Query should succeed")
            .expect("Should find previous snapshot");
        assert_eq!(previous.id, id2);
        assert_eq!(previous.content_hash, ContentHash("v2".to_string()));

        // Ask for previous of the 2nd snapshot, should get the 1st
        let previous = query::get_previous_snapshot(&conn, "/tmp/test.txt", t2, id2)
            .expect("Query should succeed")
            .expect("Should find previous snapshot");
        assert_eq!(previous.id, id1);
        assert_eq!(previous.content_hash, ContentHash("v1".to_string()));
    }

    // --- QueryBuilder Tests ---

    #[test]
    fn query_builder_simple_query_no_conditions() {
        let qb = types::QueryBuilder::new("SELECT * FROM snapshots");
        let (sql, params) = qb.build();

        assert_eq!(sql, "SELECT * FROM snapshots");
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn query_builder_single_condition() {
        let mut qb = types::QueryBuilder::new("SELECT * FROM snapshots");
        qb.add_condition("file_path = ?", "/tmp/test.txt".to_string());

        let (sql, params) = qb.build();

        assert_eq!(sql, "SELECT * FROM snapshots WHERE file_path = ?");
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn query_builder_multiple_conditions_are_and_joined() {
        let mut qb = types::QueryBuilder::new("SELECT * FROM snapshots");
        qb.add_condition("file_path = ?", "/tmp/test.txt".to_string());
        qb.add_condition("timestamp > ?", "2025-01-15T10:00:00Z".to_string());

        let (sql, params) = qb.build();

        assert_eq!(
            sql,
            "SELECT * FROM snapshots WHERE file_path = ? AND timestamp > ?"
        );
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn query_builder_with_order_by() {
        let mut qb = types::QueryBuilder::new("SELECT * FROM snapshots");
        qb.add_condition("file_path = ?", "/tmp/test.txt".to_string());
        qb.order_by("timestamp DESC");

        let (sql, params) = qb.build();

        assert_eq!(
            sql,
            "SELECT * FROM snapshots WHERE file_path = ? ORDER BY timestamp DESC"
        );
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn query_builder_with_limit() {
        let mut qb = types::QueryBuilder::new("SELECT * FROM snapshots");
        qb.add_condition("file_path = ?", "/tmp/test.txt".to_string());
        qb.order_by("timestamp DESC");
        qb.limit(10);

        let (sql, params) = qb.build();

        assert_eq!(
            sql,
            "SELECT * FROM snapshots WHERE file_path = ? ORDER BY timestamp DESC LIMIT 10"
        );
        assert_eq!(params.len(), 1);
    }

    // --- Pruning Function Tests ---

    #[test]
    fn count_snapshots_before_empty_db() {
        let conn = open_test_db();
        let cutoff = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let count = query::count_snapshots_before(&conn, cutoff).expect("Query should succeed");
        assert_eq!(count, 0);
    }

    #[test]
    fn count_snapshots_before_with_data() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t4 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 13, 0, 0).unwrap();

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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
        let count = query::count_snapshots_before(&conn, t3).expect("Query should succeed");
        assert_eq!(count, 2);

        // Cutoff before all snapshots should return 0
        let t0 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 9, 0, 0).unwrap();
        let count = query::count_snapshots_before(&conn, t0).expect("Query should succeed");
        assert_eq!(count, 0);

        // Cutoff after all snapshots should return all 4
        let t5 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 14, 0, 0).unwrap();
        let count = query::count_snapshots_before(&conn, t5).expect("Query should succeed");
        assert_eq!(count, 4);
    }

    #[test]
    fn get_previous_snapshot_same_timestamp_tiebreak() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();

        // Insert two snapshots at the same timestamp (different IDs)
        let id1 = write::insert_snapshot(
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

        let id2 = write::insert_snapshot(
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
        let previous = query::get_previous_snapshot(&conn, "/tmp/test.txt", t1, id2)
            .expect("Query should succeed")
            .expect("Should find previous snapshot");
        assert_eq!(previous.id, id1);
        assert_eq!(previous.content_hash, ContentHash("v1".to_string()));

        // Ask for previous of id1 at timestamp t1, should get None
        let previous = query::get_previous_snapshot(&conn, "/tmp/test.txt", t1, id1)
            .expect("Query should succeed");
        assert!(previous.is_none());
    }

    #[test]
    fn get_previous_snapshot_different_file_ignored() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();

        // Insert snapshot for a different file at earlier time
        write::insert_snapshot(
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
        let id2 = write::insert_snapshot(
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
        let previous = query::get_previous_snapshot(&conn, "/tmp/test.txt", t2, id2)
            .expect("Query should succeed");
        assert!(previous.is_none());
    }

    #[test]
    fn history_page_cursor_at_end_returns_empty() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();

        let id = write::insert_snapshot(
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
        let cursor = types::HistoryCursor { timestamp: t1, id };

        let snapshots = query::get_history_page(
            &conn,
            types::HistoryScope::All,
            Some(&cursor),
            10,
            None,
            None,
        )
        .expect("Query should succeed");

        assert_eq!(snapshots.len(), 0);
    }

    #[test]
    fn history_page_partial_last_page() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();

        write::insert_snapshot(
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

        write::insert_snapshot(
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
        let snapshots =
            query::get_history_page(&conn, types::HistoryScope::All, None, 10, None, None)
                .expect("Query should succeed");

        assert_eq!(snapshots.len(), 2);
    }

    #[test]
    fn history_page_multiple_files_interleaved_chronologically() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t4 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 13, 0, 0).unwrap();

        // Interleave two files chronologically
        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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
        let snapshots = query::get_history_page(
            &conn,
            types::HistoryScope::Directory("/tmp/src/"),
            None,
            10,
            None,
            None,
        )
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

    #[test]
    fn get_referenced_hashes_empty() {
        let conn = open_test_db();
        let hashes = query::get_referenced_hashes(&conn).expect("Query should succeed");
        assert_eq!(hashes.len(), 0);
    }

    #[test]
    fn query_builder_condition_without_param() {
        let mut qb = types::QueryBuilder::new("SELECT * FROM snapshots");
        qb.add_condition("file_path = ?", "/tmp/test.txt".to_string());
        qb.add_condition_no_param("size_bytes > 0");

        let (sql, params) = qb.build();

        assert_eq!(
            sql,
            "SELECT * FROM snapshots WHERE file_path = ? AND size_bytes > 0"
        );
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn query_builder_no_where_with_order_and_limit() {
        let mut qb = types::QueryBuilder::new("SELECT * FROM snapshots");
        qb.order_by("timestamp DESC");
        qb.limit(50);

        let (sql, params) = qb.build();

        assert_eq!(
            sql,
            "SELECT * FROM snapshots ORDER BY timestamp DESC LIMIT 50"
        );
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn query_builder_with_multiple_params() {
        let mut qb = types::QueryBuilder::new("SELECT * FROM snapshots");
        let params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
            Box::new("2025-01-15T10:00:00Z".to_string()),
            Box::new("2025-01-15T10:00:00Z".to_string()),
            Box::new(42i64),
        ];
        qb.add_condition_with_params("(timestamp < ? OR (timestamp = ? AND id < ?))", params_vec);

        let (sql, params) = qb.build();

        assert_eq!(
            sql,
            "SELECT * FROM snapshots WHERE (timestamp < ? OR (timestamp = ? AND id < ?))"
        );
        assert_eq!(params.len(), 3);
    }

    #[test]
    fn query_builder_complex_scenario() {
        let mut qb = types::QueryBuilder::new("SELECT id, file_path FROM snapshots");
        qb.add_condition("file_path = ?", "/tmp/test.txt".to_string());

        let cursor_params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![
            Box::new("2025-01-15T12:00:00Z".to_string()),
            Box::new("2025-01-15T12:00:00Z".to_string()),
            Box::new(100i64),
        ];
        qb.add_condition_with_params(
            "(timestamp < ? OR (timestamp = ? AND id < ?))",
            cursor_params,
        );

        qb.add_condition("timestamp >= ?", "2025-01-15T00:00:00Z".to_string());
        qb.order_by("timestamp DESC, id DESC");
        qb.limit(25);

        let (sql, params) = qb.build();

        assert_eq!(
            sql,
            "SELECT id, file_path FROM snapshots WHERE file_path = ? AND (timestamp < ? OR (timestamp = ? AND id < ?)) AND timestamp >= ? ORDER BY timestamp DESC, id DESC LIMIT 25"
        );
        assert_eq!(params.len(), 5);
    }

    #[test]
    fn query_snapshots_since_time() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t3 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 14, 0, 0).unwrap();

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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
        let snapshots = query::get_snapshots_since(&conn, t2).expect("Query should succeed");
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].file_path, "/tmp/c.txt"); // newest first
        assert_eq!(snapshots[1].file_path, "/tmp/b.txt");
    }

    #[test]
    fn get_all_tracked_files_returns_unique_paths() {
        let conn = open_test_db();
        let timestamp = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        let files = query::get_all_tracked_files(&conn).expect("Query should succeed");
        assert_eq!(files.len(), 2);
        assert!(files.contains(&"/tmp/a.txt".to_string()));
        assert!(files.contains(&"/tmp/b.txt".to_string()));
    }

    #[test]
    fn snapshot_count_is_accurate() {
        let conn = open_test_db();
        let timestamp = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        assert_eq!(query::get_snapshot_count(&conn).unwrap(), 0);

        write::insert_snapshot(
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

        assert_eq!(query::get_snapshot_count(&conn).unwrap(), 1);

        write::insert_snapshot(
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

        assert_eq!(query::get_snapshot_count(&conn).unwrap(), 2);
    }

    #[test]
    fn get_oldest_snapshot_time_returns_earliest() {
        let conn = open_test_db();

        // Empty database should return None
        let oldest = query::get_oldest_snapshot_time(&conn).expect("Query should succeed");
        assert!(oldest.is_none());

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t3 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 14, 0, 0).unwrap();

        // Insert in non-chronological order
        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        let oldest = query::get_oldest_snapshot_time(&conn)
            .expect("Query should succeed")
            .expect("Should find oldest timestamp");
        assert_eq!(oldest, t1);
    }

    #[test]
    fn get_referenced_hashes_returns_distinct() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        // Insert snapshots with some duplicate hashes
        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        let hashes = query::get_referenced_hashes(&conn).expect("Query should succeed");

        // Should have exactly 2 distinct hashes
        assert_eq!(hashes.len(), 2);
        assert!(hashes.contains("hash1"));
        assert!(hashes.contains("hash2"));
    }

    #[test]
    fn delete_snapshots_before_removes_old() {
        let conn = open_test_db();

        let t1 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 10, 0, 0).unwrap();
        let t2 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 11, 0, 0).unwrap();
        let t3 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();
        let t4 = chrono::Utc.with_ymd_and_hms(2025, 1, 15, 13, 0, 0).unwrap();

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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

        write::insert_snapshot(
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
        let deleted = write::delete_snapshots_before(&conn, t3).expect("Delete should succeed");
        assert_eq!(deleted, 2);

        // Verify only t3 and t4 remain
        let remaining = query::get_snapshot_count(&conn).expect("Query should succeed");
        assert_eq!(remaining, 2);

        // Verify the remaining snapshots are t3 and t4
        let snapshots = query::get_snapshots_since(&conn, t3).expect("Query should succeed");
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].timestamp, t4);
        assert_eq!(snapshots[1].timestamp, t3);
    }
}
