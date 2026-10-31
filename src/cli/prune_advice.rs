//! The prune advisory shown by human `unf status`.
//!
//! Two independent signals: the total size of every registered store, and any
//! project that has gone 60 days without a prune. [`evaluate`] is pure and
//! table-tested; [`collect`] is the read-only I/O half; [`human_lines`] is the
//! only place the sentences exist, so the renderer never duplicates copy.

use std::path::Path;

use chrono::{DateTime, Utc};

use crate::engine::facts;
use crate::storage;

/// Total store size above which the advisory suggests a prune. Strictly
/// greater: exactly 500 MiB is silent.
pub const SIZE_THRESHOLD_BYTES: u64 = 500 * 1024 * 1024;

/// Days without a prune that make a project overdue. Inclusive.
pub const STALE_DAYS: i64 = 60;

/// Retention the advisory recommends. The command's own default is 7 days,
/// but the product documents 30 days as the manual pruning policy. Naming the
/// cutoff matters: prune keeps newer history, so the reported total is not
/// what a prune would reclaim.
pub const ADVISED_RETENTION_DAYS: i64 = 30;

const SECONDS_PER_DAY: i64 = 86_400;

/// How loudly the advisory speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    /// The store is large but every project is maintained.
    Suggest,
    /// At least one project has gone [`STALE_DAYS`] without a prune.
    Strong,
}

/// What one registered project contributes to the advisory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectUsage {
    /// Bytes in this project's CAS object store.
    pub store_bytes: u64,
    /// When this project was last pruned for real.
    pub last_prune_at: Option<DateTime<Utc>>,
    /// Timestamp of the oldest snapshot, or `None` when empty.
    pub oldest_snapshot_at: Option<DateTime<Utc>>,
    /// Whether this is the project the user is standing in.
    pub is_current: bool,
}

/// The decision, carrying only what the renderer needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PruneAdvice {
    pub urgency: Urgency,
    pub total_store_bytes: u64,
    /// Projects with a non-zero store, so "across 4 projects" is honest.
    pub projects_measured: usize,
    pub overdue_projects: u64,
    /// Age of the least-recently maintained project, in whole days.
    pub oldest_overdue_days: u64,
    /// This project's age when it is one of the overdue ones.
    pub current_project_overdue_days: Option<u64>,
}

/// Applies the signal matrix to the collected usages.
///
/// Returns `None` when neither signal fires: silence is the default and
/// means "nothing to do".
pub fn evaluate(usages: &[ProjectUsage], now: DateTime<Utc>) -> Option<PruneAdvice> {
    if usages.is_empty() {
        return None;
    }

    let total_store_bytes: u64 = usages.iter().map(|usage| usage.store_bytes).sum();
    let projects_measured = usages.iter().filter(|usage| usage.store_bytes > 0).count();

    let mut overdue_projects = 0u64;
    let mut oldest_overdue_days = 0u64;
    let mut current_project_overdue_days = None;

    for usage in usages {
        let Some(days) = overdue_days(usage, now) else {
            continue;
        };
        overdue_projects += 1;
        oldest_overdue_days = oldest_overdue_days.max(days);
        if usage.is_current {
            current_project_overdue_days = Some(days);
        }
    }

    let urgency = if overdue_projects > 0 {
        Urgency::Strong
    } else if total_store_bytes > SIZE_THRESHOLD_BYTES {
        Urgency::Suggest
    } else {
        return None;
    };

    Some(PruneAdvice {
        urgency,
        total_store_bytes,
        projects_measured,
        overdue_projects,
        oldest_overdue_days,
        current_project_overdue_days,
    })
}

/// Days since the project was last maintained, or `None` when it is not
/// overdue. A project with no snapshots is never overdue.
///
/// `last_prune_at` wins over the oldest snapshot: a user who pruned with
/// `--older-than 30d` chose to keep old history and is not overdue.
fn overdue_days(usage: &ProjectUsage, now: DateTime<Utc>) -> Option<u64> {
    let oldest_snapshot_at = usage.oldest_snapshot_at?;
    let last_maintained = usage.last_prune_at.unwrap_or(oldest_snapshot_at);

    // A future timestamp (clock skew) clamps to zero, never negative.
    let age_seconds = now
        .signed_duration_since(last_maintained)
        .num_seconds()
        .max(0);

    if age_seconds >= STALE_DAYS * SECONDS_PER_DAY {
        Some((age_seconds / SECONDS_PER_DAY) as u64)
    } else {
        None
    }
}

