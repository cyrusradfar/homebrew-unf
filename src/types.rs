//! Domain types for the UNFUDGED filesystem flight recorder.
//!
//! Defines newtype wrappers and core data structures used throughout the
//! codebase. All types follow the SUPER principle of explicit data flow:
//! they are plain data with no hidden state or side effects.

use std::collections::BTreeSet;
use std::fmt;

use chrono::{DateTime, Utc};

/// Per-project watch behavior, persisted verbatim in both `projects.json`
/// and `intent.json`.
///
/// Dependency-free by design: this is the shared value type consumed by
/// `watcher::filter`, `registry`, `intent`, `sentinel`, and `daemon` without
/// any of them learning about each other. `watcher::filter` in particular
/// must never import `registry` or `intent`.
///
/// `#[serde(flatten)]` this into the containing entry type so the on-disk
/// JSON keeps `force_watch_gitignore` and `unignored_dirs` as top-level
/// sibling keys — never a nested object — matching the v0.19.1 file shape
/// byte-for-byte.
///
/// `unignored_dirs` is a `BTreeSet`, not a `Vec`, on purpose: sorted and
/// deduplicated is a type invariant, so `==` is order-insensitive and a
/// hand-edited, unsorted `projects.json` deserializes to an equal set. That
/// removes any need for a normalize-on-write helper and keeps a daemon
/// reload comparison a plain `!=`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WatchSettings {
    /// When true, the daemon records files `.gitignore` excludes and hidden
    /// dotfiles for this project. `#[serde(default)]` keeps files written
    /// before this field existed loading with the flag off.
    #[serde(default)]
    pub force_watch_gitignore: bool,
    /// Names of hardcoded-excluded directories (e.g. `target`, `dist`) that
    /// this project has opted back into recording. `#[serde(default)]`
    /// keeps files written before this field existed loading with an empty
    /// set.
    #[serde(default)]
    pub unignored_dirs: BTreeSet<String>,
}

/// BLAKE3 hex digest of file content.
///
/// Wraps the 64-character hex string produced by hashing file bytes with BLAKE3.
/// Used as the key for content-addressable storage lookups.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContentHash(pub String);

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Unique snapshot identifier backed by a SQLite rowid.
///
/// Lightweight copy type used to reference snapshots across the system
/// without carrying the full snapshot data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotId(pub i64);

impl fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The kind of filesystem event that triggered a snapshot.
///
/// Maps directly to the watcher events we care about.
/// Renames are decomposed into a `Delete` of the old path
/// and a `Create` at the new path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventType {
    /// A new file was created.
    Create,
    /// An existing file's content was modified.
    Modify,
    /// A file was deleted.
    Delete,
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventType::Create => f.write_str("created"),
            EventType::Modify => f.write_str("modified"),
            EventType::Delete => f.write_str("deleted"),
        }
    }
}

/// A single point-in-time record of a file's state.
///
/// Snapshots are the fundamental unit of the flight recorder. Each one
/// captures the full state of a file at a specific instant, including
/// the content hash for CAS retrieval, the file size, and the event
/// that triggered the capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Unique identifier (SQLite rowid).
    pub id: SnapshotId,
    /// Absolute path to the file at the time of capture.
    pub file_path: String,
    /// BLAKE3 hash of the file content (used for CAS lookup).
    pub content_hash: ContentHash,
    /// File size in bytes at the time of capture.
    pub size_bytes: u64,
    /// UTC timestamp when the snapshot was taken.
    pub timestamp: DateTime<Utc>,
    /// The filesystem event that triggered this snapshot.
    pub event_type: EventType,
    /// Total number of lines in the file at the time of capture.
    pub line_count: u64,
    /// Number of lines added since the previous version.
    pub lines_added: u64,
    /// Number of lines removed since the previous version.
    pub lines_removed: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn content_hash_display_shows_hex() {
        let hash = ContentHash("abc123def456".to_string());
        assert_eq!(hash.to_string(), "abc123def456");
    }

    #[test]
    fn content_hash_equality() {
        let a = ContentHash("aaa".to_string());
        let b = ContentHash("aaa".to_string());
        let c = ContentHash("bbb".to_string());
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn content_hash_clone_is_independent() {
        let original = ContentHash("abc".to_string());
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn snapshot_id_display() {
        let id = SnapshotId(42);
        assert_eq!(id.to_string(), "42");
    }

    #[test]
    fn snapshot_id_is_copy() {
        let id = SnapshotId(1);
        let copied = id; // Copy, not move
        assert_eq!(id, copied);
    }

    #[test]
    fn event_type_display() {
        assert_eq!(EventType::Create.to_string(), "created");
        assert_eq!(EventType::Modify.to_string(), "modified");
        assert_eq!(EventType::Delete.to_string(), "deleted");
    }

    #[test]
    fn event_type_equality() {
        assert_eq!(EventType::Create, EventType::Create);
        assert_ne!(EventType::Create, EventType::Modify);
        assert_ne!(EventType::Modify, EventType::Delete);
    }

    #[test]
    fn snapshot_construction() {
        let snap = Snapshot {
            id: SnapshotId(1),
            file_path: "/tmp/test.rs".to_string(),
            content_hash: ContentHash("deadbeef".to_string()),
            size_bytes: 1024,
            timestamp: Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap(),
            event_type: EventType::Create,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        assert_eq!(snap.id, SnapshotId(1));
        assert_eq!(snap.file_path, "/tmp/test.rs");
        assert_eq!(snap.content_hash, ContentHash("deadbeef".to_string()));
        assert_eq!(snap.size_bytes, 1024);
        assert_eq!(snap.event_type, EventType::Create);
    }

    #[test]
    fn snapshot_clone() {
        let snap = Snapshot {
            id: SnapshotId(1),
            file_path: "/tmp/test.rs".to_string(),
            content_hash: ContentHash("deadbeef".to_string()),
            size_bytes: 512,
            timestamp: Utc.with_ymd_and_hms(2025, 6, 1, 8, 30, 0).unwrap(),
            event_type: EventType::Modify,
            line_count: 0,
            lines_added: 0,
            lines_removed: 0,
        };
        let cloned = snap.clone();
        assert_eq!(snap, cloned);
    }

    #[test]
    fn content_hash_usable_as_hash_key() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ContentHash("aaa".to_string()));
        set.insert(ContentHash("bbb".to_string()));
        set.insert(ContentHash("aaa".to_string())); // duplicate
        assert_eq!(set.len(), 2);
    }
}
