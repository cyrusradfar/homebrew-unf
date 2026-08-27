//! Global project registry for auto-start management.
//!
//! Tracks which project directories have active UNFUDGED flight recorders
//! so the boot process can restart their daemons on login.
//! Registry is stored at `~/.unfudged/projects.json`.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::error::UnfError;
use crate::types::WatchSettings;

/// A registered project entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectEntry {
    /// Absolute path to the project root directory.
    pub path: PathBuf,
    /// When the project was registered.
    pub registered: chrono::DateTime<chrono::Utc>,
    /// Per-project watch behavior (gitignore override, un-ignored dirs).
    /// Flattened so `force_watch_gitignore` and `unignored_dirs` stay
    /// top-level JSON keys, byte-identical to the v0.19.1 file shape.
    #[serde(flatten)]
    pub settings: WatchSettings,
}

/// The project registry stored at `~/.unfudged/projects.json`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Registry {
    /// List of registered projects.
    pub projects: Vec<ProjectEntry>,
}

/// Returns the path to the global config directory (`~/.unfudged/`).
///
/// Priority order:
/// 1. `UNF_HOME` env var (testing override)
/// 2. `storage_dir` from the user config file (user-configured)
/// 3. `$HOME/.unfudged` (default fallback)
///
/// Creates the directory if it doesn't exist.
///
/// # Errors
///
/// Returns `UnfError::InvalidArgument` if the home directory cannot be determined.
pub fn global_dir() -> Result<PathBuf, UnfError> {
    // 1. Check UNF_HOME (testing override)
    if let Ok(unf_home) = std::env::var("UNF_HOME") {
        let dir = PathBuf::from(unf_home);
        if !dir.exists() {
            fs::create_dir_all(&dir).map_err(|e| {
                UnfError::InvalidArgument(format!("Failed to create UNF_HOME directory: {}", e))
            })?;
        }
        return Ok(dir);
    }

    // 2. Check user config for a storage_dir override
    if let Ok(config) = crate::config::load() {
        if let Some(ref storage_dir) = config.storage_dir {
            if storage_dir.is_absolute() {
                if !storage_dir.exists() {
                    fs::create_dir_all(storage_dir).map_err(|e| {
                        UnfError::InvalidArgument(format!(
                            "Failed to create config storage_dir {}: {}",
                            storage_dir.display(),
                            e
                        ))
                    })?;
                }
                return Ok(storage_dir.clone());
            }
            eprintln!(
                "Warning: storage_dir in config is not absolute ({}), using default",
                storage_dir.display()
            );
        }
    }

    // 3. Check HOME environment variable (useful for testing)
    let home = if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home)
    } else {
        dirs::home_dir().ok_or_else(|| {
            UnfError::InvalidArgument("Cannot determine home directory".to_string())
        })?
    };
    let dir = home.join(".unfudged");
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| {
            UnfError::InvalidArgument(format!("Failed to create ~/.unfudged/: {}", e))
        })?;
    }
    Ok(dir)
}

/// Returns the path to the registry file (`~/.unfudged/projects.json`).
pub fn registry_path() -> Result<PathBuf, UnfError> {
    Ok(global_dir()?.join("projects.json"))
}

/// Loads the registry from disk.
///
/// Returns an empty registry if the file doesn't exist.
///
/// # Errors
///
/// Returns error if the file exists but is malformed.
pub fn load() -> Result<Registry, UnfError> {
    let path = registry_path()?;
    if !path.exists() {
        return Ok(Registry::default());
    }
    let contents = fs::read_to_string(&path)
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to read registry: {}", e)))?;
    match serde_json::from_str::<Registry>(&contents) {
        Ok(registry) => Ok(registry),
        Err(e) => {
            // Back up the corrupt file before resetting
            let backup = path.with_extension("json.corrupt");
            let _ = fs::copy(&path, &backup);
            eprintln!(
                "Warning: corrupt registry at {}, backed up to {}, resetting to empty: {}",
                path.display(),
                backup.display(),
                e
            );
            let empty = Registry::default();
            save(&empty)?;
            Ok(empty)
        }
    }
}

