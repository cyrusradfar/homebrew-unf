//! Resource leak tests for UNFUDGED — verifies FDs, memory, and SQLite
//! connections don't leak across watch/unwatch cycles.
//!
//! All tests are `#[ignore]` — run with:
//! ```
//! cargo test resource_leak -- --ignored --nocapture
//! ```
//!
//! These tests use external measurement (`lsof`, `ps`) since the daemon
//! is a separate process. Unix-only (`lsof`/`ps` required).

#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use tempfile::TempDir;

mod common;
use common::{isolated_cmd, IsolatedDaemonGuard};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read the daemon PID from the `daemon.pid` file in UNF_HOME.
fn get_daemon_pid(unf_home: &Path) -> Option<u32> {
    fs::read_to_string(unf_home.join("daemon.pid"))
        .ok()?
        .trim()
        .parse::<u32>()
        .ok()
}

/// Check if a process is alive via `kill(pid, 0)`.
fn is_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Count open file descriptors for a process using `lsof -p <pid>`.
/// Returns the line count minus the header line.
fn get_fd_count(pid: u32) -> usize {
    let output = Command::new("lsof")
        .args(["-p", &pid.to_string()])
        .output()
        .expect("lsof failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    // First line is header; rest are FDs
    lines.len().saturating_sub(1)
}

/// Count open SQLite-related file descriptors (files containing "sqlite" in path).
fn get_sqlite_fd_count(pid: u32) -> usize {
    let output = Command::new("lsof")
        .args(["-p", &pid.to_string()])
        .output()
        .expect("lsof failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .skip(1) // skip header
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("sqlite") || lower.contains(".db")
        })
        .count()
}

/// Get resident set size (RSS) in KB for a process via `ps -o rss= -p <pid>`.
fn get_rss_kb(pid: u32) -> u64 {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .expect("ps failed");
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u64>()
        .unwrap_or(0)
}

/// Watch a project directory with the isolated daemon.
fn watch_project(unf_home: &Path, path: &Path) {
    isolated_cmd(unf_home)
        .current_dir(path)
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(300));
}

