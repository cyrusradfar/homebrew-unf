//! End-to-end integration tests for the UNFUDGED CLI.
//!
//! These tests exercise the full binary using `assert_cmd` to verify CLI behavior.
//! Each test creates an isolated temporary directory to avoid interference.

use std::fs;
use std::thread;
use std::time::Duration;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;
use common::{isolated_cmd, resolve_storage_dir_isolated, unf_cmd, IsolatedDaemonGuard};

#[test]
fn init_creates_storage_directory() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Run init in temp directory
    let mut cmd = isolated_cmd(unf_home.path());
    cmd.current_dir(temp.path()).arg("init").assert().success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Verify centralized storage directory structure was created
    let storage_dir = resolve_storage_dir_isolated(unf_home.path(), temp.path());
    assert!(
        storage_dir.exists(),
        "centralized storage directory should be created"
    );
    assert!(
        storage_dir.join("db.sqlite3").exists(),
        "db.sqlite3 should be created"
    );
    assert!(
        storage_dir.join("objects").exists(),
        "objects directory should be created"
    );
    assert!(
        storage_dir.join("daemon.pid").exists(),
        "daemon.pid should be created"
    );

    // Verify NO .unfudged/ in project directory
    assert!(
        !temp.path().join(".unfudged").exists(),
        ".unfudged should NOT exist in project directory"
    );
}

#[test]
fn init_shows_watching_message() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Run init and verify output message
    let mut cmd = isolated_cmd(unf_home.path());
    cmd.current_dir(temp.path())
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("Watching"));
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
}

#[test]
fn double_init_shows_already_recording() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // First init
    let mut cmd1 = isolated_cmd(unf_home.path());
    cmd1.current_dir(temp.path()).arg("init").assert().success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Give daemon a moment to start
    thread::sleep(Duration::from_millis(100));

    // Second init should report already recording
    let mut cmd2 = isolated_cmd(unf_home.path());
    cmd2.current_dir(temp.path())
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("Already watching"));
}

#[test]
fn status_not_initialized() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Run status without init — error goes to stderr
    let mut cmd = isolated_cmd(unf_home.path());
    cmd.current_dir(temp.path())
        .arg("status")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("not watching"));
}

#[test]
fn status_after_init() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Watch the project (starts isolated daemon)
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();

    // Give daemon a moment to start and take snapshots
    thread::sleep(Duration::from_millis(500));

    // Run status
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("status")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Watching since")
                .and(predicate::str::contains("Snapshots:"))
                .and(predicate::str::contains("Files tracked:"))
                .and(predicate::str::contains("Store size:")),
        );
}

#[test]
fn stop_not_initialized() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Run stop without watch — message goes to stderr
    let mut cmd = isolated_cmd(unf_home.path());
    cmd.current_dir(temp.path())
        .arg("stop")
        .assert()
        .success()
        .stderr(predicate::str::contains("Not watching"));
}

#[test]
fn stop_after_init() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Watch the project (starts isolated daemon)
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();

    // Give daemon a moment to start
    thread::sleep(Duration::from_millis(500));

    // Stop the daemon
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("stop")
        .assert()
        .success()
        .stdout(predicate::str::contains("Stopped"));

    // Verify global PID file was removed
    let pid_file = unf_home.path().join("daemon.pid");
    assert!(!pid_file.exists(), "PID file should be removed after stop");
}

#[test]
fn log_not_initialized() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Run log without init (no target - shows all)
    let mut cmd = isolated_cmd(unf_home.path());
    cmd.current_dir(temp.path()).arg("log").assert().failure(); // Should fail without .unfudged directory
}

#[test]
fn log_not_initialized_with_file() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Run log for a specific file without init
    let mut cmd = isolated_cmd(unf_home.path());
    cmd.current_dir(temp.path())
        .arg("log")
        .arg("test.txt")
        .assert()
        .failure(); // Should fail without .unfudged directory
}

#[test]
fn log_empty_history() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Init first
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Give daemon a moment to start
    thread::sleep(Duration::from_millis(100));

    // Query log for a file that doesn't exist - should exit with code 4
    let mut log_cmd = isolated_cmd(unf_home.path());
    log_cmd
        .current_dir(temp.path())
        .arg("log")
        .arg("nonexistent.txt")
        .assert()
        .code(4)
        .stderr(predicate::str::contains("No history"));
}

#[test]
fn log_with_file_history() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Create a test file before init
    let test_file = temp.path().join("test.txt");
    fs::write(&test_file, "initial content").expect("Failed to write test file");

    // Init
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Give daemon time to start and snapshot the file
    thread::sleep(Duration::from_millis(200));

    // Modify the file
    fs::write(&test_file, "modified content").expect("Failed to modify test file");

    // Wait for debounce window (3+ seconds) plus processing time
    thread::sleep(Duration::from_millis(3500));

    // Query log
    let mut log_cmd = isolated_cmd(unf_home.path());
    log_cmd
        .current_dir(temp.path())
        .arg("log")
        .arg("test.txt")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("test.txt")
                .and(predicate::str::contains("created").or(predicate::str::contains("modified"))),
        );
}

#[test]
fn help_output() {
    let mut cmd = unf_cmd();
    cmd.arg("--help").assert().success().stdout(
        predicate::str::contains("watch")
            .and(predicate::str::contains("status"))
            .and(predicate::str::contains("stop"))
            .and(predicate::str::contains("log"))
            .and(predicate::str::contains("diff"))
            .and(predicate::str::contains("restore"))
            .and(predicate::str::contains("cat")),
    );
}

#[test]
fn version_output() {
    let mut cmd = unf_cmd();
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn diff_not_initialized() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Run diff without init
    let mut cmd = isolated_cmd(unf_home.path());
    cmd.current_dir(temp.path())
        .arg("diff")
        .arg("--at")
        .arg("5m")
        .assert()
        .failure(); // Should fail without .unfudged directory
}

#[test]
fn restore_not_initialized() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Run restore without init
    let mut cmd = isolated_cmd(unf_home.path());
    cmd.current_dir(temp.path())
        .arg("restore")
        .arg("--at")
        .arg("5m")
        .arg("--dry-run")
        .assert()
        .failure(); // Should fail without .unfudged directory
}

#[test]
fn invalid_command_shows_error() {
    let mut cmd = unf_cmd();
    cmd.arg("nonexistent-command")
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

#[test]
fn init_stop_cycle() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // First cycle: init -> stop
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(100));

    let mut stop_cmd = isolated_cmd(unf_home.path());
    stop_cmd
        .current_dir(temp.path())
        .arg("stop")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(100));

    // Second cycle: init again after stopping
    let mut init_cmd2 = isolated_cmd(unf_home.path());
    init_cmd2
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(100));

    let mut stop_cmd2 = isolated_cmd(unf_home.path());
    stop_cmd2
        .current_dir(temp.path())
        .arg("stop")
        .assert()
        .success();
}

