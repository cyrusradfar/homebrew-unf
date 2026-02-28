//! Cross-platform process management utilities.
//!
//! Provides two operations: check if a process is alive, and terminate a process.
//! Uses POSIX signals on Unix and Windows process APIs on Windows.

use crate::error::{UnfError, WatcherError};
use std::path::Path;
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
}
