#[cfg(test)]
mod tests_log {
    use super::super::*;
    use chrono::Utc;

    #[test]
    fn cursor_from_page_empty() {
        let page: Vec<Snapshot> = vec![];
        assert!(super::super::format::cursor_from_page(&page).is_none());
    }

    #[test]
    fn cursor_from_page_single() {
        let snap = Snapshot {
            id: crate::types::SnapshotId(42),
            file_path: "test.txt".to_string(),
            content_hash: crate::types::ContentHash("abc123".to_string()),
            size_bytes: 100,
            timestamp: Utc::now(),
            event_type: crate::types::EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let page = vec![snap.clone()];

        let cursor = super::super::format::cursor_from_page(&page).unwrap();
        assert_eq!(cursor.id, snap.id);
        assert_eq!(cursor.timestamp, snap.timestamp);
    }

    #[test]
    fn cursor_from_page_multiple() {
        let now = Utc::now();
        let snap1 = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "test.txt".to_string(),
            content_hash: crate::types::ContentHash("abc".to_string()),
            size_bytes: 100,
            timestamp: now,
            event_type: crate::types::EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let snap2 = Snapshot {
            id: crate::types::SnapshotId(2),
            file_path: "test.txt".to_string(),
            content_hash: crate::types::ContentHash("def".to_string()),
            size_bytes: 200,
            timestamp: now,
            event_type: crate::types::EventType::Modify,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let page = vec![snap1, snap2.clone()];

        let cursor = super::super::format::cursor_from_page(&page).unwrap();
        assert_eq!(cursor.id, snap2.id);
        assert_eq!(cursor.timestamp, snap2.timestamp);
    }

    #[test]
    fn format_snapshot_line_created_no_color() {
        let snap = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "src/test.rs".to_string(),
            content_hash: crate::types::ContentHash("abc123".to_string()),
            size_bytes: 1234,
            timestamp: Utc::now(),
            event_type: crate::types::EventType::Create,
            line_count: 10,
            lines_added: 10,
            lines_removed: 0,
        };

        let line = super::super::format::format_snapshot_line(&snap, false);
        assert!(line.contains("created"));
        assert!(line.contains("src/test.rs"));
        assert!(line.contains("1.2 KB"));
        assert!(line.contains("+10/-0"));
        assert!(!line.contains("\x1b["));
    }

    #[test]
    fn format_snapshot_line_modified_no_color() {
        let snap = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "src/engine.rs".to_string(),
            content_hash: crate::types::ContentHash("def456".to_string()),
            size_bytes: 2048,
            timestamp: Utc::now(),
            event_type: crate::types::EventType::Modify,
            line_count: 30,
            lines_added: 5,
            lines_removed: 2,
        };

        let line = super::super::format::format_snapshot_line(&snap, false);
        assert!(line.contains("modified"));
        assert!(line.contains("src/engine.rs"));
        assert!(line.contains("2.0 KB"));
        assert!(line.contains("+5/-2"));
    }

    #[test]
    fn format_snapshot_line_deleted_no_color() {
        let snap = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "src/old.rs".to_string(),
            content_hash: crate::types::ContentHash("ghi789".to_string()),
            size_bytes: 500,
            timestamp: Utc::now(),
            event_type: crate::types::EventType::Delete,
            line_count: 20,
            lines_added: 0,
            lines_removed: 20,
        };