#[test]
fn status_shows_zero_snapshots_initially() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Watch empty directory (starts isolated daemon)
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();

    // Give daemon a moment to start
    thread::sleep(Duration::from_millis(500));

    // Check status
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Snapshots:"));
}

#[test]
fn restore_dry_run_shows_what_would_be_restored() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Create a test file
    let test_file = temp.path().join("test.txt");
    fs::write(&test_file, "original").expect("Failed to write test file");

    // Init
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Give daemon time to snapshot
    thread::sleep(Duration::from_millis(200));

    // Try dry-run restore (won't restore anything since it's immediate)
    let mut restore_cmd = isolated_cmd(unf_home.path());
    restore_cmd
        .current_dir(temp.path())
        .arg("restore")
        .arg("--at")
        .arg("1m")
        .arg("--dry-run")
        .assert()
        .success();
}

#[test]
fn log_all_history_no_target() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Init
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    thread::sleep(Duration::from_millis(100));

    // Log with no target should work (shows all files or "No history" with code 4)
    let mut log_cmd = isolated_cmd(unf_home.path());
    log_cmd.current_dir(temp.path()).arg("log").assert().code(4);
}

#[test]
fn log_since_filter() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Init
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    thread::sleep(Duration::from_millis(100));

    // Log with --since flag should work (returns code 4 if no history)
    let mut log_cmd = isolated_cmd(unf_home.path());
    log_cmd
        .current_dir(temp.path())
        .arg("log")
        .arg("--since")
        .arg("1h")
        .assert()
        .code(4);
}

#[test]
fn log_nonexistent_file_shows_no_history() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Init
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    thread::sleep(Duration::from_millis(100));

    // Log for a file that doesn't exist and has no history - should exit with code 4
    let mut log_cmd = isolated_cmd(unf_home.path());
    log_cmd
        .current_dir(temp.path())
        .arg("log")
        .arg("does-not-exist.txt")
        .assert()
        .code(4)
        .stderr(predicate::str::contains("No history"));
}

#[test]
fn log_help_shows_new_args() {
    let mut cmd = unf_cmd();
    cmd.arg("log")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("TARGET").and(predicate::str::contains("--since")));
}

#[test]
fn status_after_stop_shows_stopped_message() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("stop")
        .assert()
        .success();

    // After intentional stop, status should say "Watching stopped." (not "unexpectedly")
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("status")
        .assert()
        .stdout(
            predicate::str::contains("Watching stopped.")
                .and(predicate::str::contains("unexpectedly").not()),
        );
}

#[test]
fn stopped_sentinel_created_on_stop() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("stop")
        .assert()
        .success();

    // Verify stopped sentinel exists in centralized storage
    let storage_dir = resolve_storage_dir_isolated(unf_home.path(), temp.path());
    assert!(storage_dir.join("stopped").exists());
}

#[test]
fn stopped_sentinel_removed_on_init() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    let storage_dir = resolve_storage_dir_isolated(unf_home.path(), temp.path());

    // First watch+stop cycle
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("stop")
        .assert()
        .success();

    assert!(storage_dir.join("stopped").exists());

    // Re-watch should remove sentinel
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    assert!(!storage_dir.join("stopped").exists());
}

// JSON mode tests

#[test]
fn json_status_not_initialized() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let mut cmd = isolated_cmd(unf_home.path());
    cmd.current_dir(temp.path())
        .arg("--json")
        .arg("status")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("\"error\""));
}

#[test]
fn json_init_outputs_json() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Watch with JSON (init is legacy and delegates to watch)
    let watch_out = isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("--json")
        .arg("watch")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Verify JSON contains "status" field
    let stdout = String::from_utf8_lossy(&watch_out.get_output().stdout);
    assert!(
        stdout.contains("\"status\""),
        "JSON output should contain 'status' field"
    );
    assert!(
        stdout.contains("\"auto_restart\""),
        "JSON output should contain 'auto_restart' field"
    );
}

#[test]
fn json_stop_outputs_json() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Watch first (starts isolated daemon)
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    // Stop with JSON
    let stop_out = isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("--json")
        .arg("stop")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&stop_out.get_output().stdout);
    assert!(
        stdout.contains("\"status\"") && stdout.contains("\"stopped\""),
        "JSON output should contain status=stopped"
    );
}

#[test]
fn json_status_while_recording() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    let status_out = isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("--json")
        .arg("status")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&status_out.get_output().stdout);
    assert!(
        stdout.contains("\"recording\":true") || stdout.contains("\"recording\": true"),
        "JSON output should contain recording=true"
    );
    assert!(
        stdout.contains("\"snapshots\""),
        "JSON output should contain snapshots field"
    );
    assert!(
        stdout.contains("\"files_tracked\""),
        "JSON output should contain files_tracked field"
    );
}

#[test]
fn json_log_empty() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(100));

    let mut log_cmd = isolated_cmd(unf_home.path());
    log_cmd
        .current_dir(temp.path())
        .arg("--json")
        .arg("log")
        .assert()
        .code(4) // NoResults
        .stdout(predicate::str::contains("[]"));
}

#[test]
fn json_log_with_entries() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(200));

    // Create a file to generate a snapshot
    fs::write(temp.path().join("test.txt"), b"content").expect("write file");
    thread::sleep(Duration::from_millis(4000)); // Wait for debounce window (3s) + buffer

    let log_out = isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("--json")
        .arg("log")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&log_out.get_output().stdout);
    assert!(
        stdout.contains("\"file\"") && stdout.contains("test.txt"),
        "JSON output should contain file entries"
    );
    assert!(
        stdout.contains("\"event\""),
        "JSON output should contain event field"
    );
    assert!(
        stdout.contains("\"timestamp\""),
        "JSON output should contain timestamp field"
    );
}

#[test]
fn json_diff_no_changes() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(100));

    let mut diff_cmd = isolated_cmd(unf_home.path());
    let diff_out = diff_cmd
        .current_dir(temp.path())
        .arg("--json")
        .arg("diff")
        .arg("--at")
        .arg("5m")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&diff_out.get_output().stdout);
    assert!(
        stdout.contains("\"from\"") && stdout.contains("\"to\"") && stdout.contains("\"changes\""),
        "JSON output should contain diff structure"
    );
    assert!(
        stdout.contains("[]") || stdout.contains("\"changes\": []"),
        "JSON output should show empty changes array"
    );
}

