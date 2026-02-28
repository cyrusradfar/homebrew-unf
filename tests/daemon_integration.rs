//! Integration tests for the single daemon architecture.
//!
//! Tests the global daemon model where:
//! - A single daemon process manages all watched projects
//! - `unf watch` registers a project and starts/signals the daemon
//! - `unf unwatch` deregisters a project and signals daemon reload
//! - `unf stop` kills the global daemon
//! - `unf restart` stops + starts the daemon with a new PID
//!
//! Each test uses an isolated UNF_HOME so tests can run in parallel without
//! interfering with each other or the user's real daemon.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use tempfile::TempDir;

mod common;
use common::IsolatedDaemonGuard;

// ============================================================================
// Helper Functions
// ============================================================================

/// Get a Command instance with an isolated UNF_HOME and fast sentinel tick.
/// Wraps the common isolated_cmd to add the test-specific sentinel tick configuration.
fn isolated_cmd(unf_home: &Path) -> Command {
    let mut cmd = common::isolated_cmd(unf_home);
    cmd.env("UNF_SENTINEL_TICK_SECS", "2");
    cmd
}

/// Returns the PID file path for an isolated UNF_HOME.
fn pid_path(unf_home: &Path) -> PathBuf {
    unf_home.join("daemon.pid")
}

/// Returns the registry path for an isolated UNF_HOME.
fn registry_path(unf_home: &Path) -> PathBuf {
    unf_home.join("projects.json")
}

/// Read the registry and return project paths as a Vec<PathBuf>.
fn read_registry_projects(unf_home: &Path) -> Vec<PathBuf> {
    let path = registry_path(unf_home);
    if !path.exists() {
        return vec![];
    }
    let content = fs::read_to_string(&path).unwrap_or_default();
    let value: serde_json::Value =
        serde_json::from_str(&content).unwrap_or(serde_json::Value::Null);
    value
        .get("projects")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|e| e.get("path").and_then(|p| p.as_str()).map(PathBuf::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Check if a process with the given PID is alive.
fn is_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Read a PID from a file.
fn read_pid(path: &Path) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse::<u32>().ok()
}

// ============================================================================
// Tests
// ============================================================================

/// Test 1: Watch registers project and starts daemon.
#[test]
fn watch_starts_daemon_and_registers_project() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Run `unf watch` in temp directory
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();

    // Allow time for daemon to start and register
    thread::sleep(Duration::from_millis(500));

    // Verify PID file exists
    let pp = pid_path(unf_home.path());
    assert!(pp.exists(), "PID file should exist at {}", pp.display());

    // Verify daemon is running
    let pid = read_pid(&pp).expect("Should read PID from file");
    assert!(
        is_alive(pid),
        "Daemon process (PID {}) should be alive",
        pid
    );

    // Verify project is in registry
    let projects = read_registry_projects(unf_home.path());
    let canonical = temp
        .path()
        .canonicalize()
        .expect("Temp path should be canonicalizable");
    assert!(
        projects.contains(&canonical),
        "Project {} should be registered",
        canonical.display()
    );
}

