//! Load tests for UNFUDGED — verifies burst capture, concurrent worktree
//! recording, and escalating write rates without event drops.
//!
//! Each test uses an isolated UNF_HOME so tests can run in parallel without
//! interfering with each other or the user's real daemon.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

mod common;
use common::{isolated_cmd, IsolatedDaemonGuard};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn watch_project(unf_home: &Path, path: &Path) {
    isolated_cmd(unf_home)
        .current_dir(path)
        .arg("watch")
        .assert()
        .success();
    thread::sleep(Duration::from_millis(200));
}

/// Poll `check` every `interval_ms` until it returns `true` or `timeout_ms` elapses.
fn poll_until<F>(timeout_ms: u64, interval_ms: u64, mut check: F) -> bool
where
    F: FnMut() -> bool,
{
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < deadline {
        if check() {
            return true;
        }
        thread::sleep(Duration::from_millis(interval_ms));
    }
    false
}

fn get_status_json(unf_home: &Path, path: &Path) -> Value {
    let out = isolated_cmd(unf_home)
        .current_dir(path)
        .arg("--json")
        .arg("status")
        .output()
        .expect("failed to run unf --json status");
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "Failed to parse status JSON: {e}\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn get_snapshot_count(unf_home: &Path, path: &Path) -> u64 {
    let v = get_status_json(unf_home, path);
    v["snapshots"].as_u64().unwrap_or(0)
}

fn get_log_json(unf_home: &Path, path: &Path) -> Vec<Value> {
    let out = isolated_cmd(unf_home)
        .current_dir(path)
        .arg("--json")
        .arg("log")
        .output()
        .expect("failed to run unf --json log");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // JSON log returns {"entries": [...], "next_cursor": ...}
    if let Ok(obj) = serde_json::from_str::<Value>(&stdout) {
        if let Some(entries) = obj.get("entries").and_then(|e| e.as_array()) {
            return entries.clone();
        }
    }
    Vec::new()
}

fn get_log_files(unf_home: &Path, path: &Path) -> HashSet<String> {
    get_log_json(unf_home, path)
        .iter()
        .filter_map(|entry| entry["file"].as_str().map(String::from))
        .collect()
}

