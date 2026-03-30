//! Cross-platform process management utilities.
//!
//! Provides operations for process management: checking if processes are alive,
//! terminating processes, managing PID files, and inter-process communication.
//! Uses POSIX signals on Unix and Windows process APIs on Windows.

use crate::error::{UnfError, WatcherError};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

/// Checks whether a process with the given PID is alive.
///
/// Returns `true` if the process exists and we have permission to query it.
/// Returns `false` if the process does not exist.
///
/// On Unix: uses `libc::kill(pid, 0)` (signal 0 = existence check).
/// On Windows: uses `OpenProcess` + `GetExitCodeProcess`.
#[cfg(unix)]
pub fn is_alive(pid: u32) -> bool {
    // SAFETY: kill(pid, 0) is a standard POSIX existence check.
    // Signal 0 does not affect the process.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(windows)]
pub fn is_alive(pid: u32) -> bool {
    use std::ptr;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle == 0 || handle == ptr::null_mut() as _ {
            return false;
        }
        let mut exit_code: u32 = 0;
        let success = GetExitCodeProcess(handle, &mut exit_code);
        CloseHandle(handle);
        // STILL_ACTIVE = 259
        success != 0 && exit_code == 259
    }
}

/// Checks whether a process with the given PID is a zombie (state Z).
///
/// A zombie has exited but has not yet been reaped by its parent via `waitpid()`.
/// The process table entry still exists, so `kill(pid, 0)` returns success —
/// making `is_alive()` return `true` for zombies. This function distinguishes
/// zombies from genuinely running processes.
///
/// Uses `ps -o stat= -p <pid>` and checks whether the stat field starts with `Z`.
/// Returns `false` for nonexistent processes or any error (fail-open).
///
/// Called at most once per 15-second sentinel tick for a single PID;
/// the subprocess spawn cost is negligible.
#[cfg(unix)]
pub fn is_zombie(pid: u32) -> bool {
    let output = std::process::Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let stat = String::from_utf8_lossy(&out.stdout);
            stat.trim().starts_with('Z')
        }
        _ => false,
    }
}

#[cfg(windows)]
pub fn is_zombie(_pid: u32) -> bool {
    // Windows has no zombie process concept.
    false
}

/// Sends a termination signal to the process with the given PID.
///
/// On Unix: sends SIGTERM (graceful shutdown request).
/// On Windows: calls `TerminateProcess` (no graceful equivalent).
///
/// # Errors
///
/// Returns `Err` if the process does not exist or we lack permission.
#[cfg(unix)]
pub fn terminate(pid: u32) -> Result<(), UnfError> {
    // SAFETY: SIGTERM is the standard graceful shutdown signal.
    let ret = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    if ret == 0 {
        Ok(())
    } else {
        Err(UnfError::Watcher(WatcherError::Io(
            std::io::Error::last_os_error(),
        )))
    }
}

#[cfg(windows)]
pub fn terminate(pid: u32) -> Result<(), UnfError> {
    use std::ptr;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle == 0 || handle == ptr::null_mut() as _ {
            return Err(UnfError::Watcher(WatcherError::Io(
                std::io::Error::last_os_error(),
            )));
        }
        let success = TerminateProcess(handle, 1);
        CloseHandle(handle);
        if success != 0 {
            Ok(())
        } else {
            Err(UnfError::Watcher(WatcherError::Io(
                std::io::Error::last_os_error(),
            )))
        }
    }
}

/// Sends a specific signal to a process.
///
/// Used for inter-process communication (e.g., SIGUSR1 to tell daemon to reload).
///
/// # Errors
///
/// Returns `Err` if the process does not exist or we lack permission.
#[cfg(unix)]
pub fn send_signal(pid: u32, signal: i32) -> Result<(), UnfError> {
    // SAFETY: Sending a signal is a standard POSIX IPC mechanism.
    let ret = unsafe { libc::kill(pid as i32, signal) };
    if ret == 0 {
        Ok(())
    } else {
        Err(UnfError::Watcher(WatcherError::Io(
            std::io::Error::last_os_error(),
        )))
    }
}