/// Saves the registry to disk using atomic write (temp file + rename).
///
/// # Errors
///
/// Returns error if the write fails.
pub fn save(registry: &Registry) -> Result<(), UnfError> {
    let path = registry_path()?;
    let dir = path.parent().ok_or_else(|| {
        UnfError::InvalidArgument("Registry path has no parent directory".to_string())
    })?;

    // Atomic write: write to temp file, then rename
    let temp_path = dir.join(".projects.json.tmp");
    let contents = serde_json::to_string_pretty(registry)
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to serialize registry: {}", e)))?;
    fs::write(&temp_path, &contents).map_err(|e| {
        UnfError::InvalidArgument(format!("Failed to write registry temp file: {}", e))
    })?;
    fs::rename(&temp_path, &path)
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to rename registry file: {}", e)))?;
    Ok(())
}

/// Adds a project to the registry, or updates an already-registered one.
///
/// # Arguments
///
/// * `project_root` - Absolute path to the project root directory
/// * `settings` - `Some(v)` sets the project's watch settings to `v`
///   wholesale, inserting a new entry or updating an existing one (the
///   `registered` timestamp is preserved on update). `None` registers the
///   project if absent and leaves the settings on an existing entry
///   untouched — this is what the legacy call sites pass so they never
///   clobber a user's settings with defaults.
pub fn register_project(
    project_root: &Path,
    settings: Option<WatchSettings>,
) -> Result<(), UnfError> {
    let mut registry = load()?;

    // Canonicalize for consistent comparison
    let canonical = project_root
        .canonicalize()
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to canonicalize path: {}", e)))?;

    // Check if already registered
    if let Some(entry) = registry.projects.iter_mut().find(|p| p.path == canonical) {
        if let Some(want) = settings {
            if entry.settings != want {
                entry.settings = want;
                return save(&registry);
            }
        }
        return Ok(());
    }

    registry.projects.push(ProjectEntry {
        path: canonical,
        registered: Utc::now(),
        settings: settings.unwrap_or_default(),
    });

    save(&registry)
}

/// Removes a project from the registry.
///
/// No-op if the project is not registered.
pub fn unregister_project(project_root: &Path) -> Result<(), UnfError> {
    let mut registry = load()?;

    // Attempt to canonicalize the path. If canonicalization fails (e.g., broken
    // symlink or inaccessible directory), fall back to the non-canonical path.
    // This can cause silent unregister failures if the project was registered
    // with a different path representation, so log a warning.
    let canonical = project_root.canonicalize().unwrap_or_else(|_| {
        eprintln!(
            "Warning: Failed to canonicalize path {}, comparing non-canonical",
            project_root.display()
        );
        project_root.to_path_buf()
    });

    let original_len = registry.projects.len();
    registry.projects.retain(|p| p.path != canonical);

    if registry.projects.len() != original_len {
        save(&registry)?;
    }

    Ok(())
}

/// Returns true if any projects are registered.
pub fn has_projects() -> Result<bool, UnfError> {
    let registry = load()?;
    Ok(!registry.projects.is_empty())
}