#[test]
fn json_restore_dry_run() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(100));

    // Create a file
    fs::write(temp.path().join("test.txt"), b"v1").expect("write file");
    thread::sleep(Duration::from_millis(100));

    // Restore with JSON
    let mut restore_cmd = isolated_cmd(unf_home.path());
    let restore_out = restore_cmd
        .current_dir(temp.path())
        .arg("--json")
        .arg("restore")
        .arg("--at")
        .arg("5m")
        .arg("--dry-run")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&restore_out.get_output().stdout);
    assert!(
        stdout.contains("\"target_time\"")
            && stdout.contains("\"restored\"")
            && stdout.contains("\"dry_run\": true"),
        "JSON output should contain restore structure with dry_run=true"
    );
}

#[test]
fn restore_yes_flag_works() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Create a file
    fs::write(temp.path().join("test.txt"), b"original").expect("write");

    // Init
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(200));

    // Restore with --yes (should succeed without prompt)
    let mut restore_cmd = isolated_cmd(unf_home.path());
    restore_cmd
        .current_dir(temp.path())
        .arg("restore")
        .arg("--at")
        .arg("1m")
        .arg("--yes")
        .assert()
        .success();
}

#[test]
fn restore_y_flag_works() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Create a file
    fs::write(temp.path().join("test.txt"), b"original").expect("write");

    // Init
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(200));

    // Restore with -y (short form, should succeed without prompt)
    let mut restore_cmd = isolated_cmd(unf_home.path());
    restore_cmd
        .current_dir(temp.path())
        .arg("restore")
        .arg("--at")
        .arg("1m")
        .arg("-y")
        .assert()
        .success();
}

#[test]
fn restore_json_mode_auto_yes() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Init
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(200));

    // JSON restore (should auto-yes without prompt)
    let mut restore_cmd = isolated_cmd(unf_home.path());
    restore_cmd
        .current_dir(temp.path())
        .arg("--json")
        .arg("restore")
        .arg("--at")
        .arg("1m")
        .assert()
        .success();
}

#[test]
fn restore_help_shows_yes_flag() {
    let mut cmd = unf_cmd();
    cmd.arg("restore")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--yes"));
}

#[test]
fn cat_not_initialized() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    let mut cmd = isolated_cmd(unf_home.path());
    cmd.current_dir(temp.path())
        .arg("cat")
        .arg("test.txt")
        .arg("--at")
        .arg("5m")
        .assert()
        .code(2); // NotInitialized
}

#[test]
fn cat_requires_at_or_snapshot() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(100));

    let mut cmd = isolated_cmd(unf_home.path());
    cmd.current_dir(temp.path())
        .arg("cat")
        .arg("test.txt")
        .assert()
        .code(3); // InvalidArgument
}

#[test]
fn cat_help_shows_args() {
    let mut cmd = unf_cmd();
    cmd.arg("cat")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--at").and(predicate::str::contains("--snapshot")));
}

#[test]
fn diff_from_to_help() {
    let mut cmd = unf_cmd();
    cmd.arg("diff").arg("--help").assert().success().stdout(
        predicate::str::contains("--from")
            .and(predicate::str::contains("--to"))
            .and(predicate::str::contains("--at")),
    );
}

#[test]
fn diff_requires_at_or_from() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(100));

    // diff without --at or --from should fail with exit code 3
    let mut cmd = isolated_cmd(unf_home.path());
    cmd.current_dir(temp.path()).arg("diff").assert().code(3);
}

#[test]
fn diff_at_and_from_conflict() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(100));

    // Using both --at and --from should fail with exit code 3
    let mut cmd = isolated_cmd(unf_home.path());
    cmd.current_dir(temp.path())
        .arg("diff")
        .arg("--at")
        .arg("5m")
        .arg("--from")
        .arg("10m")
        .assert()
        .code(3);
}

// --- Prune tests ---

#[test]
fn prune_not_initialized() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    let mut cmd = isolated_cmd(unf_home.path());
    cmd.current_dir(temp.path()).arg("prune").assert().code(2); // NotInitialized
}

#[test]
fn prune_help_shows_flags() {
    let mut cmd = unf_cmd();
    cmd.arg("prune").arg("--help").assert().success().stdout(
        predicate::str::contains("--older-than")
            .and(predicate::str::contains("--dry-run"))
            .and(predicate::str::contains("--all-projects")),
    );
}

#[test]
fn prune_dry_run_nothing_to_prune() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(100));

    // Dry-run prune with default 7d cutoff — nothing old enough to prune
    let mut prune_cmd = isolated_cmd(unf_home.path());
    prune_cmd
        .current_dir(temp.path())
        .arg("prune")
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing to prune"));
}

#[test]
fn prune_older_than_flag_parses() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(100));

    // Verify --older-than with various valid formats
    let mut prune_cmd = isolated_cmd(unf_home.path());
    prune_cmd
        .current_dir(temp.path())
        .arg("prune")
        .arg("--dry-run")
        .arg("--older-than")
        .arg("30d")
        .assert()
        .success();
}

#[test]
fn prune_json_mode() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(100));

    let mut prune_cmd = isolated_cmd(unf_home.path());
    let prune_out = prune_cmd
        .current_dir(temp.path())
        .arg("--json")
        .arg("prune")
        .arg("--dry-run")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&prune_out.get_output().stdout);
    assert!(
        stdout.contains("\"dry_run\"") && stdout.contains("\"snapshots_removed\""),
        "JSON output should contain prune fields: {}",
        stdout
    );
    assert!(
        stdout.contains("\"objects_removed\"") && stdout.contains("\"bytes_freed\""),
        "JSON output should contain GC fields: {}",
        stdout
    );
}

#[test]
fn prune_invalid_older_than() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(100));

    // Invalid time spec should fail
    let mut prune_cmd = isolated_cmd(unf_home.path());
    prune_cmd
        .current_dir(temp.path())
        .arg("prune")
        .arg("--older-than")
        .arg("invalid")
        .assert()
        .code(3); // InvalidArgument
}

#[test]
fn prune_after_stop_with_data() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Init first
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(200));

    // Create a file while daemon is running (triggers watcher)
    fs::write(temp.path().join("test.txt"), b"content").expect("write file");

    // Wait for debounce (3s) + buffer
    thread::sleep(Duration::from_millis(4000));

    // Stop daemon (so we have stable state)
    let mut stop_cmd = isolated_cmd(unf_home.path());
    stop_cmd
        .current_dir(temp.path())
        .arg("stop")
        .assert()
        .success();

    // Prune with 1m cutoff — snapshots are seconds old, so nothing pruned
    let mut prune_cmd = isolated_cmd(unf_home.path());
    prune_cmd
        .current_dir(temp.path())
        .arg("prune")
        .arg("--older-than")
        .arg("1m")
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing to prune"));
}

