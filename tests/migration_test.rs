//! Integration tests for the storage migration flow.
//!
//! Tests individual migration phases end-to-end using isolated temp
//! directories. Each test that sets process-level env vars holds `ENV_LOCK`
//! to prevent interference with concurrent tests.
//!
//! NOTE: `unfudged::test_util::ENV_LOCK` is `#[cfg(test)]`-only, so it is not
//! accessible from `tests/`. We define our own lock here with the same
//! semantics. Both locks use `Mutex<()>` which is compatible — as long as
//! all tests in a single test binary (including unit tests compiled with
//! `--test unfudged`) hold *some* lock, there is no cross-test interference.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tempfile::TempDir;
use unfudged::cli::migrate;

// ---------------------------------------------------------------------------
// Shared env-var mutex
// ---------------------------------------------------------------------------

/// Serializes tests that mutate process-level env vars (HOME, UNF_HOME, etc.).
///
/// Integration tests compiled with `cargo test --test migration_test` run in
/// the same process with the default multi-threaded test runner. Without this
/// lock, two tests that both call `std::env::set_var("HOME", …)` would race.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------------------
// Helper: restore env vars after a test
// ---------------------------------------------------------------------------

struct EnvGuard {
    home: Option<String>,
    xdg: Option<String>,
    unf_home: Option<String>,
}

