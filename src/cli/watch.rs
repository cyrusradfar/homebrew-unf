//! `unf watch` command implementation.
//!
//! Registers a project for watching and manages the global daemon.
//! The watch command replaces the project-level logic from `unf init`
//! and integrates with the single global daemon architecture.

use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::cli::OutputFormat;
use crate::engine::Engine;
use crate::error::UnfError;
use crate::process::PidFile;
use crate::storage;
use crate::types::WatchSettings;
use crate::watcher::filter::{self, Filter};

/// JSON output for the watch command.
#[derive(serde::Serialize)]
struct WatchOutput {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshots_preserved: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_restart: Option<bool>,
    /// Always serialized. Machine consumers must read the flag directly
    /// instead of treating an absent field as `false`.
    force_watch_gitignore: bool,
    /// Excluded directories this project now records, from `--unignore-dir`.
    /// Holds bare names (`target`) and `.git`-rooted paths (`.git/hooks`)
    /// in the same array, each verbatim as validated. Always serialized as
    /// an array, empty when none, so consumers never have to treat an
    /// absent field as empty.
    unignored_dirs: Vec<String>,
    /// Excluded directories found in the project root by a shallow scan,
    /// whether or not they are un-ignored. Always serialized as an array.
    /// The un-recorded remainder is this list minus `unignored_dirs`.
    excluded_dirs_present: Vec<String>,
}

/// Prints the gitignore-override safety warning to stderr.
///
/// Line 1 uses the shared `warning:` prefix. Lines 2 to 4 use the same
/// two-space hint indent that `print_error` applies to its hint.
fn print_gitignore_override_warning() {
    super::output::print_warning(".gitignore protection is off for this project");
    eprintln!("  UNF records the files that .gitignore excludes, and hidden dotfiles.");
    eprintln!("  Secrets in those files go into the recording. Example: .env.local.");
    eprintln!("  Run `unf watch` with no flag to turn this off.");
}

/// Lines of the "not recorded in this project" note.
///
/// Element 0 carries the `note:` prefix; the rest are indented hint lines.
/// The hint names the first directory so it is paste-ready — a generic
/// `<NAME>` placeholder would make the reader do the substitution.
///
/// Returns an empty `Vec` when there is nothing to report, so the caller
/// prints nothing rather than an empty note.
fn not_recorded_note_lines(not_recorded: &[&str]) -> Vec<String> {
    if not_recorded.is_empty() {
        return Vec::new();
    }

    vec![
        format!("not recorded in this project: {}", not_recorded.join(", ")),
        super::output::unignore_hint(not_recorded[0]),
    ]
}

/// Lines of the `.gitignore`-shadow warning.
///
/// Element 0 carries the `warning:` prefix; the rest are indented hint
/// lines. This is a warning, not a note: the user asked for something that
/// will not happen. `--unignore-dir target` alone records nothing on a
/// typical Rust project, because `.gitignore` drops it one rule later.
///
/// Returns an empty `Vec` when nothing is shadowed.
fn gitignore_shadow_warning_lines(shadowed: &[&str]) -> Vec<String> {
    if shadowed.is_empty() {
        return Vec::new();
    }

    let names = shadowed.join(", ");
    vec![
        format!(".gitignore also excludes {names}, so it is still not recorded"),
        "--unignore-dir lifts UNF's built-in exclusion only.".to_string(),
        format!("Add --force-watch-gitignore as well, or remove {names} from .gitignore."),
    ]
}

/// True when an `unignored_dirs` entry is a `.git`-rooted path rather than
/// a bare directory name.
///
/// Entries are always stored `/`-joined — `parse_unignore_dir` normalises
/// `\` to `/` and strips trailing slashes — and a bare `.git` is never
/// accepted, so a `.git/` prefix is an exact test for the path shape.
///
/// Used to keep `.git` paths out of the `.gitignore`-shadow check. Git
/// never consults `.gitignore` inside `.git`, so naming `.gitignore` as the
/// reason a hook is not recorded points the reader at the wrong file and
/// asks for an edit that should not matter.
///
/// Known gap, and the reason this is suppression at the CLI edge rather
/// than a fix: `Filter::should_track` still applies the `.gitignore`
/// matcher to `.git`-rooted paths, so a rule like `hooks/` does currently
/// stop `.git/hooks` from being recorded. Aligning the filter with git —
/// skipping the `.gitignore` rules for allowlisted `.git` paths — belongs
/// in `watcher::filter`, not here.
fn is_git_path_entry(dir: &str) -> bool {
    dir.starts_with(".git/")
}

