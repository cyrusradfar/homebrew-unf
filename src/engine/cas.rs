//! Content-Addressable Storage (CAS) implementation.
//!
//! Provides a flat-file object store using BLAKE3 hashing for content
//! deduplication. Objects are stored immutably and addressed by their
//! content hash.
//!
//! ## Storage Layout
//!
//! Objects are stored in a two-level directory structure using the first
//! two hex characters of the hash as a prefix:
//!
//! ```text
//! {store_path}/
//!   ab/
//!     cd1234567890...  (remaining hex chars)
//!   de/
//!     f0abcdef1234...
//! ```
//!
//! This follows the SUPER principle: pure hashing functions have no side
//! effects, while I/O functions explicitly interact with the filesystem.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use blake3;

use crate::error::CasError;
use crate::types::ContentHash;

/// Computes the BLAKE3 hash of the given data.
///
/// This is a pure function: same input always produces the same output,
/// with no side effects.
///
/// # Arguments
///
/// * `data` - The bytes to hash
///
/// # Returns
///
/// A `ContentHash` containing the 64-character hex digest.
///
/// # Examples
///
/// ```
/// use unfudged::engine::cas::hash_content;
///
/// let data = b"hello world";
/// let hash1 = hash_content(data);
/// let hash2 = hash_content(data);
/// assert_eq!(hash1, hash2); // Deterministic
/// ```
pub fn hash_content(data: &[u8]) -> ContentHash {
    let hash = blake3::hash(data);
    ContentHash(hash.to_hex().to_string())
}

/// Verifies that data matches the expected content hash.
///
/// This is a pure function that performs hash comparison without side effects.
///
/// # Arguments
///
/// * `data` - The bytes to verify
/// * `expected` - The hash to verify against
///
/// # Returns
///
/// `true` if the hash of `data` matches `expected`, `false` otherwise.
///
/// # Examples
///
/// ```
/// use unfudged::engine::cas::{hash_content, verify_hash};
///
/// let data = b"hello world";
/// let hash = hash_content(data);
/// assert!(verify_hash(data, &hash));
/// assert!(!verify_hash(b"goodbye", &hash));
/// ```
pub fn verify_hash(data: &[u8], expected: &ContentHash) -> bool {
    let actual = hash_content(data);
    actual == *expected
}

/// Counts the number of lines in a byte slice.
///
/// Empty content returns 0. Content with no newlines returns 1.
/// Trailing newline does not add an extra line.
pub fn count_lines(data: &[u8]) -> u64 {
    if data.is_empty() {
        return 0;
    }
    let count = data.iter().filter(|&&b| b == b'\n').count() as u64;
    if data.last() == Some(&b'\n') {
        count
    } else {
        count + 1
    }
}

/// Statistics from a CAS garbage collection run.
#[derive(Debug, Clone, Default)]
pub struct GcStats {
    /// Number of unreferenced objects deleted.
    pub objects_removed: u64,
    /// Total bytes freed by deleting unreferenced objects.
    pub bytes_freed: u64,
}

/// Constructs the filesystem path for an object given its hash.
///
/// Uses the first two hex characters as a directory prefix, with the
/// remaining characters as the filename.
///
/// # Arguments
///
/// * `store_path` - Root directory of the object store
/// * `hash` - The content hash
///
/// # Returns
///
/// The full path where the object should be stored.
fn object_path(store_path: &Path, hash: &ContentHash) -> PathBuf {
    let hash_str = &hash.0;
    let prefix = &hash_str[..2];
    let suffix = &hash_str[2..];
    store_path.join(prefix).join(suffix)
}

/// Constructs the filesystem path for an object given its hash string.
///
/// Helper function for GC operations that work with hash strings
/// instead of ContentHash types.
///
/// # Arguments
///
/// * `store_path` - Root directory of the object store
/// * `hash` - The content hash as a string
///
/// # Returns
///
/// The full path where the object should be stored.
fn object_path_from_str(store_path: &Path, hash: &str) -> PathBuf {
    let prefix = &hash[..2];
    let suffix = &hash[2..];
    store_path.join(prefix).join(suffix)
}

/// Checks if an object exists in the store.
///
/// This is a non-mutating I/O operation that checks for file existence.
///
/// # Arguments
///
/// * `store_path` - Root directory of the object store
/// * `hash` - The content hash to check
///
/// # Returns
///
/// `true` if the object exists, `false` otherwise.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use unfudged::engine::cas::{object_exists, hash_content};
/// use unfudged::types::ContentHash;
///
/// let store = Path::new(".unfudged/objects");
/// let hash = ContentHash("ab1234567890".to_string());
/// if object_exists(store, &hash) {
///     println!("Object already stored");
/// }
/// ```
pub fn object_exists(store_path: &Path, hash: &ContentHash) -> bool {
    object_path(store_path, hash).exists()
}