impl EnvGuard {
    /// Snapshot current env; set test values.
    fn set(home: &Path, xdg: &Path, unf_home: &Path) -> Self {
        let g = EnvGuard {
            home: std::env::var("HOME").ok(),
            xdg: std::env::var("XDG_CONFIG_HOME").ok(),
            unf_home: std::env::var("UNF_HOME").ok(),
        };
        std::env::set_var("HOME", home);
        std::env::set_var("XDG_CONFIG_HOME", xdg);
        std::env::set_var("UNF_HOME", unf_home);
        g
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match &self.xdg {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        match &self.unf_home {
            Some(v) => std::env::set_var("UNF_HOME", v),
            None => std::env::remove_var("UNF_HOME"),
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: minimal storage directory structure
// ---------------------------------------------------------------------------

/// Creates a minimal storage layout that mimics real UNFUDGED data.
///
/// Layout:
/// ```
/// root/
///   projects.json
///   daemon.pid      ← runtime file (must be skipped by copy)
///   sentinel.pid    ← runtime file (must be skipped by copy)
///   data/
///     Users/test/myproject/
///       db.sqlite3
///       db.sqlite3-wal   ← WAL file (must be skipped)
///       db.sqlite3-shm   ← SHM file (must be skipped)
///       objects/
///         abc123.blob
/// ```
fn create_test_storage(root: &Path) {
    let project_dir = root
        .join("data")
        .join("Users")
        .join("test")
        .join("myproject");
    fs::create_dir_all(&project_dir).unwrap();

    fs::write(root.join("projects.json"), r#"{"projects":[]}"#).unwrap();
    fs::write(root.join("daemon.pid"), b"12345").unwrap();
    fs::write(root.join("sentinel.pid"), b"67890").unwrap();

    fs::write(project_dir.join("db.sqlite3"), b"fake-sqlite-data").unwrap();
    fs::write(project_dir.join("db.sqlite3-wal"), b"wal-data").unwrap();
    fs::write(project_dir.join("db.sqlite3-shm"), b"shm-data").unwrap();

    let objects_dir = project_dir.join("objects");
    fs::create_dir_all(&objects_dir).unwrap();
    fs::write(objects_dir.join("abc123.blob"), b"content-hash-data").unwrap();
}

// ===========================================================================
// resolve_destination tests
// ===========================================================================

#[test]
fn test_resolve_destination_default_returns_home_unfudged() {
    let _lock = env_lock();
    let tmp = TempDir::new().unwrap();
    // Set HOME so dirs::home_dir() points to our temp dir.
    std::env::set_var("HOME", tmp.path());
    let prev_home = std::env::var("HOME").ok();

    let result = migrate::resolve_destination("default");

    // Restore HOME before asserting (so the guard cleanup is safe).
    match prev_home {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
    }

    let (path, is_default) = result.expect("resolve_destination(default) should succeed");
    assert!(is_default, "default keyword must set is_default=true");
    assert!(
        path.ends_with(".unfudged"),
        "default path must end with .unfudged, got: {}",
        path.display()
    );
}

#[test]
fn test_resolve_destination_absolute_path() {
    let (path, is_default) =
        migrate::resolve_destination("/tmp/my_storage").expect("absolute path must resolve");
    assert_eq!(path, PathBuf::from("/tmp/my_storage"));
    assert!(
        !is_default,
        "custom absolute path must not be marked as default"
    );
}

#[test]
fn test_resolve_destination_relative_path_rejected() {
    let result = migrate::resolve_destination("relative/path");
    assert!(result.is_err(), "relative paths must be rejected");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("No changes made"),
        "error message must include 'No changes made', got: {}",
        msg
    );
}

// ===========================================================================
// preflight_checks tests
// ===========================================================================

#[test]
fn test_preflight_same_path_rejected() {
    let tmp = TempDir::new().unwrap();
    let result = migrate::preflight_checks(tmp.path(), tmp.path(), false);
    assert!(result.is_err(), "same source and dest must be rejected");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("No changes made"),
        "error must mention 'No changes made', got: {}",
        msg
    );
}

#[test]
fn test_preflight_dest_inside_source_rejected() {
    let source = TempDir::new().unwrap();
    // Destination is a subdirectory of source that does not yet exist.
    let dest = source.path().join("nested").join("dest");

    let result = migrate::preflight_checks(source.path(), &dest, false);
    assert!(result.is_err(), "dest inside source must be rejected");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("No changes made"),
        "error must mention 'No changes made', got: {}",
        msg
    );
}

#[test]
fn test_preflight_nonempty_dest_rejected() {
    let source = TempDir::new().unwrap();
    let dest = TempDir::new().unwrap();

    // Make dest non-empty.
    fs::write(dest.path().join("existing_file.txt"), b"content").unwrap();

    let result = migrate::preflight_checks(source.path(), dest.path(), false);
    assert!(result.is_err(), "non-empty dest must be rejected");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("No changes made"),
        "error must mention 'No changes made', got: {}",
        msg
    );
}

#[test]
fn test_preflight_empty_dest_dir_accepted() {
    // An empty directory at dest is explicitly allowed (the check only rejects
    // non-empty destinations).
    let source = TempDir::new().unwrap();
    fs::write(source.path().join("data.txt"), b"some data").unwrap();
    let dest = TempDir::new().unwrap(); // newly-created, empty

    let result = migrate::preflight_checks(source.path(), dest.path(), false);
    assert!(
        result.is_ok(),
        "empty dest should pass preflight: {:?}",
        result
    );
}

// ===========================================================================
// copy_dir_recursive tests
// ===========================================================================

#[test]
fn test_copy_dir_recursive_copies_files() {
    let src = TempDir::new().unwrap();
    let dst = TempDir::new().unwrap();

    // Build a nested source structure.
    fs::create_dir_all(src.path().join("sub").join("deep")).unwrap();
    fs::write(src.path().join("root.txt"), b"root content").unwrap();
    fs::write(src.path().join("sub").join("mid.txt"), b"mid content").unwrap();
    fs::write(
        src.path().join("sub").join("deep").join("leaf.txt"),
        b"leaf content",
    )
    .unwrap();

    migrate::copy_dir_recursive(src.path(), dst.path(), &[], &[])
        .expect("copy_dir_recursive must succeed");

    assert!(
        dst.path().join("root.txt").exists(),
        "root.txt must be copied"
    );
    assert!(
        dst.path().join("sub").join("mid.txt").exists(),
        "sub/mid.txt must be copied"
    );
    assert!(
        dst.path()
            .join("sub")
            .join("deep")
            .join("leaf.txt")
            .exists(),
        "sub/deep/leaf.txt must be copied"
    );

    let content = fs::read(dst.path().join("root.txt")).unwrap();
    assert_eq!(content, b"root content", "file content must be preserved");
}

#[test]
fn test_copy_dir_recursive_skips_runtime_files() {
    let src = TempDir::new().unwrap();
    let dst = TempDir::new().unwrap();

    // Runtime files that must be skipped.
    fs::write(src.path().join("daemon.pid"), b"12345").unwrap();
    fs::write(src.path().join("sentinel.pid"), b"67890").unwrap();
    fs::write(src.path().join("stopped"), b"").unwrap();
    fs::write(src.path().join("db.sqlite3-wal"), b"wal").unwrap();
    fs::write(src.path().join("db.sqlite3-shm"), b"shm").unwrap();

    // Normal file that must be copied.
    fs::write(src.path().join("projects.json"), b"{}").unwrap();

    // Use the same skip lists as the production code.
    let skip_files = &["daemon.pid", "sentinel.pid", "stopped"];
    let skip_extensions = &["sqlite3-wal", "sqlite3-shm"];

    migrate::copy_dir_recursive(src.path(), dst.path(), skip_files, skip_extensions)
        .expect("copy_dir_recursive must succeed");

    assert!(
        !dst.path().join("daemon.pid").exists(),
        "daemon.pid must NOT be copied"
    );
    assert!(
        !dst.path().join("sentinel.pid").exists(),
        "sentinel.pid must NOT be copied"
    );
    assert!(
        !dst.path().join("stopped").exists(),
        "stopped must NOT be copied"
    );
    assert!(
        !dst.path().join("db.sqlite3-wal").exists(),
        "sqlite3-wal files must NOT be copied"
    );
    assert!(
        !dst.path().join("db.sqlite3-shm").exists(),
        "sqlite3-shm files must NOT be copied"
    );
    assert!(
        dst.path().join("projects.json").exists(),
        "projects.json MUST be copied"
    );
}

// ===========================================================================
// swap_config tests
// ===========================================================================

#[test]
fn test_swap_config_sets_storage_dir() {
    let _lock = env_lock();
    let tmp = TempDir::new().unwrap();
    let dest = tmp.path().join("new_storage");
    let config_dir = tmp.path().join("config");

    let _env = EnvGuard::set(tmp.path(), &config_dir, &tmp.path().join("unf_home"));

    migrate::swap_config(&dest, false).expect("swap_config must succeed");

    let cfg = unfudged::config::load().expect("config::load must succeed after swap_config");
    assert_eq!(
        cfg.storage_dir,
        Some(dest.clone()),
        "storage_dir must point to dest after swap_config(is_default=false)"
    );
}

#[test]
fn test_swap_config_default_clears_storage_dir() {
    let _lock = env_lock();
    let tmp = TempDir::new().unwrap();
    let dest = tmp.path().join("new_storage");
    let config_dir = tmp.path().join("config");

    let _env = EnvGuard::set(tmp.path(), &config_dir, &tmp.path().join("unf_home"));

    // First write a custom storage_dir so we can verify it's cleared.
    {
        let mut cfg = unfudged::config::load().unwrap_or_default();
        cfg.storage_dir = Some(tmp.path().join("old_storage"));
        unfudged::config::save(&cfg).expect("initial config save");
    }

    migrate::swap_config(&dest, true).expect("swap_config(is_default=true) must succeed");

    let cfg = unfudged::config::load().expect("config::load after swap_config(default)");
    assert!(
        cfg.storage_dir.is_none(),
        "storage_dir must be None after swap_config(is_default=true)"
    );
}

// ===========================================================================
// cleanup_old tests
// ===========================================================================

#[test]
fn test_cleanup_old_renames_to_migrated() {
    let base = TempDir::new().unwrap();
    let source = base.path().join("storage");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("data.txt"), b"data").unwrap();