/// Unwatch a project directory from the isolated daemon.
fn unwatch_project(unf_home: &Path, path: &Path) {
    isolated_cmd(unf_home)
        .current_dir(path)
        .arg("unwatch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(300));
}

/// Burst-write `count` text files into `dir` with a given prefix.
fn burst_write(dir: &Path, count: usize, prefix: &str) {
    for i in 0..count {
        let name = format!("{prefix}_{i:04}.txt");
        let content = format!("{prefix} file {i} — resource leak test content");
        fs::write(dir.join(&name), &content).expect("burst_write failed");
    }
}

// ---------------------------------------------------------------------------
// Test 1: FD leak across watch/unwatch cycles
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn fd_leak_watch_unwatch_cycle() {
    let unf_home = TempDir::new().expect("UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Start daemon with an anchor project (keeps daemon alive throughout)
    let anchor = TempDir::new().expect("anchor dir");
    watch_project(unf_home.path(), anchor.path());

    let pid = get_daemon_pid(unf_home.path()).expect("daemon should be running");
    assert!(is_alive(pid), "daemon should be alive");

    // Let daemon settle
    thread::sleep(Duration::from_secs(1));
    let baseline_fds = get_fd_count(pid);
    eprintln!("Baseline FD count: {baseline_fds}");
    eprintln!();

    const CYCLES: usize = 10;

    for cycle in 0..CYCLES {
        let project = TempDir::new().expect("project dir");
        watch_project(unf_home.path(), project.path());

        // Burst-write files to exercise storage/watcher
        burst_write(project.path(), 5, &format!("cycle{cycle}"));
        thread::sleep(Duration::from_secs(1)); // let daemon process events

        unwatch_project(unf_home.path(), project.path());
        // TempDir drops here, removing the project directory

        // Let daemon clean up
        thread::sleep(Duration::from_millis(500));

        let current_fds = get_fd_count(pid);
        eprintln!(
            "Cycle {cycle:>2}: FDs = {current_fds} (delta = {delta:+})",
            delta = current_fds as i64 - baseline_fds as i64
        );
    }

    // Final measurement after all cycles
    thread::sleep(Duration::from_secs(1));
    let final_fds = get_fd_count(pid);
    eprintln!();
    eprintln!(
        "Final FD count: {final_fds} (baseline: {baseline_fds}, delta: {delta:+})",
        delta = final_fds as i64 - baseline_fds as i64
    );

    assert!(
        final_fds <= baseline_fds + 10,
        "FD leak detected: baseline={baseline_fds}, final={final_fds}, delta={}. \
         Expected at most +10 FDs after {CYCLES} watch/unwatch cycles.",
        final_fds as i64 - baseline_fds as i64
    );
}

// ---------------------------------------------------------------------------
// Test 2: FD leak with many simultaneous projects
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn fd_leak_many_projects_simultaneous() {
    let unf_home = TempDir::new().expect("UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Start daemon with an anchor project
    let anchor = TempDir::new().expect("anchor dir");
    watch_project(unf_home.path(), anchor.path());

    let pid = get_daemon_pid(unf_home.path()).expect("daemon should be running");
    assert!(is_alive(pid), "daemon should be alive");

    thread::sleep(Duration::from_secs(1));
    let baseline_fds = get_fd_count(pid);
    eprintln!("Baseline FD count: {baseline_fds}");

    const PROJECT_COUNT: usize = 8;

    // Watch 8 additional projects
    let mut projects: Vec<TempDir> = Vec::new();
    for i in 0..PROJECT_COUNT {
        let project = TempDir::new().expect("project dir");
        watch_project(unf_home.path(), project.path());
        burst_write(project.path(), 3, &format!("proj{i}"));
        projects.push(project);
    }

    // Let daemon process all events
    thread::sleep(Duration::from_secs(2));
    let peak_fds = get_fd_count(pid);
    eprintln!(
        "Peak FD count ({PROJECT_COUNT} projects): {peak_fds} (delta = {delta:+})",
        delta = peak_fds as i64 - baseline_fds as i64
    );

    // Unwatch all additional projects
    for project in &projects {
        unwatch_project(unf_home.path(), project.path());
    }

    // Drop project directories and let daemon clean up
    drop(projects);
    thread::sleep(Duration::from_secs(2));

    let final_fds = get_fd_count(pid);
    eprintln!(
        "Final FD count: {final_fds} (delta from baseline = {delta:+})",
        delta = final_fds as i64 - baseline_fds as i64
    );

    // Sanity: peak should be higher than baseline (we actually opened things)
    assert!(
        peak_fds > baseline_fds,
        "Peak FDs ({peak_fds}) should exceed baseline ({baseline_fds}) — \
         sanity check that watching projects actually opens FDs"
    );

    assert!(
        final_fds <= baseline_fds + 10,
        "FD leak detected: baseline={baseline_fds}, final={final_fds}, delta={}. \
         Expected at most +10 FDs after unwatching {PROJECT_COUNT} projects.",
        final_fds as i64 - baseline_fds as i64
    );
}

// ---------------------------------------------------------------------------
// Test 3: RSS leak across watch/unwatch cycles
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn rss_leak_watch_unwatch_cycle() {
    let unf_home = TempDir::new().expect("UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Start daemon with an anchor project
    let anchor = TempDir::new().expect("anchor dir");
    watch_project(unf_home.path(), anchor.path());

    let pid = get_daemon_pid(unf_home.path()).expect("daemon should be running");
    assert!(is_alive(pid), "daemon should be alive");

    // Warm-up cycles (let allocator settle)
    eprintln!("Running 3 warm-up cycles...");
    for i in 0..3 {
        let project = TempDir::new().expect("warmup dir");
        watch_project(unf_home.path(), project.path());
        burst_write(project.path(), 10, &format!("warmup{i}"));
        thread::sleep(Duration::from_secs(1));
        unwatch_project(unf_home.path(), project.path());
        thread::sleep(Duration::from_millis(500));
    }

    thread::sleep(Duration::from_secs(1));
    let baseline_rss = get_rss_kb(pid);
    eprintln!("Baseline RSS: {baseline_rss} KB");
    eprintln!();

    const CYCLES: usize = 10;

    for cycle in 0..CYCLES {
        let project = TempDir::new().expect("project dir");
        watch_project(unf_home.path(), project.path());

        // Heavier writes to stress memory
        burst_write(project.path(), 20, &format!("rss_cycle{cycle}"));
        thread::sleep(Duration::from_secs(1));

        unwatch_project(unf_home.path(), project.path());
        thread::sleep(Duration::from_millis(500));

        let current_rss = get_rss_kb(pid);
        eprintln!(
            "Cycle {cycle:>2}: RSS = {current_rss} KB (delta = {delta:+} KB)",
            delta = current_rss as i64 - baseline_rss as i64
        );
    }

    thread::sleep(Duration::from_secs(1));
    let final_rss = get_rss_kb(pid);
    eprintln!();
    eprintln!(
        "Final RSS: {final_rss} KB (baseline: {baseline_rss} KB, delta: {delta:+} KB)",
        delta = final_rss as i64 - baseline_rss as i64
    );

    // Allow 2x baseline or baseline + 10MB, whichever is larger
    let max_allowed = std::cmp::max(2 * baseline_rss, baseline_rss + 10_240);
    assert!(
        final_rss <= max_allowed,
        "RSS leak detected: baseline={baseline_rss} KB, final={final_rss} KB, \
         delta={} KB. Max allowed: {max_allowed} KB (2x baseline or +10MB).",
        final_rss as i64 - baseline_rss as i64
    );
}

// ---------------------------------------------------------------------------
// Test 4: SQLite connection cleanup on unwatch
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn sqlite_connection_cleanup_on_unwatch() {
    let unf_home = TempDir::new().expect("UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    // Start daemon with an anchor project
    let anchor = TempDir::new().expect("anchor dir");
    watch_project(unf_home.path(), anchor.path());

    let pid = get_daemon_pid(unf_home.path()).expect("daemon should be running");
    assert!(is_alive(pid), "daemon should be alive");

    thread::sleep(Duration::from_secs(1));
    let baseline_sqlite_fds = get_sqlite_fd_count(pid);
    eprintln!("Baseline SQLite FD count: {baseline_sqlite_fds}");

    const PROJECT_COUNT: usize = 5;

    // Watch 5 projects and burst-write to each
    let mut projects: Vec<TempDir> = Vec::new();
    for i in 0..PROJECT_COUNT {
        let project = TempDir::new().expect("project dir");
        watch_project(unf_home.path(), project.path());
        burst_write(project.path(), 5, &format!("sqlite{i}"));
        projects.push(project);
    }

    // Let daemon process and open connections
    thread::sleep(Duration::from_secs(2));
    let active_sqlite_fds = get_sqlite_fd_count(pid);
    eprintln!("Active SQLite FD count ({PROJECT_COUNT} projects): {active_sqlite_fds}");

    // Unwatch all projects
    for project in &projects {
        unwatch_project(unf_home.path(), project.path());
    }

    // Drop project directories and let daemon clean up
    drop(projects);
    thread::sleep(Duration::from_secs(2));

    let final_sqlite_fds = get_sqlite_fd_count(pid);
    eprintln!("Final SQLite FD count: {final_sqlite_fds} (baseline: {baseline_sqlite_fds})");

    // Allow +3 for WAL checkpoint tolerance
    assert!(
        final_sqlite_fds <= baseline_sqlite_fds + 3,
        "SQLite FD leak detected: baseline={baseline_sqlite_fds}, final={final_sqlite_fds}, \
         delta={}. Expected at most +3 (WAL checkpoint tolerance) after \
         unwatching {PROJECT_COUNT} projects.",
        final_sqlite_fds as i64 - baseline_sqlite_fds as i64
    );

    // Verify daemon is still healthy
    assert!(
        is_alive(pid),
        "Daemon should still be alive after all unwatches"
    );
}