/// Stores an object in the CAS.
///
/// Writes the data to disk at the path determined by the hash. If an object
/// with the same hash already exists, this is a no-op (content-addressable
/// deduplication).
///
/// Creates the parent directory if it doesn't exist.
///
/// # Arguments
///
/// * `store_path` - Root directory of the object store
/// * `hash` - The content hash (should match `hash_content(data)`)
/// * `data` - The bytes to store
///
/// # Returns
///
/// `Ok(())` on success, or a `CasError` if I/O fails.
///
/// # Errors
///
/// Returns `CasError::Io` if directory creation or file writing fails.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use unfudged::engine::cas::{hash_content, store_object};
///
/// let data = b"hello world";
/// let hash = hash_content(data);
/// let store = Path::new(".unfudged/objects");
/// store_object(store, &hash, data).expect("Failed to store object");
/// ```
pub fn store_object(store_path: &Path, hash: &ContentHash, data: &[u8]) -> Result<(), CasError> {
    let path = object_path(store_path, hash);

    // Skip if already exists (deduplication)
    if path.exists() {
        return Ok(());
    }

    // Create parent directory if needed
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Write object to disk
    fs::write(&path, data)?;

    Ok(())
}

/// Loads an object from the CAS.
///
/// Reads the object identified by the given hash from disk.
///
/// # Arguments
///
/// * `store_path` - Root directory of the object store
/// * `hash` - The content hash to load
///
/// # Returns
///
/// The object's bytes on success, or a `CasError` if the object is not found
/// or I/O fails.
///
/// # Errors
///
/// * `CasError::ObjectNotFound` if the object does not exist
/// * `CasError::Io` if reading the file fails
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use unfudged::engine::cas::{hash_content, store_object, load_object};
///
/// let data = b"hello world";
/// let hash = hash_content(data);
/// let store = Path::new(".unfudged/objects");
///
/// store_object(store, &hash, data).expect("Failed to store");
/// let loaded = load_object(store, &hash).expect("Failed to load");
/// assert_eq!(data, &loaded[..]);
/// ```
pub fn load_object(store_path: &Path, hash: &ContentHash) -> Result<Vec<u8>, CasError> {
    let path = object_path(store_path, hash);

    if !path.exists() {
        return Err(CasError::ObjectNotFound(hash.to_string()));
    }

    let data = fs::read(&path)?;
    Ok(data)
}

/// Lists all object hashes in the CAS store by walking the objects directory.
///
/// Returns the full hash string (prefix + remainder) for each object.
/// Returns an empty vec if the store directory does not exist.
///
/// # Arguments
///
/// * `store_path` - Root directory of the object store
///
/// # Returns
///
/// A sorted vector of all object hashes found in the store.
///
/// # Errors
///
/// Returns `CasError::Io` if directory traversal fails.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use unfudged::engine::cas::list_all_objects;
///
/// let store = Path::new(".unfudged/objects");
/// let all_objects = list_all_objects(store).expect("Failed to list objects");
/// println!("Found {} objects", all_objects.len());
/// ```
pub fn list_all_objects(store_path: &Path) -> Result<Vec<String>, CasError> {
    // Return empty vec if store doesn't exist
    if !store_path.exists() {
        return Ok(Vec::new());
    }

    let mut hashes = Vec::new();

    // Walk the store directory
    for entry in fs::read_dir(store_path)? {
        let entry = entry?;
        let path = entry.path();

        // Skip non-directories
        if !path.is_dir() {
            continue;
        }

        // Get the 2-char prefix from the directory name
        let prefix = match path.file_name() {
            Some(name) => match name.to_str() {
                Some(s) => s,
                None => continue,
            },
            None => continue,
        };

        // Read files in this prefix directory
        for file_entry in fs::read_dir(&path)? {
            let file_entry = file_entry?;
            let file_path = file_entry.path();

            // Skip directories
            if file_path.is_dir() {
                continue;
            }

            // Get the filename (remainder of hash)
            let suffix = match file_path.file_name() {
                Some(name) => match name.to_str() {
                    Some(s) => s,
                    None => continue,
                },
                None => continue,
            };

            // Reconstruct full hash: prefix + suffix
            hashes.push(format!("{}{}", prefix, suffix));
        }
    }

    // Sort for deterministic output
    hashes.sort();
    Ok(hashes)
}

