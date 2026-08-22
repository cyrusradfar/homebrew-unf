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

/// Returns true if `unf log <rel_path>` finds at least one snapshot.
///
/// `unf log` exits 0 when the target has history and exits 4 ("No history for
/// ...") when it does not (see `error.rs::ExitCode::NoResults`, exercised by
/// `log_nonexistent_file_shows_no_history` in integration.rs). Reading the
/// exit code is cheaper and more reliable than scraping stdout text.
fn is_recorded(unf_home: &Path, project_root: &Path, rel_path: &str) -> bool {
    isolated_cmd(unf_home)
        .current_dir(project_root)
        .arg("log")
        .arg(rel_path)
        .output()
        .expect("run log command")
        .status
        .success()
}

/// Poll for `rel_path` to show up in the log, rather than sleeping a fixed
/// interval that may or may not clear the 3-second debounce window plus
/// daemon processing time. Matches the polling style `restart_gives_new_pid`
/// already uses in this file (bounded retries, short sleep between).
fn wait_until_recorded(unf_home: &Path, project_root: &Path, rel_path: &str) {
    for _ in 0..40 {
        if is_recorded(unf_home, project_root, rel_path) {
            return;
        }
        thread::sleep(Duration::from_millis(250));
    }
    panic!("{} was not recorded within the poll timeout", rel_path);
}

/// Poll for `rel_path` to be recorded, REWRITING it between rounds.
///
/// `wait_until_recorded` writes once and then only polls, which cannot
/// recover from an event the daemon legitimately dropped. After a
/// SIGUSR1 the reload is asynchronous: a write that lands before the
/// Filter is rebuilt is rejected by the OLD filter, and nothing ever
/// re-offers it, because the file never changes again. Waiting longer
/// does not help — the event is already gone. Rewriting gives the
/// reloaded Filter a fresh event.
///
/// Each round polls for longer than the 3s debounce window before
/// rewriting. Rewriting faster than that would keep resetting the
/// debouncer's silence window and the batch would never flush at all.
fn wait_until_recorded_with_rewrite(unf_home: &Path, project_root: &Path, rel_path: &str) {
    for round in 0..5 {
        fs::write(
            project_root.join(rel_path),
            format!("// reload probe {}\n", round),
        )
        .unwrap_or_else(|e| panic!("write {}: {}", rel_path, e));

        for _ in 0..24 {
            thread::sleep(Duration::from_millis(250));
            if is_recorded(unf_home, project_root, rel_path) {
                return;
            }
        }
    }
    panic!(
        "{} was not recorded after 5 rewrite rounds; the daemon never picked up the reload",
        rel_path
    );
}

/// Sleep past the debounce window, with margin for daemon processing time.
///
/// Proving a file was NOT recorded needs a real wait, not an immediate
/// check — an immediate check would pass even for a file that WILL be
/// recorded a moment later, simply because the snapshot has not landed yet.
/// Waiting here turns that false pass into a real assertion.
fn wait_past_debounce() {
    thread::sleep(Duration::from_millis(4500));
}

/// Assert `rel_path` has no snapshot. Callers must call `wait_past_debounce`
/// (or otherwise guarantee steady state) before this, or a true result here
/// says nothing about whether the file was correctly filtered.
fn assert_not_recorded(unf_home: &Path, project_root: &Path, rel_path: &str) {
    assert!(
        !is_recorded(unf_home, project_root, rel_path),
        "{} should not have been recorded",
        rel_path
    );
}