/// Renders the advisory as indented, ready-to-print lines.
pub fn human_lines(advice: &PruneAdvice) -> Vec<String> {
    match advice.urgency {
        Urgency::Suggest => suggest_lines(advice),
        Urgency::Strong => strong_lines(advice),
    }
}

fn suggest_lines(advice: &PruneAdvice) -> Vec<String> {
    vec![
        format!(
            "  Prune:  {} stored across {}.",
            crate::cli::format_size(advice.total_store_bytes),
            project_count(advice.projects_measured)
        ),
        format!(
            "    Run `{}` to delete versions older than {} days.",
            retention_command(),
            ADVISED_RETENTION_DAYS
        ),
    ]
}

fn strong_lines(advice: &PruneAdvice) -> Vec<String> {
    let mut action = String::new();
    if advice.total_store_bytes > 0 {
        action.push_str(&format!(
            "{} stored across {}. ",
            crate::cli::format_size(advice.total_store_bytes),
            project_count(advice.projects_measured)
        ));
    }
    action.push_str(&format!(
        "Run `{}` to delete versions older than {} days.",
        retention_command(),
        ADVISED_RETENTION_DAYS
    ));

    vec![
        format!("  Warning:  {}", overdue_sentence(advice)),
        format!("    {}", action),
        "    Preview with `--dry-run`.".to_string(),
    ]
}

fn overdue_sentence(advice: &PruneAdvice) -> String {
    match advice.current_project_overdue_days {
        Some(days) => format!("This project has gone {} days without a prune.", days),
        None => format!(
            "{} gone {}+ days without a prune (oldest: {} days).",
            project_subject(advice.overdue_projects),
            STALE_DAYS,
            advice.oldest_overdue_days
        ),
    }
}

/// The command the advisory recommends, with the retention cutoff spelled out.
fn retention_command() -> String {
    format!(
        "unf prune --all-projects --older-than {}d",
        ADVISED_RETENTION_DAYS
    )
}

fn project_count(count: usize) -> String {
    if count == 1 {
        "1 project".to_string()
    } else {
        format!("{} projects", count)
    }
}

fn project_subject(count: u64) -> String {
    if count == 1 {
        "1 other project has".to_string()
    } else {
        format!("{} other projects have", count)
    }
}