#[cfg(windows)]
pub fn send_signal(_pid: u32, _signal: i32) -> Result<(), UnfError> {
    // Windows doesn't support Unix signals; future work could use named events.
    Ok(())
}

/// Finds all process IDs that have a file open.
///
/// Uses `lsof -t <path>` on Unix. Returns an empty vec on any failure
/// (lsof missing, file doesn't exist, permission error, etc.).
/// Filters out the current process's own PID.
///
/// On Windows: returns an empty vec (stub).
#[cfg(unix)]
pub fn find_processes_using_file(path: &Path) -> Vec<u32> {
    let output = match std::process::Command::new("lsof")
        .arg("-t")
        .arg(path)
        .output()
    {
        Ok(output) => output,
        Err(_) => return Vec::new(),
    };

    if !output.status.success() {
        return Vec::new();
    }

    let stdout = match std::str::from_utf8(&output.stdout) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let current_pid = std::process::id();

    stdout
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .filter(|&pid| pid != current_pid)
        .collect()
}

#[cfg(windows)]
pub fn find_processes_using_file(_path: &Path) -> Vec<u32> {
    Vec::new()
}

/// A PID file utility for reading, writing, and checking process lifecycle.
///
/// Encapsulates all PID file operations (read, write, remove, staleness check)
/// into a single abstraction. Handles parsing and process existence validation.
///
/// # Example
///
/// ```ignore
/// let pid_file = PidFile::new(PathBuf::from("/var/run/daemon.pid"));
/// pid_file.write(12345)?;
/// assert_eq!(pid_file.read()?, Some(12345));
/// assert!(pid_file.is_running());
/// pid_file.remove()?;
/// ```
pub struct PidFile {
    path: PathBuf,
}

impl PidFile {
    /// Creates a new PidFile pointing to the given path.
    ///
    /// Does not create the file or validate the path.
    pub fn new(path: PathBuf) -> Self {
        PidFile { path }
    }

    /// Writes a PID to the file.
    ///
    /// Creates parent directories if needed. Overwrites any existing content.
    ///
    /// # Errors
    ///
    /// Returns `Err` if parent directory creation or file write fails.
    pub fn write(&self, pid: u32) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut file = fs::File::create(&self.path)?;
        use std::io::Write;
        file.write_all(pid.to_string().as_bytes())?;

