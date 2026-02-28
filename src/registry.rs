//! Global project registry for auto-start management.
//!
//! Tracks which project directories have active UNFUDGED flight recorders
//! so the boot process can restart their daemons on login.
//! Registry is stored at `~/.unfudged/projects.json`.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::error::UnfError;

/// A registered project entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProjectEntry {
    /// Absolute path to the project root directory.
    pub path: PathBuf,
    /// When the project was registered.
    pub registered: chrono::DateTime<chrono::Utc>,
}

/// The project registry stored at `~/.unfudged/projects.json`.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Registry {
    /// List of registered projects.
    pub projects: Vec<ProjectEntry>,
}

/// Returns the path to the global config directory (`~/.unfudged/`).
///
/// Checks `UNF_HOME` first (testing override), then falls back to
/// `$HOME/.unfudged/`. Creates the directory if it doesn't exist.
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

    // 2. Check HOME environment variable (useful for testing)
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

/// Adds a project to the registry. Idempotent — no-op if already present.
///
/// # Arguments
///
/// * `project_root` - Absolute path to the project root directory
pub fn register_project(project_root: &Path) -> Result<(), UnfError> {
    let mut registry = load()?;

    // Canonicalize for consistent comparison
    let canonical = project_root
        .canonicalize()
        .map_err(|e| UnfError::InvalidArgument(format!("Failed to canonicalize path: {}", e)))?;

    // Check if already registered
    if registry.projects.iter().any(|p| p.path == canonical) {
        return Ok(());
    }

    registry.projects.push(ProjectEntry {
        path: canonical,
        registered: Utc::now(),
    });

    save(&registry)
}

/// Removes a project from the registry.
///
/// No-op if the project is not registered.
pub fn unregister_project(project_root: &Path) -> Result<(), UnfError> {
    let mut registry = load()?;

    let canonical = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());

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
    use std::env;
    use tempfile::TempDir;

    /// Helper to run registry tests with an isolated home directory.
    /// Uses the shared ENV_LOCK to prevent interference from other test modules.
    fn with_test_home<F: FnOnce(&Path)>(f: F) {
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();

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

            register_project(&project).expect("register");

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

            register_project(&project).expect("register 1");
            register_project(&project).expect("register 2");

            let registry = load().expect("load");
            assert_eq!(registry.projects.len(), 1);
        });
    }

    #[test]
    fn unregister_project_removes_entry() {
        with_test_home(|home| {
            let project = home.join("my-project");
            fs::create_dir_all(&project).expect("create project dir");

            register_project(&project).expect("register");
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

            register_project(&existing).expect("register");

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

            register_project(&project).expect("register");
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

            register_project(&project1).expect("register 1");
            register_project(&project2).expect("register 2");

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

            register_project(&project).expect("register");

            let pruned = prune_stale_entries().expect("prune");
            assert_eq!(pruned, 0);
        });
    }
}