    let backup = migrate::cleanup_old(&source).expect("cleanup_old must succeed");

    assert!(!source.exists(), "source must not exist after cleanup_old");
    assert!(backup.exists(), "backup path must exist after rename");
    assert!(
        backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".migrated"),
        "backup must end with .migrated, got: {}",
        backup.display()
    );
}

#[test]
fn test_cleanup_old_with_existing_migrated() {
    let base = TempDir::new().unwrap();
    let source = base.path().join("storage");
    let pre_existing_migrated = base.path().join("storage.migrated");

    fs::create_dir_all(&source).unwrap();
    fs::create_dir_all(&pre_existing_migrated).unwrap();
    fs::write(source.join("data.txt"), b"data").unwrap();

    let backup = migrate::cleanup_old(&source).expect("cleanup_old must succeed");

    assert!(!source.exists(), "source must not exist after cleanup_old");
    assert!(
        pre_existing_migrated.exists(),
        "pre-existing .migrated must still exist"
    );

    // The new backup must have a timestamp suffix to avoid colliding with the
    // pre-existing .migrated directory.
    let backup_name = backup.file_name().unwrap().to_string_lossy().to_string();
    assert!(
        backup_name.contains(".migrated."),
        "backup must have timestamp suffix when .migrated already exists, got: {}",
        backup_name
    );
    assert!(backup.exists(), "timestamped backup must exist on disk");
}