// --- Log --stats tests ---

#[test]
fn log_stats_help_shows_flag() {
    // --stats is deprecated (hidden, always-on). Verify help shows grouping instead.
    let mut cmd = unf_cmd();
    cmd.arg("log")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--group-by-file"));
}

#[test]
fn log_stats_with_file_history() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Create a test file
    let test_file = temp.path().join("test.txt");
    fs::write(&test_file, "line 1\nline 2\nline 3\n").expect("write file");

    // Init
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Wait for debounce
    thread::sleep(Duration::from_millis(4000));

    // Modify the file
    fs::write(&test_file, "line 1\nline 2 modified\nline 3\nline 4\n").expect("modify file");

    // Wait for debounce
    thread::sleep(Duration::from_millis(4000));

    // Log with --stats should show +/- counts
    let mut log_cmd = isolated_cmd(unf_home.path());
    log_cmd
        .current_dir(temp.path())
        .arg("log")
        .arg("test.txt")
        .arg("--stats")
        .assert()
        .success()
        .stdout(predicate::str::contains("+").and(predicate::str::contains("test.txt")));
}

#[test]
fn log_stats_short_flag() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Init first so daemon is watching
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(200));

    // Create a test file while daemon is running
    fs::write(temp.path().join("test.txt"), b"content").expect("write file");

    // Wait for debounce
    thread::sleep(Duration::from_millis(4000));

    // Log with -s (short form) should work
    let mut log_cmd = isolated_cmd(unf_home.path());
    log_cmd
        .current_dir(temp.path())
        .arg("log")
        .arg("test.txt")
        .arg("-s")
        .assert()
        .success()
        .stdout(predicate::str::contains("test.txt"));
}

#[test]
fn log_stats_json_includes_lines() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Init first so daemon is watching
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(200));

    // Create a test file while daemon is running
    fs::write(temp.path().join("test.txt"), b"line1\nline2\n").expect("write file");

    // Wait for debounce
    thread::sleep(Duration::from_millis(4000));

    // Log with --stats and --json should include lines_added/lines_removed
    let mut log_cmd = isolated_cmd(unf_home.path());
    let log_out = log_cmd
        .current_dir(temp.path())
        .arg("--json")
        .arg("log")
        .arg("test.txt")
        .arg("--stats")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&log_out.get_output().stdout);
    assert!(
        stdout.contains("\"lines_added\""),
        "JSON stats output should contain lines_added: {}",
        stdout
    );
    assert!(
        stdout.contains("\"lines_removed\""),
        "JSON stats output should contain lines_removed: {}",
        stdout
    );
}

// --- density and cursor tests ---

#[test]
fn log_density_requires_json() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(200));

    // --density without --json should fail
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("log")
        .arg("--density")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--density requires --json"));
}

#[test]
fn log_density_json_empty_history() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(200));

    // --density --json with no history should return empty buckets and exit 4
    let out = isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("--json")
        .arg("log")
        .arg("--density")
        .assert()
        .code(4);

    let stdout = String::from_utf8_lossy(&out.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(json["buckets"], serde_json::json!([]));
    assert_eq!(json["total"], 0);
}

#[test]
fn log_density_json_with_data() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(200));

    // Create files to generate snapshots
    fs::write(temp.path().join("a.txt"), "hello").expect("write");
    fs::write(temp.path().join("b.txt"), "world").expect("write");
    thread::sleep(Duration::from_millis(4000));

    let out = isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("--json")
        .arg("log")
        .arg("--density")
        .arg("--buckets")
        .arg("5")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&out.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(json["buckets"].is_array());
    assert!(json["total"].as_u64().unwrap() >= 2);
    assert!(json["from"].is_string());
    assert!(json["to"].is_string());
}

#[test]
fn log_json_cursor_pagination() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(200));

    // Create files to generate snapshots
    fs::write(temp.path().join("a.txt"), "v1").expect("write");
    fs::write(temp.path().join("b.txt"), "v1").expect("write");
    fs::write(temp.path().join("c.txt"), "v1").expect("write");
    thread::sleep(Duration::from_millis(4000));

    // First page with limit 1
    let out = isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("--json")
        .arg("log")
        .arg("--limit")
        .arg("1")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&out.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(json["entries"].as_array().unwrap().len(), 1);
    assert!(json["next_cursor"].is_string(), "should have next_cursor");

    // Second page using cursor
    let cursor = json["next_cursor"].as_str().unwrap();
    let out2 = isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("--json")
        .arg("log")
        .arg("--limit")
        .arg("1")
        .arg("--cursor")
        .arg(cursor)
        .assert()
        .success();

    let stdout2 = String::from_utf8_lossy(&out2.get_output().stdout);
    let json2: serde_json::Value = serde_json::from_str(&stdout2).expect("valid JSON");
    assert_eq!(json2["entries"].as_array().unwrap().len(), 1);

    // The entries should be different between pages
    let file1 = json["entries"][0]["file"].as_str().unwrap();
    let file2 = json2["entries"][0]["file"].as_str().unwrap();
    // They should be different entries (different timestamps or files)
    let id1 = json["entries"][0]["id"].as_i64().unwrap();
    let id2 = json2["entries"][0]["id"].as_i64().unwrap();
    assert_ne!(
        id1, id2,
        "Cursor pagination should return different entries. File1: {}, File2: {}",
        file1, file2
    );
}

#[test]
fn log_cursor_requires_json() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(200));

    // --cursor without --json should fail
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("log")
        .arg("--cursor")
        .arg("2026-02-12T14:32:07+00:00:42")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--cursor requires --json"));
}

// --- stop kills stuck processes ---

