//! CLI handler for the `unf prune` command.
//!
//! Removes old snapshots and cleans up orphaned CAS objects.
//! Supports both single-project and all-projects modes.
//!
//! Prune is the only command here that destroys history with no undo, so
//! `--all-projects` runs through the confirmation gate in
//! [`gate`] before anything is deleted. Rendering lives in [`render`];
//! this module is orchestration and I/O only.

pub mod gate;
pub mod render;

use std::io::{self, IsTerminal};
use std::path::Path;

use chrono::{DateTime, Utc};

use crate::cli::OutputFormat;
use crate::engine::Engine;
use crate::error::UnfError;
use crate::registry;
use crate::storage;
use gate::PruneGate;
use render::ProjectPrune;

/// JSON output for the prune command.
#[derive(serde::Serialize)]
struct PruneOutput {
    dry_run: bool,
    snapshots_removed: u64,
    objects_removed: u64,
    bytes_freed: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    registry_entries_cleaned: Option<u64>,
}

/// Runs the prune command for a single project or all projects.
pub fn run(
    project_root: &Path,
    older_than: &str,
    dry_run: bool,
    all_projects: bool,
    yes: bool,
    format: OutputFormat,
) -> Result<(), UnfError> {
    // Parse older_than using the existing parse_time_spec
    let cutoff = super::parse_time_spec(older_than)?;

    let gate = gate::decide(
        all_projects,
        yes,
        dry_run,
        io::stdout().is_terminal(),
        format,
    );

    if all_projects {
        run_all_projects(cutoff, gate, format)
    } else {
        // The gate never prompts or refuses for a single project, so this is
        // exactly the old behaviour: preview on --dry-run, delete otherwise.
        run_single_project(project_root, cutoff, gate == PruneGate::DryRunOnly, format)
    }
}

/// Prunes a single project.
fn run_single_project(
    project_root: &Path,
    cutoff: DateTime<Utc>,
    dry_run: bool,
    format: OutputFormat,
) -> Result<(), UnfError> {
    // Resolve storage dir and check if initialized
    let storage_dir = storage::resolve_storage_dir(project_root)?;
    if !storage_dir.exists() {
        return Err(UnfError::NotInitialized);
    }

    // Open engine and run prune
    let engine = Engine::open(project_root, &storage_dir)?;
    let stats = engine.prune(cutoff, dry_run)?;

    // Format and output results
    let output = PruneOutput {
        dry_run,
        snapshots_removed: stats.snapshots_removed,
        objects_removed: stats.objects_removed,
        bytes_freed: stats.bytes_freed,
        registry_entries_cleaned: None,
    };

    if format == OutputFormat::Json {
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else {
        print!("{}", render::human_output(dry_run, cutoff, &stats));
    }

    Ok(())
}

/// Prunes all registered projects, behind the confirmation gate.
///
/// The preview is computed by running every project's prune in dry-run mode,
/// so what the user is shown is produced by the same code path that will do
/// the deleting.
fn run_all_projects(
    cutoff: DateTime<Utc>,
    gate: PruneGate,
    format: OutputFormat,
) -> Result<(), UnfError> {
    match gate {
        PruneGate::RefuseNoTty => return Err(gate::refusal()),
        PruneGate::DryRunOnly => {
            let projects = prune_every_project(cutoff, true)?;
            return report(&projects, cutoff, true, 0, format);
        }
        PruneGate::Prompt => {
            let projects = prune_every_project(cutoff, true)?;
            print!("{}", render::preview(&projects, cutoff));
            gate::confirm()?;
        }
        PruneGate::Proceed => {}
    }

    let projects = prune_every_project(cutoff, false)?;
    let registry_entries_cleaned = registry::prune_stale_entries()? as u64;
    report(&projects, cutoff, false, registry_entries_cleaned, format)
}

/// Prunes every registered project and collects per-project results.
///
/// A project that cannot be opened is warned about and skipped, never fatal:
/// one unreadable registry entry must not stop the rest.
fn prune_every_project(
    cutoff: DateTime<Utc>,
    dry_run: bool,
) -> Result<Vec<ProjectPrune>, UnfError> {
    let registry = registry::load()?;
    Ok(registry
        .projects
        .iter()
        .filter_map(|entry| prune_project(&entry.path, cutoff, dry_run))
        .collect())
}

/// Prunes one project. `None` means "skipped", and any reason was warned about.
fn prune_project(path: &Path, cutoff: DateTime<Utc>, dry_run: bool) -> Option<ProjectPrune> {
    let storage_dir = storage::resolve_storage_dir_canonical(path)
        .map_err(|e| warn_skipped(path, "Could not resolve storage", &e))
        .ok()?;

    // Skip projects that haven't been initialized
    if !storage_dir.exists() {
        return None;
    }

    let engine = Engine::open(path, &storage_dir)
        .map_err(|e| warn_skipped(path, "Could not open engine", &e))
        .ok()?;

    let stats = engine
        .prune(cutoff, dry_run)
        .map_err(|e| warn_skipped(path, "Prune failed", &e))
        .ok()?;

    // On a dry run nothing was deleted, so the live count still includes the
    // snapshots the run only counted.
    let total = engine.get_snapshot_count().unwrap_or(0);
    let snapshots_kept = if dry_run {
        total.saturating_sub(stats.snapshots_removed)
    } else {
        total
    };

    Some(ProjectPrune {
        path: path.to_path_buf(),
        snapshots_kept,
        retained: render::retained_range(
            engine.get_oldest_snapshot_time().ok().flatten(),
            engine.get_newest_snapshot_time().ok().flatten(),
            cutoff,
        ),
        stats,
    })
}

/// Warns that one project was skipped, and why.
fn warn_skipped(path: &Path, what: &str, error: &dyn std::fmt::Display) {
    eprintln!("Warning: {} for {}: {}", what, path.display(), error);
}

/// Emits the all-projects result in the requested format.
fn report(
    projects: &[ProjectPrune],
    cutoff: DateTime<Utc>,
    dry_run: bool,
    registry_entries_cleaned: u64,
    format: OutputFormat,
) -> Result<(), UnfError> {
    let snapshots: u64 = projects.iter().map(|p| p.stats.snapshots_removed).sum();
    let objects: u64 = projects.iter().map(|p| p.stats.objects_removed).sum();
    let bytes: u64 = projects.iter().map(|p| p.stats.bytes_freed).sum();

    if format == OutputFormat::Json {
        let output = PruneOutput {
            dry_run,
            snapshots_removed: snapshots,
            objects_removed: objects,
            bytes_freed: bytes,
            registry_entries_cleaned: Some(registry_entries_cleaned),
        };
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
    } else if dry_run {
        // A preview earns the detail: the user is deciding, not reading a
        // receipt.
        print!("{}", render::preview(projects, cutoff));
        println!("\nNothing was deleted. Re-run with --yes to prune.");
    } else {
        print!(
            "{}",
            render::human_output_with_registry(
                false,
                cutoff,
                snapshots,
                objects,
                bytes,
                registry_entries_cleaned
            )
        );
    }

    Ok(())
}