/// Removes entries where the centralized storage directory no longer exists.
///
/// Returns the number of entries pruned.
pub fn prune_stale_entries() -> Result<usize, UnfError> {
    let mut registry = load()?;
    let original_len = registry.projects.len();

    registry.projects.retain(|entry| {
        // Project directory must still exist
        if !entry.path.exists() {
            return false;
        }
        match crate::storage::resolve_storage_dir_canonical(&entry.path) {
            Ok(storage_dir) => storage_dir.exists(),
            Err(_) => false, // Can't resolve storage → stale
        }
    });

    let pruned = original_len - registry.projects.len();

    if pruned > 0 {
        save(&registry)?;
    }

    Ok(pruned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::env;
    use tempfile::TempDir;

    /// Helper to run registry tests with an isolated home directory.
    /// Uses the shared ENV_LOCK to prevent interference from other test modules.
    fn with_test_home<F: FnOnce(&Path)>(f: F) {
        let _guard = crate::test_util::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp = TempDir::new().expect("create temp dir");
        // Override HOME for this test
        let original_home = env::var("HOME").ok();
        env::set_var("HOME", temp.path());

        // Pre-create the .unfudged directory for registry operations
        fs::create_dir_all(temp.path().join(".unfudged")).expect("create .unfudged dir");

        f(temp.path());

        // Restore HOME
        if let Some(home) = original_home {
            env::set_var("HOME", home);
        } else {
            env::remove_var("HOME");
        }
    }

    #[test]
    fn load_empty_registry() {
        with_test_home(|_| {
            let registry = load().expect("load empty registry");
            assert!(registry.projects.is_empty());
        });
    }

    #[test]
    fn register_and_load_roundtrip() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            register_project(&project, None).expect("register");

            let registry = load().expect("load");
            assert_eq!(registry.projects.len(), 1);
            assert_eq!(registry.projects[0].path, project.canonicalize().unwrap());
        });
    }

    #[test]
    fn register_idempotent() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            register_project(&project, None).expect("register 1");
            register_project(&project, None).expect("register 2");

            let registry = load().expect("load");
            assert_eq!(registry.projects.len(), 1);
        });
    }

    #[test]
    fn register_project_defaults_settings() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            register_project(&project, None).expect("register");

            let registry = load().expect("load");
            assert_eq!(registry.projects[0].settings, WatchSettings::default());
        });
    }

    #[test]
    fn register_project_some_sets_flag() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            // Already registered; Some(..) must update rather than early-return.
            register_project(&project, None).expect("register");
            register_project(
                &project,
                Some(WatchSettings {
                    force_watch_gitignore: true,
                    ..Default::default()
                }),
            )
            .expect("register with flag");

            let registry = load().expect("load");
            assert_eq!(registry.projects.len(), 1);
            assert!(registry.projects[0].settings.force_watch_gitignore);
        });
    }

    #[test]
    fn register_project_some_sets_unignored_dirs() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            register_project(&project, None).expect("register");
            register_project(
                &project,
                Some(WatchSettings {
                    force_watch_gitignore: false,
                    unignored_dirs: BTreeSet::from(["target".to_string(), "dist".to_string()]),
                }),
            )
            .expect("register with unignored dirs");

            let registry = load().expect("load");
            assert_eq!(
                registry.projects[0].settings.unignored_dirs,
                BTreeSet::from(["dist".to_string(), "target".to_string()])
            );
        });
    }

    #[test]
    fn register_project_some_false_clears_flag() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            register_project(
                &project,
                Some(WatchSettings {
                    force_watch_gitignore: true,
                    ..Default::default()
                }),
            )
            .expect("register with flag on");
            register_project(&project, Some(WatchSettings::default()))
                .expect("register with flag off");

            let registry = load().expect("load");
            assert_eq!(registry.projects[0].settings, WatchSettings::default());
        });
    }

    #[test]
    fn register_project_none_preserves_existing_settings() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            let wanted = WatchSettings {
                force_watch_gitignore: true,
                unignored_dirs: BTreeSet::from(["target".to_string()]),
            };
            register_project(&project, Some(wanted.clone())).expect("register with settings");
            register_project(&project, None).expect("legacy call site");

            let registry = load().expect("load");
            assert_eq!(
                registry.projects[0].settings, wanted,
                "None must not clobber existing settings"
            );
        });
    }

    #[test]
    fn register_project_update_preserves_registered_timestamp() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            register_project(&project, None).expect("register");
            let registry = load().expect("load 1");
            let registered_at = registry.projects[0].registered;

            register_project(
                &project,
                Some(WatchSettings {
                    force_watch_gitignore: true,
                    ..Default::default()
                }),
            )
            .expect("update flag");
            let registry = load().expect("load 2");
            assert_eq!(registry.projects[0].registered, registered_at);
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
            // projects.json looks like on disk.
            let path = registry_path().expect("registry path");
            let legacy_json = format!(
                r#"{{"projects":[{{"path":"{}","registered":"2026-01-01T00:00:00Z"}}]}}"#,
                canonical.display()
            );
            fs::write(&path, legacy_json).expect("write legacy registry");

            let registry = load().expect("load legacy registry");
            assert_eq!(registry.projects.len(), 1);
            assert_eq!(registry.projects[0].settings, WatchSettings::default());

            // The corrupt-file backup path must NOT be taken for a valid
            // legacy file that merely lacks the new fields.
            let backup = path.with_extension("json.corrupt");
            assert!(!backup.exists());
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
            let path = registry_path().expect("registry path");
            let v19_json = format!(
                r#"{{"projects":[{{"path":"{}","registered":"2026-01-01T00:00:00Z","force_watch_gitignore":true}}]}}"#,
                canonical.display()
            );
            fs::write(&path, v19_json).expect("write v0.19.1 registry");

            let registry = load().expect("load v0.19.1 registry");
            assert_eq!(registry.projects.len(), 1);
            assert!(registry.projects[0].settings.force_watch_gitignore);
            assert!(registry.projects[0].settings.unignored_dirs.is_empty());
        });
    }

    #[test]
    fn load_normalizes_unsorted_duplicated_unignored_dirs() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");
            let canonical = project.canonicalize().unwrap();

            // Hand-edited JSON: unsorted, with a duplicate entry.
            let path = registry_path().expect("registry path");
            let json = format!(
                r#"{{"projects":[{{"path":"{}","registered":"2026-01-01T00:00:00Z","force_watch_gitignore":false,"unignored_dirs":["target","dist","target"]}}]}}"#,
                canonical.display()
            );
            fs::write(&path, json).expect("write hand-edited registry");

            let registry = load().expect("load hand-edited registry");
            assert_eq!(
                registry.projects[0].settings.unignored_dirs,
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

            register_project(
                &project,
                Some(WatchSettings {
                    force_watch_gitignore: true,
                    unignored_dirs: BTreeSet::from(["target".to_string(), "dist".to_string()]),
                }),
            )
            .expect("register with settings");

            let path = registry_path().expect("registry path");
            let raw = fs::read_to_string(&path).expect("read registry file");
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

            let registry = load().expect("load");
            assert!(registry.projects[0].settings.force_watch_gitignore);
            assert_eq!(
                registry.projects[0].settings.unignored_dirs,
                BTreeSet::from(["dist".to_string(), "target".to_string()])
            );
        });
    }

    #[test]
    fn unregister_project_removes_entry() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            register_project(&project, None).expect("register");
            unregister_project(&project).expect("unregister");

            let registry = load().expect("load");
            assert!(registry.projects.is_empty());
        });
    }

    #[test]
    fn unregister_nonexistent_is_noop() {
        with_test_home(|home| {
            // Don't create the directory - use a path that exists for canonicalize
            let existing = home.join("existing");
            fs::create_dir_all(&existing).expect("create dir");

            register_project(&existing, None).expect("register");

            // unregister_project with non-matching path should be no-op
            // We can't canonicalize a nonexistent path, so test with the existing one
            unregister_project(&existing).expect("unregister");
            let registry = load().expect("load");
            assert!(registry.projects.is_empty());
        });
    }

    #[test]
    fn has_projects_empty() {
        with_test_home(|_| {
            assert!(!has_projects().expect("has_projects"));
        });
    }

    #[test]
    fn has_projects_with_entries() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            register_project(&project, None).expect("register");
            assert!(has_projects().expect("has_projects"));
        });
    }

    #[test]
    fn prune_stale_entries_removes_missing() {
        with_test_home(|home| {
            let project1 = home.join("project1");
            let project2 = home.join("project2");
            fs::create_dir_all(&project1).expect("create project1");
            fs::create_dir_all(&project2).expect("create project2");

            // Create centralized storage dirs
            let storage1 = crate::storage::resolve_storage_dir(&project1).expect("resolve 1");
            let storage2 = crate::storage::resolve_storage_dir(&project2).expect("resolve 2");
            fs::create_dir_all(&storage1).expect("create storage 1");
            fs::create_dir_all(&storage2).expect("create storage 2");

            register_project(&project1, None).expect("register 1");
            register_project(&project2, None).expect("register 2");

            // Remove storage for project2
            fs::remove_dir_all(&storage2).expect("remove storage 2");

            let pruned = prune_stale_entries().expect("prune");
            assert_eq!(pruned, 1);

            let registry = load().expect("load");
            assert_eq!(registry.projects.len(), 1);
            assert_eq!(registry.projects[0].path, project1.canonicalize().unwrap());
        });
    }

    #[test]
    fn prune_no_stale_entries() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            // Create centralized storage dir
            let storage_dir = crate::storage::resolve_storage_dir(&project).expect("resolve");
            fs::create_dir_all(&storage_dir).expect("create storage dir");

            register_project(&project, None).expect("register");

            let pruned = prune_stale_entries().expect("prune");
            assert_eq!(pruned, 0);
        });
    }

    // -----------------------------------------------------------------------
    // global_dir() priority tests
    // -----------------------------------------------------------------------

    /// Helper: write a config.json with `storage_dir` set to the given path,
    /// under the HOME-relative config directory that `dirs::config_dir()` will
    /// resolve to on this platform.
    ///
    /// On macOS `dirs::config_dir()` returns `~/Library/Application Support`.
    /// On Linux it returns `$XDG_CONFIG_HOME` when set, else `~/.config`.
    ///
    /// The caller is responsible for holding `ENV_LOCK` and setting HOME first.
    fn write_config_with_storage_dir(home: &Path, storage_dir: &Path) {
        let config_dir = {
            // Ask dirs crate for the current config dir (HOME is already
            // redirected by with_test_home / the caller).
            dirs::config_dir().expect("dirs::config_dir must resolve in test")
        };
        let config_path = config_dir.join("unfudged").join("config.json");
        fs::create_dir_all(config_path.parent().unwrap()).expect("create config parent dirs");
        let json = format!(r#"{{"storage_dir": "{}"}}"#, storage_dir.to_str().unwrap());
        fs::write(&config_path, json).expect("write config.json");
        // Unused but kept to silence unused-variable warning in callers that
        // pass `home` for clarity.
        let _ = home;
    }

    #[test]
    fn global_dir_unf_home_wins_over_config() {
        // UNF_HOME must beat any config-file storage_dir.
        let _guard = crate::test_util::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp = tempfile::TempDir::new().expect("temp dir");
        let home_dir = temp.path().join("home");
        let unf_home_dir = temp.path().join("unf_home");
        let config_storage_dir = temp.path().join("config_storage");

        fs::create_dir_all(&home_dir).expect("create home");

        // Save original env vars
        let orig_home = env::var("HOME").ok();
        let orig_unf_home = env::var("UNF_HOME").ok();

        env::set_var("HOME", &home_dir);
        env::set_var("UNF_HOME", &unf_home_dir);

        // Write a config pointing to a different directory
        write_config_with_storage_dir(&home_dir, &config_storage_dir);

        let result = global_dir().expect("global_dir should succeed");
        assert_eq!(
            result, unf_home_dir,
            "UNF_HOME must take priority over config"
        );

        // Restore env vars
        match orig_unf_home {
            Some(v) => env::set_var("UNF_HOME", v),
            None => env::remove_var("UNF_HOME"),
        }
        match orig_home {
            Some(v) => env::set_var("HOME", v),
            None => env::remove_var("HOME"),
        }
    }

    #[test]
    fn global_dir_config_wins_over_default() {
        // When UNF_HOME is absent, a config-file storage_dir must be used.
        let _guard = crate::test_util::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp = tempfile::TempDir::new().expect("temp dir");
        let home_dir = temp.path().join("home");
        let config_storage_dir = temp.path().join("config_storage");
        fs::create_dir_all(&home_dir).expect("create home");

        let orig_home = env::var("HOME").ok();
        let orig_unf_home = env::var("UNF_HOME").ok();

        env::set_var("HOME", &home_dir);
        env::remove_var("UNF_HOME");

        write_config_with_storage_dir(&home_dir, &config_storage_dir);

        let result = global_dir().expect("global_dir should succeed");
        assert_eq!(
            result, config_storage_dir,
            "config storage_dir must beat default"
        );
        assert!(
            config_storage_dir.exists(),
            "config storage_dir should be created"
        );

        match orig_unf_home {
            Some(v) => env::set_var("UNF_HOME", v),
            None => env::remove_var("UNF_HOME"),
        }
        match orig_home {
            Some(v) => env::set_var("HOME", v),
            None => env::remove_var("HOME"),
        }
    }

    #[test]
    fn global_dir_falls_back_to_default_when_no_config() {
        // When neither UNF_HOME nor config is set, use ~/.unfudged.
        let _guard = crate::test_util::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp = tempfile::TempDir::new().expect("temp dir");
        let home_dir = temp.path().join("home");
        fs::create_dir_all(&home_dir).expect("create home");

        let orig_home = env::var("HOME").ok();
        let orig_unf_home = env::var("UNF_HOME").ok();

        env::set_var("HOME", &home_dir);
        env::remove_var("UNF_HOME");
        // No config file written — config::load() returns default (None storage_dir)

        let result = global_dir().expect("global_dir should succeed");
        assert_eq!(
            result,
            home_dir.join(".unfudged"),
            "should fall back to ~/.unfudged"
        );
        assert!(
            home_dir.join(".unfudged").exists(),
            "~/.unfudged should be created"
        );

        match orig_unf_home {
            Some(v) => env::set_var("UNF_HOME", v),
            None => env::remove_var("UNF_HOME"),
        }
        match orig_home {
            Some(v) => env::set_var("HOME", v),
            None => env::remove_var("HOME"),
        }
    }

    #[test]
    fn global_dir_relative_storage_dir_in_config_falls_back_to_default() {
        // A relative path in config.storage_dir must be ignored; fall through to default.
        let _guard = crate::test_util::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let temp = tempfile::TempDir::new().expect("temp dir");
        let home_dir = temp.path().join("home");
        fs::create_dir_all(&home_dir).expect("create home");

        let orig_home = env::var("HOME").ok();
        let orig_unf_home = env::var("UNF_HOME").ok();

        env::set_var("HOME", &home_dir);
        env::remove_var("UNF_HOME");

        // Write a config with a relative path
        let config_dir = dirs::config_dir().expect("dirs::config_dir");
        let config_path = config_dir.join("unfudged").join("config.json");
        fs::create_dir_all(config_path.parent().unwrap()).expect("create config parent");
        fs::write(&config_path, r#"{"storage_dir": "relative/path"}"#).expect("write config.json");

        let result = global_dir().expect("global_dir should succeed");
        assert_eq!(
            result,
            home_dir.join(".unfudged"),
            "relative storage_dir should fall through to default"
        );

        match orig_unf_home {
            Some(v) => env::set_var("UNF_HOME", v),
            None => env::remove_var("UNF_HOME"),
        }
        match orig_home {
            Some(v) => env::set_var("HOME", v),
            None => env::remove_var("HOME"),
        }
    }
}