/// Test 2: Watching two projects results in single daemon.
#[test]
fn watch_two_projects_single_daemon() {
    let temp_a = TempDir::new().expect("Failed to create temp dir A");
    let temp_b = TempDir::new().expect("Failed to create temp dir B");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Watch project A
    isolated_cmd(unf_home.path())
        .current_dir(temp_a.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    let pid1 = read_pid(&pid_path(unf_home.path())).expect("Should read PID after watching A");
    assert!(is_alive(pid1), "Daemon should be alive after first watch");

    // Watch project B (should reuse same daemon)
    isolated_cmd(unf_home.path())
        .current_dir(temp_b.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    let pid2 = read_pid(&pid_path(unf_home.path())).expect("Should read PID after watching B");

    // Same daemon PID
    assert_eq!(
        pid1, pid2,
        "Should reuse same daemon process for multiple projects"
    );

    // Both projects registered
    let projects = read_registry_projects(unf_home.path());
    let canonical_a = temp_a
        .path()
        .canonicalize()
        .expect("Temp A should be canonicalizable");
    let canonical_b = temp_b
        .path()
        .canonicalize()
        .expect("Temp B should be canonicalizable");
    assert!(
        projects.contains(&canonical_a),
        "Project A should be registered"
    );
    assert!(
        projects.contains(&canonical_b),
        "Project B should be registered"
    );
}

/// Test 3: Unwatch one project, daemon continues with other project.
#[test]
fn unwatch_one_project_daemon_continues() {
    let temp_a = TempDir::new().expect("Failed to create temp dir A");
    let temp_b = TempDir::new().expect("Failed to create temp dir B");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Watch both projects
    isolated_cmd(unf_home.path())
        .current_dir(temp_a.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(300));

    isolated_cmd(unf_home.path())
        .current_dir(temp_b.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    let pid = read_pid(&pid_path(unf_home.path())).expect("Should read PID");

    // Unwatch A
    isolated_cmd(unf_home.path())
        .current_dir(temp_a.path())
        .arg("unwatch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(300));

    // Daemon still running with same PID
    assert!(
        is_alive(pid),
        "Daemon should still be alive after unwatching one project"
    );
    let new_pid = read_pid(&pid_path(unf_home.path()));
    assert_eq!(
        Some(pid),
        new_pid,
        "Daemon PID should not change when unwatching one project"
    );

    // Only B in registry
    let projects = read_registry_projects(unf_home.path());
    let canonical_a = temp_a
        .path()
        .canonicalize()
        .expect("Temp A should be canonicalizable");
    let canonical_b = temp_b
        .path()
        .canonicalize()
        .expect("Temp B should be canonicalizable");
    assert!(
        !projects.contains(&canonical_a),
        "Project A should be removed from registry"
    );
    assert!(
        projects.contains(&canonical_b),
        "Project B should remain in registry"
    );
}

/// Test 4: Stop kills daemon and removes PID file.
#[test]
fn stop_kills_daemon() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    let pp = pid_path(unf_home.path());
    let pid = read_pid(&pp).expect("Should read PID before stop");
    assert!(is_alive(pid), "Daemon should be alive before stop");

    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("stop")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(800));

    // Daemon should be dead
    assert!(
        !is_alive(pid),
        "Daemon process (PID {}) should be dead after stop",
        pid
    );

    // PID file should be removed
    assert!(!pp.exists(), "PID file should be removed after stop");
}

/// Test 5: Restart spawns new daemon with different PID.
#[test]
fn restart_gives_new_pid() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    let old_pid = read_pid(&pid_path(unf_home.path())).expect("Should read PID before restart");
    assert!(is_alive(old_pid), "Daemon should be alive before restart");

    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("restart")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    // Sentinel spawns daemon on first tick — wait for it to appear
    let pp = pid_path(unf_home.path());
    let mut new_pid = None;
    for _ in 0..40 {
        if let Some(pid) = read_pid(&pp) {
            if pid != old_pid && is_alive(pid) {
                new_pid = Some(pid);
                break;
            }
        }
        thread::sleep(Duration::from_millis(200));
    }
    let new_pid = new_pid.expect("Should read new PID after restart");

    // New PID should be different (different daemon process)
    assert_ne!(
        old_pid, new_pid,
        "Restart should spawn new daemon with different PID"
    );
    assert!(
        is_alive(new_pid),
        "New daemon (PID {}) should be alive",
        new_pid
    );
    // Old daemon should be dead
    assert!(
        !is_alive(old_pid),
        "Old daemon (PID {}) should be dead",
        old_pid
    );
}

/// Test 6: Status shows watching for watched project.
#[test]
fn status_shows_recording_for_watched_project() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(800));

    let output = isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("status")
        .output()
        .expect("status command should execute");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "status command should succeed");
    assert!(
        stdout.contains("Watching since")
            || stdout.contains("watching")
            || stdout.contains("Watching"),
        "Status output should indicate watching state: {}",
        stdout
    );
}

/// Test 7: Stop preserves registry — projects remain registered after stop.
#[test]
fn stop_preserves_registry() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Watch a project
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    let canonical = temp
        .path()
        .canonicalize()
        .expect("Temp path should be canonicalizable");

    // Verify project is registered
    let projects_before = read_registry_projects(unf_home.path());
    assert!(
        projects_before.contains(&canonical),
        "Project should be registered before stop"
    );

    // Stop the daemon
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("stop")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    // Registry should still contain the project
    let projects_after = read_registry_projects(unf_home.path());
    assert!(
        projects_after.contains(&canonical),
        "Project should still be registered after stop"
    );
}

/// Test 8: Restart after stop resumes projects — registry preserved through stop+restart cycle.
#[test]
fn restart_after_stop_resumes_projects() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Watch a project
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    let canonical = temp
        .path()
        .canonicalize()
        .expect("Temp path should be canonicalizable");

    // Stop the daemon
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("stop")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    // Restart the daemon
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("restart")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    // Sentinel spawns daemon on first tick — wait for it to appear
    let pp = pid_path(unf_home.path());
    let mut pid = None;
    for _ in 0..40 {
        if let Some(p) = read_pid(&pp) {
            if is_alive(p) {
                pid = Some(p);
                break;
            }
        }
        thread::sleep(Duration::from_millis(200));
    }
    let pid = pid.expect("Should read PID after restart");
    assert!(
        is_alive(pid),
        "Daemon (PID {}) should be alive after restart",
        pid
    );

    // Registry should still contain the project
    let projects = read_registry_projects(unf_home.path());
    assert!(
        projects.contains(&canonical),
        "Project should still be registered after stop+restart"
    );
}
