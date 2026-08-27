//! Intent registry for UNFUDGED.
//!
//! The intent registry is the **source of truth** for what the user wants
//! watched. It is stored at `~/.unfudged/intent.json` and is ONLY modified
//! by `unf watch` (add) and `unf unwatch` (remove). No other code path
//! (daemon, boot, stop, restart, sentinel, prune) may modify it.
//!
//! Uses advisory file locking (`flock`) on every read and write to prevent
//! concurrent CLI processes from racing on the file.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use fs2::FileExt;

use crate::error::UnfError;
use crate::types::WatchSettings;

/// A project the user intends to watch.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IntentEntry {
    /// Absolute (canonical) path to the project root.
    pub path: PathBuf,
    /// When the user ran `unf watch` for this project.
    pub watched_at: chrono::DateTime<chrono::Utc>,
    /// Per-project watch behavior (gitignore override, un-ignored dirs).
    /// Flattened so `force_watch_gitignore` and `unignored_dirs` stay
    /// top-level JSON keys, byte-identical to the v0.19.1 file shape.
    #[serde(flatten)]
    pub settings: WatchSettings,
}

/// The intent registry stored at `~/.unfudged/intent.json`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Intent {
    /// Projects the user intends to watch.
    pub projects: Vec<IntentEntry>,
}

/// Returns the path to the intent file (`~/.unfudged/intent.json`).
pub fn intent_path() -> Result<PathBuf, UnfError> {
    Ok(crate::registry::global_dir()?.join("intent.json"))
}

/// Loads the intent registry from disk, holding a shared (read) lock.
///
/// Returns an empty intent if the file doesn't exist.
pub fn load() -> Result<Intent, UnfError> {
    let path = intent_path()?;
    if !path.exists() {
        return Ok(Intent::default());
    }

    let file = fs::File::open(&path)
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to open intent file: {}", e)))?;
    FileExt::lock_shared(&file)
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to lock intent file: {}", e)))?;

    let contents = fs::read_to_string(&path)
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to read intent file: {}", e)))?;

    // Unlock happens on drop.
    // If the intent file is corrupt, log a warning and reset to empty intent.
    // This is intentional fallback behavior: users can recover by re-running `unf watch`.
    let intent = serde_json::from_str::<Intent>(&contents).unwrap_or_else(|e| {
        eprintln!("Warning: corrupt intent file, resetting to empty: {}", e);
        Intent::default()
    });

    Ok(intent)
}

/// Saves the intent registry to disk using atomic write, holding an exclusive lock.
pub fn save(intent: &Intent) -> Result<(), UnfError> {
    let path = intent_path()?;
    let dir = path.parent().ok_or_else(|| {
        UnfError::InvalidArgument("Intent path has no parent directory".to_string())
    })?;

    // Create directory if needed
    if !dir.exists() {
        fs::create_dir_all(dir).map_err(|e| {
            UnfError::InvalidArgument(format!("Failed to create intent directory: {}", e))
        })?;
    }

    // Open (or create) the intent file for exclusive locking
    let lock_file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|e| {
            UnfError::InvalidArgument(format!("Failed to open intent file for lock: {}", e))
        })?;
    FileExt::lock_exclusive(&lock_file)
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to lock intent file: {}", e)))?;

    // Atomic write: temp file + rename
    let temp_path = dir.join(".intent.json.tmp");
    let contents = serde_json::to_string_pretty(intent)
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to serialize intent: {}", e)))?;
    fs::write(&temp_path, &contents).map_err(|e| {
        UnfError::InvalidArgument(format!("Failed to write intent temp file: {}", e))
    })?;
    fs::rename(&temp_path, &path)
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to rename intent file: {}", e)))?;

    // Unlock happens on drop of lock_file
    Ok(())
}

/// Records that the user wants to watch a project, or updates an
/// already-recorded intent.
///
/// * `settings` - `Some(v)` sets the project's watch settings to `v`
///   wholesale, inserting a new entry or updating an existing one (the
///   `watched_at` timestamp is preserved on update). `None` records intent
///   if absent and leaves the settings on an existing entry untouched.
pub fn add_project(project_root: &Path, settings: Option<WatchSettings>) -> Result<(), UnfError> {
    let canonical = project_root
        .canonicalize()
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to canonicalize path: {}", e)))?;

    let mut intent = load()?;

    if let Some(entry) = intent.projects.iter_mut().find(|p| p.path == canonical) {
        if let Some(want) = settings {
            if entry.settings != want {
                entry.settings = want;
                return save(&intent);
            }
        }
        return Ok(());
    }

    intent.projects.push(IntentEntry {
        path: canonical,
        watched_at: Utc::now(),
        settings: settings.unwrap_or_default(),
    });

    save(&intent)
}