/// Un-ignored directories that the project's `.gitignore` also excludes.
///
/// Asks the same matcher the daemon will use, by building one throwaway
/// `Filter`, so the diagnostic cannot drift from rule 3's real behavior.
///
/// `.git`-rooted entries are never reported, whatever the matcher says:
/// git does not apply `.gitignore` inside `.git`, so the warning would name
/// a cause that is not the real one and ask for an edit that changes
/// nothing. See [`is_git_path_entry`].
///
/// Returns an empty `Vec` when `Filter::new` fails — a malformed
/// `.gitignore` must not make `unf watch` fail for the sake of a
/// diagnostic. The daemon reports the parse error itself. Also empty when
/// `force_watch_gitignore` is set, because no matcher is loaded and
/// nothing is shadowed.
fn shadowed_unignored_dirs(project_root: &Path, settings: &WatchSettings) -> Vec<String> {
    if settings.unignored_dirs.is_empty() {
        return Vec::new();
    }

    let Ok(filter) = Filter::new(project_root, settings.clone()) else {
        return Vec::new();
    };

    settings
        .unignored_dirs
        .iter()
        .filter(|dir| !is_git_path_entry(dir))
        .filter(|dir| filter.gitignore_shadows(dir))
        .cloned()
        .collect()
}

/// The one-line confirmation of what this project now also records.
///
/// Entries are `, `-joined in `BTreeSet` order, which sorts `.git` paths
/// ahead of bare names. Comma-space, not comma alone: a `.git/hooks` entry
/// already contains a `/`, so the separator has to stay visually distinct
/// from the characters inside an entry.
///
/// Returns `None` when nothing is un-ignored, so the caller prints nothing
/// rather than a note with an empty list.
fn recording_note_line(unignored: &BTreeSet<String>) -> Option<String> {
    if unignored.is_empty() {
        return None;
    }

    let entries = unignored
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!("recording normally-excluded: {entries}"))
}

/// Prints the human-format un-ignore notices to stderr.
///
/// Two notes, each at most two lines: what is now also being recorded, and
/// what is still being skipped. A flag with no visible effect reads as
/// broken, so the confirmation prints whenever the list is non-empty.
///
/// Callers must skip this under `--json`. On stderr these lines would not
/// corrupt the document, but machine consumers should read the
/// `unignored_dirs` and `excluded_dirs_present` fields, not parse prose.
fn print_unignore_notes(settings: &WatchSettings, excluded_present: &[&str]) {
    if let Some(headline) = recording_note_line(&settings.unignored_dirs) {
        super::output::print_note(&headline);
    }

    let not_recorded = filter::not_recorded_dirs(excluded_present, &settings.unignored_dirs);
    let mut lines = not_recorded_note_lines(&not_recorded).into_iter();
    if let Some(headline) = lines.next() {
        super::output::print_note(&headline);
        for hint in lines {
            eprintln!("  {hint}");
        }
    }
}

/// Prints the `.gitignore`-shadow warning to stderr when it applies.
fn print_gitignore_shadow_warning(project_root: &Path, settings: &WatchSettings) {
    let shadowed = shadowed_unignored_dirs(project_root, settings);
    let shadowed: Vec<&str> = shadowed.iter().map(String::as_str).collect();

    let mut lines = gitignore_shadow_warning_lines(&shadowed).into_iter();
    if let Some(headline) = lines.next() {
        super::output::print_warning(&headline);
        for hint in lines {
            eprintln!("  {hint}");
        }
    }
}