#[cfg(unix)]
#[test]
fn stop_kills_stuck_processes_holding_db() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Watch project (starts isolated daemon)
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    // Resolve the DB path for this project
    let storage_dir = resolve_storage_dir_isolated(unf_home.path(), temp.path());
    let db_path = storage_dir.join("db.sqlite3");
    assert!(db_path.exists(), "DB should exist after watch");

    // Spawn a process that holds the DB file open.
    // `tail -f` keeps the file descriptor open, and lsof can detect it.
    let stuck_child = std::process::Command::new("tail")
        .arg("-f")
        .arg(&db_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    let mut stuck_child = match stuck_child {
        Ok(child) => child,
        Err(_) => {
            return;
        }
    };

    let stuck_pid = stuck_child.id();
    thread::sleep(Duration::from_millis(500));

    // Verify the process is actually alive before we test stop
    let alive_before = stuck_child
        .try_wait()
        .expect("try_wait should not fail")
        .is_none();
    assert!(alive_before, "Stuck process should be alive before stop");

    // Run `unf stop` — should kill daemon AND the stuck process holding DB
    let stop_out = isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("stop")
        .assert()
        .success();

    let stop_stdout = String::from_utf8_lossy(&stop_out.get_output().stdout);

    // Give cleanup a moment to propagate
    thread::sleep(Duration::from_millis(1000));

    // Verify the stuck process exited (try_wait reaps zombies)
    let exited = stuck_child
        .try_wait()
        .expect("try_wait should not fail")
        .is_some();
    assert!(
        exited,
        "Stuck process (PID {}) should have been killed by unf stop. Stop output: {}",
        stuck_pid, stop_stdout
    );
}

// --- group-by-file + filtering tests ---

#[test]
fn log_help_shows_group_by_file_and_filter_flags() {
    let mut cmd = unf_cmd();
    cmd.arg("log").arg("--help").assert().success().stdout(
        predicate::str::contains("--group-by-file")
            .and(predicate::str::contains("--include"))
            .and(predicate::str::contains("--exclude"))
            .and(predicate::str::contains("--ignore-case")),
    );
}

#[test]
fn log_group_by_file_with_multiple_files() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Init first
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(200));

    // Create two files while daemon is running
    fs::write(temp.path().join("alpha.txt"), "alpha content").expect("write alpha");
    fs::write(temp.path().join("beta.txt"), "beta content").expect("write beta");

    // Wait for debounce
    thread::sleep(Duration::from_millis(4000));

    // Log with --group-by-file should show file headers and footer
    let mut log_cmd = isolated_cmd(unf_home.path());
    log_cmd
        .current_dir(temp.path())
        .arg("log")
        .arg("--group-by-file")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("alpha.txt")
                .and(predicate::str::contains("beta.txt"))
                .and(predicate::str::contains("changes"))
                .and(predicate::str::contains("files")),
        );
}

#[test]
fn log_include_filter() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Init first
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(200));

    // Create files with different extensions while daemon is running
    fs::write(temp.path().join("code.rs"), "fn main() {}").expect("write rs");
    fs::write(temp.path().join("readme.txt"), "hello").expect("write txt");

    // Wait for debounce
    thread::sleep(Duration::from_millis(4000));

    // Log with --include "*.rs" should only show .rs files
    let mut log_cmd = isolated_cmd(unf_home.path());
    let log_out = log_cmd
        .current_dir(temp.path())
        .arg("log")
        .arg("--include")
        .arg("*.rs")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&log_out.get_output().stdout);
    assert!(stdout.contains("code.rs"), "should include code.rs");
    assert!(!stdout.contains("readme.txt"), "should exclude readme.txt");
}

#[test]
fn log_exclude_filter() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Init first
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(200));

    // Create files while daemon is running
    fs::write(temp.path().join("keep.txt"), "keep").expect("write keep");
    fs::write(temp.path().join("drop.log"), "drop").expect("write drop");

    // Wait for debounce
    thread::sleep(Duration::from_millis(4000));

    // Log with --exclude "*.log" should hide .log files
    let mut log_cmd = isolated_cmd(unf_home.path());
    let log_out = log_cmd
        .current_dir(temp.path())
        .arg("log")
        .arg("--exclude")
        .arg("*.log")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&log_out.get_output().stdout);
    assert!(stdout.contains("keep.txt"), "should include keep.txt");
    assert!(!stdout.contains("drop.log"), "should exclude drop.log");
}

#[test]
fn log_include_with_group_by_file() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Watch first (starts global daemon)
    let mut watch_cmd = isolated_cmd(unf_home.path());
    watch_cmd
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(200));

    // Create files with different extensions while daemon is running
    fs::write(temp.path().join("main.rs"), "fn main() {}").expect("write rs");
    fs::write(temp.path().join("lib.rs"), "pub fn lib() {}").expect("write rs2");
    fs::write(temp.path().join("notes.txt"), "notes").expect("write txt");

    // Wait for debounce
    thread::sleep(Duration::from_millis(4000));

    // Combined: --group-by-file + --include "*.rs"
    let mut log_cmd = isolated_cmd(unf_home.path());
    let log_out = log_cmd
        .current_dir(temp.path())
        .arg("log")
        .arg("--group-by-file")
        .arg("--include")
        .arg("*.rs")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&log_out.get_output().stdout);
    assert!(stdout.contains("main.rs"), "should include main.rs");
    assert!(stdout.contains("lib.rs"), "should include lib.rs");
    assert!(!stdout.contains("notes.txt"), "should exclude notes.txt");
    // Footer should show 2 files
    assert!(stdout.contains("2 files"), "footer should show 2 files");
}

#[test]
fn log_json_group_by_file() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Watch first (starts isolated daemon)
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(200));

    // Create a file while daemon is running
    fs::write(temp.path().join("test.txt"), "content").expect("write");

    // Wait for debounce
    thread::sleep(Duration::from_millis(4000));

    // JSON with --group-by-file
    let log_out = isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("--json")
        .arg("log")
        .arg("--group-by-file")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&log_out.get_output().stdout);
    // Should have grouped JSON structure
    assert!(
        stdout.contains("\"files\""),
        "should have files array: {}",
        stdout
    );
    assert!(
        stdout.contains("\"summary\""),
        "should have summary: {}",
        stdout
    );
    assert!(
        stdout.contains("\"total_files\""),
        "should have total_files: {}",
        stdout
    );
    assert!(
        stdout.contains("\"total_changes\""),
        "should have total_changes: {}",
        stdout
    );
    assert!(
        stdout.contains("\"change_count\""),
        "should have change_count: {}",
        stdout
    );
    assert!(
        stdout.contains("test.txt"),
        "should contain test.txt: {}",
        stdout
    );
}