        let line = super::super::format::format_snapshot_line(&snap, false);
        assert!(line.contains("deleted"));
        assert!(line.contains("src/old.rs"));
        assert!(!line.contains("KB")); // Delete events don't show size
        assert!(!line.contains("+")); // Delete events don't show stats
    }

    #[test]
    fn format_snapshot_line_pre_migration_no_stats() {
        let snap = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "src/legacy.rs".to_string(),
            content_hash: crate::types::ContentHash("oldstyle".to_string()),
            size_bytes: 5000,
            timestamp: Utc::now(),
            event_type: crate::types::EventType::Create,
            line_count: 50,
            lines_added: 0,
            lines_removed: 0,
        };

        let line = super::super::format::format_snapshot_line(&snap, false);
        assert!(line.contains("created"));
        assert!(line.contains("src/legacy.rs"));
        assert!(line.contains("4.9 KB"));
        assert!(!line.contains("+")); // Pre-migration snapshots don't show stats
        assert!(!line.contains("-/-")); // No longer shows cryptic -/-
    }

    #[test]
    fn group_by_file_empty() {
        let snapshots: Vec<Snapshot> = vec![];
        let groups = super::super::filters::group_by_file(snapshots);
        assert!(groups.is_empty());
    }

    #[test]
    fn group_by_file_single_file_single_entry() {
        let now = Utc::now();
        let snap = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "test.txt".to_string(),
            content_hash: crate::types::ContentHash("abc".to_string()),
            size_bytes: 100,
            timestamp: now,
            event_type: crate::types::EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };

        let groups = super::super::filters::group_by_file(vec![snap.clone()]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].path, "test.txt");
        assert_eq!(groups[0].entries.len(), 1);
        assert_eq!(groups[0].entries[0], snap);
    }

    #[test]
    fn group_by_file_single_file_multiple_entries_chronological() {
        let base_time = Utc::now();
        let snap1 = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "test.txt".to_string(),
            content_hash: crate::types::ContentHash("abc".to_string()),
            size_bytes: 100,
            timestamp: base_time,
            event_type: crate::types::EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let snap2 = Snapshot {
            id: crate::types::SnapshotId(2),
            file_path: "test.txt".to_string(),
            content_hash: crate::types::ContentHash("def".to_string()),
            size_bytes: 200,
            timestamp: base_time + chrono::Duration::seconds(10),
            event_type: crate::types::EventType::Modify,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let snap3 = Snapshot {
            id: crate::types::SnapshotId(3),
            file_path: "test.txt".to_string(),
            content_hash: crate::types::ContentHash("ghi".to_string()),
            size_bytes: 300,
            timestamp: base_time + chrono::Duration::seconds(5),
            event_type: crate::types::EventType::Modify,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };

        // Input in non-chronological order
        let groups =
            super::super::filters::group_by_file(vec![snap2.clone(), snap1.clone(), snap3.clone()]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].path, "test.txt");
        assert_eq!(groups[0].entries.len(), 3);
        // Entries should be sorted oldest-first
        assert_eq!(groups[0].entries[0], snap1);
        assert_eq!(groups[0].entries[1], snap3);
        assert_eq!(groups[0].entries[2], snap2);
    }

    #[test]
    fn group_by_file_multiple_files_newest_first() {
        let t1 = Utc::now();
        let t2 = t1 + chrono::Duration::seconds(100);
        let t3 = t1 + chrono::Duration::seconds(50);

        let snap_file_a = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "a.txt".to_string(),
            content_hash: crate::types::ContentHash("abc".to_string()),
            size_bytes: 100,
            timestamp: t1,
            event_type: crate::types::EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let snap_file_b = Snapshot {
            id: crate::types::SnapshotId(2),
            file_path: "b.txt".to_string(),
            content_hash: crate::types::ContentHash("def".to_string()),
            size_bytes: 200,
            timestamp: t2,
            event_type: crate::types::EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let snap_file_c = Snapshot {
            id: crate::types::SnapshotId(3),
            file_path: "c.txt".to_string(),
            content_hash: crate::types::ContentHash("ghi".to_string()),
            size_bytes: 300,
            timestamp: t3,
            event_type: crate::types::EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };

        let groups = super::super::filters::group_by_file(vec![
            snap_file_a,
            snap_file_b.clone(),
            snap_file_c.clone(),
        ]);
        assert_eq!(groups.len(), 3);
        // b.txt has the newest timestamp (t2), so should be first
        assert_eq!(groups[0].path, "b.txt");
        assert_eq!(groups[0].entries[0], snap_file_b);
        // c.txt is second (t3)
        assert_eq!(groups[1].path, "c.txt");
        assert_eq!(groups[1].entries[0], snap_file_c);
        // a.txt is oldest (t1)
        assert_eq!(groups[2].path, "a.txt");
    }

    #[test]
    fn group_by_file_mixed_files_and_entries() {
        let t1 = Utc::now();
        let t2 = t1 + chrono::Duration::seconds(10);
        let t3 = t1 + chrono::Duration::seconds(20);
        let t4 = t1 + chrono::Duration::seconds(30);

        let snap1 = Snapshot {
            id: crate::types::SnapshotId(1),
            file_path: "a.txt".to_string(),
            content_hash: crate::types::ContentHash("h1".to_string()),
            size_bytes: 100,
            timestamp: t1,
            event_type: crate::types::EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let snap2 = Snapshot {
            id: crate::types::SnapshotId(2),
            file_path: "b.txt".to_string(),
            content_hash: crate::types::ContentHash("h2".to_string()),
            size_bytes: 200,
            timestamp: t2,
            event_type: crate::types::EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let snap3 = Snapshot {
            id: crate::types::SnapshotId(3),
            file_path: "a.txt".to_string(),
            content_hash: crate::types::ContentHash("h3".to_string()),
            size_bytes: 150,
            timestamp: t3,
            event_type: crate::types::EventType::Modify,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let snap4 = Snapshot {
            id: crate::types::SnapshotId(4),
            file_path: "b.txt".to_string(),
            content_hash: crate::types::ContentHash("h4".to_string()),
            size_bytes: 250,
            timestamp: t4,
            event_type: crate::types::EventType::Modify,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };

        let groups = super::super::filters::group_by_file(vec![
            snap1.clone(),
            snap2.clone(),
            snap3.clone(),
            snap4.clone(),
        ]);
        assert_eq!(groups.len(), 2);

        // b.txt has newest activity (t4), so first
        assert_eq!(groups[0].path, "b.txt");
        assert_eq!(groups[0].entries.len(), 2);
        assert_eq!(groups[0].entries[0], snap2); // chronological (older)
        assert_eq!(groups[0].entries[1], snap4); // chronological (newer)

        // a.txt has older newest activity (t3), so second
        assert_eq!(groups[1].path, "a.txt");
        assert_eq!(groups[1].entries.len(), 2);
        assert_eq!(groups[1].entries[0], snap1); // chronological (older)
        assert_eq!(groups[1].entries[1], snap3); // chronological (newer)
    }

    // --- Density bucket tests ---

    #[test]
    fn density_empty_timestamps() {
        let from = Utc::now();
        let to = from + chrono::Duration::hours(1);
        let buckets = super::super::compute_density_buckets(&[], from, to, 10);
        assert!(buckets.is_empty());
    }

    #[test]
    fn density_zero_buckets() {
        let now = Utc::now();
        let buckets = super::super::compute_density_buckets(&[now], now, now, 0);
        assert!(buckets.is_empty());
    }

    #[test]
    fn density_single_timestamp_single_bucket() {
        let now = Utc::now();
        let buckets = super::super::compute_density_buckets(&[now], now, now, 1);
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].count, 1);
    }

    #[test]
    fn density_even_distribution() {
        use chrono::TimeZone;
        let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        // 4 timestamps: at 0h, 1h, 2h, 3h
        let timestamps: Vec<_> = (0..4).map(|i| base + chrono::Duration::hours(i)).collect();
        let from = base;
        let to = base + chrono::Duration::hours(3);
        let buckets = super::super::compute_density_buckets(&timestamps, from, to, 3);
        assert_eq!(buckets.len(), 3);
        // Each bucket should have at least 1 timestamp
        assert_eq!(buckets[0].count, 1); // 0h
        assert_eq!(buckets[1].count, 1); // 1h
        assert_eq!(buckets[2].count, 2); // 2h and 3h (3h falls in last bucket)
    }

    #[test]
    fn density_all_in_one_bucket() {
        use chrono::TimeZone;
        let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let timestamps = vec![base, base, base];
        let buckets = super::super::compute_density_buckets(&timestamps, base, base, 5);
        assert_eq!(buckets.len(), 5);
        // All timestamps at the same instant go into first bucket
        assert_eq!(buckets[0].count, 3);
        for b in &buckets[1..] {
            assert_eq!(b.count, 0);
        }
    }

    // --- Cursor parsing/formatting tests ---

    #[test]
    fn parse_cursor_valid_utc() {
        let cursor = super::super::parse_cursor("2026-02-12T14:32:07+00:00:42").unwrap();
        assert_eq!(cursor.id, crate::types::SnapshotId(42));
        assert_eq!(
            cursor.timestamp,
            chrono::DateTime::parse_from_rfc3339("2026-02-12T14:32:07+00:00")
                .unwrap()
                .with_timezone(&Utc)
        );
    }

    #[test]
    fn parse_cursor_valid_z() {
        let cursor = super::super::parse_cursor("2026-02-12T14:32:07Z:100").unwrap();
        assert_eq!(cursor.id, crate::types::SnapshotId(100));
    }

    #[test]
    fn parse_cursor_valid_offset() {
        let cursor = super::super::parse_cursor("2026-02-12T14:32:07+05:30:7").unwrap();
        assert_eq!(cursor.id, crate::types::SnapshotId(7));
    }

    #[test]
    fn parse_cursor_invalid_no_colon() {
        assert!(super::super::parse_cursor("invalid").is_err());
    }

    #[test]
    fn parse_cursor_invalid_bad_timestamp() {
        assert!(super::super::parse_cursor("not-a-date:42").is_err());
    }

    #[test]
    fn parse_cursor_invalid_bad_id() {
        assert!(super::super::parse_cursor("2026-02-12T14:32:07Z:abc").is_err());
    }

    #[test]
    fn format_cursor_roundtrip() {
        use chrono::TimeZone;
        let cursor = crate::engine::db::HistoryCursor {
            timestamp: Utc.with_ymd_and_hms(2026, 2, 12, 14, 32, 7).unwrap(),
            id: crate::types::SnapshotId(42),
        };
        let formatted = super::super::format_cursor(&cursor);
        let parsed = super::super::parse_cursor(&formatted).unwrap();
        assert_eq!(parsed.timestamp, cursor.timestamp);
        assert_eq!(parsed.id, cursor.id);
    }

    // --- resolve_filter_path tests ---

    #[test]
    fn resolve_filter_path_tilde_expansion() {
        let home = std::env::var("HOME").unwrap();
        let result = super::super::filters::resolve_filter_path("~/.claude");
        assert!(result.starts_with(&home));
        assert!(result.contains(".claude"));
    }

    #[test]
    fn resolve_filter_path_absolute_passthrough() {
        let result = super::super::filters::resolve_filter_path("/tmp/nonexistent_test_path_12345");
        assert_eq!(result, "/tmp/nonexistent_test_path_12345");
    }

    #[test]
    fn resolve_filter_path_existing_canonicalized() {
        // /tmp should exist and canonicalize to /private/tmp on macOS
        let result = super::super::filters::resolve_filter_path("/tmp");
        // On macOS, /tmp -> /private/tmp
        assert!(result == "/tmp" || result == "/private/tmp");
    }

    // --- LogParams tests ---

    #[test]
    fn log_params_struct_literal() {
        let params = super::super::LogParams {
            target: Some("test.txt".to_string()),
            since: Some("5m".to_string()),
            until: None,
            limit: 100,
            include: vec!["*.rs".to_string()],
            exclude: vec!["*.bak".to_string()],
            ignore_case: true,
            grouped: false,
            format: crate::cli::OutputFormat::Json,
            density: false,
            num_buckets: 50,
            cursor_str: None,
        };

        assert_eq!(params.target, Some("test.txt".to_string()));
        assert_eq!(params.since, Some("5m".to_string()));
        assert_eq!(params.limit, 100);
        assert_eq!(params.include, vec!["*.rs".to_string()]);
        assert_eq!(params.exclude, vec!["*.bak".to_string()]);
        assert!(params.ignore_case);
        assert!(!params.grouped);
        assert_eq!(params.format, crate::cli::OutputFormat::Json);
        assert!(!params.density);
        assert_eq!(params.num_buckets, 50);
        assert_eq!(params.cursor_str, None);
    }

    #[test]
    fn log_params_default() {
        let params = super::super::LogParams::default();

        assert_eq!(params.target, None);
        assert_eq!(params.since, None);
        assert_eq!(params.limit, 1000);
        assert!(params.include.is_empty());
        assert!(params.exclude.is_empty());
        assert!(!params.ignore_case);
        assert!(!params.grouped);
        assert_eq!(params.format, crate::cli::OutputFormat::Human);
        assert!(!params.density);
        assert_eq!(params.num_buckets, 100);
        assert_eq!(params.cursor_str, None);
    }

    // --- GlobalLogParams tests ---

    #[test]
    fn global_log_params_struct_literal() {
        let params = super::super::GlobalLogParams {
            include_project: vec!["/path/to/proj1".to_string()],
            exclude_project: vec!["/path/to/excluded".to_string()],
            since: Some("1h".to_string()),
            until: None,
            limit: 500,
            include: vec!["*.py".to_string()],
            exclude: vec!["*.pyc".to_string()],
            ignore_case: false,
            grouped: true,
            format: crate::cli::OutputFormat::Human,
        };

        assert_eq!(params.include_project, vec!["/path/to/proj1".to_string()]);
        assert_eq!(
            params.exclude_project,
            vec!["/path/to/excluded".to_string()]
        );
        assert_eq!(params.since, Some("1h".to_string()));
        assert_eq!(params.limit, 500);
        assert_eq!(params.include, vec!["*.py".to_string()]);
        assert_eq!(params.exclude, vec!["*.pyc".to_string()]);
        assert!(!params.ignore_case);
        assert!(params.grouped);
        assert_eq!(params.format, crate::cli::OutputFormat::Human);
    }

    #[test]
    fn global_log_params_default() {
        let params = super::super::GlobalLogParams::default();

        assert!(params.include_project.is_empty());
        assert!(params.exclude_project.is_empty());
        assert_eq!(params.since, None);
        assert_eq!(params.limit, 1000);
        assert!(params.include.is_empty());
        assert!(params.exclude.is_empty());
        assert!(!params.ignore_case);
        assert!(!params.grouped);
        assert_eq!(params.format, crate::cli::OutputFormat::Human);
    }

    // --- DensityParams tests ---

    #[test]
    fn density_params_new() {
        let params = super::super::DensityParams::new(
            Some("2d".to_string()),
            vec!["*.txt".to_string()],
            vec!["*.tmp".to_string()],
            true,
            75,
        );

        assert_eq!(params.since, Some("2d".to_string()));
        assert_eq!(params.include, vec!["*.txt".to_string()]);
        assert_eq!(params.exclude, vec!["*.tmp".to_string()]);
        assert!(params.ignore_case);
        assert_eq!(params.num_buckets, 75);
    }

    #[test]
    fn density_params_default() {
        let params = super::super::DensityParams::default();

        assert_eq!(params.since, None);
        assert!(params.include.is_empty());
        assert!(params.exclude.is_empty());
        assert!(!params.ignore_case);
        assert_eq!(params.num_buckets, 100);
    }

    // --- GlobalDensityParams tests ---

    #[test]
    fn global_density_params_new() {
        let params = super::super::GlobalDensityParams::new(
            vec!["/proj1".to_string(), "/proj2".to_string()],
            vec!["/excluded".to_string()],
            Some("7d".to_string()),
            vec!["*.md".to_string()],
            vec!["*.swp".to_string()],
            false,
            200,
        );

        assert_eq!(
            params.include_project,
            vec!["/proj1".to_string(), "/proj2".to_string()]
        );
        assert_eq!(params.exclude_project, vec!["/excluded".to_string()]);
        assert_eq!(params.since, Some("7d".to_string()));
        assert_eq!(params.include, vec!["*.md".to_string()]);
        assert_eq!(params.exclude, vec!["*.swp".to_string()]);
        assert!(!params.ignore_case);
        assert_eq!(params.num_buckets, 200);
    }

    #[test]
    fn global_density_params_default() {
        let params = super::super::GlobalDensityParams::default();

        assert!(params.include_project.is_empty());
        assert!(params.exclude_project.is_empty());
        assert_eq!(params.since, None);
        assert!(params.include.is_empty());
        assert!(params.exclude.is_empty());
        assert!(!params.ignore_case);
        assert_eq!(params.num_buckets, 100);
    }

    #[test]
    fn log_params_clone() {
        let params = super::super::LogParams {
            target: Some("file.txt".to_string()),
            since: Some("1h".to_string()),
            until: None,
            limit: 123,
            include: vec!["*.rs".to_string()],
            exclude: vec!["*.bak".to_string()],
            ignore_case: true,
            grouped: true,
            format: crate::cli::OutputFormat::Json,
            density: true,
            num_buckets: 42,
            cursor_str: Some("cursor:123".to_string()),
        };

        let cloned = params.clone();
        assert_eq!(params.target, cloned.target);
        assert_eq!(params.since, cloned.since);
        assert_eq!(params.limit, cloned.limit);
        assert_eq!(params.include, cloned.include);
        assert_eq!(params.exclude, cloned.exclude);
        assert_eq!(params.ignore_case, cloned.ignore_case);
        assert_eq!(params.grouped, cloned.grouped);
        assert_eq!(params.format, cloned.format);
        assert_eq!(params.density, cloned.density);
        assert_eq!(params.num_buckets, cloned.num_buckets);
        assert_eq!(params.cursor_str, cloned.cursor_str);
    }
}