/// Runs the `unf watch` command.
///
/// Registers a project for watching and manages the global daemon.
/// Unlike `init`, this command works with the global daemon architecture.
///
/// # Behavior
///
/// 1. Resolve storage dir for the project
/// 2. Remove stopped sentinel if present (re-activation)
/// 3. Initialize engine if needed (if storage doesn't exist, create it; else open)
/// 4. Register project in global registry
/// 5. Install auto-start capability
/// 6. Check if global daemon is running:
///    - If running: send SIGUSR1 signal to trigger registry reload
///    - If not running: spawn `unf __daemon` and write global PID file
/// 7. Output success message with status
///
/// # Arguments
///
/// * `project_root` - The root directory to watch (typically current directory)
/// * `format` - Output format (human or JSON)
/// * `force_watch_gitignore` - When true, record gitignored files and hidden
///   dotfiles for this project. The value is always written, so a plain
///   `unf watch` turns a previously set override back off.
/// * `unignore_dir` - Excluded directories and `.git` paths to record for
///   this project, from repeated `--unignore-dir NAME` flags. Already
///   validated by clap's value parser, so every value is either an eligible
///   `IGNORED_DIRS` name or an allowlisted `.git`-rooted path such as
///   `.git/hooks`; bare `.git` and everything off the allowlist is rejected
///   before this function runs. Written as a whole set, so a plain
///   `unf watch` clears a previously set list, exactly like
///   `force_watch_gitignore`.
///
/// # Returns
///
/// `Ok(())` on success, or `UnfError` if watch fails.
///
/// # Errors
///
/// - `UnfError::Db` if database operations fail
/// - `UnfError::Cas` if directory creation fails
/// - `UnfError::Watcher` if daemon spawn or signal operations fail
#[allow(clippy::cognitive_complexity)]
// TODO(v0.18): reduce complexity
pub fn run(
    project_root: &Path,
    format: OutputFormat,
    force_watch_gitignore: bool,
    unignore_dir: &[String],
) -> Result<(), UnfError> {
    let storage_dir = storage::resolve_storage_dir(project_root)?;

    // Remove stopped markers (re-activation).
    // Per-project marker:
    let stopped_path = storage::stopped_path(&storage_dir);
    let _ = fs::remove_file(&stopped_path);
    // Global marker (created by `unf stop`; blocks sentinel startup):
    if let Ok(global_stopped) = storage::global_stopped_path() {
        let _ = fs::remove_file(&global_stopped);
    }

    // Initialize engine if needed
    let engine = if storage_dir.exists() {
        Engine::open(project_root, &storage_dir)?
    } else {
        Engine::init(project_root, &storage_dir)?
    };

    // Record user intent (source of truth for what should be watched).
    // The whole settings value is written on every run, so a plain
    // `unf watch` resets both fields — no flag means no override.
    let settings = WatchSettings {
        force_watch_gitignore,
        unignored_dirs: unignore_dir.iter().cloned().collect::<BTreeSet<String>>(),
    };
    if let Err(e) = crate::intent::add_project(project_root, Some(settings.clone())) {
        super::output::print_warning(&format!("Failed to record intent: {}", e));
    }

    // Register project in global registry
    if let Err(e) = crate::registry::register_project(project_root, Some(settings.clone())) {
        super::output::print_warning(&format!("Failed to register project: {}", e));
    }

    // Install auto-start
    let auto_restart = match crate::autostart::ensure_installed() {
        Ok((_installed_now, is_installed)) => is_installed,
        Err(e) => {
            super::output::print_warning(&format!("Failed to install auto-start: {}", e));
            false
        }
    };

    // Check if global daemon is already running
    let global_pid_path = storage::global_pid_path()?;
    let daemon_running = is_global_daemon_running(&global_pid_path);

    if daemon_running {
        // Send SIGUSR1 to trigger registry reload
        let pid_file = PidFile::new(global_pid_path.clone());
        if let Ok(Some(pid)) = pid_file.read() {
            if let Err(e) = crate::process::send_signal(pid, signal_hook::consts::SIGUSR1) {
                super::output::print_warning(&format!("Failed to signal daemon: {}", e));
            }
        }
    } else {
        // Spawn global daemon
        spawn_global_daemon(&global_pid_path)?;
    }

    // Start sentinel watchdog if not running
    if let Err(e) = crate::sentinel::ensure_sentinel_running() {
        super::output::print_warning(&format!("Failed to start sentinel: {}", e));
    }

    // Audit log
    crate::audit::log_event("WATCH", &project_root.display().to_string());

    // Also write per-project PID file for backward compatibility with status
    let per_project_pid = storage::pid_path(&storage_dir);
    let global_pid_file = PidFile::new(global_pid_path.clone());
    if let Ok(Some(pid)) = global_pid_file.read() {
        let _ = fs::write(&per_project_pid, pid.to_string());
    }

    // Get snapshot count to determine status
    let snapshot_count = engine.get_snapshot_count().unwrap_or(0);

    // One shallow scan of the project root, shared by the JSON field and
    // the human note. Never fails: an unreadable root yields an empty list.
    let excluded_present = filter::eligible_unignore_dirs(project_root);

    // Output
    let (status, snapshots_preserved) = if snapshot_count > 0 {
        ("resumed", Some(snapshot_count))
    } else {
        ("started", None)
    };
    let output = WatchOutput {
        status: status.to_string(),
        snapshots_preserved,
        auto_restart: Some(auto_restart),
        force_watch_gitignore,
        unignored_dirs: settings.unignored_dirs.iter().cloned().collect(),
        excluded_dirs_present: excluded_present.iter().map(|d| (*d).to_string()).collect(),
    };

    if format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else if snapshot_count > 0 {
        let subject = format!(
            "{} ({} snapshots preserved)",
            project_root.display(),
            snapshot_count
        );
        super::output::print_status("Watching", &subject);
    } else {
        super::output::print_status("Watching", &project_root.display().to_string());
    }

    // Human-format only: JSON consumers read the fields instead. Notes go
    // to stderr like every other message: stdout carries data only.
    if format != OutputFormat::Json {
        print_unignore_notes(&settings, &excluded_present);
    }

    // Human-format only: JSON consumers read `force_watch_gitignore` instead.
    if force_watch_gitignore && format != OutputFormat::Json {
        print_gitignore_override_warning();
    }

    // stderr: the user asked for a directory that still will not be
    // recorded, so this is an unmet request, not a note.
    if format != OutputFormat::Json {
        print_gitignore_shadow_warning(project_root, &settings);
    }

    Ok(())
}