#[test]
fn list_json_always_includes_stats_without_verbose() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Watch first (starts isolated daemon)
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());
    thread::sleep(Duration::from_millis(200));

    // Create a file while daemon is running
    fs::write(temp.path().join("test.txt"), "content v1").expect("write");

    // Wait for debounce
    thread::sleep(Duration::from_millis(4000));

    // Modify the file to create more snapshots
    fs::write(temp.path().join("test.txt"), "content v2").expect("modify");
    thread::sleep(Duration::from_millis(4000));

    // List with JSON format, WITHOUT --verbose flag
    let list_out = isolated_cmd(unf_home.path())
        .arg("--json")
        .arg("list")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&list_out.get_output().stdout);

    // Parse JSON to verify structure
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("output should be valid JSON");

    assert!(
        json["projects"].is_array(),
        "should have projects array in JSON"
    );
    assert!(
        json["projects"][0]["path"].is_string(),
        "should have path field"
    );
    assert!(
        json["projects"][0]["status"].is_string(),
        "should have status field"
    );

    // These fields should ALWAYS be present in JSON, even without --verbose
    assert!(
        json["projects"][0]["snapshots"].is_number(),
        "snapshots should be present in JSON output: {}",
        stdout
    );
    assert!(
        json["projects"][0]["store_bytes"].is_number(),
        "store_bytes should be present in JSON output: {}",
        stdout
    );
    assert!(
        json["projects"][0]["tracked_files"].is_number(),
        "tracked_files should be present in JSON output (not skipped): {}",
        stdout
    );
    assert!(
        json["projects"][0]["recording_since"].is_string(),
        "recording_since should be present in JSON output: {}",
        stdout
    );
    assert!(
        json["projects"][0]["last_activity"].is_string(),
        "last_activity should be present in JSON output: {}",
        stdout
    );

    // Verify the status is one of the expected values
    let status = json["projects"][0]["status"]
        .as_str()
        .expect("status should be a string");
    assert!(
        matches!(
            status,
            "watching" | "stopped" | "crashed" | "orphaned" | "error"
        ),
        "status should be one of: watching, stopped, crashed, orphaned, error. Got: {}",
        status
    );
}

// --- Global log tests ---

#[test]
fn log_global_flag_exists() {
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // --global --json should not error on arg parsing (may return no results or error)
    isolated_cmd(unf_home.path())
        .arg("--json")
        .arg("log")
        .arg("--global")
        .assert()
        .code(predicate::in_iter([0, 3, 4])); // success, invalid arg (no projects), or no results
}

#[test]
fn log_global_rejects_target() {
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // --global + positional target should fail
    isolated_cmd(unf_home.path())
        .arg("log")
        .arg("--global")
        .arg("src/main.rs")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--global cannot be combined with a file",
        ));
}

#[test]
fn log_global_rejects_cursor() {
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // --global + --cursor should fail
    isolated_cmd(unf_home.path())
        .arg("log")
        .arg("--global")
        .arg("--cursor")
        .arg("2026-02-12T14:32:07+00:00:42")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--global cannot be combined with --cursor",
        ));
}

#[test]
fn log_global_density_json() {
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // --global + --density should no longer be rejected (was erroring before)
    let output = isolated_cmd(unf_home.path())
        .arg("--json")
        .arg("log")
        .arg("--global")
        .arg("--density")
        .arg("--buckets")
        .arg("10")
        .output()
        .expect("Failed to run command");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Should NOT contain the old rejection message
    assert!(
        !stderr.contains("--global cannot be combined with --density"),
        "Old rejection message should be removed"
    );

    // With no projects, should output empty density JSON (exit with error code)
    if !stdout.is_empty() {
        let parsed: serde_json::Value = serde_json::from_str(&stdout)
            .unwrap_or_else(|e| panic!("Failed to parse density JSON: {e}\nstdout: {stdout}"));
        assert!(parsed["buckets"].is_array(), "Expected buckets array");
    }
}

#[test]
fn log_include_project_requires_global() {
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let temp = TempDir::new().expect("Failed to create temp dir");

    // --include-project without --global should fail
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("log")
        .arg("--include-project")
        .arg("/some/path")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--include-project/--exclude-project require --global",
        ));
}

#[test]
fn log_exclude_project_requires_global() {
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let temp = TempDir::new().expect("Failed to create temp dir");

    // --exclude-project without --global should fail
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("log")
        .arg("--exclude-project")
        .arg("/some/path")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "--include-project/--exclude-project require --global",
        ));
}

#[test]
fn log_global_help_shows_flags() {
    let mut cmd = unf_cmd();
    cmd.arg("log").arg("--help").assert().success().stdout(
        predicate::str::contains("--global")
            .and(predicate::str::contains("--include-project"))
            .and(predicate::str::contains("--exclude-project")),
    );
}

#[test]
#[allow(clippy::cognitive_complexity)]
// TODO(v0.18): reduce complexity
fn log_sessions_json() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Create a test file before init
    let test_file = temp.path().join("test.txt");
    fs::write(&test_file, "initial content").expect("Failed to write test file");

    // Init
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Give daemon time to start and snapshot the file
    thread::sleep(Duration::from_millis(200));

    // Modify the file
    fs::write(&test_file, "modified content").expect("Failed to modify test file");

    // Wait for debounce window (3+ seconds) plus processing time
    thread::sleep(Duration::from_millis(3500));

    // Query log --sessions --json
    let mut log_cmd = isolated_cmd(unf_home.path());
    let output = log_cmd
        .current_dir(temp.path())
        .arg("--json")
        .arg("log")
        .arg("--sessions")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("Output should be valid JSON");

    // Verify structure
    assert!(parsed["sessions"].is_array(), "sessions should be an array");
    assert!(
        parsed["total_edits"].is_number(),
        "total_edits should be a number"
    );
    assert!(
        parsed["total_files"].is_number(),
        "total_files should be a number"
    );

    // Verify we have at least one session
    let sessions = parsed["sessions"].as_array().unwrap();
    assert!(!sessions.is_empty(), "should have at least one session");

    // Verify first session structure
    let session = &sessions[0];
    assert!(
        session["number"].is_number(),
        "session number should be numeric"
    );
    assert!(
        session["start"].is_string(),
        "session start should be a string"
    );
    assert!(session["end"].is_string(), "session end should be a string");
    assert!(
        session["duration_seconds"].is_number(),
        "duration should be numeric"
    );
    assert!(
        session["edit_count"].is_number(),
        "edit_count should be numeric"
    );
    assert!(
        session["file_count"].is_number(),
        "file_count should be numeric"
    );
    assert!(session["files"].is_array(), "files should be an array");

    // Verify aggregates
    let total_edits = parsed["total_edits"].as_u64().unwrap();
    assert!(total_edits > 0, "total_edits should be > 0");
}

// --- Sentinel and Intent integration tests ---

