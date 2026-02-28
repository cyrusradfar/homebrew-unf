//! Shared test infrastructure for UNFUDGED integration tests.
//!
//! Provides a single canonical `IsolatedDaemonGuard` that all test files
//! should use. This guard kills sentinel first (preventing daemon respawn),
//! waits for sentinel exit, then kills the daemon.

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use assert_cmd::Command as CargoCmd;

/// RAII guard that kills an isolated test daemon (and its sentinel) on drop.
///
/// Kill order: sentinel first (prevents respawn), then daemon.
/// Used by all integration tests to ensure cleanup even on panic.
pub struct IsolatedDaemonGuard {
    unf_home: PathBuf,
}

impl IsolatedDaemonGuard {
    pub fn new(unf_home: &Path) -> Self {
        Self {
            unf_home: unf_home.to_path_buf(),
        }
    }
}

impl Drop for IsolatedDaemonGuard {
    fn drop(&mut self) {
        // 1. Kill sentinel first (prevents it from respawning daemon)
        let sentinel_pid_file = self.unf_home.join("sentinel.pid");
        if let Ok(content) = fs::read_to_string(&sentinel_pid_file) {
            if let Ok(pid) = content.trim().parse::<i32>() {
                unsafe {
                    libc::kill(pid, libc::SIGTERM);
                }
                // Poll-wait for sentinel to exit (up to 2s)
                for _ in 0..20 {
                    if unsafe { libc::kill(pid, 0) } != 0 {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }

        // 2. Kill daemon
        let pid_file = self.unf_home.join("daemon.pid");
        if let Ok(content) = fs::read_to_string(&pid_file) {
            if let Ok(pid) = content.trim().parse::<i32>() {
                unsafe {
                    libc::kill(pid, libc::SIGTERM);
                }
                // Brief wait to confirm exit
                for _ in 0..10 {
                    if unsafe { libc::kill(pid, 0) } != 0 {
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
}

/// Get a Command instance for the unf binary.
#[allow(deprecated)]
pub fn unf_cmd() -> CargoCmd {
    CargoCmd::cargo_bin("unf").expect("Failed to find unf binary")
}

/// Get a Command instance with an isolated UNF_HOME.
pub fn isolated_cmd(unf_home: &Path) -> CargoCmd {
    let mut cmd = unf_cmd();
    cmd.env("UNF_HOME", unf_home);
    cmd
}

/// Resolve the centralized storage directory for a project under an isolated UNF_HOME.
#[allow(dead_code)]
pub fn resolve_storage_dir_isolated(unf_home: &Path, project_root: &Path) -> PathBuf {
    let canonical = project_root
        .canonicalize()
        .expect("project root must be canonicalizable");
    let stripped = canonical
        .to_string_lossy()
        .strip_prefix('/')
        .unwrap_or(&canonical.to_string_lossy())
        .to_string();
    unf_home.join("data").join(stripped)
}