/// Deletes a single object from the CAS store.
///
/// Returns the size in bytes of the deleted object.
/// If the object does not exist, returns 0 (idempotent).
/// After deletion, removes the parent prefix directory if it's now empty.
///
/// # Arguments
///
/// * `store_path` - Root directory of the object store
/// * `hash` - The content hash to delete
///
/// # Returns
///
/// The size in bytes of the deleted object, or 0 if it didn't exist.
///
/// # Errors
///
/// Returns `CasError::Io` if deletion fails.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
/// use unfudged::engine::cas::{hash_content, store_object, delete_object};
///
/// let data = b"temporary data";
/// let hash = hash_content(data);
/// let store = Path::new(".unfudged/objects");
///
/// store_object(store, &hash, data).expect("Failed to store");
/// let freed = delete_object(store, &hash.0).expect("Failed to delete");
/// println!("Freed {} bytes", freed);
/// ```
pub fn delete_object(store_path: &Path, hash: &str) -> Result<u64, CasError> {
    let path = object_path_from_str(store_path, hash);

    // If object doesn't exist, return 0 (idempotent)
    if !path.exists() {
        return Ok(0);
    }

    // Get file size before deletion
    let metadata = fs::metadata(&path)?;
    let size = metadata.len();

    // Delete the file
    fs::remove_file(&path)?;

    // Try to remove parent directory if it's now empty
    if let Some(parent) = path.parent() {
        let _ = fs::remove_dir(parent); // Ignore error if not empty
    }

    Ok(size)
}