// ===========================================================================
// Full migration flow (without daemon start/stop)
// ===========================================================================

#[test]
fn test_full_migration_flow() {
    let _lock = env_lock();
    let tmp = TempDir::new().unwrap();

    let source = tmp.path().join("source_storage");
    let dest = tmp.path().join("dest_storage");
    let config_dir = tmp.path().join("config");

    // Create source with realistic test data.
    create_test_storage(&source);

    // Redirect env so config::load()/save() and registry::global_dir() use
    // our isolated temp directories.
    let _env = EnvGuard::set(tmp.path(), &config_dir, &source);

    // Phase 1: resolve destination.
    let (resolved_dest, is_default) = migrate::resolve_destination(dest.to_str().unwrap())
        .expect("resolve_destination must succeed for absolute path");
    assert_eq!(resolved_dest, dest);
    assert!(!is_default);

    // Phase 2: pre-flight.
    migrate::preflight_checks(&source, &dest, false)
        .expect("preflight_checks must pass for valid source/dest");

    // Phase 3: copy data (skip runtime files).
    let skip_files = &["daemon.pid", "sentinel.pid", "stopped"];
    let skip_extensions = &["sqlite3-wal", "sqlite3-shm"];
    migrate::copy_dir_recursive(&source, &dest, skip_files, skip_extensions)
        .expect("copy_dir_recursive must succeed");

    // Phase 4: swap config.
    migrate::swap_config(&dest, false).expect("swap_config must succeed");

    // Phase 5: verify destination.
    migrate::verify_destination(&dest).expect("verify_destination must succeed");

    // Phase 6: cleanup old (rename source to .migrated).
    let backup = migrate::cleanup_old(&source).expect("cleanup_old must succeed");

    // --- Assertions ---

    // Source must no longer exist under its original name.
    assert!(
        !source.exists(),
        "source must be renamed away after cleanup_old"
    );

    // Backup must exist.
    assert!(backup.exists(), "backup path must exist");
    assert!(
        backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".migrated"),
        "backup must end with .migrated"
    );

    // Destination must contain data files.
    assert!(
        dest.join("projects.json").exists(),
        "projects.json must be at dest"
    );
    assert!(
        dest.join("data")
            .join("Users")
            .join("test")
            .join("myproject")
            .join("db.sqlite3")
            .exists(),
        "db.sqlite3 must be copied to dest"
    );
    assert!(
        dest.join("data")
            .join("Users")
            .join("test")
            .join("myproject")
            .join("objects")
            .join("abc123.blob")
            .exists(),
        "blob object must be copied to dest"
    );

    // Runtime files must NOT be at dest.
    assert!(
        !dest.join("daemon.pid").exists(),
        "daemon.pid must NOT be at dest"
    );
    assert!(
        !dest.join("sentinel.pid").exists(),
        "sentinel.pid must NOT be at dest"
    );
    assert!(
        !dest
            .join("data")
            .join("Users")
            .join("test")
            .join("myproject")
            .join("db.sqlite3-wal")
            .exists(),
        "sqlite3-wal file must NOT be at dest"
    );
    assert!(
        !dest
            .join("data")
            .join("Users")
            .join("test")
            .join("myproject")
            .join("db.sqlite3-shm")
            .exists(),
        "sqlite3-shm file must NOT be at dest"
    );

    // Config must point to dest.
    let cfg = unfudged::config::load().expect("config::load must succeed after migration");
    assert_eq!(
        cfg.storage_dir,
        Some(dest.clone()),
        "config.storage_dir must be updated to dest"
    );
}