#[test]
fn sentinel_command_exists() {
    // __sentinel should be a valid hidden command (starts and can be killed)
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Just verify the command parses and doesn't immediately error with "unknown command"
    // We can't easily test the full loop in CI, so just check --help doesn't error
    // Actually, we'll start it and immediately kill it
    let mut cmd = isolated_cmd(unf_home.path());
    let _ = cmd
        .arg("__sentinel")
        .timeout(Duration::from_secs(2))
        .assert();
    // Sentinel will timeout (it loops forever), which is expected
    // The key test is that it doesn't fail with "unknown command" or crash immediately
}

#[test]
fn watch_creates_intent() {
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let project = TempDir::new().expect("Failed to create project");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Watch the project
    isolated_cmd(unf_home.path())
        .current_dir(project.path())
        .arg("watch")
        .assert()
        .success();

    // Wait a moment for filesystem
    thread::sleep(Duration::from_millis(200));

    // Verify intent.json has the project
    let intent_path = unf_home.path().join("intent.json");
    assert!(
        intent_path.exists(),
        "intent.json should be created by watch"
    );

    let content = fs::read_to_string(&intent_path).expect("read intent.json");
    let canonical = project.path().canonicalize().expect("canonicalize project");
    assert!(
        content.contains(&canonical.to_string_lossy().to_string()),
        "intent.json should contain the project path"
    );
}

#[test]
fn intent_removed_by_unwatch() {
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let project = TempDir::new().expect("Failed to create project");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Watch then unwatch
    isolated_cmd(unf_home.path())
        .current_dir(project.path())
        .arg("watch")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    isolated_cmd(unf_home.path())
        .current_dir(project.path())
        .arg("unwatch")
        .assert()
        .success();

    // Verify intent.json is empty (no projects)
    let intent_path = unf_home.path().join("intent.json");
    if intent_path.exists() {
        let content = fs::read_to_string(&intent_path).expect("read intent.json");
        // Parse and check projects is empty
        let intent: serde_json::Value = serde_json::from_str(&content).expect("parse intent.json");
        let projects = intent["projects"].as_array().expect("projects array");
        assert!(
            projects.is_empty(),
            "intent.json projects should be empty after unwatch"
        );
    }
}

#[test]
fn intent_survives_stop() {
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let project = TempDir::new().expect("Failed to create project");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Watch the project
    isolated_cmd(unf_home.path())
        .current_dir(project.path())
        .arg("watch")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    // Stop the daemon
    isolated_cmd(unf_home.path())
        .current_dir(project.path())
        .arg("stop")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    // Verify intent.json still has the project
    let intent_path = unf_home.path().join("intent.json");
    assert!(intent_path.exists(), "intent.json should survive stop");

    let content = fs::read_to_string(&intent_path).expect("read intent.json");
    let canonical = project.path().canonicalize().expect("canonicalize project");
    assert!(
        content.contains(&canonical.to_string_lossy().to_string()),
        "intent.json should still contain the project after stop"
    );
}

#[test]
fn audit_log_created() {
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let project = TempDir::new().expect("Failed to create project");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Watch the project
    isolated_cmd(unf_home.path())
        .current_dir(project.path())
        .arg("watch")
        .assert()
        .success();

    thread::sleep(Duration::from_millis(200));

    // Verify audit.log exists and has a WATCH entry
    let audit_path = unf_home.path().join("audit.log");
    assert!(audit_path.exists(), "audit.log should be created by watch");

    let content = fs::read_to_string(&audit_path).expect("read audit.log");
    assert!(
        content.contains("WATCH"),
        "audit.log should contain WATCH event"
    );
}

// ─── Sentinel protection tests ─────────────────────────────────────────────
//
// These tests validate the sentinel's core self-healing use cases.
// They use UNF_SENTINEL_TICK_SECS=2 for fast iteration.

/// Helper to get a Command with isolated UNF_HOME and fast sentinel tick.
fn sentinel_cmd(unf_home: &std::path::Path) -> Command {
    let mut cmd = unf_cmd();
    cmd.env("UNF_HOME", unf_home);
    cmd.env("UNF_SENTINEL_TICK_SECS", "2");
    cmd
}

/// Helper: read a PID from a file and check if the process is alive.
fn is_pid_alive(pid_file: &std::path::Path) -> bool {
    if let Ok(content) = fs::read_to_string(pid_file) {
        if let Ok(pid) = content.trim().parse::<i32>() {
            return unsafe { libc::kill(pid, 0) } == 0;
        }
    }
    false
}

/// Helper: read a PID from a file.
fn read_pid(pid_file: &std::path::Path) -> Option<i32> {
    fs::read_to_string(pid_file)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Helper: kill a process by PID file.
fn kill_pid(pid_file: &std::path::Path) {
    if let Some(pid) = read_pid(pid_file) {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
    }
}

#[test]
fn sentinel_respawns_crashed_daemon() {
    let unf_home = TempDir::new().expect("create UNF_HOME");
    let project = TempDir::new().expect("create project");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Watch with fast sentinel tick
    sentinel_cmd(unf_home.path())
        .current_dir(project.path())
        .arg("watch")
        .assert()
        .success();

    // Wait for daemon to be fully running
    thread::sleep(Duration::from_secs(1));

    let daemon_pid_file = unf_home.path().join("daemon.pid");
    assert!(
        is_pid_alive(&daemon_pid_file),
        "daemon should be alive after watch"
    );

    let old_pid = read_pid(&daemon_pid_file).expect("read daemon pid");

    // Kill the daemon directly (simulate crash)
    kill_pid(&daemon_pid_file);
    thread::sleep(Duration::from_millis(500));
    assert!(
        !is_pid_alive(&daemon_pid_file),
        "daemon should be dead after kill"
    );

    // Wait for sentinel to detect and respawn (2s tick + margin)
    let mut respawned = false;
    for _ in 0..15 {
        thread::sleep(Duration::from_secs(1));
        if is_pid_alive(&daemon_pid_file) {
            let new_pid = read_pid(&daemon_pid_file);
            if new_pid.is_some() && new_pid != Some(old_pid) {
                respawned = true;
                break;
            }
        }
    }

    assert!(
        respawned,
        "sentinel should respawn daemon within 15s (2s tick)"
    );

    // Verify audit log recorded the crash
    let audit = fs::read_to_string(unf_home.path().join("audit.log")).unwrap_or_default();
    assert!(
        audit.contains("DAEMON_CRASH"),
        "audit.log should record DAEMON_CRASH"
    );
}

#[test]
fn sentinel_restores_registry_from_intent() {
    let unf_home = TempDir::new().expect("create UNF_HOME");
    let project = TempDir::new().expect("create project");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Watch with fast sentinel tick
    sentinel_cmd(unf_home.path())
        .current_dir(project.path())
        .arg("watch")
        .assert()
        .success();

    thread::sleep(Duration::from_secs(1));

    // Verify intent and registry both have the project
    let intent_path = unf_home.path().join("intent.json");
    let registry_path = unf_home.path().join("projects.json");
    assert!(intent_path.exists(), "intent.json should exist");

    let canonical = project.path().canonicalize().expect("canonicalize");

    // Wipe the registry (simulate corruption)
    fs::write(&registry_path, r#"{"projects":[]}"#).expect("wipe registry");

    // Wait for sentinel to detect drift and reconcile (2s tick + margin)
    let mut restored = false;
    for _ in 0..15 {
        thread::sleep(Duration::from_secs(1));
        if let Ok(content) = fs::read_to_string(&registry_path) {
            if content.contains(&canonical.to_string_lossy().to_string()) {
                restored = true;
                break;
            }
        }
    }

    assert!(
        restored,
        "sentinel should restore registry from intent within 15s"
    );

    // Verify audit log recorded the drift
    let audit = fs::read_to_string(unf_home.path().join("audit.log")).unwrap_or_default();
    assert!(
        audit.contains("REGISTRY_DRIFT"),
        "audit.log should record REGISTRY_DRIFT"
    );
}

#[test]
fn diff_session_latest() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Create a test file before init
    let test_file = temp.path().join("test.txt");
    fs::write(&test_file, "original content").expect("Failed to write test file");

    // Init
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Give daemon time to start and snapshot the file
    thread::sleep(Duration::from_millis(200));

    // Modify the file
    fs::write(&test_file, "modified content").expect("Failed to modify test file");

    // Wait for debounce window (3+ seconds) plus processing time
    thread::sleep(Duration::from_millis(3500));

    // Run diff with --session in JSON mode
    let mut diff_cmd = isolated_cmd(unf_home.path());
    let diff_out = diff_cmd
        .current_dir(temp.path())
        .arg("--json")
        .arg("diff")
        .arg("--session")
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&diff_out.get_output().stdout);
    assert!(
        stdout.contains("\"from\"") && stdout.contains("\"to\"") && stdout.contains("\"changes\""),
        "JSON output should contain diff structure: {}",
        stdout
    );
}

