//! Platform-specific auto-start installation and removal.
//!
//! Manages OS-level mechanisms for the sentinel watchdog, which handles both
//! boot initialization and daemon lifecycle management.
//!
//! - macOS: LaunchAgent plist in `~/Library/LaunchAgents/` with KeepAlive
//! - Linux: systemd user service in `~/.config/systemd/user/` with Restart=always
//!
//! The sentinel runs continuously and performs boot initialization on startup,
//! replacing the legacy separate boot agent.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::UnfError;

/// The label/name for the sentinel KeepAlive entry.
#[cfg(target_os = "macos")]
const SENTINEL_LAUNCHD_LABEL: &str = "com.unfudged.sentinel";
#[cfg(target_os = "macos")]
const SENTINEL_PLIST_NAME: &str = "com.unfudged.sentinel.plist";
#[cfg(target_os = "linux")]
const SENTINEL_SERVICE_NAME: &str = "unfudged-sentinel.service";

// Legacy boot constants (for backwards compat cleanup only)
#[cfg(target_os = "macos")]
const LEGACY_BOOT_PLIST_NAME: &str = "com.unfudged.boot.plist";
#[cfg(target_os = "linux")]
const LEGACY_BOOT_SERVICE_NAME: &str = "unfudged-boot.service";

/// Whether to drive the OS service manager (launchd / systemd).
///
/// The test harness sets `UNF_AUTOSTART_INERT` so the suite exercises the
/// plist/unit *files* without touching the real login session. `launchctl
/// unload` resolves jobs by the `Label` inside the plist, not by the path
/// argument, so an unguarded call from a HOME-isolated test still unloads the
/// developer's real sentinel.
#[cfg_attr(not(any(target_os = "macos", target_os = "linux")), allow(dead_code))]
fn service_manager_enabled() -> bool {
    std::env::var_os("UNF_AUTOSTART_INERT").is_none()
}

// --- macOS: launchd ---

/// Returns the path to the LaunchAgents directory.
#[cfg(target_os = "macos")]
fn launchd_dir() -> Result<PathBuf, UnfError> {
    let home = dirs::home_dir()
        .ok_or_else(|| UnfError::InvalidArgument("Cannot determine home directory".to_string()))?;
    Ok(home.join("Library/LaunchAgents"))
}

/// Generates the sentinel launchd plist XML content with KeepAlive.
///
/// Unlike the boot plist (RunAtLoad, one-shot), the sentinel plist uses
/// KeepAlive to ensure the sentinel is always running.
///
/// Pure function — no I/O. Testable.
#[cfg(target_os = "macos")]
fn format_sentinel_plist(exe_path: &std::path::Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>__sentinel</string>
  </array>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>/tmp/unfudged-sentinel.log</string>
  <key>StandardErrorPath</key>
  <string>/tmp/unfudged-sentinel.log</string>
</dict>
</plist>
"#,
        label = SENTINEL_LAUNCHD_LABEL,
        exe = exe_path.display()
    )
}

/// Generates the sentinel systemd service unit content with Restart=always.
///
/// Pure function — no I/O. Testable.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn format_sentinel_service(exe_path: &std::path::Path) -> String {
    format!(
        r#"[Unit]
Description=UNFUDGED sentinel - daemon watchdog and intent reconciliation

[Service]
Type=simple
ExecStart={exe} __sentinel
Restart=always
RestartSec=10

[Install]
WantedBy=default.target
"#,
        exe = exe_path.display()
    )
}

// --- Binary path resolution ---

/// Well-known install locations for the `unf` binary, checked in order.
const STABLE_PATHS: &[&str] = &[
    "/opt/homebrew/bin/unf", // Apple Silicon Homebrew
    "/usr/local/bin/unf",    // Intel Homebrew / manual install
];

