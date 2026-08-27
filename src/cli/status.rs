//! The `unf status` command implementation.
//!
//! Shows the current state of the UNFUDGED flight recorder:
//! - Whether recording is active
//! - Snapshot count, tracked files, and store size
//! - Time since recording started

use std::collections::BTreeSet;
use std::path::Path;

use chrono::Utc;

use crate::cli::OutputFormat;
use crate::engine::Engine;
use crate::error::UnfError;
use crate::process::PidFile;
use crate::storage;
use crate::types::WatchSettings;
use crate::watcher::filter;

/// JSON output for the status command.
#[derive(serde::Serialize)]
struct StatusOutput {
    recording: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshots: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    files_tracked: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    store_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    newest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status_mode: Option<u8>,
    auto_restart: bool,
    /// True when this project runs with `--force-watch-gitignore`.
    /// Always serialized so scripts can read it without a presence check.
    force_watch_gitignore: bool,
    /// Excluded directories found in the project root by a shallow scan,
    /// whether or not they are un-ignored. Always serialized as an array,
    /// empty when none, so scripts never treat absence as empty.
    excluded_dirs_present: Vec<String>,
    /// Excluded directories this project records, from `--unignore-dir`.
    /// Always serialized as an array. The still-skipped remainder is
    /// `excluded_dirs_present` minus this list.
    unignored_dirs: Vec<String>,
}

/// Status modes for unwatched directories.
///
/// Mode 0: Never watched — directory has no history in the registry.
/// Mode 1: Previously watched but inactive — directory was registered but daemon isn't active.
/// Mode 2: Actively being watched — daemon is recording changes.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusMode {
    /// Directory has never been watched.
    NeverWatched = 0,
    /// Directory was watched before but is currently inactive.
    PreviouslyWatched = 1,
    /// Directory is currently being watched.
    ActivelyWatching = 2,
}

/// Executes the `unf status` command.
///
/// Handles three modes for unwatched directories:
/// - Mode 0 (Never watched): Directory has no registry entry
/// - Mode 1 (Previously watched): Directory was registered but daemon isn't active
/// - Mode 2 (Actively watching): Daemon is recording changes
///
/// # Arguments
///
/// * `project_root` - The root directory of the project
/// * `format` - Output format (human or JSON)
///
/// # Errors
///
/// Returns an error only if storage path cannot be resolved or database queries fail.
/// Returns `Ok(())` for all unwatched directory modes (shows appropriate message).
pub fn run(project_root: &Path, format: OutputFormat) -> Result<(), UnfError> {
    // Step 1: Resolve storage dir (may or may not exist yet)
    let storage_dir = storage::resolve_storage_dir(project_root)?;

    // Query auto-restart state (used in all output paths)
    let auto_restart = crate::autostart::is_installed().unwrap_or(false);

    // Determine the status mode
    let canonical_project_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());

    // One registry load serves the mode decision and every settings-derived
    // line and field below.
    let facts = lookup_registry_facts(&canonical_project_root);
    let force_watch_gitignore = facts.settings.force_watch_gitignore;
    let unignored_dirs: Vec<String> = facts.settings.unignored_dirs.iter().cloned().collect();

    // One shallow scan of the project root, shared by the JSON field and the
    // human lines. It runs in every mode: a machine consumer must not see an
    // empty list just because the daemon happens to be down. Never fails —
    // an unreadable root yields an empty list.
    let excluded_present = filter::eligible_unignore_dirs(project_root);
    let excluded_dirs_present: Vec<String> =
        excluded_present.iter().map(|d| (*d).to_string()).collect();

    let mode = determine_status_mode(&facts);

    // Step 2: Handle each mode
    match mode {
        StatusMode::NeverWatched => {
            // Mode 0: Directory has never been watched
            let output = StatusOutput {
                recording: false,
                since: None,
                snapshots: None,
                files_tracked: None,
                store_bytes: None,
                newest: None,
                reason: Some("never_watched".to_string()),
                status_mode: Some(0),
                auto_restart,
                force_watch_gitignore,
                excluded_dirs_present,
                unignored_dirs,
            };

            if format == OutputFormat::Json {
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
            } else {
                println!("This directory is not being watched.");
                println!("Run 'unf watch' to start recording changes.");
            }
            return Ok(());
        }

        StatusMode::PreviouslyWatched => {
            // Mode 1: Directory was watched but is currently inactive
            let output = StatusOutput {
                recording: false,
                since: None,
                snapshots: None,
                files_tracked: None,
                store_bytes: None,
                newest: None,
                reason: Some("previously_watched_inactive".to_string()),
                status_mode: Some(1),
                auto_restart,
                force_watch_gitignore,
                excluded_dirs_present,
                unignored_dirs,
            };

            if format == OutputFormat::Json {
                println!("{}", serde_json::to_string_pretty(&output).unwrap());
            } else {
                println!("This directory was previously watched but is not currently active.");
                println!("Run 'unf watch' to resume recording. Your history is safe.");
            }
            return Ok(());
        }

        StatusMode::ActivelyWatching => {
            // Mode 2: Actively watching — continue to stats display
        }
    }

    // Step 3: Open engine and query stats
    let engine = Engine::open(project_root, &storage_dir)?;

    let snapshot_count = engine.get_snapshot_count()?;
    let file_count = engine.get_tracked_file_count()?;
    let store_size = engine.get_store_size()?;

    // Get the time running (from oldest snapshot to now)
    let duration_str = get_duration_string(&engine)?;
    let since_time = get_oldest_snapshot_time(&engine)?;
    let newest_time = get_newest_snapshot_time(&engine)?;

    // Step 4: Build and print output
    let output = StatusOutput {
        recording: true,
        since: since_time,
        snapshots: Some(snapshot_count),
        files_tracked: Some(file_count),
        store_bytes: Some(store_size),
        newest: newest_time,
        reason: None,
        status_mode: Some(2),
        auto_restart,
        force_watch_gitignore,
        excluded_dirs_present,
        unignored_dirs,
    };

    if format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        println!("Watching since {}", duration_str);
        println!("  Snapshots:  {}", format_number(snapshot_count));
        println!("  Files tracked:  {}", format_number(file_count));
        println!("  Store size:  {}", super::format_size(store_size));
        // Mode 2 only, and each line only when it applies. Modes 0 and 1
        // record nothing, so these lines would be noise there.
        for line in unignore_report_lines(&excluded_present, &facts.settings.unignored_dirs) {
            println!("{}", line);
        }
        // Only the non-default state earns a line. Respecting .gitignore is
        // what users expect, so silence means "respected".
        if force_watch_gitignore {
            println!("  Gitignore:  not applied (--force-watch-gitignore)");
        }
        println!(
            "  Auto-restart: {}",
            if auto_restart { "enabled" } else { "disabled" }
        );
    }

    Ok(())
}