/// Checks if the global daemon is running.
///
/// Reads the global PID file and checks if the process is alive.
/// Returns false if the PID file doesn't exist or the process is dead.
fn is_global_daemon_running(global_pid_path: &Path) -> bool {
    let pid_file = PidFile::new(global_pid_path.to_path_buf());
    pid_file.is_running()
}

/// Spawns the global daemon process.
///
/// Forks a new process running `unf __daemon` (without --root) and writes
/// its PID to the global PID file at `~/.unfudged/daemon.pid`.
///
/// # Arguments
///
/// * `global_pid_path` - Path to the global daemon PID file
///
/// # Errors
///
/// Returns `UnfError::Watcher` if spawning or writing the PID file fails.
fn spawn_global_daemon(global_pid_path: &Path) -> Result<(), UnfError> {
    let current_exe = env::current_exe().map_err(|e| {
        UnfError::Watcher(crate::error::WatcherError::Io(std::io::Error::other(
            format!("Failed to get current executable path: {}", e),
        )))
    })?;

    let child = Command::new(&current_exe)
        .arg("__daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0) // Detach so daemon survives parent exit
        .spawn()
        .map_err(|e| {
            UnfError::Watcher(crate::error::WatcherError::Io(std::io::Error::other(
                format!("Failed to spawn daemon: {}", e),
            )))
        })?;

    let pid = child.id();
    let pid_file = PidFile::new(global_pid_path.to_path_buf());
    pid_file.write(pid).map_err(|e| {
        UnfError::Watcher(crate::error::WatcherError::Io(std::io::Error::other(
            format!("Failed to write global PID file: {}", e),
        )))
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn is_global_daemon_running_nonexistent_returns_false() {
        let temp = TempDir::new().expect("create temp dir");
        let pid_path = temp.path().join("daemon.pid");
        assert!(!is_global_daemon_running(&pid_path));
    }

    #[test]
    fn is_global_daemon_running_invalid_pid_returns_false() {
        let temp = TempDir::new().expect("create temp dir");
        let pid_path = temp.path().join("daemon.pid");
        fs::write(&pid_path, "invalid").expect("write invalid pid");
        assert!(!is_global_daemon_running(&pid_path));
    }

    /// Serializes a `WatchOutput` to a JSON value for field assertions.
    fn to_json(output: &WatchOutput) -> serde_json::Value {
        serde_json::to_value(output).expect("serialize WatchOutput")
    }

    /// Builds a `WatchSettings` the way `run` does, from raw flag values.
    fn settings_from_flags(force: bool, dirs: &[&str]) -> WatchSettings {
        WatchSettings {
            force_watch_gitignore: force,
            unignored_dirs: dirs.iter().map(|d| (*d).to_string()).collect(),
        }
    }

    #[test]
    fn watch_output_always_serializes_gitignore_flag_when_false() {
        let json = to_json(&WatchOutput {
            status: "started".to_string(),
            snapshots_preserved: None,
            auto_restart: Some(true),
            force_watch_gitignore: false,
            unignored_dirs: Vec::new(),
            excluded_dirs_present: Vec::new(),
        });

        assert_eq!(json["force_watch_gitignore"], serde_json::json!(false));
        // Absent optional fields still collapse; the flag must not.
        assert!(json.get("snapshots_preserved").is_none());
    }

    #[test]
    fn watch_output_serializes_gitignore_flag_when_true() {
        let json = to_json(&WatchOutput {
            status: "resumed".to_string(),
            snapshots_preserved: Some(7),
            auto_restart: Some(false),
            force_watch_gitignore: true,
            unignored_dirs: Vec::new(),
            excluded_dirs_present: Vec::new(),
        });

        assert_eq!(json["force_watch_gitignore"], serde_json::json!(true));
        assert_eq!(json["status"], serde_json::json!("resumed"));
        assert_eq!(json["snapshots_preserved"], serde_json::json!(7));
    }

    #[test]
    fn watch_output_always_serializes_unignore_fields_as_arrays() {
        let json = to_json(&WatchOutput {
            status: "started".to_string(),
            snapshots_preserved: None,
            auto_restart: Some(true),
            force_watch_gitignore: false,
            unignored_dirs: Vec::new(),
            excluded_dirs_present: Vec::new(),
        });

        // Empty, never absent: scripts must not have to treat a missing
        // field as an empty list.
        assert_eq!(json["unignored_dirs"], serde_json::json!([]));
        assert_eq!(json["excluded_dirs_present"], serde_json::json!([]));
    }

    #[test]
    fn watch_output_serializes_unignore_fields_when_set() {
        let json = to_json(&WatchOutput {
            status: "started".to_string(),
            snapshots_preserved: None,
            auto_restart: Some(true),
            force_watch_gitignore: false,
            unignored_dirs: vec!["target".to_string()],
            excluded_dirs_present: vec!["node_modules".to_string(), "target".to_string()],
        });

        assert_eq!(json["unignored_dirs"], serde_json::json!(["target"]));
        assert_eq!(
            json["excluded_dirs_present"],
            serde_json::json!(["node_modules", "target"])
        );
    }

    #[test]
    fn settings_from_flags_sorts_and_dedupes_repeated_values() {
        let settings = settings_from_flags(false, &["target", "dist", "target"]);

        let names: Vec<&str> = settings.unignored_dirs.iter().map(String::as_str).collect();
        assert_eq!(names, vec!["dist", "target"]);
    }

    #[test]
    fn settings_from_flags_without_flag_is_empty() {
        // Plain `unf watch` resets the list, like --force-watch-gitignore.
        let settings = settings_from_flags(false, &[]);
        assert!(settings.unignored_dirs.is_empty());
        assert!(!settings.force_watch_gitignore);
    }

    #[test]
    fn not_recorded_dirs_omits_unignored_ones() {
        let unignored: BTreeSet<String> = ["target".to_string()].into_iter().collect();
        let present = ["node_modules", "target", "dist"];

        assert_eq!(
            filter::not_recorded_dirs(&present, &unignored),
            vec!["node_modules", "dist"]
        );
    }

    #[test]
    fn not_recorded_dirs_empty_when_everything_is_unignored() {
        let unignored: BTreeSet<String> = ["target".to_string(), "dist".to_string()]
            .into_iter()
            .collect();

        assert!(filter::not_recorded_dirs(&["target", "dist"], &unignored).is_empty());
    }

    #[test]
    fn not_recorded_note_lines_hint_names_a_real_directory() {
        let lines = not_recorded_note_lines(&["node_modules", "dist"]);

        assert_eq!(lines[0], "not recorded in this project: node_modules, dist");
        assert_eq!(
            lines[1],
            "Record one with `unf watch --unignore-dir node_modules`. See `unf watch --help`."
        );
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn not_recorded_note_lines_empty_when_nothing_to_report() {
        assert!(not_recorded_note_lines(&[]).is_empty());
    }

    #[test]
    fn recording_note_line_is_none_when_nothing_unignored() {
        assert!(recording_note_line(&BTreeSet::new()).is_none());
    }

    /// A mixed list must stay readable now that entries can contain `/`.
    /// `.git` paths sort first, and the comma-space separator keeps each
    /// entry legible against the slashes inside it.
    #[test]
    fn recording_note_line_joins_names_and_git_paths_readably() {
        let settings = settings_from_flags(false, &["target", ".git/hooks", ".git/info/exclude"]);

        assert_eq!(
            recording_note_line(&settings.unignored_dirs),
            Some("recording normally-excluded: .git/hooks, .git/info/exclude, target".to_string())
        );
    }

    /// The flag round-trips a `.git` path: clap's value parser accepts it,
    /// and the value `run` persists is byte-identical to what was typed.
    /// Both `projects.json` and `intent.json` are written from this one
    /// `WatchSettings`, so one serde round-trip covers both files.
    #[test]
    fn validated_git_paths_round_trip_through_watch_settings() {
        let typed = [".git/hooks", "target"];
        let validated: Vec<String> = typed
            .iter()
            .map(|value| filter::parse_unignore_dir(value).expect("accepted by the value parser"))
            .collect();

        let settings = WatchSettings {
            force_watch_gitignore: false,
            unignored_dirs: validated.iter().cloned().collect::<BTreeSet<String>>(),
        };

        let json = serde_json::to_string(&settings).expect("serialize WatchSettings");
        let restored: WatchSettings = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(restored.unignored_dirs, settings.unignored_dirs);
        assert!(restored.unignored_dirs.contains(".git/hooks"));
        assert!(restored.unignored_dirs.contains("target"));
    }

    /// A refused path fails validation, which aborts the command before
    /// `run` is ever called — so nothing is written. Pinned here because
    /// "writes nothing" is a property of the CLI wiring, not of the
    /// validator alone.
    #[test]
    fn refused_git_paths_never_reach_the_settings_write() {
        for value in [".git", ".git/objects", ".git/refs", ".git/hooks/../objects"] {
            assert!(
                filter::parse_unignore_dir(value).is_err(),
                "'{value}' must never produce a value to persist"
            );
        }
    }

    #[test]
    fn watch_output_carries_git_path_entries_unchanged() {
        let json = to_json(&WatchOutput {
            status: "started".to_string(),
            snapshots_preserved: None,
            auto_restart: Some(true),
            force_watch_gitignore: false,
            unignored_dirs: vec![".git/hooks".to_string(), "target".to_string()],
            excluded_dirs_present: vec!["target".to_string()],
        });

        assert_eq!(
            json["unignored_dirs"],
            serde_json::json!([".git/hooks", "target"])
        );
    }

    #[test]
    fn gitignore_shadow_warning_lines_name_the_second_flag() {
        let lines = gitignore_shadow_warning_lines(&["target"]);

        assert_eq!(
            lines[0],
            ".gitignore also excludes target, so it is still not recorded"
        );
        assert_eq!(
            lines[1],
            "--unignore-dir lifts UNF's built-in exclusion only."
        );
        assert_eq!(
            lines[2],
            "Add --force-watch-gitignore as well, or remove target from .gitignore."
        );
    }

    #[test]
    fn gitignore_shadow_warning_lines_empty_when_nothing_shadowed() {
        assert!(gitignore_shadow_warning_lines(&[]).is_empty());
    }

    #[test]
    fn shadowed_unignored_dirs_reports_gitignored_directory() {
        let temp = TempDir::new().expect("create temp dir");
        fs::write(temp.path().join(".gitignore"), "/target\n").expect("write .gitignore");

        let settings = settings_from_flags(false, &["target", "dist"]);

        assert_eq!(
            shadowed_unignored_dirs(temp.path(), &settings),
            vec!["target".to_string()]
        );
    }

    /// The shadow warning never names a `.git` path. The matcher will
    /// happily match one — a `hooks/` rule matches `.git/hooks` — so this
    /// test pins both halves: the matcher matches, and the diagnostic
    /// stays silent anyway. Without both assertions the suppression could
    /// be deleted as an apparent no-op.
    #[test]
    fn shadowed_unignored_dirs_never_warns_about_git_paths() {
        let temp = TempDir::new().expect("create temp dir");
        fs::write(temp.path().join(".gitignore"), "hooks/\nconfig\n").expect("write .gitignore");

        let settings = settings_from_flags(false, &[".git/hooks", ".git/config"]);

        let filter = Filter::new(temp.path(), settings.clone()).expect("build filter");
        assert!(
            filter.gitignore_shadows(".git/hooks"),
            "precondition: the raw matcher does shadow this path"
        );

        assert!(shadowed_unignored_dirs(temp.path(), &settings).is_empty());
    }

    /// Suppression is per entry, not per invocation: a genuinely shadowed
    /// bare name is still reported when a `.git` path sits beside it.
    #[test]
    fn shadowed_unignored_dirs_still_reports_names_beside_git_paths() {
        let temp = TempDir::new().expect("create temp dir");
        fs::write(temp.path().join(".gitignore"), "/target\nhooks/\n").expect("write .gitignore");

        let settings = settings_from_flags(false, &["target", ".git/hooks"]);

        assert_eq!(
            shadowed_unignored_dirs(temp.path(), &settings),
            vec!["target".to_string()]
        );
    }

    #[test]
    fn is_git_path_entry_matches_only_git_rooted_paths() {
        assert!(is_git_path_entry(".git/hooks"));
        assert!(is_git_path_entry(".git/info/exclude"));

        assert!(!is_git_path_entry("target"));
        // Neighbours that merely start with the same letters. Suppressing
        // these would silence a warning the user can act on.
        assert!(!is_git_path_entry(".github"));
        assert!(!is_git_path_entry(".gitignore"));
        assert!(!is_git_path_entry(".gitmodules"));
    }

    #[test]
    fn shadowed_unignored_dirs_empty_without_gitignore() {
        let temp = TempDir::new().expect("create temp dir");
        let settings = settings_from_flags(false, &["target"]);

        assert!(shadowed_unignored_dirs(temp.path(), &settings).is_empty());
    }

    #[test]
    fn shadowed_unignored_dirs_empty_when_gitignore_is_forced_off() {
        let temp = TempDir::new().expect("create temp dir");
        fs::write(temp.path().join(".gitignore"), "/target\n").expect("write .gitignore");

        // With --force-watch-gitignore the matcher is never loaded, so
        // nothing is shadowed and the warning must stay silent.
        let settings = settings_from_flags(true, &["target"]);

        assert!(shadowed_unignored_dirs(temp.path(), &settings).is_empty());
    }

    #[test]
    fn shadowed_unignored_dirs_empty_when_no_dirs_unignored() {
        let temp = TempDir::new().expect("create temp dir");
        fs::write(temp.path().join(".gitignore"), "/target\n").expect("write .gitignore");

        let settings = settings_from_flags(false, &[]);

        assert!(shadowed_unignored_dirs(temp.path(), &settings).is_empty());
    }

    #[test]
    fn shadowed_unignored_dirs_survives_malformed_gitignore() {
        let temp = TempDir::new().expect("create temp dir");
        // An unparseable pattern must not make `unf watch` fail for a
        // diagnostic; the daemon reports the real parse error.
        fs::write(temp.path().join(".gitignore"), "[\n").expect("write .gitignore");

        let settings = settings_from_flags(false, &["target"]);

        assert!(shadowed_unignored_dirs(temp.path(), &settings).is_empty());
    }
}