// ===========================================================================
// Regression: migrate back to default path via explicit absolute path
// ===========================================================================

/// Regression test for the bug where `unf config --move-storage ~/.unfudged`
/// (after a previous `--move-storage <temp>`) left `config.json` with
/// `storage_dir` pointing to the temp path instead of being reset to `null`.
///
/// The root cause was that `resolve_destination` returned `is_default=false`
/// for any explicitly-specified absolute path, even when that path equals
/// `$HOME/.unfudged`. `swap_config` then wrote `Some(path)` rather than
/// `None`, so subsequent calls to `registry::global_dir()` used the stale
/// (and potentially non-existent) path.
#[test]
fn test_migrate_back_to_default_path_clears_storage_dir() {
    let _lock = env_lock();
    let tmp = TempDir::new().unwrap();

    // HOME/.unfudged is the canonical default location.
    let default_storage = tmp.path().join(".unfudged");
    let intermediate_storage = tmp.path().join("moved_storage");
    let config_dir = tmp.path().join("config");

    // Use HOME = tmp so that dirs::home_dir() and resolve_destination() both
    // agree on what the "default" path is.
    let _env = EnvGuard::set(tmp.path(), &config_dir, &default_storage);

    // --- Step 1: simulate initial migration to an intermediate location ---

    migrate::swap_config(&intermediate_storage, false)
        .expect("swap_config to intermediate must succeed");

    let cfg = unfudged::config::load().expect("load config after move to intermediate");
    assert_eq!(
        cfg.storage_dir,
        Some(intermediate_storage.clone()),
        "after first move, config must point to intermediate path"
    );

    // --- Step 2: migrate back by passing the default path explicitly ---

    // This is the failing scenario from the E2E test:
    //   unf config --move-storage ~/.unfudged
    // The shell expands ~/.unfudged to an absolute path; we pass that directly.
    let (resolved, is_default) = migrate::resolve_destination(default_storage.to_str().unwrap())
        .expect("resolve_destination must succeed for $HOME/.unfudged");

    assert_eq!(
        resolved, default_storage,
        "resolved path must match $HOME/.unfudged"
    );
    assert!(
        is_default,
        "explicit $HOME/.unfudged must be recognized as the default (is_default=true)"
    );

    migrate::swap_config(&resolved, is_default).expect("swap_config back to default must succeed");

    // --- Assertions ---

    let cfg_after = unfudged::config::load().expect("load config after migrating back to default");

    assert!(
        cfg_after.storage_dir.is_none(),
        "after migrating back to $HOME/.unfudged, storage_dir must be None (not {:?})",
        cfg_after.storage_dir
    );
}