        Ok(())
    }

    /// Reads the PID from the file.
    ///
    /// Returns `Ok(Some(pid))` if file exists and contains a valid integer.
    /// Returns `Ok(None)` if file doesn't exist or is empty.
    ///
    /// # Errors
    ///
    /// Returns `Err` if file read fails for reasons other than "not found".
    pub fn read(&self) -> io::Result<Option<u32>> {
        match fs::read_to_string(&self.path) {
            Ok(content) => {
                let pid = content.trim().parse::<u32>();
                match pid {
                    Ok(p) => Ok(Some(p)),
                    Err(_) => Ok(None), // Invalid PID in file, treat as absent
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Removes the PID file.
    ///
    /// Returns `Ok(())` even if the file doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns `Err` if removal fails for reasons other than "not found".
    pub fn remove(&self) -> io::Result<()> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Returns the path to the PID file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Checks if the process recorded in the PID file is currently running.
    ///
    /// Returns `false` if:
    /// - The file doesn't exist
    /// - The file contains an invalid PID
    /// - The process with that PID is not alive
    ///
    /// Returns `true` only if a valid PID is read and the process exists.
    pub fn is_running(&self) -> bool {
        match self.read() {
            Ok(Some(pid)) => is_alive(pid),
            _ => false,
        }
    }
}

/// Sends SIGTERM, then escalates to SIGKILL if the process doesn't exit within the timeout.
///
/// Polls `is_alive()` every 100ms after SIGTERM. If the process is still alive
/// after `timeout_ms`, sends SIGKILL.
///
/// # Errors
///
/// Returns `Err` if both SIGTERM and SIGKILL fail (e.g., process doesn't exist).
#[cfg(unix)]
pub fn force_terminate(pid: u32, timeout_ms: u64) -> Result<(), UnfError> {
    // 1. Send SIGTERM via existing terminate()
    //    - If SIGTERM fails (process doesn't exist), return the error
    terminate(pid)?;

    // 2. Poll is_alive() every 100ms up to timeout_ms
    let poll_interval = Duration::from_millis(100);
    let max_polls = timeout_ms.div_ceil(100);

    for _ in 0..max_polls {
        thread::sleep(poll_interval);
        if !is_alive(pid) {
            return Ok(());
        }
    }

    // 3. If still alive after timeout, send SIGKILL
    // SAFETY: SIGKILL is a standard forceful termination signal.
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }

    Ok(())
}

#[cfg(windows)]
pub fn force_terminate(pid: u32, _timeout_ms: u64) -> Result<(), UnfError> {
    terminate(pid) // Windows terminate() is already forceful
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ---- PidFile tests (TDD approach) ----

    #[test]
    fn pidfile_write_then_read_returns_same_pid() {
        let temp = TempDir::new().expect("create temp dir");
        let pid_path = temp.path().join("test.pid");
        let pf = PidFile::new(pid_path);

        let test_pid = 12345u32;
        pf.write(test_pid).expect("write pid");

        let read_pid = pf.read().expect("read pid");
        assert_eq!(read_pid, Some(test_pid));
    }

    #[test]
    fn pidfile_read_nonexistent_returns_none() {
        let temp = TempDir::new().expect("create temp dir");
        let pid_path = temp.path().join("nonexistent.pid");
        let pf = PidFile::new(pid_path);

        let result = pf.read().expect("read should not fail");
        assert_eq!(result, None);
    }

    #[test]
    fn pidfile_read_invalid_content_returns_none() {
        let temp = TempDir::new().expect("create temp dir");
        let pid_path = temp.path().join("invalid.pid");
        fs::write(&pid_path, b"not-a-number").expect("write invalid content");

        let pf = PidFile::new(pid_path);
        let result = pf.read().expect("read should not fail");
        assert_eq!(result, None);
    }

    #[test]
    fn pidfile_remove_deletes_file() {
        let temp = TempDir::new().expect("create temp dir");
        let pid_path = temp.path().join("test.pid");
        let pf = PidFile::new(pid_path.clone());

        pf.write(99999).expect("write pid");
        assert!(pid_path.exists());

        pf.remove().expect("remove pid");
        assert!(!pid_path.exists());
    }

    #[test]
    fn pidfile_remove_nonexistent_succeeds() {
        let temp = TempDir::new().expect("create temp dir");
        let pid_path = temp.path().join("nonexistent.pid");
        let pf = PidFile::new(pid_path);

        // Should not fail even if file doesn't exist
        let result = pf.remove();
        assert!(result.is_ok());
    }

    #[test]
    fn pidfile_is_running_nonexistent_returns_false() {
        let temp = TempDir::new().expect("create temp dir");
        let pid_path = temp.path().join("nonexistent.pid");
        let pf = PidFile::new(pid_path);

        assert!(!pf.is_running());
    }

    #[test]
    fn pidfile_is_running_dead_pid_returns_false() {
        let temp = TempDir::new().expect("create temp dir");
        let pid_path = temp.path().join("test.pid");
        let pf = PidFile::new(pid_path);

        // Write a very high PID that almost certainly doesn't exist
        pf.write(999999).expect("write pid");
        assert!(!pf.is_running());
    }

    #[test]
    fn pidfile_is_running_live_pid_returns_true() {
        let temp = TempDir::new().expect("create temp dir");
        let pid_path = temp.path().join("test.pid");
        let pf = PidFile::new(pid_path);

        // Write the current process's PID
        let current_pid = std::process::id();
        pf.write(current_pid).expect("write pid");
        assert!(pf.is_running());
    }

    #[test]
    fn pidfile_path_returns_correct_path() {
        let pid_path = PathBuf::from("/tmp/test.pid");
        let pf = PidFile::new(pid_path.clone());

        assert_eq!(pf.path(), &pid_path);
    }

    #[test]
    fn pidfile_write_creates_parent_directories() {
        let temp = TempDir::new().expect("create temp dir");
        let pid_path = temp.path().join("subdir1/subdir2/test.pid");
        let pf = PidFile::new(pid_path.clone());

        pf.write(54321).expect("write pid");
        assert!(pid_path.exists());

        let read_pid = pf.read().expect("read pid");
        assert_eq!(read_pid, Some(54321));
    }

    #[test]
    fn pidfile_write_overwrites_existing() {
        let temp = TempDir::new().expect("create temp dir");
        let pid_path = temp.path().join("test.pid");
        let pf = PidFile::new(pid_path);

        pf.write(11111).expect("write pid 1");
        pf.write(22222).expect("write pid 2");

        let read_pid = pf.read().expect("read pid");
        assert_eq!(read_pid, Some(22222));
    }

    // ---- Original process tests ----

    #[test]
    fn current_process_is_alive() {
        assert!(is_alive(std::process::id()));
    }

    #[test]
    fn nonexistent_process_is_not_alive() {
        assert!(!is_alive(999999));
    }

    #[test]
    fn terminate_nonexistent_process_fails() {
        let result = terminate(999999);
        assert!(result.is_err());
    }

    #[test]
    fn send_signal_to_self_succeeds() {
        // Signal 0 is an existence check, should succeed for our own process
        let result = send_signal(std::process::id(), 0);
        assert!(result.is_ok());
    }

    #[test]
    fn send_signal_to_nonexistent_fails() {
        let result = send_signal(999999, 0);
        assert!(result.is_err());
    }

    #[test]
    fn find_processes_nonexistent_file_returns_empty() {
        let result = find_processes_using_file(Path::new("/nonexistent/file/12345"));
        assert!(result.is_empty());
    }

    #[test]
    fn find_processes_filters_out_self() {
        // The test process itself has its own binary open, but we filter self out
        // Just verify the function runs without panic on a real file
        let result = find_processes_using_file(Path::new("/dev/null"));
        // Current PID should never appear
        assert!(!result.contains(&std::process::id()));
    }

    #[test]
    fn force_terminate_nonexistent_process_fails() {
        let result = force_terminate(999999, 100);
        assert!(result.is_err());
    }

    #[test]
    #[cfg(unix)]
    fn current_process_is_not_zombie() {
        assert!(!is_zombie(std::process::id()));
    }

    #[test]
    #[cfg(unix)]
    fn nonexistent_process_is_not_zombie() {
        assert!(!is_zombie(999_999));
    }

    /// Proves the zombie bug: `is_alive()` returns true for zombie processes.
    ///
    /// This test spawns a child that exits immediately, does NOT call wait(),
    /// and verifies that `kill(pid, 0)` still succeeds — exactly the bug that
    /// caused the sentinel to think a dead daemon was alive for days.
    ///
    /// After confirming the bug, it reaps the zombie via `child.wait()` and
    /// verifies `is_alive()` then correctly returns false.
    #[test]
    #[cfg(unix)]
    fn zombie_process_fools_is_alive() {
        use std::process::Command;

        // Spawn a child that exits immediately
        let mut child = Command::new("true").spawn().expect("failed to spawn");
        let pid = child.id();

        // Wait for child to exit (becomes zombie since we hold the Child handle
        // but haven't called wait() — the OS keeps the process table entry)
        std::thread::sleep(std::time::Duration::from_millis(500));

        // BUG: is_alive() returns true for the zombie
        assert!(
            is_alive(pid),
            "Expected is_alive() to return true for zombie (this IS the bug)"
        );

        // Verify it's actually a zombie via ps
        let ps_output = Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .expect("ps failed");
        let stat = String::from_utf8_lossy(&ps_output.stdout);
        assert!(
            stat.trim().starts_with('Z'),
            "Expected zombie state 'Z', got '{}'",
            stat.trim()
        );

        // Reap the zombie
        let _ = child.wait();

        // After reaping, is_alive() correctly returns false
        assert!(
            !is_alive(pid),
            "After reaping, is_alive() should return false"
        );
    }
}