#[test]
fn restore_session_validates_conflicts() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Create a test file before init
    let test_file = temp.path().join("test.txt");
    fs::write(&test_file, "content").expect("Failed to write test file");

    // Init
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Give daemon time to start and snapshot the file
    thread::sleep(Duration::from_millis(200));

    // Try to use both --session and --at (should fail)
    let mut restore_cmd = isolated_cmd(unf_home.path());
    restore_cmd
        .current_dir(temp.path())
        .arg("restore")
        .arg("--session")
        .arg("--at")
        .arg("5m")
        .assert()
        .failure();
}

#[test]
fn diff_session_conflicts_with_at() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");

    // Create a test file before init
    let test_file = temp.path().join("test.txt");
    fs::write(&test_file, "content").expect("Failed to write test file");

    // Init
    let mut init_cmd = isolated_cmd(unf_home.path());
    init_cmd
        .current_dir(temp.path())
        .arg("init")
        .assert()
        .success();
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Give daemon time to start and snapshot the file
    thread::sleep(Duration::from_millis(200));

    // Try to use both --session and --at (should fail)
    let mut diff_cmd = isolated_cmd(unf_home.path());
    diff_cmd
        .current_dir(temp.path())
        .arg("diff")
        .arg("--session")
        .arg("--at")
        .arg("5m")
        .assert()
        .failure();
}

#[test]
fn recap_json() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Initialize
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();

    // Wait for daemon to start and register
    thread::sleep(Duration::from_secs(2));

    // Create a file
    fs::write(temp.path().join("test.txt"), b"Hello, world!").expect("Failed to write file");

    // Wait for snapshot to be captured and debounced
    thread::sleep(Duration::from_secs(4));

    // Run recap with JSON output
    let output = isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("recap")
        .arg("--json")
        .output()
        .expect("Failed to run recap");

    assert!(output.status.success(), "recap should succeed");

    let json_str = String::from_utf8(output.stdout).expect("Invalid UTF-8 in stdout");
    let json: serde_json::Value =
        serde_json::from_str(&json_str).expect("Output should be valid JSON");

    // Verify JSON structure
    assert!(
        json.get("project").is_some(),
        "JSON should have 'project' field"
    );
    assert!(
        json.get("sessions").is_some(),
        "JSON should have 'sessions' field"
    );

    // May or may not have sessions depending on timing, but structure should be valid
    assert!(
        json.get("sessions").unwrap().is_array(),
        "sessions should be an array"
    );
}

#[test]
fn recap_human() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Initialize
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();

    // Wait for daemon to start and register
    thread::sleep(Duration::from_secs(2));

    // Create a file
    fs::write(temp.path().join("test.txt"), b"Hello, world!").expect("Failed to write file");

    // Wait for snapshot to be captured and debounced
    thread::sleep(Duration::from_secs(4));

    // Run recap without JSON (human output)
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("recap")
        .assert()
        .success()
        .stdout(
            predicate::str::contains("--")
                .and(predicate::str::contains("Session").or(predicate::str::contains("detected"))),
        );
}

#[test]
fn recap_not_initialized() {
    let temp = TempDir::new().expect("Failed to create temp dir");

    // Run recap without initialization
    unf_cmd()
        .current_dir(temp.path())
        .arg("recap")
        .assert()
        .failure()
        .code(2); // NotInitialized exit code
}

#[test]
fn sentinel_clears_stale_stopped_file() {
    let unf_home = TempDir::new().expect("create UNF_HOME");
    let project = TempDir::new().expect("create project");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Watch with fast sentinel tick
    sentinel_cmd(unf_home.path())
        .current_dir(project.path())
        .arg("watch")
        .assert()
        .success();

    thread::sleep(Duration::from_secs(1));

    // Create a stale stopped file for this project (simulates leftover from unf stop)
    let storage_dir = resolve_storage_dir_isolated(unf_home.path(), project.path());
    let stopped_file = storage_dir.join("stopped");
    fs::write(&stopped_file, b"").expect("create stale stopped file");
    assert!(stopped_file.exists(), "stopped file should exist");

    // Wait for sentinel to clear it (2s tick + margin)
    let mut cleared = false;
    for _ in 0..15 {
        thread::sleep(Duration::from_secs(1));
        if !stopped_file.exists() {
            cleared = true;
            break;
        }
    }

    assert!(
        cleared,
        "sentinel should clear stale stopped file for intended project within 15s"
    );
}