/// The un-ignore lines of the human status report, indented and ready to
/// print in order.
///
/// Line 1 names what this project also records. Line 2 names what it still
/// skips, and line 3 is the paste-ready hint for opting one of them in —
/// naming a real directory rather than a `<NAME>` placeholder, so the reader
/// does no substitution.
///
/// Each line appears only when it applies, matching the `Gitignore:`
/// precedent: the default state is the expected state and earns no line. A
/// project with an empty set and no excluded directories in its root — the
/// common case — gets an empty `Vec` and prints nothing.
fn unignore_report_lines(present: &[&str], unignored: &BTreeSet<String>) -> Vec<String> {
    let mut lines = Vec::new();

    if !unignored.is_empty() {
        let names: Vec<&str> = unignored.iter().map(String::as_str).collect();
        lines.push(format!("  Un-ignored:  {}", names.join(", ")));
    }

    let not_recorded = filter::not_recorded_dirs(present, unignored);
    if let Some(first) = not_recorded.first() {
        lines.push(format!("  Not recorded:  {}", not_recorded.join(", ")));
        lines.push(format!("    {}", super::output::unignore_hint(first)));
    }

    lines
}

/// What the global registry says about one project.
///
/// Both facts come from a single `registry::load()` so the status path never
/// reads `projects.json` more than once.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RegistryFacts {
    /// The project has an entry in the registry.
    registered: bool,
    /// The whole per-project watch settings from the entry. Holding the
    /// value type rather than one field per setting keeps this struct from
    /// growing a field every time a new knob lands.
    settings: WatchSettings,
}

/// Looks up a canonicalized project path in the global registry.
///
/// Returns the default (`registered: false`) when the registry cannot be read
/// or the project has no entry.
fn lookup_registry_facts(canonical_project_root: &Path) -> RegistryFacts {
    let Ok(registry) = crate::registry::load() else {
        return RegistryFacts::default();
    };

    match registry
        .projects
        .iter()
        .find(|p| p.path == canonical_project_root)
    {
        Some(entry) => RegistryFacts {
            registered: true,
            settings: entry.settings.clone(),
        },
        None => RegistryFacts::default(),
    }
}