/// Poll for a SIGUSR1-triggered registry reload to have actually rebuilt the
/// daemon's Filter back to the gitignore-respecting default, by writing
/// successive gitignored probe files until one is confirmed NOT recorded.
///
/// `unf watch` (without `--force-watch-gitignore`) returns as soon as it has
/// signaled the daemon; it does not wait for the daemon to finish
/// `sync_with_registry`'s `to_reload` path. A fixed sleep between "signal
/// sent" and "write the real test file" is a race: on a slow/loaded runner
/// the daemon can still be running the OLD (permissive) Filter when the real
/// file is written, so it gets recorded and an absence assertion fails
/// intermittently. This proves the new filter is live with a real,
/// self-verifying signal instead of guessing a delay — a bigger fixed sleep
/// is the same race with a wider window, not a fix.
///
/// Each probe costs one `wait_past_debounce` (the only way to safely observe
/// "not recorded"), so this is bounded and normally resolves on the first
/// probe: the reload is typically well under one debounce window.
fn wait_until_gitignore_reapplied(unf_home: &Path, project_root: &Path) {
    const MAX_PROBES: u32 = 10;
    for i in 0..MAX_PROBES {
        let rel_path = format!("probe_{}.log", i);
        fs::write(project_root.join(&rel_path), "probe\n").expect("write probe file");
        wait_past_debounce();
        if !is_recorded(unf_home, project_root, &rel_path) {
            return;
        }
    }
    panic!(
        "gitignore filter was not reapplied by the daemon within {} probes",
        MAX_PROBES
    );
}

/// Returns the intent-file path for an isolated UNF_HOME.
fn intent_path(unf_home: &Path) -> PathBuf {
    unf_home.join("intent.json")
}

/// Directly rewrites `unignored_dirs` for `project_root`'s entry in BOTH the
/// registry (`projects.json`) and the intent file (`intent.json`), bypassing
/// the CLI's `parse_unignore_dir` validator entirely.
///
/// Used only by the UD-10 `.git` defence-in-depth scenario: the ticket
/// requires proving the hard refusal inside `Filter::should_track`, not just
/// that the CLI parser rejects `--unignore-dir .git`. Both files must be
/// edited, not just the registry — UD-06's sentinel resync treats
/// `intent.json` as the source of truth and would otherwise revert a
/// registry-only edit on its next tick, which would make the test's
/// "not recorded" assertion pass for the wrong reason (reverted settings,
/// not the hard refusal).
fn hand_edit_unignored_dirs(unf_home: &Path, project_root: &Path, dirs: &[&str]) {
    let canonical = project_root
        .canonicalize()
        .expect("project root should be canonicalizable");

    for path in [registry_path(unf_home), intent_path(unf_home)] {
        let content =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
        let mut value: serde_json::Value = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
        let projects = value
            .get_mut("projects")
            .and_then(|p| p.as_array_mut())
            .unwrap_or_else(|| panic!("no \"projects\" array in {}", path.display()));
        let entry = projects
            .iter_mut()
            .find(|e| {
                e.get("path").and_then(|p| p.as_str()).map(PathBuf::from) == Some(canonical.clone())
            })
            .unwrap_or_else(|| {
                panic!("no entry for {} in {}", canonical.display(), path.display())
            });
        entry["unignored_dirs"] = serde_json::json!(dirs);
        let rewritten = serde_json::to_string_pretty(&value).expect("serialize edited JSON");
        fs::write(&path, rewritten).unwrap_or_else(|e| panic!("write {}: {}", path.display(), e));
    }
}