/// Records that the user no longer wants to watch a project. No-op if absent.
pub fn remove_project(project_root: &Path) -> Result<(), UnfError> {
    let canonical = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());

    let mut intent = load()?;
    let original_len = intent.projects.len();
    intent.projects.retain(|p| p.path != canonical);

    if intent.projects.len() != original_len {
        save(&intent)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::env;
    use tempfile::TempDir;

    fn with_test_home<F: FnOnce(&Path)>(f: F) {
        let _guard = crate::test_util::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp = TempDir::new().expect("create temp dir");
        let original = env::var("UNF_HOME").ok();
        env::set_var("UNF_HOME", temp.path());

        f(temp.path());

        if let Some(val) = original {
            env::set_var("UNF_HOME", val);
        } else {
            env::remove_var("UNF_HOME");
        }
    }

    #[test]
    fn load_empty_intent() {
        with_test_home(|_| {
            let intent = load().expect("load empty intent");
            assert!(intent.projects.is_empty());
        });
    }

    #[test]
    fn add_and_load_roundtrip() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            add_project(&project, None).expect("add project");

            let intent = load().expect("load");
            assert_eq!(intent.projects.len(), 1);
            assert_eq!(intent.projects[0].path, project.canonicalize().unwrap());
        });
    }

    #[test]
    fn add_project_is_idempotent() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            add_project(&project, None).expect("add 1");
            add_project(&project, None).expect("add 2");

            let intent = load().expect("load");
            assert_eq!(intent.projects.len(), 1);
        });
    }

    #[test]
    fn add_project_defaults_settings() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            add_project(&project, None).expect("add");

            let intent = load().expect("load");
            assert_eq!(intent.projects[0].settings, WatchSettings::default());
        });
    }

    #[test]
    fn add_project_some_sets_flag() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            // Already recorded; Some(..) must update rather than early-return.
            add_project(&project, None).expect("add");
            add_project(
                &project,
                Some(WatchSettings {
                    force_watch_gitignore: true,
                    ..Default::default()
                }),
            )
            .expect("add with flag");

            let intent = load().expect("load");
            assert_eq!(intent.projects.len(), 1);
            assert!(intent.projects[0].settings.force_watch_gitignore);
        });
    }

    #[test]
    fn add_project_some_sets_unignored_dirs() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            add_project(&project, None).expect("add");
            add_project(
                &project,
                Some(WatchSettings {
                    force_watch_gitignore: false,
                    unignored_dirs: BTreeSet::from(["target".to_string(), "dist".to_string()]),
                }),
            )
            .expect("add with unignored dirs");

            let intent = load().expect("load");
            assert_eq!(
                intent.projects[0].settings.unignored_dirs,
                BTreeSet::from(["dist".to_string(), "target".to_string()])
            );
        });
    }

    #[test]
    fn add_project_some_false_clears_flag() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            add_project(
                &project,
                Some(WatchSettings {
                    force_watch_gitignore: true,
                    ..Default::default()
                }),
            )
            .expect("add with flag on");
            add_project(&project, Some(WatchSettings::default())).expect("add with flag off");

            let intent = load().expect("load");
            assert_eq!(intent.projects[0].settings, WatchSettings::default());
        });
    }

    #[test]
    fn add_project_none_preserves_existing_settings() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            let wanted = WatchSettings {
                force_watch_gitignore: true,
                unignored_dirs: BTreeSet::from(["target".to_string()]),
            };
            add_project(&project, Some(wanted.clone())).expect("add with settings on");
            add_project(&project, None).expect("legacy call site");

            let intent = load().expect("load");
            assert_eq!(
                intent.projects[0].settings, wanted,
                "None must not clobber existing settings"
            );
        });
    }

    #[test]
    fn add_project_update_preserves_watched_at_timestamp() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            add_project(&project, None).expect("add");
            let intent = load().expect("load 1");
            let watched_at = intent.projects[0].watched_at;

            add_project(
                &project,
                Some(WatchSettings {
                    force_watch_gitignore: true,
                    ..Default::default()
                }),
            )
            .expect("update flag");
            let intent = load().expect("load 2");
            assert_eq!(intent.projects[0].watched_at, watched_at);
        });
    }

    #[test]
    fn load_accepts_pre_0_19_json_with_neither_field() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");
            let canonical = project.canonicalize().unwrap();

            // Literal old-format JSON: no `force_watch_gitignore` and no
            // `unignored_dirs` at all. This is what a v0.18-or-earlier
            // intent.json looks like on disk.
            let path = intent_path().expect("intent path");
            let legacy_json = format!(
                r#"{{"projects":[{{"path":"{}","watched_at":"2026-01-01T00:00:00Z"}}]}}"#,
                canonical.display()
            );
            fs::write(&path, legacy_json).expect("write legacy intent");

            let intent = load().expect("load legacy intent");
            assert_eq!(intent.projects.len(), 1);
            assert_eq!(intent.projects[0].settings, WatchSettings::default());
        });
    }

    #[test]
    fn load_accepts_0_19_1_json_with_flag_but_no_unignored_dirs() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");
            let canonical = project.canonicalize().unwrap();

            // Literal v0.19.1-format JSON: has `force_watch_gitignore` but
            // predates `unignored_dirs` entirely.
            let path = intent_path().expect("intent path");
            let v19_json = format!(
                r#"{{"projects":[{{"path":"{}","watched_at":"2026-01-01T00:00:00Z","force_watch_gitignore":true}}]}}"#,
                canonical.display()
            );
            fs::write(&path, v19_json).expect("write v0.19.1 intent");

            let intent = load().expect("load v0.19.1 intent");
            assert_eq!(intent.projects.len(), 1);
            assert!(intent.projects[0].settings.force_watch_gitignore);
            assert!(intent.projects[0].settings.unignored_dirs.is_empty());
        });
    }

    #[test]
    fn load_normalizes_unsorted_duplicated_unignored_dirs() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");
            let canonical = project.canonicalize().unwrap();

            // Hand-edited JSON: unsorted, with a duplicate entry.
            let path = intent_path().expect("intent path");
            let json = format!(
                r#"{{"projects":[{{"path":"{}","watched_at":"2026-01-01T00:00:00Z","force_watch_gitignore":false,"unignored_dirs":["target","dist","target"]}}]}}"#,
                canonical.display()
            );
            fs::write(&path, json).expect("write hand-edited intent");

            let intent = load().expect("load hand-edited intent");
            assert_eq!(
                intent.projects[0].settings.unignored_dirs,
                BTreeSet::from(["dist".to_string(), "target".to_string()]),
                "unsorted, duplicated on-disk list must deserialize to an equal, deduped set"
            );
        });
    }

    #[test]
    fn round_trip_preserves_both_fields_and_json_shape() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            add_project(
                &project,
                Some(WatchSettings {
                    force_watch_gitignore: true,
                    unignored_dirs: BTreeSet::from(["target".to_string(), "dist".to_string()]),
                }),
            )
            .expect("add with settings");

            let path = intent_path().expect("intent path");
            let raw = fs::read_to_string(&path).expect("read intent file");
            let json: serde_json::Value = serde_json::from_str(&raw).expect("parse json");
            let entry = &json["projects"][0];

            // `force_watch_gitignore` and `unignored_dirs` are top-level
            // sibling keys, not nested under a "settings" object — the
            // v0.19.1 shape is unchanged.
            assert!(entry.get("settings").is_none());
            assert_eq!(entry["force_watch_gitignore"], serde_json::json!(true));
            assert_eq!(
                entry["unignored_dirs"],
                serde_json::json!(["dist", "target"])
            );

            let intent = load().expect("load");
            assert!(intent.projects[0].settings.force_watch_gitignore);
            assert_eq!(
                intent.projects[0].settings.unignored_dirs,
                BTreeSet::from(["dist".to_string(), "target".to_string()])
            );
        });
    }

    #[test]
    fn remove_project_removes_entry() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            add_project(&project, None).expect("add");
            remove_project(&project).expect("remove");

            let intent = load().expect("load");
            assert!(intent.projects.is_empty());
        });
    }

    #[test]
    fn remove_nonexistent_is_noop() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            add_project(&project, None).expect("add");

            let other = home.join("other-project");
            fs::create_dir_all(&other).expect("create other dir");
            remove_project(&other).expect("remove nonexistent");

            let intent = load().expect("load");
            assert_eq!(intent.projects.len(), 1);
        });
    }

    #[test]
    fn multiple_projects() {
        with_test_home(|home| {
            let p1 = home.join("project1");
            let p2 = home.join("project2");
            fs::create_dir_all(&p1).expect("create p1");
            fs::create_dir_all(&p2).expect("create p2");

            add_project(&p1, None).expect("add p1");
            add_project(&p2, None).expect("add p2");

            let intent = load().expect("load");
            assert_eq!(intent.projects.len(), 2);

            remove_project(&p1).expect("remove p1");
            let intent = load().expect("load after remove");
            assert_eq!(intent.projects.len(), 1);
            assert_eq!(intent.projects[0].path, p2.canonicalize().unwrap());
        });
    }

    #[test]
    fn save_and_load_preserves_timestamps() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            add_project(&project, None).expect("add");
            let intent1 = load().expect("load 1");
            let ts = intent1.projects[0].watched_at;

            let intent2 = load().expect("load 2");
            assert_eq!(intent2.projects[0].watched_at, ts);
        });
    }
}