/// Garbage-collects CAS objects not referenced by any snapshot.
///
/// Compares all stored objects against the set of referenced hashes.
/// Any object whose hash is NOT in `referenced` is deleted.
///
/// If `dry_run` is true, counts what would be deleted but doesn't delete.
///
/// # Arguments
///
/// * `store_path` - Root directory of the object store
/// * `referenced` - Set of hashes that are still referenced
/// * `dry_run` - If true, count but don't delete
///
/// # Returns
///
/// Statistics about the garbage collection operation.
///
/// # Errors
///
/// Returns `CasError::Io` if object traversal or deletion fails.
///
/// # Examples
///
/// ```no_run
/// use std::collections::HashSet;
/// use std::path::Path;
/// use unfudged::engine::cas::gc_unreferenced;
///
/// let store = Path::new(".unfudged/objects");
/// let mut referenced = HashSet::new();
/// referenced.insert("abcd1234...".to_string());
///
/// let stats = gc_unreferenced(store, &referenced, false)
///     .expect("GC failed");
/// println!("Removed {} objects, freed {} bytes",
///     stats.objects_removed, stats.bytes_freed);
/// ```
pub fn gc_unreferenced(
    store_path: &Path,
    referenced: &HashSet<String>,
    dry_run: bool,
) -> Result<GcStats, CasError> {
    let all_objects = list_all_objects(store_path)?;
    let mut stats = GcStats::default();

    for hash in &all_objects {
        if !referenced.contains(hash) {
            if dry_run {
                // Just count the size
                let path = object_path_from_str(store_path, hash);
                if let Ok(metadata) = fs::metadata(&path) {
                    stats.bytes_freed += metadata.len();
                }
                stats.objects_removed += 1;
            } else {
                let freed = delete_object(store_path, hash)?;
                stats.bytes_freed += freed;
                stats.objects_removed += 1;
            }
        }
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn hash_content_is_deterministic() {
        let data = b"hello world";
        let hash1 = hash_content(data);
        let hash2 = hash_content(data);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn hash_content_produces_64_hex_chars() {
        let data = b"test";
        let hash = hash_content(data);
        assert_eq!(hash.0.len(), 64);
        assert!(hash.0.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hash_content_differs_for_different_input() {
        let data1 = b"hello";
        let data2 = b"world";
        let hash1 = hash_content(data1);
        let hash2 = hash_content(data2);
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn verify_hash_accepts_matching_data() {
        let data = b"test data";
        let hash = hash_content(data);
        assert!(verify_hash(data, &hash));
    }

    #[test]
    fn verify_hash_rejects_mismatched_data() {
        let data = b"original";
        let hash = hash_content(data);
        assert!(!verify_hash(b"modified", &hash));
    }

    #[test]
    fn object_path_uses_two_char_prefix() {
        let store = Path::new("/tmp/store");
        let hash = ContentHash("abcdef1234567890".to_string());
        let path = object_path(store, &hash);
        assert_eq!(path, Path::new("/tmp/store/ab/cdef1234567890"));
    }

    #[test]
    fn object_exists_returns_false_for_missing() {
        let store = TempDir::new().expect("Failed to create temp dir");
        let hash = ContentHash("ff".repeat(32));
        assert!(!object_exists(store.path(), &hash));
    }

    #[test]
    fn store_and_load_roundtrip() {
        let store = TempDir::new().expect("Failed to create temp dir");
        let data = b"roundtrip test data";
        let hash = hash_content(data);

        store_object(store.path(), &hash, data).expect("Failed to store");
        let loaded = load_object(store.path(), &hash).expect("Failed to load");

        assert_eq!(data, &loaded[..]);
    }

    #[test]
    fn store_object_is_idempotent() {
        let store = TempDir::new().expect("Failed to create temp dir");
        let data = b"idempotent test";
        let hash = hash_content(data);

        // Store twice
        store_object(store.path(), &hash, data).expect("First store failed");
        store_object(store.path(), &hash, data).expect("Second store failed");

        // Should still load correctly
        let loaded = load_object(store.path(), &hash).expect("Failed to load");
        assert_eq!(data, &loaded[..]);
    }

    #[test]
    fn object_exists_returns_true_after_store() {
        let store = TempDir::new().expect("Failed to create temp dir");
        let data = b"existence test";
        let hash = hash_content(data);

        assert!(!object_exists(store.path(), &hash));
        store_object(store.path(), &hash, data).expect("Failed to store");
        assert!(object_exists(store.path(), &hash));
    }

    #[test]
    fn load_nonexistent_object_returns_error() {
        let store = TempDir::new().expect("Failed to create temp dir");
        let hash = ContentHash("ee".repeat(32));

        let result = load_object(store.path(), &hash);
        assert!(result.is_err());

        match result {
            Err(CasError::ObjectNotFound(h)) => assert_eq!(h, hash.to_string()),
            _ => panic!("Expected ObjectNotFound error"),
        }
    }

    #[test]
    fn deduplication_same_content_same_hash() {
        let store = TempDir::new().expect("Failed to create temp dir");
        let data = b"duplicate content";
        let hash = hash_content(data);

        // Store the same content twice
        store_object(store.path(), &hash, data).expect("First store failed");
        store_object(store.path(), &hash, data).expect("Second store failed");

        // Verify only one file exists on disk
        let path = object_path(store.path(), &hash);
        assert!(path.exists());

        // Load and verify content
        let loaded = load_object(store.path(), &hash).expect("Failed to load");
        assert_eq!(data, &loaded[..]);
    }

    #[test]
    fn store_creates_parent_directories() {
        let store = TempDir::new().expect("Failed to create temp dir");
        let data = b"nested directory test";
        let hash = hash_content(data);

        // Store should create the prefix directory
        store_object(store.path(), &hash, data).expect("Failed to store");

        let path = object_path(store.path(), &hash);
        assert!(path.exists());
        assert!(path.parent().expect("No parent dir").exists());
    }

    #[test]
    fn verify_hash_after_load() {
        let store = TempDir::new().expect("Failed to create temp dir");
        let data = b"verify after load";
        let hash = hash_content(data);

        store_object(store.path(), &hash, data).expect("Failed to store");
        let loaded = load_object(store.path(), &hash).expect("Failed to load");

        // Verify the loaded data matches the original hash
        assert!(verify_hash(&loaded, &hash));
    }

    #[test]
    fn list_all_objects_empty_store() {
        let store = TempDir::new().expect("Failed to create temp dir");
        let objects = list_all_objects(store.path()).expect("Failed to list objects");
        assert_eq!(objects.len(), 0);
    }

    #[test]
    fn list_all_objects_finds_stored_objects() {
        let store = TempDir::new().expect("Failed to create temp dir");

        // Store three objects
        let data1 = b"first object";
        let data2 = b"second object";
        let data3 = b"third object";

        let hash1 = hash_content(data1);
        let hash2 = hash_content(data2);
        let hash3 = hash_content(data3);

        store_object(store.path(), &hash1, data1).expect("Failed to store object 1");
        store_object(store.path(), &hash2, data2).expect("Failed to store object 2");
        store_object(store.path(), &hash3, data3).expect("Failed to store object 3");

        // List all objects
        let objects = list_all_objects(store.path()).expect("Failed to list objects");

        // Should find all three
        assert_eq!(objects.len(), 3);
        assert!(objects.contains(&hash1.0));
        assert!(objects.contains(&hash2.0));
        assert!(objects.contains(&hash3.0));
    }

    #[test]
    fn delete_object_removes_file() {
        let store = TempDir::new().expect("Failed to create temp dir");
        let data = b"delete me";
        let hash = hash_content(data);

        // Store the object
        store_object(store.path(), &hash, data).expect("Failed to store");
        assert!(object_exists(store.path(), &hash));

        // Delete it
        let freed = delete_object(store.path(), &hash.0).expect("Failed to delete");
        assert!(freed > 0);
        assert!(!object_exists(store.path(), &hash));
    }

    #[test]
    fn delete_object_nonexistent_returns_zero() {
        let store = TempDir::new().expect("Failed to create temp dir");
        let hash = ContentHash("aa".repeat(32));

        // Delete a non-existent object (idempotent)
        let freed = delete_object(store.path(), &hash.0).expect("Failed to delete");
        assert_eq!(freed, 0);
    }

    #[test]
    fn delete_object_cleans_empty_prefix_dir() {
        let store = TempDir::new().expect("Failed to create temp dir");
        let data = b"cleanup test";
        let hash = hash_content(data);

        // Store the object
        store_object(store.path(), &hash, data).expect("Failed to store");

        // Get the prefix directory
        let prefix = &hash.0[..2];
        let prefix_dir = store.path().join(prefix);
        assert!(prefix_dir.exists());

        // Delete the object
        delete_object(store.path(), &hash.0).expect("Failed to delete");

        // Prefix directory should be cleaned up
        assert!(!prefix_dir.exists());
    }

    #[test]
    fn gc_unreferenced_removes_orphans() {
        let store = TempDir::new().expect("Failed to create temp dir");

        // Store three objects
        let data1 = b"keep this one";
        let data2 = b"delete this";
        let data3 = b"delete this too";

        let hash1 = hash_content(data1);
        let hash2 = hash_content(data2);
        let hash3 = hash_content(data3);

        store_object(store.path(), &hash1, data1).expect("Failed to store");
        store_object(store.path(), &hash2, data2).expect("Failed to store");
        store_object(store.path(), &hash3, data3).expect("Failed to store");

        // Only reference hash1
        let mut referenced = HashSet::new();
        referenced.insert(hash1.0.clone());

        // Run GC
        let stats = gc_unreferenced(store.path(), &referenced, false).expect("GC failed");

        // Should have removed 2 objects
        assert_eq!(stats.objects_removed, 2);
        assert!(stats.bytes_freed > 0);

        // Verify hash1 still exists, hash2 and hash3 are gone
        assert!(object_exists(store.path(), &hash1));
        assert!(!object_exists(store.path(), &hash2));
        assert!(!object_exists(store.path(), &hash3));
    }

    #[test]
    fn gc_unreferenced_dry_run_deletes_nothing() {
        let store = TempDir::new().expect("Failed to create temp dir");

        // Store three objects
        let data1 = b"keep this one";
        let data2 = b"should not delete";
        let data3 = b"should not delete either";

        let hash1 = hash_content(data1);
        let hash2 = hash_content(data2);
        let hash3 = hash_content(data3);

        store_object(store.path(), &hash1, data1).expect("Failed to store");
        store_object(store.path(), &hash2, data2).expect("Failed to store");
        store_object(store.path(), &hash3, data3).expect("Failed to store");

        // Only reference hash1
        let mut referenced = HashSet::new();
        referenced.insert(hash1.0.clone());

        // Run dry-run GC
        let stats = gc_unreferenced(store.path(), &referenced, true).expect("GC failed");

        // Should have counted 2 objects for removal
        assert_eq!(stats.objects_removed, 2);
        assert!(stats.bytes_freed > 0);

        // Verify all objects still exist (dry run doesn't delete)
        assert!(object_exists(store.path(), &hash1));
        assert!(object_exists(store.path(), &hash2));
        assert!(object_exists(store.path(), &hash3));
    }

    #[test]
    fn count_lines_empty() {
        assert_eq!(count_lines(b""), 0);
    }

    #[test]
    fn count_lines_single_no_newline() {
        assert_eq!(count_lines(b"hello"), 1);
    }

    #[test]
    fn count_lines_single_with_newline() {
        assert_eq!(count_lines(b"hello\n"), 1);
    }

    #[test]
    fn count_lines_multi_no_trailing() {
        assert_eq!(count_lines(b"line1\nline2\nline3"), 3);
    }

    #[test]
    fn count_lines_multi_with_trailing() {
        assert_eq!(count_lines(b"line1\nline2\nline3\n"), 3);
    }

    #[test]
    fn count_lines_only_newlines() {
        assert_eq!(count_lines(b"\n\n\n"), 3);
    }
}