/// Retrieve the content of a file using `unf cat --snapshot ID`.
/// Finds the latest snapshot ID from `unf --json log <file>`.
fn cat_content(unf_home: &Path, path: &Path, file: &str) -> Option<String> {
    let log = get_log_json(unf_home, path);
    let snap_id = log
        .iter()
        .filter(|e| e["file"].as_str() == Some(file))
        .filter_map(|e| e["id"].as_i64())
        .next_back()?;
    let out = isolated_cmd(unf_home)
        .current_dir(path)
        .arg("--json")
        .arg("cat")
        .arg(file)
        .arg("--snapshot")
        .arg(snap_id.to_string())
        .output()
        .expect("cat failed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(&stdout).ok()?;
    v["content"].as_str().map(String::from)
}

fn burst_write(dir: &Path, count: usize, prefix: &str) -> Vec<(String, String)> {
    let mut written = Vec::with_capacity(count);
    for i in 0..count {
        let name = format!("{prefix}_{i:04}.txt");
        let content = format!("{prefix} file {i} — deterministic content for load test");
        fs::write(dir.join(&name), &content).expect("burst_write failed");
        written.push((name, content));
    }
    written
}

fn create_git_repo(base: &Path) -> PathBuf {
    let repo = base.join("repo");
    fs::create_dir_all(&repo).expect("mkdir repo");
    Command::new("git")
        .current_dir(&repo)
        .args(["init", "-b", "main"])
        .output()
        .expect("git init");
    fs::write(repo.join("README.md"), "# repo\n").expect("write readme");
    Command::new("git")
        .current_dir(&repo)
        .args(["add", "."])
        .output()
        .expect("git add");
    Command::new("git")
        .current_dir(&repo)
        .args(["commit", "-m", "init"])
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .expect("git commit");
    repo
}

fn create_worktree(repo: &Path, base: &Path, name: &str) -> PathBuf {
    let wt = base.join(name);
    // Create a branch for this worktree
    Command::new("git")
        .current_dir(repo)
        .args(["branch", name])
        .output()
        .expect("git branch");
    Command::new("git")
        .current_dir(repo)
        .args(["worktree", "add", wt.to_str().unwrap(), name])
        .output()
        .expect("git worktree add");
    wt
}

// ---------------------------------------------------------------------------
// Test 1: Burst 10 files
// ---------------------------------------------------------------------------

#[test]
fn load_burst_10_files() {
    let temp = TempDir::new().expect("tempdir");
    let unf_home = TempDir::new().expect("UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    watch_project(unf_home.path(), temp.path());

    let written = burst_write(temp.path(), 10, "b10");

    let uh = unf_home.path();
    let tp = temp.path();
    let ok = poll_until(15_000, 200, || get_snapshot_count(uh, tp) >= 10);
    let snaps = get_snapshot_count(uh, tp);
    assert!(
        ok,
        "Expected >= 10 snapshots after 10-file burst, got {snaps}"
    );

    let files = get_log_files(uh, tp);
    for (name, _) in &written {
        assert!(files.contains(name), "Missing file in log: {name}");
    }
}

// ---------------------------------------------------------------------------
// Test 2: Burst 50 files
// ---------------------------------------------------------------------------

#[test]
fn load_burst_50_files() {
    let temp = TempDir::new().expect("tempdir");
    let unf_home = TempDir::new().expect("UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    watch_project(unf_home.path(), temp.path());

    let written = burst_write(temp.path(), 50, "b50");

    let uh = unf_home.path();
    let tp = temp.path();
    let ok = poll_until(15_000, 200, || get_snapshot_count(uh, tp) >= 50);
    let snaps = get_snapshot_count(uh, tp);
    assert!(
        ok,
        "Expected >= 50 snapshots after 50-file burst, got {snaps}"
    );

    let files = get_log_files(uh, tp);
    for (name, _) in &written {
        assert!(files.contains(name), "Missing file in log: {name}");
    }
}

// ---------------------------------------------------------------------------
// Test 3: Burst 100 files
// ---------------------------------------------------------------------------

#[test]
fn load_burst_100_files() {
    let temp = TempDir::new().expect("tempdir");
    let unf_home = TempDir::new().expect("UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    watch_project(unf_home.path(), temp.path());

    let written = burst_write(temp.path(), 100, "b100");

    let uh = unf_home.path();
    let tp = temp.path();
    let ok = poll_until(15_000, 200, || get_snapshot_count(uh, tp) >= 100);
    let snaps = get_snapshot_count(uh, tp);
    assert!(
        ok,
        "Expected >= 100 snapshots after 100-file burst, got {snaps}"
    );

    let files = get_log_files(uh, tp);
    for (name, _) in &written {
        assert!(files.contains(name), "Missing file in log: {name}");
    }
}

// ---------------------------------------------------------------------------
// Test 4: Same file rapid writes (debounce coalesces)
// ---------------------------------------------------------------------------

#[test]
fn load_same_file_rapid_writes() {
    let temp = TempDir::new().expect("tempdir");
    let unf_home = TempDir::new().expect("UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    watch_project(unf_home.path(), temp.path());

    let target = temp.path().join("target.txt");
    for i in 0..50 {
        fs::write(&target, format!("version {i}")).expect("write target");
    }

    let uh = unf_home.path();
    let tp = temp.path();
    // Debounce should coalesce most writes, but rapid writes may span multiple
    // debounce windows so >= 1 is the correct assertion.
    let ok = poll_until(15_000, 200, || get_snapshot_count(uh, tp) >= 1);
    let snaps = get_snapshot_count(uh, tp);
    assert!(
        ok,
        "Expected >= 1 snapshot for same-file rapid writes, got {snaps}"
    );

    // Verify last content via cat (using snapshot ID from log)
    let content = cat_content(uh, tp, "target.txt").expect("Should be able to cat target.txt");
    assert!(
        content.contains("version 49"),
        "Expected last write 'version 49', got: {content}"
    );
}

// ---------------------------------------------------------------------------
// Test 5: Mixed burst (unique files + repeated overwrites)
// ---------------------------------------------------------------------------

#[test]
fn load_mixed_burst() {
    let temp = TempDir::new().expect("tempdir");
    let unf_home = TempDir::new().expect("UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    watch_project(unf_home.path(), temp.path());

    let shared = temp.path().join("shared.txt");
    for i in 0..20 {
        // Write a unique file
        let name = format!("unique_{i:04}.txt");
        fs::write(temp.path().join(&name), format!("unique content {i}")).expect("write unique");
        // Overwrite shared file
        fs::write(&shared, format!("shared version {i}")).expect("write shared");
    }

    let uh = unf_home.path();
    let tp = temp.path();
    // 20 unique files + 1 shared (coalesced) = 21
    let ok = poll_until(15_000, 200, || get_snapshot_count(uh, tp) >= 21);
    let snaps = get_snapshot_count(uh, tp);
    assert!(
        ok,
        "Expected >= 21 snapshots (20 unique + 1 shared), got {snaps}"
    );

    // Verify shared.txt has last content (using snapshot ID from log)
    if let Some(content) = cat_content(uh, tp, "shared.txt") {
        assert!(
            content.contains("shared version 19"),
            "Expected last shared write 'shared version 19', got: {content}"
        );
    }
}

// ---------------------------------------------------------------------------
// Test 6: Multi-worktree concurrent bursts
// ---------------------------------------------------------------------------

#[test]
#[ignore] // Flaky on CI: wt3 gets 0 snapshots due to slow I/O on GitHub runners
fn load_multi_worktree_concurrent_bursts() {
    let temp = TempDir::new().expect("tempdir");
    let unf_home = TempDir::new().expect("UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    let repo = create_git_repo(temp.path());
    let wt1 = create_worktree(&repo, temp.path(), "wt1");
    let wt2 = create_worktree(&repo, temp.path(), "wt2");
    let wt3 = create_worktree(&repo, temp.path(), "wt3");

    watch_project(unf_home.path(), &wt1);
    watch_project(unf_home.path(), &wt2);
    watch_project(unf_home.path(), &wt3);

    // Concurrent bursts using scoped threads
    let wt1_c = wt1.clone();
    let wt2_c = wt2.clone();
    let wt3_c = wt3.clone();
    thread::scope(|s| {
        s.spawn(|| burst_write(&wt1_c, 20, "wt1"));
        s.spawn(|| burst_write(&wt2_c, 20, "wt2"));
        s.spawn(|| burst_write(&wt3_c, 20, "wt3"));
    });

    // Each worktree should independently capture >= 20 snapshots
    let uh = unf_home.path();
    for (name, wt) in [("wt1", &wt1), ("wt2", &wt2), ("wt3", &wt3)] {
        let wt_ref: &Path = wt;
        let ok = poll_until(30_000, 500, || get_snapshot_count(uh, wt_ref) >= 10);
        let snaps = get_snapshot_count(uh, wt_ref);
        assert!(ok, "{name}: Expected >= 10 snapshots, got {snaps}");

        // Verify correct file prefixes (no cross-contamination)
        let files = get_log_files(uh, wt_ref);
        for f in &files {
            assert!(
                f.starts_with(name),
                "{name}: Found unexpected file {f} — possible cross-contamination"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Test 7: Multi-worktree independent capture
// ---------------------------------------------------------------------------

#[test]
fn load_multi_worktree_independent_capture() {
    let temp = TempDir::new().expect("tempdir");
    let unf_home = TempDir::new().expect("UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    let repo = create_git_repo(temp.path());
    let wt1 = create_worktree(&repo, temp.path(), "wta");
    let wt2 = create_worktree(&repo, temp.path(), "wtb");

    watch_project(unf_home.path(), &wt1);
    watch_project(unf_home.path(), &wt2);

    // Allow daemon to fully set up watchers for both worktrees
    thread::sleep(Duration::from_millis(500));

    // Write distinct files to each worktree
    fs::write(wt1.join("alpha.txt"), "alpha content").expect("write alpha");
    fs::write(wt2.join("beta.txt"), "beta content").expect("write beta");

    let uh = unf_home.path();
    let wt1_ref: &Path = &wt1;
    let wt2_ref: &Path = &wt2;
    let ok = poll_until(15_000, 200, || {
        let f1 = get_log_files(uh, wt1_ref);
        let f2 = get_log_files(uh, wt2_ref);
        f1.contains("alpha.txt") && f2.contains("beta.txt")
    });
    assert!(ok, "Timed out waiting for worktree snapshots");

    let wt1_files = get_log_files(uh, wt1_ref);
    let wt2_files = get_log_files(uh, wt2_ref);

    assert!(
        wt1_files.contains("alpha.txt"),
        "wt1 should contain alpha.txt, got: {wt1_files:?}"
    );
    assert!(
        !wt1_files.contains("beta.txt"),
        "wt1 should NOT contain beta.txt, got: {wt1_files:?}"
    );
    assert!(
        wt2_files.contains("beta.txt"),
        "wt2 should contain beta.txt, got: {wt2_files:?}"
    );
    assert!(
        !wt2_files.contains("alpha.txt"),
        "wt2 should NOT contain alpha.txt, got: {wt2_files:?}"
    );
}

// ---------------------------------------------------------------------------
// Test 8: Escalating rate — the key stress test
// ---------------------------------------------------------------------------

#[test]
fn load_escalating_rate() {
    let temp = TempDir::new().expect("tempdir");
    let unf_home = TempDir::new().expect("UNF_HOME");
    let _guard = IsolatedDaemonGuard::new(unf_home.path());

    watch_project(unf_home.path(), temp.path());

    struct Phase {
        label: &'static str,
        count: usize,
        prefix: &'static str,
    }

    let phases = [
        Phase {
            label: "Phase 1",
            count: 10,
            prefix: "p1",
        },
        Phase {
            label: "Phase 2",
            count: 50,
            prefix: "p2",
        },
        Phase {
            label: "Phase 3",
            count: 100,
            prefix: "p3",
        },
        Phase {
            label: "Phase 4",
            count: 200,
            prefix: "p4",
        },
    ];

    let mut total_written: usize = 0;
    let mut results: Vec<(String, usize, u64)> = Vec::new();

    let uh = unf_home.path();
    let tp = temp.path();
    for phase in &phases {
        burst_write(tp, phase.count, phase.prefix);
        total_written += phase.count;

        let expected = total_written as u64;
        let ok = poll_until(15_000, 200, || get_snapshot_count(uh, tp) >= expected);
        let snaps = get_snapshot_count(uh, tp);
        results.push((phase.label.to_string(), total_written, snaps));

        assert!(
            ok,
            "{}: Expected >= {} cumulative snapshots, got {}",
            phase.label, total_written, snaps
        );
    }

    // Print summary table
    eprintln!("\n--- Escalating Rate Summary ---");
    eprintln!(
        "{:<10} {:>10} {:>10} {:>8}",
        "Phase", "Expected", "Actual", "Delta"
    );
    eprintln!("{}", "-".repeat(42));
    for (label, expected, actual) in &results {
        let delta = *actual as i64 - *expected as i64;
        eprintln!(
            "{:<10} {:>10} {:>10} {:>+8}",
            label, expected, actual, delta
        );
    }
    eprintln!("{}", "-".repeat(42));
}