/// Poll for a SIGUSR1-triggered registry reload to have rebuilt the daemon's
/// Filter back to excluding `dir` again, by writing successive probe files
/// inside `project_root/dir` until one is confirmed NOT recorded.
///
/// Mirrors `wait_until_gitignore_reapplied` (see its comment for why a fixed
/// sleep between "signal sent" and "write the real test file" is a race),
/// but targets a subdirectory instead of the project root so it can confirm
/// a `--unignore-dir` list reset rather than a gitignore-flag reset.
fn wait_until_unignore_dir_reverted(unf_home: &Path, project_root: &Path, dir: &str) {
    const MAX_PROBES: u32 = 10;
    let probe_dir = project_root.join(dir);
    fs::create_dir_all(&probe_dir).expect("create probe dir");
    for i in 0..MAX_PROBES {
        let rel_path = format!("{}/revert_probe_{}.log", dir, i);
        fs::write(project_root.join(&rel_path), "probe\n").expect("write probe file");
        wait_past_debounce();
        if !is_recorded(unf_home, project_root, &rel_path) {
            return;
        }
    }
    panic!(
        "'{}' was not re-excluded by the daemon within {} probes",
        dir, MAX_PROBES
    );
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

/// Test 9: `--force-watch-gitignore` end-to-end (GI-08).
///
/// Walks all 5 scenarios from `docs/tickets/watch-gitignore-override.md` in
/// one function, because state carries between steps: step 2's no-restart
/// assertion depends on step 1's daemon still being the one running, and
/// steps 3-5 depend on the flag flips from steps 2 and 5.
#[test]
fn force_watch_gitignore_end_to_end() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    fs::write(temp.path().join(".gitignore"), "*.log\n").expect("write .gitignore");

    // --- Step 1: plain `unf watch` respects .gitignore ---
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    let pid_after_step1 =
        read_pid(&pid_path(unf_home.path())).expect("Should read PID after first watch");
    assert!(
        is_alive(pid_after_step1),
        "Daemon should be alive after first watch"
    );

    fs::write(temp.path().join("a.log"), "line one\n").expect("write a.log");
    wait_past_debounce();
    assert_not_recorded(unf_home.path(), temp.path(), "a.log");

    // --- Step 2: re-watch with --force-watch-gitignore, SAME daemon ---
    // This is the ticket's most important assertion: it proves the daemon's
    // `to_reload` path (GI-03) rebuilds the Filter on a flag change without
    // a restart. If the daemon restarted here, this test would prove nothing.
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .arg("--force-watch-gitignore")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(300));

    let pid_after_step2 = read_pid(&pid_path(unf_home.path()))
        .expect("Should read PID after re-watch with --force-watch-gitignore");
    assert_eq!(
        pid_after_step1, pid_after_step2,
        "re-watching with --force-watch-gitignore must reuse the running \
         daemon (same PID), not restart it"
    );
    assert!(
        is_alive(pid_after_step2),
        "Daemon should still be alive after re-watch"
    );

    fs::write(temp.path().join("b.log"), "line one\n").expect("write b.log");
    wait_until_recorded(unf_home.path(), temp.path(), "b.log");

    // --- Step 3: hidden dotfiles are tracked once the flag is on ---
    fs::write(temp.path().join(".env.local"), "SECRET=1\n").expect("write .env.local");
    wait_until_recorded(unf_home.path(), temp.path(), ".env.local");

    // --- Step 4: hardcoded dir exclusions still apply even with the flag on ---
    fs::create_dir_all(temp.path().join("node_modules")).expect("mkdir node_modules");
    fs::write(temp.path().join("node_modules/x.js"), "x\n").expect("write node_modules/x.js");
    fs::create_dir_all(temp.path().join("target")).expect("mkdir target");
    fs::write(temp.path().join("target/x"), "x\n").expect("write target/x");
    wait_past_debounce();
    assert_not_recorded(unf_home.path(), temp.path(), "node_modules/x.js");
    assert_not_recorded(unf_home.path(), temp.path(), "target/x");

    // --- Step 5: re-watch with plain `unf watch` turns the flag back off ---
    isolated_cmd(unf_home.path())
        .current_dir(temp.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(300));

    let pid_after_step5 =
        read_pid(&pid_path(unf_home.path())).expect("Should read PID after final re-watch");
    assert_eq!(
        pid_after_step2, pid_after_step5,
        "turning the flag back off must also reuse the running daemon"
    );

    // Confirm the daemon has actually applied the reload (not just been
    // signaled) before writing the real probe file. See
    // `wait_until_gitignore_reapplied` for why a fixed sleep here is racy.
    wait_until_gitignore_reapplied(unf_home.path(), temp.path());

    fs::write(temp.path().join("c.log"), "line one\n").expect("write c.log");
    wait_past_debounce();
    assert_not_recorded(unf_home.path(), temp.path(), "c.log");
}