/// Resolves the binary path to a stable install location.
///
/// If `current_exe()` points to a development build (target/debug/ or target/release/),
/// checks well-known install paths and falls back to `which unf`.
///
/// Pure logic with minimal I/O (existence checks only).
fn resolve_stable_binary_path(current: &Path) -> PathBuf {
    let path_str = current.to_string_lossy();

    // If not a dev build, use as-is
    if !path_str.contains("/target/debug/") && !path_str.contains("/target/release/") {
        return current.to_path_buf();
    }

    // Check well-known stable locations
    for candidate in STABLE_PATHS {
        let p = Path::new(candidate);
        if p.exists() {
            return p.to_path_buf();
        }
    }

    // Fall back to `which unf`
    if let Ok(output) = Command::new("which").arg("unf").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty()
                && !path.contains("/target/debug/")
                && !path.contains("/target/release/")
            {
                return PathBuf::from(path);
            }
        }
    }

    // Last resort: use the dev path
    current.to_path_buf()
}

// --- Public API ---

/// Installs the auto-start entry for the current platform.
///
/// - macOS: creates LaunchAgent plist and loads it with `launchctl`
/// - Linux: creates systemd user service and enables it
///
/// Resolves the binary path to a stable install location to avoid
/// LaunchAgents pointing at ephemeral dev builds.
///
/// Errors from the OS service manager are logged as warnings, not fatal.
/// This function is idempotent.
pub fn install() -> Result<(), UnfError> {
    let exe_path = std::env::current_exe()
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to get executable path: {}", e)))?;
    let exe_path = resolve_stable_binary_path(&exe_path);

    install_platform(&exe_path)
}