/// Determines the status mode for a directory.
///
/// Mode 0 (NeverWatched): Directory has no registry entry.
/// Mode 1 (PreviouslyWatched): Directory is in the registry but the daemon isn't running.
/// Mode 2 (ActivelyWatching): Daemon is alive and this directory is registered.
fn determine_status_mode(facts: &RegistryFacts) -> StatusMode {
    if !facts.registered {
        return StatusMode::NeverWatched;
    }

    if is_daemon_watching_project(facts) {
        StatusMode::ActivelyWatching
    } else {
        StatusMode::PreviouslyWatched
    }
}

/// Checks if the global daemon is actively watching this project.
///
/// A project is being watched when it has a registry entry and the global
/// daemon process is alive.
///
/// # Arguments
///
/// * `facts` - Registry facts for the project, from [`lookup_registry_facts`]
///
/// # Returns
///
/// `true` if the daemon is alive and this project is registered.
fn is_daemon_watching_project(facts: &RegistryFacts) -> bool {
    if !facts.registered {
        return false;
    }

    let Ok(global_pid_path) = storage::global_pid_path() else {
        return false;
    };
    let pid_file = PidFile::new(global_pid_path);
    let Ok(Some(pid)) = pid_file.read() else {
        return false;
    };

    crate::process::is_alive(pid)
}

use super::output::format_number;

/// Formats the duration since recording started as a human-readable string.
///
/// # Examples
///
/// ```text
/// 30 seconds ago
/// 5 minutes ago
/// 2 hours ago
/// 3 days ago
/// ```
fn format_duration(duration_secs: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * 60;
    const DAY: u64 = 24 * 60 * 60;

    if duration_secs < MINUTE {
        format!("{} seconds ago", duration_secs)
    } else if duration_secs < HOUR {
        let minutes = duration_secs / MINUTE;
        format!("{} {} ago", minutes, plural(minutes, "minute"))
    } else if duration_secs < DAY {
        let hours = duration_secs / HOUR;
        format!("{} {} ago", hours, plural(hours, "hour"))
    } else {
        let days = duration_secs / DAY;
        format!("{} {} ago", days, plural(days, "day"))
    }
}

/// Returns the singular or plural form of a word.
fn plural(count: u64, singular: &str) -> String {
    if count == 1 {
        singular.to_string()
    } else {
        format!("{}s", singular)
    }
}

/// Gets the oldest snapshot time as ISO 8601 string, or None if no snapshots.
fn get_oldest_snapshot_time(engine: &Engine) -> Result<Option<String>, UnfError> {
    match engine.get_oldest_snapshot_time()? {
        Some(oldest_time) => Ok(Some(oldest_time.to_rfc3339())),
        None => Ok(None),
    }
}

/// Gets the newest snapshot time as ISO 8601 string, or None if no snapshots.
fn get_newest_snapshot_time(engine: &Engine) -> Result<Option<String>, UnfError> {
    match engine.get_newest_snapshot_time()? {
        Some(newest_time) => Ok(Some(newest_time.to_rfc3339())),
        None => Ok(None),
    }
}