/// Collects usage for every registered project, read-only.
///
/// Never returns `Result`: `unf status` must succeed even when a sibling
/// project is broken. A project whose store cannot be inspected contributes
/// neither bytes nor staleness. Registry order is preserved.
pub fn collect(registry: &crate::registry::Registry, current_project: &Path) -> Vec<ProjectUsage> {
    let mut usages = Vec::new();

    for entry in &registry.projects {
        let Ok(storage_dir) = storage::resolve_storage_dir_canonical(&entry.path) else {
            continue;
        };

        // A missing database means the project has no store to measure.
        if !storage::db_path(&storage_dir).is_file() {
            continue;
        }

        let Ok(Some(facts)) = facts::read_storage_facts(&storage_dir) else {
            continue;
        };

        // A failed walk under-reports rather than dropping the staleness
        // signal we already read successfully.
        let store_bytes = facts::dir_size(&storage::objects_path(&storage_dir)).unwrap_or_default();

        usages.push(ProjectUsage {
            store_bytes,
            last_prune_at: facts.last_prune_at,
            oldest_snapshot_at: facts.oldest_snapshot_at,
            is_current: entry.path == current_project,
        });
    }

    usages
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    use crate::registry::{ProjectEntry, Registry};
    use crate::types::WatchSettings;

    /// A fixed clock keeps every boundary assertion exact.
    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 30, 12, 0, 0).unwrap()
    }

    fn days_ago(days: i64) -> DateTime<Utc> {
        now() - chrono::Duration::days(days)
    }

    fn usage(
        store_bytes: u64,
        last_prune_at: Option<DateTime<Utc>>,
        oldest_snapshot_at: Option<DateTime<Utc>>,
        is_current: bool,
    ) -> ProjectUsage {
        ProjectUsage {
            store_bytes,
            last_prune_at,
            oldest_snapshot_at,
            is_current,
        }
    }

    fn advice(
        urgency: Urgency,
        total_store_bytes: u64,
        projects_measured: usize,
        overdue_projects: u64,
        oldest_overdue_days: u64,
        current_project_overdue_days: Option<u64>,
    ) -> PruneAdvice {
        PruneAdvice {
            urgency,
            total_store_bytes,
            projects_measured,
            overdue_projects,
            oldest_overdue_days,
            current_project_overdue_days,
        }
    }

    // --- Thresholds ---

    #[test]
    fn constants_are_five_hundred_mebibytes_and_sixty_days() {
        // Hardcoded on purpose: comparing a constant to itself proves nothing.
        assert_eq!(SIZE_THRESHOLD_BYTES, 500 * 1024 * 1024);
        assert_eq!(STALE_DAYS, 60);
        assert_eq!(ADVISED_RETENTION_DAYS, 30);
    }

    #[test]
    fn size_threshold_is_strict() {
        let at_threshold = vec![usage(SIZE_THRESHOLD_BYTES, None, None, false)];
        assert_eq!(evaluate(&at_threshold, now()), None);

        let above = vec![usage(SIZE_THRESHOLD_BYTES + 1, None, None, false)];
        let advice = evaluate(&above, now()).expect("just over the threshold suggests");
        assert_eq!(advice.urgency, Urgency::Suggest);
    }

    #[test]
    fn sixty_day_boundary_is_inclusive() {
        let just_under = vec![usage(
            1,
            None,
            Some(now() - chrono::Duration::days(60) + chrono::Duration::seconds(1)),
            false,
        )];
        assert_eq!(evaluate(&just_under, now()), None);

        let exactly = vec![usage(1, None, Some(days_ago(60)), false)];
        let advice = evaluate(&exactly, now()).expect("exactly 60 days is overdue");
        assert_eq!(advice.urgency, Urgency::Strong);
        assert_eq!(advice.oldest_overdue_days, 60);
    }

    // --- The staleness rule ---

    #[test]
    fn never_pruned_falls_back_to_the_oldest_snapshot() {
        let usages = vec![usage(1, None, Some(days_ago(94)), false)];
        let advice = evaluate(&usages, now()).expect("stale store warns");
        assert_eq!(advice.urgency, Urgency::Strong);
        assert_eq!(advice.oldest_overdue_days, 94);
    }

    #[test]
    fn a_recent_prune_outranks_an_old_snapshot() {
        let usages = vec![usage(1, Some(days_ago(10)), Some(days_ago(300)), false)];
        assert_eq!(
            evaluate(&usages, now()),
            None,
            "the user chose to keep old history; that is not neglect"
        );
    }

    #[test]
    fn a_project_with_no_snapshots_is_never_overdue() {
        let usages = vec![usage(1, Some(days_ago(100)), None, false)];
        assert_eq!(evaluate(&usages, now()), None);
    }

    #[test]
    fn one_overdue_project_of_three_is_counted() {
        let usages = vec![
            usage(1, None, Some(days_ago(5)), false),
            usage(1, None, Some(days_ago(61)), false),
            usage(1, None, Some(days_ago(5)), false),
        ];
        let advice = evaluate(&usages, now()).expect("one stale project warns");
        assert_eq!(advice.overdue_projects, 1);
        assert_eq!(advice.oldest_overdue_days, 61);
    }

    #[test]
    fn two_overdue_projects_report_the_count_and_the_oldest_age() {
        let usages = vec![
            usage(1, None, Some(days_ago(70)), false),
            usage(1, None, Some(days_ago(94)), false),
            usage(1, None, Some(days_ago(5)), false),
        ];
        let advice = evaluate(&usages, now()).expect("two stale projects warn");
        assert_eq!(advice.overdue_projects, 2);
        assert_eq!(advice.oldest_overdue_days, 94);
    }

    #[test]
    fn a_future_timestamp_clamps_to_zero() {
        let usages = vec![usage(
            1,
            Some(now() + chrono::Duration::days(1)),
            Some(days_ago(100)),
            false,
        )];
        assert_eq!(
            evaluate(&usages, now()),
            None,
            "clock skew must not invent a duration or a warning"
        );
    }

    // --- The matrix ---

    #[test]
    fn empty_input_is_silent() {
        assert_eq!(evaluate(&[], now()), None);
    }

    #[test]
    fn small_maintained_store_is_silent() {
        let usages = vec![usage(1, Some(days_ago(1)), Some(days_ago(2)), true)];
        assert_eq!(evaluate(&usages, now()), None);
    }

    #[test]
    fn both_signals_are_strong_and_carry_the_total() {
        let usages = vec![
            usage(SIZE_THRESHOLD_BYTES + 1, None, Some(days_ago(61)), true),
            usage(10, Some(days_ago(1)), Some(days_ago(2)), false),
        ];
        let advice = evaluate(&usages, now()).expect("both signals fire");
        assert_eq!(advice.urgency, Urgency::Strong);
        assert_eq!(advice.total_store_bytes, SIZE_THRESHOLD_BYTES + 11);
        assert_eq!(advice.projects_measured, 2);
        assert_eq!(advice.current_project_overdue_days, Some(61));
    }

    #[test]
    fn projects_measured_excludes_zero_byte_stores() {
        let usages = vec![
            usage(0, None, None, false),
            usage(SIZE_THRESHOLD_BYTES + 1, None, None, false),
        ];
        let advice = evaluate(&usages, now()).expect("the large store fires");
        assert_eq!(advice.projects_measured, 1);
    }

    // --- Copy ---

    #[test]
    fn suggest_copy_names_the_command_but_not_dry_run() {
        let lines = human_lines(&advice(
            Urgency::Suggest,
            SIZE_THRESHOLD_BYTES + 1,
            4,
            0,
            0,
            None,
        ));

        assert!(lines[0].starts_with("  Prune:  "), "{:?}", lines);
        assert!(lines[0].contains("across 4 projects"), "{:?}", lines);
        assert!(
            lines
                .iter()
                .any(|line| line.contains("unf prune --all-projects")),
            "{:?}",
            lines
        );
        assert!(
            !lines.iter().any(|line| line.contains("--dry-run")),
            "a routine suggestion does not need the preview: {:?}",
            lines
        );
    }

    #[test]
    fn strong_copy_names_the_command_and_dry_run() {
        let lines = human_lines(&advice(Urgency::Strong, 612 * 1024 * 1024, 4, 2, 94, None));

        assert!(lines[0].starts_with("  Warning:  "), "{:?}", lines);
        assert!(lines[0].contains("2 other projects have"), "{:?}", lines);
        assert!(lines[0].contains("60+ days"), "{:?}", lines);
        assert!(lines[0].contains("oldest: 94 days"), "{:?}", lines);
        assert!(
            lines
                .iter()
                .any(|line| line.contains("unf prune --all-projects")),
            "{:?}",
            lines
        );
        assert!(
            lines.iter().any(|line| line.contains("--dry-run")),
            "strong advice must mention the preview: {:?}",
            lines
        );
    }

    #[test]
    fn strong_copy_pluralises_one_other_project() {
        let lines = human_lines(&advice(Urgency::Strong, 1, 1, 1, 61, None));
        assert!(lines[0].contains("1 other project has"), "{:?}", lines);
        assert!(!lines[0].contains("projects have"), "{:?}", lines);
    }

    #[test]
    fn current_project_branch_names_this_project() {
        let lines = human_lines(&advice(Urgency::Strong, 1, 1, 1, 94, Some(94)));
        assert!(
            lines[0].contains("This project has gone 94 days"),
            "{:?}",
            lines
        );
        assert!(!lines[0].contains("other project"), "{:?}", lines);
    }

    #[test]
    fn zero_total_omits_the_byte_sentence() {
        let lines = human_lines(&advice(Urgency::Strong, 0, 0, 1, 94, None));
        assert!(
            !lines.iter().any(|line| line.contains("0 B")),
            "0 B stored is noise: {:?}",
            lines
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("delete versions older than 30 days")),
            "{:?}",
            lines
        );
    }

    // --- collect (I/O half) ---

    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    /// Restores `HOME` and `UNF_HOME` on drop, so a failed assert cannot leak
    /// the environment into the next test.
    struct EnvGuard {
        home: Option<String>,
        unf_home: Option<String>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn new(root: &Path) -> Self {
            let lock = crate::test_util::ENV_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let home = std::env::var("HOME").ok();
            let unf_home = std::env::var("UNF_HOME").ok();
            std::env::set_var("HOME", root);
            std::env::set_var("UNF_HOME", root.join("unf"));
            Self {
                home,
                unf_home,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            restore_env("HOME", &self.home);
            restore_env("UNF_HOME", &self.unf_home);
        }
    }

    fn restore_env(key: &str, value: &Option<String>) {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    fn project_dir(root: &Path, name: &str) -> PathBuf {
        let dir = root.join(name);
        fs::create_dir_all(&dir).expect("create project dir");
        dir.canonicalize().expect("canonicalize project")
    }

    /// Creates a project with a real v3 store and returns its storage dir.
    fn init_store(project: &Path) -> PathBuf {
        let storage_dir =
            storage::resolve_storage_dir_canonical(project).expect("resolve storage dir");
        crate::engine::Engine::init(project, &storage_dir).expect("init store");
        storage_dir
    }

    fn write_objects(storage_dir: &Path, sizes: &[usize]) {
        let objects = storage::objects_path(storage_dir);
        for (index, size) in sizes.iter().enumerate() {
            fs::write(objects.join(format!("object-{index}")), vec![b'x'; *size])
                .expect("write object");
        }
    }

    fn registry(projects: &[&Path]) -> Registry {
        Registry {
            projects: projects
                .iter()
                .map(|path| ProjectEntry {
                    path: (*path).to_path_buf(),
                    registered: Utc::now(),
                    settings: WatchSettings::default(),
                })
                .collect(),
        }
    }

    #[test]
    fn collect_sums_object_sizes_in_registry_order() {
        let temp = TempDir::new().expect("create temp dir");
        let _env = EnvGuard::new(temp.path());
        let root = temp.path().join("projects");

        let first = project_dir(&root, "first");
        let second = project_dir(&root, "second");
        write_objects(&init_store(&first), &[100]);
        write_objects(&init_store(&second), &[200]);

        let usages = collect(&registry(&[&first, &second]), &first);

        assert_eq!(usages.len(), 2);
        assert_eq!(usages[0].store_bytes, 100);
        assert_eq!(usages[1].store_bytes, 200);
        assert_eq!(
            usages.iter().filter(|usage| usage.is_current).count(),
            1,
            "exactly one entry is the project the user is in"
        );
        assert!(usages[0].is_current);
    }

    #[test]
    fn collect_skips_a_missing_storage_dir() {
        let temp = TempDir::new().expect("create temp dir");
        let _env = EnvGuard::new(temp.path());
        let root = temp.path().join("projects");

        let present = project_dir(&root, "present");
        write_objects(&init_store(&present), &[10]);
        let missing = project_dir(&root, "missing"); // registered, never watched

        let usages = collect(&registry(&[&present, &missing]), &present);

        assert_eq!(usages.len(), 1);
        assert_eq!(usages[0].store_bytes, 10);
    }

    #[test]
    fn collect_skips_a_corrupt_sibling_entirely() {
        let temp = TempDir::new().expect("create temp dir");
        let _env = EnvGuard::new(temp.path());
        let root = temp.path().join("projects");

        let broken = project_dir(&root, "broken");
        let storage_dir = init_store(&broken);
        write_objects(&storage_dir, &[500]);
        fs::write(storage::db_path(&storage_dir), b"not a database").expect("corrupt the database");

        let usages = collect(&registry(&[&broken]), &broken);

        assert!(
            usages.is_empty(),
            "a store we cannot inspect contributes neither bytes nor staleness"
        );
    }

    #[cfg(unix)]
    #[test]
    fn collect_keeps_staleness_when_the_size_walk_fails() {
        // Root ignores directory permissions, so this failure cannot be built.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }

        use std::os::unix::fs::PermissionsExt;

        let temp = TempDir::new().expect("create temp dir");
        let _env = EnvGuard::new(temp.path());
        let root = temp.path().join("projects");

        let project = project_dir(&root, "unreadable-objects");
        let storage_dir = init_store(&project);
        {
            let conn =
                rusqlite::Connection::open(storage::db_path(&storage_dir)).expect("open database");
            crate::engine::db::meta::set_meta(
                &conn,
                crate::engine::db::meta::LAST_PRUNE_AT,
                &days_ago(90).to_rfc3339(),
            )
            .expect("record an old prune");
        }

        let objects = storage::objects_path(&storage_dir);
        fs::set_permissions(&objects, fs::Permissions::from_mode(0o000))
            .expect("make objects unreadable");

        let usages = collect(&registry(&[&project]), &project);

        // Restore before asserting so TempDir cleanup can run either way.
        fs::set_permissions(&objects, fs::Permissions::from_mode(0o755))
            .expect("restore objects permissions");

        assert_eq!(usages.len(), 1);
        assert_eq!(
            usages[0].store_bytes, 0,
            "a failed walk under-reports rather than guessing"
        );
        assert!(
            usages[0].last_prune_at.is_some(),
            "the staleness signal we already read survives the failed walk"
        );
    }
}