#[cfg(target_os = "macos")]
fn install_platform(exe_path: &std::path::Path) -> Result<(), UnfError> {
    let dir = launchd_dir()?;
    fs::create_dir_all(&dir).map_err(|e| {
        UnfError::InvalidArgument(format!("Failed to create LaunchAgents directory: {}", e))
    })?;

    // Install sentinel plist (KeepAlive)
    let sentinel_path = dir.join(SENTINEL_PLIST_NAME);
    let sentinel_content = format_sentinel_plist(exe_path);
    fs::write(&sentinel_path, &sentinel_content)
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to write sentinel plist: {}", e)))?;

    if service_manager_enabled() {
        let status = Command::new("launchctl")
            .args(["load", "-w"])
            .arg(&sentinel_path)
            .output();
        if let Err(e) = status {
            eprintln!("Warning: Failed to load sentinel launchd agent: {}", e);
        }
    }

    // Migration: remove legacy boot plist if present
    let boot_path = dir.join(LEGACY_BOOT_PLIST_NAME);
    if boot_path.exists() {
        if service_manager_enabled() {
            let _ = Command::new("launchctl")
                .args(["unload", "-w"])
                .arg(&boot_path)
                .output();
        }
        let _ = fs::remove_file(&boot_path);
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn install_platform(exe_path: &std::path::Path) -> Result<(), UnfError> {
    let config_dir = dirs::config_dir().ok_or_else(|| {
        UnfError::InvalidArgument("Cannot determine config directory".to_string())
    })?;
    let service_dir = config_dir.join("systemd/user");
    fs::create_dir_all(&service_dir).map_err(|e| {
        UnfError::InvalidArgument(format!("Failed to create systemd user directory: {}", e))
    })?;

    // Install sentinel service (Restart=always)
    let sentinel_path = service_dir.join(SENTINEL_SERVICE_NAME);
    let sentinel_content = format_sentinel_service(exe_path);
    fs::write(&sentinel_path, &sentinel_content).map_err(|e| {
        UnfError::InvalidArgument(format!("Failed to write sentinel systemd service: {}", e))
    })?;

    if service_manager_enabled() {
        let status = Command::new("systemctl")
            .args(["--user", "enable", SENTINEL_SERVICE_NAME])
            .output();
        if let Err(e) = status {
            eprintln!("Warning: Failed to enable sentinel systemd service: {}", e);
        }
    }

    // Migration: remove legacy boot service if present
    let boot_path = service_dir.join(LEGACY_BOOT_SERVICE_NAME);
    if boot_path.exists() {
        if service_manager_enabled() {
            let _ = Command::new("systemctl")
                .args(["--user", "disable", LEGACY_BOOT_SERVICE_NAME])
                .output();
        }
        let _ = fs::remove_file(&boot_path);
    }

    Ok(())
}

// Fallback for non-macOS/non-Linux (e.g., Windows -- deferred)
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn install_platform(_exe_path: &std::path::Path) -> Result<(), UnfError> {
    eprintln!("Warning: Auto-start is not supported on this platform");
    Ok(())
}

/// Removes the auto-start entry for the current platform.
pub fn remove() -> Result<(), UnfError> {
    remove_platform()
}

#[cfg(target_os = "macos")]
fn remove_platform() -> Result<(), UnfError> {
    let dir = launchd_dir()?;

    // Remove legacy boot plist (backwards compat)
    let boot_path = dir.join(LEGACY_BOOT_PLIST_NAME);
    if boot_path.exists() {
        if service_manager_enabled() {
            let _ = Command::new("launchctl")
                .args(["unload", "-w"])
                .arg(&boot_path)
                .output();
        }
        let _ = fs::remove_file(&boot_path);
    }

    // Remove sentinel plist
    let sentinel_path = dir.join(SENTINEL_PLIST_NAME);
    if sentinel_path.exists() {
        if service_manager_enabled() {
            let _ = Command::new("launchctl")
                .args(["unload", "-w"])
                .arg(&sentinel_path)
                .output();
        }
        fs::remove_file(&sentinel_path).map_err(|e| {
            UnfError::InvalidArgument(format!("Failed to remove sentinel plist: {}", e))
        })?;
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn remove_platform() -> Result<(), UnfError> {
    let config_dir = dirs::config_dir().ok_or_else(|| {
        UnfError::InvalidArgument("Cannot determine config directory".to_string())
    })?;
    let service_dir = config_dir.join("systemd/user");

    // Remove legacy boot service (backwards compat)
    if service_manager_enabled() {
        let _ = Command::new("systemctl")
            .args(["--user", "disable", LEGACY_BOOT_SERVICE_NAME])
            .output();
    }
    let boot_path = service_dir.join(LEGACY_BOOT_SERVICE_NAME);
    if boot_path.exists() {
        let _ = fs::remove_file(&boot_path);
    }

    // Remove sentinel service
    if service_manager_enabled() {
        let _ = Command::new("systemctl")
            .args(["--user", "disable", SENTINEL_SERVICE_NAME])
            .output();
    }
    let sentinel_path = service_dir.join(SENTINEL_SERVICE_NAME);
    if sentinel_path.exists() {
        fs::remove_file(&sentinel_path).map_err(|e| {
            UnfError::InvalidArgument(format!("Failed to remove sentinel service: {}", e))
        })?;
    }

    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn remove_platform() -> Result<(), UnfError> {
    Ok(())
}

/// Returns true if auto-start is currently installed.
pub fn is_installed() -> Result<bool, UnfError> {
    is_installed_platform()
}

#[cfg(target_os = "macos")]
fn is_installed_platform() -> Result<bool, UnfError> {
    // Check if sentinel plist exists
    let sentinel = launchd_dir()?.join(SENTINEL_PLIST_NAME).exists();
    Ok(sentinel)
}

#[cfg(target_os = "linux")]
fn is_installed_platform() -> Result<bool, UnfError> {
    let config_dir = dirs::config_dir().ok_or_else(|| {
        UnfError::InvalidArgument("Cannot determine config directory".to_string())
    })?;
    let service_dir = config_dir.join("systemd/user");
    let file_exists = service_dir.join(SENTINEL_SERVICE_NAME).exists();

    // Cheap gate: `unf status` is a read path. Skip the subprocess entirely
    // on the common negative case (no unit file at all).
    if !file_exists {
        return Ok(false);
    }

    // Under the test harness, do not spawn `systemctl` at all -- that would
    // re-arm the landmine `service_manager_enabled` exists to defuse (see
    // its doc comment). `None` below falls through to "trust the file".
    let systemctl_enabled = if service_manager_enabled() {
        match Command::new("systemctl")
            .args(["--user", "is-enabled", SENTINEL_SERVICE_NAME])
            .output()
        {
            // Exit status is the source of truth: 0 means `systemctl` printed
            // "enabled"; nonzero covers disabled/static/masked and also "no
            // user D-Bus session" -- exactly the false-positive this fixes
            // (`systemctl --user enable` silently failed, but the unit file
            // it wrote first still exists).
            Ok(output) => Some(output.status.success()),
            // `systemctl` itself could not be spawned (e.g. missing from
            // PATH). That is not evidence the unit is disabled -- the file
            // was written by our own install_platform -- so we choose to
            // trust the file rather than manufacture a false negative out of
            // an unrelated PATH problem. Documented choice, not an oversight.
            Err(_) => None,
        }
    } else {
        None
    };

    Ok(is_installed_decision(file_exists, systemctl_enabled))
}

/// Decides whether auto-start is installed from the unit file's existence
/// and, when consulted, whether `systemctl --user is-enabled` exited
/// successfully.
///
/// Pure and platform-independent by construction, so it is unit-tested on
/// any host regardless of whether a live systemd user session -- or even
/// Linux -- is available. `systemctl_enabled` is `None` when the check was
/// skipped (test harness) or could not be performed (spawn failure); both
/// cases fall back to trusting the file, per the documented choice in
/// `is_installed_platform`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn is_installed_decision(file_exists: bool, systemctl_enabled: Option<bool>) -> bool {
    file_exists && systemctl_enabled.unwrap_or(true)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn is_installed_platform() -> Result<bool, UnfError> {
    Ok(false)
}

/// Installs auto-start only if it is not already installed.
///
/// Returns `(installed_now, is_installed_after)`. Pure composition over the
/// existing `install()` / `is_installed()` pair — no new platform logic.
pub fn ensure_installed() -> Result<(bool, bool), UnfError> {
    if is_installed().unwrap_or(false) {
        return Ok((false, true));
    }
    install()?;
    Ok((true, is_installed().unwrap_or(false)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ensure_installed ---

    /// Path that `is_installed()` checks on this platform.
    #[cfg(target_os = "macos")]
    fn sentinel_entry_path() -> PathBuf {
        launchd_dir()
            .expect("launchd dir")
            .join(SENTINEL_PLIST_NAME)
    }

    #[cfg(target_os = "linux")]
    fn sentinel_entry_path() -> PathBuf {
        dirs::config_dir()
            .expect("config dir")
            .join("systemd/user")
            .join(SENTINEL_SERVICE_NAME)
    }

    /// When the entry already exists, `ensure_installed()` must report
    /// `(false, true)` and leave the file byte-for-byte untouched — no plist
    /// rewrite, no `launchctl`/`systemctl` call on the healthy path.
    ///
    /// Holds `ENV_LOCK` because it overrides `HOME`.
    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn ensure_installed_leaves_existing_entry_alone() {
        let _guard = crate::test_util::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp = tempfile::TempDir::new().expect("create temp dir");
        let original_home = std::env::var("HOME").ok();
        let original_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        let original_inert = std::env::var("UNF_AUTOSTART_INERT").ok();
        std::env::set_var("HOME", temp.path());
        std::env::set_var("XDG_CONFIG_HOME", temp.path().join(".config"));
        // Without this, `is_installed()` on Linux also asks
        // `systemctl --user is-enabled`, which reports "not enabled" on any CI
        // runner with no user D-Bus session. The unit file this test writes by
        // hand was never enabled through systemd, so `is_installed()` returns
        // false and `ensure_installed()` reinstalls instead of early-returning.
        // The subject here is the early return, not the service manager.
        std::env::set_var("UNF_AUTOSTART_INERT", "1");

        let entry = sentinel_entry_path();
        fs::create_dir_all(entry.parent().expect("parent dir")).expect("create entry dir");
        fs::write(&entry, "PRE-EXISTING").expect("write entry");

        let result = ensure_installed();

        // Restore env before asserting so a failure cannot leak HOME.
        match original_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match original_xdg {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        match original_inert {
            Some(v) => std::env::set_var("UNF_AUTOSTART_INERT", v),
            None => std::env::remove_var("UNF_AUTOSTART_INERT"),
        }

        assert_eq!(result.expect("ensure_installed"), (false, true));
        assert_eq!(
            fs::read_to_string(&entry).expect("read entry"),
            "PRE-EXISTING",
            "existing entry must not be rewritten"
        );
    }

    /// Under `UNF_AUTOSTART_INERT`, `remove()` must delete the plist file but
    /// must NOT invoke the OS service manager.
    ///
    /// Verified by placing a fake `launchctl` script first on `PATH` that
    /// records invocation to a marker file. This makes the test safe even if
    /// `service_manager_enabled()` were buggy: `PATH` is overridden before
    /// `remove()` runs, so no code path here can ever reach the real
    /// `launchctl` binary — a broken guard would only touch the fake script.
    ///
    /// Holds `ENV_LOCK` because it overrides `HOME`/`PATH`/`UNF_AUTOSTART_INERT`.
    #[test]
    #[cfg(target_os = "macos")]
    fn remove_under_inert_flag_deletes_file_without_subprocess() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = crate::test_util::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp = tempfile::TempDir::new().expect("create temp dir");
        let original_home = std::env::var("HOME").ok();
        let original_path = std::env::var("PATH").ok();
        let original_inert = std::env::var("UNF_AUTOSTART_INERT").ok();

        // Fake `launchctl` on PATH that records invocation to `marker`.
        let bin_dir = temp.path().join("bin");
        fs::create_dir_all(&bin_dir).expect("create fake bin dir");
        let marker = temp.path().join("launchctl-invoked");
        let fake_launchctl = bin_dir.join("launchctl");
        fs::write(
            &fake_launchctl,
            format!("#!/bin/sh\ntouch {}\n", marker.display()),
        )
        .expect("write fake launchctl");
        let mut perms = fs::metadata(&fake_launchctl)
            .expect("stat fake launchctl")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&fake_launchctl, perms).expect("chmod fake launchctl");

        std::env::set_var("HOME", temp.path());
        std::env::set_var(
            "PATH",
            format!(
                "{}:{}",
                bin_dir.display(),
                original_path.clone().unwrap_or_default()
            ),
        );
        std::env::set_var("UNF_AUTOSTART_INERT", "1");

        let sentinel_path = temp
            .path()
            .join("Library/LaunchAgents")
            .join(SENTINEL_PLIST_NAME);
        fs::create_dir_all(sentinel_path.parent().expect("parent dir"))
            .expect("create LaunchAgents dir");
        fs::write(&sentinel_path, "PLIST").expect("write plist");

        let result = remove();

        // Restore env before asserting so a failure cannot leak overrides.
        match original_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match original_path {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
        match original_inert {
            Some(v) => std::env::set_var("UNF_AUTOSTART_INERT", v),
            None => std::env::remove_var("UNF_AUTOSTART_INERT"),
        }

        result.expect("remove");
        assert!(!sentinel_path.exists(), "plist file must be deleted");
        assert!(
            !marker.exists(),
            "launchctl must not be invoked when UNF_AUTOSTART_INERT is set"
        );
    }

    // --- is_installed_decision (pure, runs on every host) ---

    #[test]
    fn is_installed_decision_false_when_file_missing() {
        // Even a confirmed `systemctl` "enabled" cannot outrun a missing file.
        assert!(!is_installed_decision(false, Some(true)));
        assert!(!is_installed_decision(false, None));
        assert!(!is_installed_decision(false, Some(false)));
    }

    #[test]
    fn is_installed_decision_true_when_file_present_and_systemctl_confirms_enabled() {
        assert!(is_installed_decision(true, Some(true)));
    }

    #[test]
    fn is_installed_decision_false_when_file_present_but_systemctl_says_disabled() {
        // This is the bug this ticket fixes: `systemctl --user enable` failed
        // (e.g. no user D-Bus session) but the unit file it wrote still
        // exists. The file alone must no longer be enough to say "installed".
        assert!(!is_installed_decision(true, Some(false)));
    }

    #[test]
    fn is_installed_decision_trusts_file_when_systemctl_not_consulted() {
        // Test harness (UNF_AUTOSTART_INERT) or a spawn failure (systemctl
        // missing from PATH) both report `None` -- trust the file.
        assert!(is_installed_decision(true, None));
    }

    // Sentinel plist tests

    #[test]
    #[cfg(target_os = "macos")]
    fn format_sentinel_plist_contains_label() {
        let plist = format_sentinel_plist(std::path::Path::new("/usr/local/bin/unf"));
        assert!(plist.contains("com.unfudged.sentinel"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn format_sentinel_plist_contains_sentinel_command() {
        let plist = format_sentinel_plist(std::path::Path::new("/usr/local/bin/unf"));
        assert!(plist.contains("__sentinel"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn format_sentinel_plist_has_keep_alive() {
        let plist = format_sentinel_plist(std::path::Path::new("/usr/local/bin/unf"));
        assert!(plist.contains("<key>KeepAlive</key>"));
        assert!(plist.contains("<true/>"));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn format_sentinel_plist_no_run_at_load() {
        let plist = format_sentinel_plist(std::path::Path::new("/usr/local/bin/unf"));
        // Sentinel uses KeepAlive, not RunAtLoad
        assert!(!plist.contains("RunAtLoad"));
    }

    // Sentinel systemd service tests

    #[test]
    fn format_sentinel_service_contains_sentinel_command() {
        let service = format_sentinel_service(std::path::Path::new("/usr/local/bin/unf"));
        assert!(service.contains("/usr/local/bin/unf __sentinel"));
    }

    #[test]
    fn format_sentinel_service_is_simple() {
        let service = format_sentinel_service(std::path::Path::new("/usr/local/bin/unf"));
        assert!(service.contains("Type=simple"));
    }

    #[test]
    fn format_sentinel_service_has_restart_always() {
        let service = format_sentinel_service(std::path::Path::new("/usr/local/bin/unf"));
        assert!(service.contains("Restart=always"));
        assert!(service.contains("RestartSec=10"));
    }

    // Binary path resolution tests

    #[test]
    fn stable_path_passthrough() {
        // Non-dev paths should be returned as-is
        let path = Path::new("/opt/homebrew/bin/unf");
        assert_eq!(resolve_stable_binary_path(path), path);
    }

    #[test]
    fn stable_path_passthrough_usr_local() {
        let path = Path::new("/usr/local/bin/unf");
        assert_eq!(resolve_stable_binary_path(path), path);
    }

    #[test]
    fn dev_path_detected() {
        // Dev paths should trigger the search
        let dev_path = Path::new("/Users/dev/code/unfudged/target/debug/unf");
        let resolved = resolve_stable_binary_path(dev_path);
        // If a stable install exists, it should be used; otherwise falls back to dev path
        let resolved_str = resolved.to_string_lossy();
        // The resolved path should either be a stable path or the original dev path
        assert!(
            !resolved_str.contains("/target/debug/") || resolved == dev_path,
            "Dev path should resolve to stable install or fall back to itself"
        );
    }

    #[test]
    fn release_path_detected() {
        let release_path = Path::new("/Users/dev/code/unfudged/target/release/unf");
        let resolved = resolve_stable_binary_path(release_path);
        let resolved_str = resolved.to_string_lossy();
        assert!(
            !resolved_str.contains("/target/release/") || resolved == release_path,
            "Release path should resolve to stable install or fall back to itself"
        );
    }
}