/// Computes the duration string from the oldest snapshot to now.
///
/// Returns a formatted string like "3 hours ago" if snapshots exist,
/// or "0 seconds ago" if no snapshots exist yet.
fn get_duration_string(engine: &Engine) -> Result<String, UnfError> {
    match engine.get_oldest_snapshot_time()? {
        Some(oldest_time) => {
            let now = Utc::now();
            let duration = now.signed_duration_since(oldest_time);
            let duration_secs = std::cmp::max(0, duration.num_seconds()) as u64;
            Ok(format_duration(duration_secs))
        }
        None => {
            // No snapshots yet, recording just started
            Ok("0 seconds ago".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_duration_seconds() {
        assert_eq!(format_duration(0), "0 seconds ago");
        assert_eq!(format_duration(1), "1 seconds ago");
        assert_eq!(format_duration(30), "30 seconds ago");
        assert_eq!(format_duration(59), "59 seconds ago");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(60), "1 minute ago");
        assert_eq!(format_duration(120), "2 minutes ago");
        assert_eq!(format_duration(300), "5 minutes ago");
    }

    #[test]
    fn format_duration_hours() {
        assert_eq!(format_duration(3600), "1 hour ago");
        assert_eq!(format_duration(7200), "2 hours ago");
        assert_eq!(format_duration(18000), "5 hours ago");
    }

    #[test]
    fn format_duration_days() {
        assert_eq!(format_duration(86400), "1 day ago");
        assert_eq!(format_duration(172800), "2 days ago");
        assert_eq!(format_duration(259200), "3 days ago");
    }

    #[test]
    fn is_pid_alive_with_invalid_pid() {
        // PID 999999 is almost certainly not running
        assert!(!crate::process::is_alive(999999));
    }

    #[test]
    fn plural_singular() {
        assert_eq!(plural(1, "minute"), "minute");
        assert_eq!(plural(1, "hour"), "hour");
        assert_eq!(plural(1, "day"), "day");
    }

    #[test]
    fn plural_multiple() {
        assert_eq!(plural(0, "minute"), "minutes");
        assert_eq!(plural(2, "hour"), "hours");
        assert_eq!(plural(5, "day"), "days");
    }

    #[test]
    fn is_daemon_watching_project_unregistered() {
        // An unregistered project is never "watching", whatever the daemon does.
        assert!(!is_daemon_watching_project(&RegistryFacts::default()));
    }

    #[test]
    fn is_daemon_watching_project_no_global_daemon() {
        // Registered, but no global PID file in an isolated home -> not watching.
        let _guard = crate::test_util::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp = tempfile::TempDir::new().expect("create temp");
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", temp.path());

        let watching = is_daemon_watching_project(&RegistryFacts {
            registered: true,
            settings: WatchSettings::default(),
        });

        if let Some(h) = original_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }

        assert!(!watching);
    }

    #[test]
    fn determine_mode_never_watched() {
        // Test Mode 0: Directory never registered, daemon not watching
        let _guard = crate::test_util::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp = tempfile::TempDir::new().expect("create temp");
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", temp.path());

        let project_dir = temp.path().join("project");
        std::fs::create_dir_all(&project_dir).expect("create project dir");

        let canonical = project_dir.canonicalize().expect("canonicalize");

        let facts = lookup_registry_facts(&canonical);

        // Cleanup
        if let Some(h) = original_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }

        assert!(!facts.registered);
        assert_eq!(determine_status_mode(&facts), StatusMode::NeverWatched);
    }

    #[test]
    fn determine_mode_previously_watched() {
        // Test Mode 1: Directory registered but daemon not watching
        let _guard = crate::test_util::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp = tempfile::TempDir::new().expect("create temp");
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", temp.path());

        // Create project and registry directories
        let project_dir = temp.path().join("project");
        std::fs::create_dir_all(&project_dir).expect("create project dir");
        std::fs::create_dir_all(temp.path().join(".unfudged")).expect("create .unfudged");

        // Register the project
        crate::registry::register_project(&project_dir, None).expect("register project");

        let canonical = project_dir.canonicalize().expect("canonicalize");

        let facts = lookup_registry_facts(&canonical);
        assert!(facts.registered);
        assert_eq!(determine_status_mode(&facts), StatusMode::PreviouslyWatched);

        // Cleanup
        if let Some(h) = original_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }
    }

    #[test]
    fn lookup_registry_facts_reads_force_watch_gitignore() {
        let _guard = crate::test_util::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp = tempfile::TempDir::new().expect("create temp");
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", temp.path());

        let project_dir = temp.path().join("project");
        std::fs::create_dir_all(&project_dir).expect("create project dir");
        crate::registry::register_project(
            &project_dir,
            Some(crate::types::WatchSettings {
                force_watch_gitignore: true,
                ..Default::default()
            }),
        )
        .expect("register project");

        let canonical = project_dir.canonicalize().expect("canonicalize");
        let facts = lookup_registry_facts(&canonical);

        if let Some(h) = original_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }

        assert!(facts.registered);
        assert!(facts.settings.force_watch_gitignore);
    }

    /// Builds a minimal Mode 0 payload for serialization tests.
    fn never_watched_output(force_watch_gitignore: bool) -> StatusOutput {
        StatusOutput {
            recording: false,
            since: None,
            snapshots: None,
            files_tracked: None,
            store_bytes: None,
            newest: None,
            reason: Some("never_watched".to_string()),
            status_mode: Some(0),
            auto_restart: false,
            force_watch_gitignore,
            excluded_dirs_present: Vec::new(),
            unignored_dirs: Vec::new(),
        }
    }

    /// Builds a `BTreeSet` of un-ignored directory names from string slices.
    fn unignored(dirs: &[&str]) -> BTreeSet<String> {
        dirs.iter().map(|d| (*d).to_string()).collect()
    }

    #[test]
    fn json_always_carries_force_watch_gitignore() {
        let off = serde_json::to_value(never_watched_output(false)).expect("serialize");
        assert_eq!(off["force_watch_gitignore"], serde_json::Value::Bool(false));

        let on = serde_json::to_value(never_watched_output(true)).expect("serialize");
        assert_eq!(on["force_watch_gitignore"], serde_json::Value::Bool(true));
    }

    #[test]
    fn json_always_carries_unignore_fields_as_arrays() {
        // Empty, never absent: a script must not have to treat a missing
        // field as an empty list.
        let value = serde_json::to_value(never_watched_output(false)).expect("serialize");

        assert_eq!(value["unignored_dirs"], serde_json::json!([]));
        assert_eq!(value["excluded_dirs_present"], serde_json::json!([]));
    }

    #[test]
    fn json_carries_unignore_fields_when_set() {
        let mut output = never_watched_output(false);
        output.unignored_dirs = vec!["target".to_string()];
        output.excluded_dirs_present = vec!["node_modules".to_string(), "target".to_string()];

        let value = serde_json::to_value(output).expect("serialize");

        assert_eq!(value["unignored_dirs"], serde_json::json!(["target"]));
        assert_eq!(
            value["excluded_dirs_present"],
            serde_json::json!(["node_modules", "target"])
        );
    }

    #[test]
    fn lookup_registry_facts_reads_unignored_dirs() {
        let _guard = crate::test_util::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp = tempfile::TempDir::new().expect("create temp");
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", temp.path());

        let project_dir = temp.path().join("project");
        std::fs::create_dir_all(&project_dir).expect("create project dir");
        crate::registry::register_project(
            &project_dir,
            Some(WatchSettings {
                force_watch_gitignore: false,
                unignored_dirs: unignored(&["dist", "target"]),
            }),
        )
        .expect("register project");

        let canonical = project_dir.canonicalize().expect("canonicalize");
        let facts = lookup_registry_facts(&canonical);

        if let Some(h) = original_home {
            std::env::set_var("HOME", h);
        } else {
            std::env::remove_var("HOME");
        }

        assert!(facts.registered);
        assert_eq!(
            facts.settings.unignored_dirs,
            unignored(&["dist", "target"])
        );
    }

    #[test]
    fn not_recorded_dirs_omits_unignored_ones() {
        let present = ["node_modules", "target", "dist"];

        assert_eq!(
            filter::not_recorded_dirs(&present, &unignored(&["target"])),
            vec!["node_modules", "dist"]
        );
    }

    #[test]
    fn not_recorded_dirs_empty_when_everything_is_unignored() {
        let present = ["target", "dist"];

        assert!(filter::not_recorded_dirs(&present, &unignored(&["target", "dist"])).is_empty());
    }

    #[test]
    fn unignore_report_lines_silent_in_the_default_case() {
        // Nothing opted in and nothing excluded present: silence means the
        // default, exactly like the `Gitignore:` line.
        assert!(unignore_report_lines(&[], &BTreeSet::new()).is_empty());
    }

    #[test]
    fn unignore_report_lines_reports_only_the_unignored_list() {
        // Everything present is already opted in, so no "Not recorded" line
        // and no hint.
        let lines = unignore_report_lines(&["target"], &unignored(&["target"]));

        assert_eq!(lines, vec!["  Un-ignored:  target".to_string()]);
    }

    /// The `Un-ignored:` line carries `.git` paths beside bare names.
    /// Entries sort `.git` first and are `, `-joined, so the slashes inside
    /// an entry never read as the separator between two entries.
    #[test]
    fn unignore_report_lines_render_git_paths_beside_names() {
        let lines = unignore_report_lines(&["target"], &unignored(&["target", ".git/hooks"]));

        assert_eq!(lines[0], "  Un-ignored:  .git/hooks, target");
        // `.git` is never in `present`, so it adds no "Not recorded" line.
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn unignore_report_lines_reports_only_the_remainder() {
        let lines = unignore_report_lines(&["node_modules", "dist"], &BTreeSet::new());

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "  Not recorded:  node_modules, dist");
        assert_eq!(
            lines[1],
            "    Record one with `unf watch --unignore-dir node_modules`. See `unf watch --help`."
        );
    }

    #[test]
    fn unignore_report_lines_full_report_is_ordered() {
        let lines =
            unignore_report_lines(&["node_modules", "target", "dist"], &unignored(&["target"]));

        assert_eq!(
            lines,
            vec![
                "  Un-ignored:  target".to_string(),
                "  Not recorded:  node_modules, dist".to_string(),
                "    Record one with `unf watch --unignore-dir node_modules`. See `unf watch --help`."
                    .to_string(),
            ]
        );
    }

    #[test]
    fn unignore_report_hint_names_a_real_directory() {
        // A `<NAME>` placeholder would make the reader do the substitution.
        let lines = unignore_report_lines(&["dist"], &BTreeSet::new());

        assert!(
            lines[1].contains("--unignore-dir dist"),
            "hint was not paste-ready: {}",
            lines[1]
        );
    }
}