/// Test 10: `--unignore-dir` end-to-end (UD-10).
///
/// Walks all 7 scenarios from `docs/tickets/watch-unignore-dirs.md` UD-10 in
/// one function sharing a single daemon, mirroring
/// `force_watch_gitignore_end_to_end`: scenario 2's no-restart assertion
/// depends on scenario 1's daemon still being the one running, and scenario
/// 7 depends on scenario 2's flag flip carrying forward. Scenarios 3 and 5
/// use their own project directories (B and C) added to the same daemon,
/// since they start from preconditions (an existing `.gitignore` entry, a
/// hand-edited registry) that would otherwise interfere with project A's
/// phases if reused.
#[test]
fn unignore_dir_end_to_end() {
    let temp_a = TempDir::new().expect("Failed to create temp dir A");
    let temp_b = TempDir::new().expect("Failed to create temp dir B");
    let temp_c = TempDir::new().expect("Failed to create temp dir C");
    let unf_home = TempDir::new().expect("Failed to create UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // ------------------------------------------------------------------
    // Scenario 1: target/notes.rs is NOT recorded by default.
    // ------------------------------------------------------------------
    // Excluded directories are created BEFORE the first `unf watch` in each
    // project, deliberately. On Linux, inotify registers one watch per
    // directory, so a file written into a directory that was created after
    // the recursive watch was established can be missed entirely while the
    // watch for it is still being added. macOS FSEvents is path-based and
    // does not have this window, which is why creating them late passed
    // locally and failed on the Linux CI runner. Creating them up front
    // also stops every "not recorded" assertion below from passing
    // vacuously because the watcher never saw the file at all.
    fs::create_dir_all(temp_a.path().join("target")).expect("mkdir target (A)");
    fs::create_dir_all(temp_a.path().join("node_modules")).expect("mkdir node_modules (A)");

    isolated_cmd(unf_home.path())
        .current_dir(temp_a.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    let pid_after_a1 =
        read_pid(&pid_path(unf_home.path())).expect("Should read PID after watching A");
    assert!(
        is_alive(pid_after_a1),
        "Daemon should be alive after watching A"
    );

    fs::write(temp_a.path().join("target/notes.rs"), "// notes\n").expect("write target/notes.rs");
    wait_past_debounce();
    assert_not_recorded(unf_home.path(), temp_a.path(), "target/notes.rs");

    // ------------------------------------------------------------------
    // Scenario 2: re-watch A with --unignore-dir target, no .gitignore
    // entry: recorded, with NO daemon restart. PID is asserted unchanged
    // BEFORE the recording check, per the ticket.
    // ------------------------------------------------------------------
    isolated_cmd(unf_home.path())
        .current_dir(temp_a.path())
        .arg("watch")
        .arg("--unignore-dir")
        .arg("target")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(300));

    let pid_after_a2 = read_pid(&pid_path(unf_home.path()))
        .expect("Should read PID after re-watch with --unignore-dir target");
    assert_eq!(
        pid_after_a1, pid_after_a2,
        "re-watching A with --unignore-dir target must reuse the running \
         daemon (same PID), not restart it"
    );

    fs::write(temp_a.path().join("target/tracked.rs"), "// tracked\n")
        .expect("write target/tracked.rs");
    wait_until_recorded(unf_home.path(), temp_a.path(), "target/tracked.rs");

    // ------------------------------------------------------------------
    // Scenario 4: node_modules/ stays excluded while target is un-ignored.
    // Scenario 6: a .rlib inside the now-un-ignored target/ is still
    // rejected by the extension rule (UD-01).
    //
    // Both are written only after `wait_until_recorded` above already
    // proved the reload landed, so a "not recorded" result here cannot be
    // explained away by the OLD (pre-reload) filter simply not having
    // gotten to them yet.
    // ------------------------------------------------------------------
    fs::write(temp_a.path().join("node_modules/x.js"), "x\n").expect("write node_modules/x.js");
    fs::write(
        temp_a.path().join("target/lib.rlib"),
        "not a real archive\n",
    )
    .expect("write target/lib.rlib");
    wait_past_debounce();
    assert_not_recorded(unf_home.path(), temp_a.path(), "node_modules/x.js");
    assert_not_recorded(unf_home.path(), temp_a.path(), "target/lib.rlib");

    // ------------------------------------------------------------------
    // Scenario 7: plain `unf watch` resets the un-ignore list; target
    // stops being recorded.
    // ------------------------------------------------------------------
    isolated_cmd(unf_home.path())
        .current_dir(temp_a.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(300));

    let pid_after_a3 =
        read_pid(&pid_path(unf_home.path())).expect("Should read PID after final re-watch of A");
    assert_eq!(
        pid_after_a2, pid_after_a3,
        "resetting the un-ignore list must also reuse the running daemon"
    );

    wait_until_unignore_dir_reverted(unf_home.path(), temp_a.path(), "target");

    // ------------------------------------------------------------------
    // Scenario 3: target un-ignored AND gitignored — still not recorded
    // until --force-watch-gitignore is added too. This is the combination
    // real users will hit, since target is gitignored in nearly every
    // Rust project. Project B: same daemon, fresh project.
    // ------------------------------------------------------------------
    fs::write(temp_b.path().join(".gitignore"), "/target\n").expect("write .gitignore");
    fs::create_dir_all(temp_b.path().join("target")).expect("mkdir target (B)");

    isolated_cmd(unf_home.path())
        .current_dir(temp_b.path())
        .arg("watch")
        .arg("--unignore-dir")
        .arg("target")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    let pid_after_b1 =
        read_pid(&pid_path(unf_home.path())).expect("Should read PID after watching B");
    assert_eq!(
        pid_after_a3, pid_after_b1,
        "adding project B must reuse the running daemon, not spawn a new one"
    );

    fs::create_dir_all(temp_b.path().join("target")).expect("mkdir target (B)");
    fs::write(temp_b.path().join("target/shadowed.rs"), "// shadowed\n")
        .expect("write target/shadowed.rs");
    wait_past_debounce();
    assert_not_recorded(unf_home.path(), temp_b.path(), "target/shadowed.rs");

    isolated_cmd(unf_home.path())
        .current_dir(temp_b.path())
        .arg("watch")
        .arg("--unignore-dir")
        .arg("target")
        .arg("--force-watch-gitignore")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(300));

    let pid_after_b2 = read_pid(&pid_path(unf_home.path()))
        .expect("Should read PID after adding --force-watch-gitignore to B");
    assert_eq!(
        pid_after_b1, pid_after_b2,
        "adding --force-watch-gitignore to B must reuse the running daemon"
    );

    fs::write(temp_b.path().join("target/recorded.rs"), "// recorded\n")
        .expect("write target/recorded.rs");
    wait_until_recorded(unf_home.path(), temp_b.path(), "target/recorded.rs");

    // ------------------------------------------------------------------
    // Scenario 5: .git/config is never recorded even if ".git" is forced
    // into unignored_dirs BY HAND (both projects.json and intent.json,
    // bypassing the CLI parser entirely). This proves the hard refusal
    // inside `Filter::should_track`, not just CLI validation.
    //
    // "target" is hand-edited in alongside ".git" purely as a reload
    // witness: a positive recording under target/ proves the daemon
    // actually picked up the hand-edited settings, so the negative .git
    // assertion below cannot be explained by "the reload never happened."
    // ------------------------------------------------------------------
    fs::create_dir_all(temp_c.path().join("target")).expect("mkdir target (C)");
    fs::create_dir_all(temp_c.path().join(".git")).expect("mkdir .git (C)");

    isolated_cmd(unf_home.path())
        .current_dir(temp_c.path())
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(500));

    let pid_after_c1 =
        read_pid(&pid_path(unf_home.path())).expect("Should read PID after watching C");
    assert_eq!(
        pid_after_b2, pid_after_c1,
        "adding project C must reuse the running daemon, not spawn a new one"
    );

    hand_edit_unignored_dirs(unf_home.path(), temp_c.path(), &["target", ".git"]);
    // Signal the daemon directly, bypassing `unf watch` (which would
    // overwrite the hand edit with CLI-validated settings). Only PIDs read
    // from this test's own UNF_HOME are ever signaled — see the daemon
    // safety notes at the top of this file.
    let kill_result = unsafe { libc::kill(pid_after_c1 as i32, libc::SIGUSR1) };
    assert_eq!(
        kill_result, 0,
        "failed to signal daemon (PID {}) to reload the hand-edited registry",
        pid_after_c1
    );

    // The SIGUSR1 reload is asynchronous, so this rewrites the probe until
    // the rebuilt Filter accepts it. A single write can land before the
    // reload is applied, get rejected by the old Filter, and never be
    // re-offered.
    wait_until_recorded_with_rewrite(unf_home.path(), temp_c.path(), "target/proof.rs");

    fs::write(temp_c.path().join(".git/config"), "[core]\n").expect("write .git/config");
    wait_past_debounce();
    assert_not_recorded(unf_home.path(), temp_c.path(), ".git/config");
}
