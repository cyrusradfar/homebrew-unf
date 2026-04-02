//! UNFUDGED binary entry point.
//!
//! Thin layer for argument parsing and command dispatch. All logic lives
//! in the library modules following the SUPER principle.

use std::env;
use std::path::{Path, PathBuf};
use std::process;

use clap::{Args, Parser, Subcommand};

use unfudged::cli::{self, OutputFormat};
use unfudged::error::{ExitCode, UnfError};
use unfudged::watcher;

/// UNFUDGED - filesystem flight recorder
///
/// Never lose a file change again. UNFUDGED captures every text-based file change
/// in real-time, so you can rewind to any point in time.
#[derive(Parser, Debug)]
#[command(name = "unf")]
#[command(about = "Filesystem flight recorder")]
#[command(long_about = include_str!("help/main.txt"))]
#[command(version)]
struct Cli {
    /// Output in JSON format
    #[arg(long, global = true)]
    json: bool,

    /// Override project directory (default: current directory)
    #[arg(long, global = true, value_name = "PATH")]
    project: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Legacy per-project init (use `watch` instead)
    #[command(hide = true)]
    Init,

    /// Start watching the current directory
    Watch,

    /// Stop watching the current directory
    Unwatch,

    /// Stop the global daemon
    Stop,

    /// Restart the global daemon
    Restart,

    /// Show flight recorder status
    Status,

    /// Stream file change history
    #[command(after_help = include_str!("help/log.txt"))]
    Log(LogArgs),

    /// Show changes since a point in time
    #[command(after_help = include_str!("help/diff.txt"))]
    Diff(DiffArgs),

    /// Restore files to a point in time
    #[command(after_help = include_str!("help/restore.txt"))]
    Restore(RestoreArgs),

    /// Output file content at a point in time
    #[command(after_help = include_str!("help/cat.txt"))]
    Cat(CatArgs),

    /// Show all registered projects on this machine
    #[command(after_help = include_str!("help/list.txt"))]
    List(ListArgs),

    /// Remove old snapshots and clean up storage
    #[command(after_help = include_str!("help/prune.txt"))]
    Prune(PruneArgs),

    /// Show or change storage configuration
    #[command(after_help = include_str!("help/config.txt"))]
    Config(ConfigArgs),

    /// Reconstruct context after a crash or context overflow
    #[command(after_help = include_str!("help/recap.txt"))]
    Recap(RecapArgs),

    /// Hidden daemon subcommand (not shown in help)
    #[command(hide = true)]
    #[command(name = "__daemon")]
    Daemon(DaemonArgs),

    /// Hidden boot subcommand - spawns daemons for all registered projects
    #[command(hide = true)]
    #[command(name = "__boot")]
    Boot,

    /// Hidden sentinel subcommand - watchdog that monitors daemon health
    #[command(hide = true)]
    #[command(name = "__sentinel")]
    Sentinel,
}

#[derive(Args, Debug)]
struct LogArgs {
    /// File or directory to show history for (omit for all)
    target: Option<String>,

    /// Only show changes since this time (e.g., "5m", "1h", "2d")
    #[arg(long)]
    since: Option<String>,

    /// Only show changes until this time (e.g., "1h", ISO 8601 timestamp)
    #[arg(long)]
    until: Option<String>,

    /// Maximum number of entries to return (only effective in JSON mode).
    /// Defaults to 1000, or unlimited when --until is set.
    #[arg(long)]
    limit: Option<u32>,

    /// Resume from a cursor position (format: RFC3339:SnapshotId, JSON mode only)
    #[arg(long)]
    cursor: Option<String>,

    /// Show snapshot density histogram (JSON mode only)
    #[arg(long)]
    density: bool,

    /// Number of time buckets for --density (default: 100)
    #[arg(long, default_value = "100")]
    buckets: u32,

    /// Deprecated: stats are now always shown. This flag is a no-op.
    #[arg(long, short = 's', hide = true)]
    stats: bool,

    /// Only show files matching these glob patterns (repeatable, OR'd)
    #[arg(long)]
    include: Vec<String>,

    /// Exclude files matching these glob patterns (repeatable, OR'd)
    #[arg(long)]
    exclude: Vec<String>,

    /// Case-insensitive glob matching for --include/--exclude
    #[arg(long, short = 'i')]
    ignore_case: bool,

    /// Group output by file path (tree view)
    #[arg(long)]
    group_by_file: bool,

    /// Show changes across all registered projects
    #[arg(long)]
    global: bool,

    /// Only include these projects (repeatable, prefix-matched on canonical path)
    #[arg(long, value_name = "PATH")]
    include_project: Vec<String>,

    /// Exclude these projects (repeatable, prefix-matched on canonical path)
    #[arg(long, value_name = "PATH")]
    exclude_project: Vec<String>,

    /// Detect and list work sessions
    #[arg(long)]
    sessions: bool,
}

#[derive(Args, Debug)]
struct DiffArgs {
    /// Time to compare against (e.g., "5m", "1h", "2d")
    #[arg(long)]
    at: Option<String>,

    /// Start time for two-point diff
    #[arg(long)]
    from: Option<String>,

    /// End time for two-point diff (defaults to now)
    #[arg(long)]
    to: Option<String>,

    /// Diff a specific snapshot against its predecessor
    #[arg(long)]
    snapshot: Option<i64>,

    /// Show diff for a session (defaults to most recent; specify N for session N)
    #[arg(long, num_args = 0..=1, default_missing_value = "0")]
    session: Option<usize>,

    /// Only consider snapshots since this time for session detection (e.g., "2h", "7d")
    #[arg(long)]
    since: Option<String>,

    /// Optional file to limit diff to
    file: Option<String>,

    /// Number of context lines around changes (default: 3)
    #[arg(long, default_value = "3")]
    context: usize,
}

#[derive(Args, Debug)]
struct RestoreArgs {
    /// Time to restore to (e.g., "5m", "1h", "2d")
    #[arg(long)]
    at: Option<String>,

    /// Restore to the start of a session (defaults to most recent; specify N for session N)
    #[arg(long, num_args = 0..=1, default_missing_value = "0")]
    session: Option<usize>,

    /// Only consider snapshots since this time for session detection (e.g., "2h", "7d")
    #[arg(long)]
    since: Option<String>,

    /// Optional file to restore (if not specified, restores all files)
    file: Option<String>,

    /// Perform dry run (show what would be restored without doing it)
    #[arg(long)]
    dry_run: bool,

    /// Skip confirmation prompt (required for non-interactive use)
    #[arg(long, short = 'y')]
    yes: bool,
}

#[derive(Args, Debug)]
struct CatArgs {
    /// File to output
    file: String,

    /// Time to read file at (e.g., "5m", "1h", "2d")
    #[arg(long)]
    at: Option<String>,

    /// Snapshot ID to read
    #[arg(long)]
    snapshot: Option<i64>,
}

#[derive(Args, Debug)]
struct DaemonArgs {
    /// Project root to watch (legacy compatibility; will be registered before daemon starts)
    #[arg(long)]
    root: Option<std::path::PathBuf>,
}

#[derive(Args, Debug)]
struct ListArgs {
    /// Show additional project details (tracked files, recording times)
    #[arg(long, short = 'v')]
    verbose: bool,
}

#[derive(Args, Debug)]
struct PruneArgs {
    /// Time cutoff: keep snapshots newer than this (default: "7d")
    #[arg(long, default_value = "7d")]
    older_than: String,

    /// Show what would be pruned without doing it
    #[arg(long)]
    dry_run: bool,

    /// Prune all registered projects (default: current project only)
    #[arg(long)]
    all_projects: bool,
}

#[derive(Args, Debug)]
struct RecapArgs {
    /// How far back to look for sessions (e.g., "2h", "7d")
    #[arg(long)]
    since: Option<String>,

    /// Show recap across all registered projects
    #[arg(long)]
    global: bool,

    /// Only include these projects (repeatable, prefix-matched on canonical path)
    #[arg(long, value_name = "PATH")]
    include_project: Vec<String>,

    /// Exclude these projects (repeatable, prefix-matched on canonical path)
    #[arg(long, value_name = "PATH")]
    exclude_project: Vec<String>,
}

#[derive(Args, Debug)]
struct ConfigArgs {
    /// Move storage to a new location (use "default" for ~/.unfudged)
    #[arg(long, value_name = "PATH")]
    move_storage: Option<PathBuf>,

    /// Overwrite destination if it already contains data
    #[arg(long, short)]
    force: bool,
}

/// Resolves the project root directory.
///
/// Uses `--project` if provided, otherwise falls back to the current working
/// directory. If the resolved path isn't itself a registered project, walks
/// up the directory tree to find a registered ancestor (so commands work
/// from subdirectories of watched projects).
fn resolve_project_root(cli_project: Option<&Path>) -> Result<PathBuf, UnfError> {
    let path = match cli_project {
        Some(p) => resolve_project_path(p)?,
        None => env::current_dir().map_err(|e| {
            UnfError::InvalidArgument(format!("Failed to get current directory: {}", e))
        })?,
    };

    // Canonicalize the path, but fall back to the original if the path contains
    // broken symlinks or the filesystem is inaccessible. This is intentional: we
    // allow commands like `unf log --project /path` to work even when the directory
    // is temporarily unreachable, as long as it's registered in the project registry.
    let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());

    // Check if this path (or an ancestor) is a registered project
    if let Ok(registry) = unfudged::registry::load() {
        // Direct match — return immediately
        if registry.projects.iter().any(|p| p.path == canonical) {
            return Ok(canonical);
        }

        // Walk up to find a registered ancestor (deepest match wins since
        // we start from the most specific path)
        let mut ancestor = canonical.parent();
        while let Some(dir) = ancestor {
            if registry.projects.iter().any(|p| p.path == dir) {
                return Ok(dir.to_path_buf());
            }
            ancestor = dir.parent();
        }
    }

    // No registered ancestor found — return original path
    // (allows `unf watch` to register new projects)
    Ok(canonical)
}

/// Resolves a user-provided project path.
///
/// If the path exists on disk, canonicalizes it. If it does not exist (e.g. the
/// directory was deleted), falls back to looking up the path in the global
/// project registry so that commands like `unf log --project /old/path` still
/// work against orphaned projects.
fn resolve_project_path(path: &Path) -> Result<PathBuf, UnfError> {
    // If path exists on disk, canonicalize normally
    if path.exists() {
        return path
            .canonicalize()
            .map_err(|e| UnfError::InvalidArgument(format!("Failed to resolve path: {}", e)));
    }

    // Path doesn't exist - check registry for orphaned project
    let registry = unfudged::registry::load()?;

    // Convert to absolute path for comparison
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map_err(|e| UnfError::InvalidArgument(format!("Failed to get cwd: {}", e)))?
            .join(path)
    };

    for entry in &registry.projects {
        if entry.path == absolute {
            return Ok(entry.path.clone());
        }
    }

    Err(UnfError::InvalidArgument(format!(
        "Project not found: {}",
        path.display()
    )))
}

#[allow(clippy::cognitive_complexity)]
// TODO(v0.18): reduce complexity
fn main() {
    // Reset SIGPIPE to default behavior so piping to `head`, `jq`, etc.
    // doesn't cause a panic with "Broken pipe".
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    let cli = Cli::parse();
    let format = if cli.json {
        OutputFormat::Json
    } else {
        OutputFormat::Human
    };

    let result = match cli.command {
        Commands::Init => resolve_project_root(cli.project.as_deref())
            .and_then(|root| cli::init::run(&root, format)),

        Commands::Watch => resolve_project_root(cli.project.as_deref())
            .and_then(|root| cli::watch::run(&root, format)),

        Commands::Unwatch => resolve_project_root(cli.project.as_deref())
            .and_then(|root| cli::unwatch::run(&root, format)),

        Commands::Stop => resolve_project_root(cli.project.as_deref())
            .and_then(|root| cli::stop::run(&root, format)),

        Commands::Restart => resolve_project_root(cli.project.as_deref())
            .and_then(|root| cli::restart::run(&root, format)),

        Commands::Status => resolve_project_root(cli.project.as_deref())
            .and_then(|root| cli::status::run(&root, format)),

        Commands::Log(args) => {
            // Validate --sessions mutual exclusivity
            if args.sessions {
                if args.density {
                    Err(UnfError::InvalidArgument(
                        "--sessions cannot be combined with --density".to_string(),
                    ))
                } else if args.group_by_file {
                    Err(UnfError::InvalidArgument(
                        "--sessions cannot be combined with --group-by-file".to_string(),
                    ))
                } else if args.cursor.is_some() {
                    Err(UnfError::InvalidArgument(
                        "--sessions cannot be combined with --cursor".to_string(),
                    ))
                } else if args.global {
                    cli::log::run_global_sessions(
                        &args.include_project,
                        &args.exclude_project,
                        args.since.as_deref(),
                        &args.include,
                        &args.exclude,
                        args.ignore_case,
                        format,
                    )
                } else if !args.include_project.is_empty() || !args.exclude_project.is_empty() {
                    Err(UnfError::InvalidArgument(
                        "--include-project/--exclude-project require --global".to_string(),
                    ))
                } else {
                    resolve_project_root(cli.project.as_deref()).and_then(|root| {
                        cli::log::run_sessions(
                            &root,
                            args.since.as_deref(),
                            &args.include,
                            &args.exclude,
                            args.ignore_case,
                            format,
                        )
                    })
                }
            } else if args.global {
                if args.target.is_some() {
                    Err(UnfError::InvalidArgument(
                        "--global cannot be combined with a file/directory target".to_string(),
                    ))
                } else if args.cursor.is_some() {
                    Err(UnfError::InvalidArgument(
                        "--global cannot be combined with --cursor".to_string(),
                    ))
                } else if args.density {
                    let params = cli::log::GlobalDensityParams::new(
                        args.include_project.clone(),
                        args.exclude_project.clone(),
                        args.since.clone(),
                        args.include.clone(),
                        args.exclude.clone(),
                        args.ignore_case,
                        args.buckets,
                    );
                    cli::log::run_global_density(&params)
                } else {
                    let limit =
                        args.limit
                            .unwrap_or(if args.until.is_some() { u32::MAX } else { 1000 });
                    let params = cli::log::GlobalLogParams {
                        include_project: args.include_project.clone(),
                        exclude_project: args.exclude_project.clone(),
                        since: args.since.clone(),
                        until: args.until.clone(),
                        limit,
                        include: args.include.clone(),
                        exclude: args.exclude.clone(),
                        ignore_case: args.ignore_case,
                        grouped: args.group_by_file,
                        format,
                    };
                    cli::log::run_global(&params)
                }
            } else if !args.include_project.is_empty() || !args.exclude_project.is_empty() {
                Err(UnfError::InvalidArgument(
                    "--include-project/--exclude-project require --global".to_string(),
                ))
            } else {
                resolve_project_root(cli.project.as_deref()).and_then(|root| {
                    let limit =
                        args.limit
                            .unwrap_or(if args.until.is_some() { u32::MAX } else { 1000 });
                    let params = cli::log::LogParams {
                        target: args.target.clone(),
                        since: args.since.clone(),
                        until: args.until.clone(),
                        limit,
                        include: args.include.clone(),
                        exclude: args.exclude.clone(),
                        ignore_case: args.ignore_case,
                        grouped: args.group_by_file,
                        format,
                        density: args.density,
                        num_buckets: args.buckets,
                        cursor_str: args.cursor.clone(),
                    };
                    cli::log::run(&root, &params)
                })
            }
        }

        Commands::Diff(args) => resolve_project_root(cli.project.as_deref()).and_then(|root| {
            if let Some(session_num) = args.session {
                // Validate mutual exclusivity
                if args.at.is_some() {
                    return Err(UnfError::InvalidArgument(
                        "--session cannot be combined with --at".to_string(),
                    ));
                }
                if args.from.is_some() || args.to.is_some() {
                    return Err(UnfError::InvalidArgument(
                        "--session cannot be combined with --from/--to".to_string(),
                    ));
                }
                if args.snapshot.is_some() {
                    return Err(UnfError::InvalidArgument(
                        "--session cannot be combined with --snapshot".to_string(),
                    ));
                }
                cli::diff::run_session(
                    &root,
                    session_num,
                    args.since.as_deref(),
                    args.file.as_deref(),
                    format,
                    args.context,
                )
            } else {
                cli::diff::run(
                    &root,
                    args.at.as_deref(),
                    args.from.as_deref(),
                    args.to.as_deref(),
                    args.snapshot,
                    args.file.as_deref(),
                    format,
                    args.context,
                )
            }
        }),

        Commands::Restore(args) => resolve_project_root(cli.project.as_deref()).and_then(|root| {
            if let Some(session_num) = args.session {
                if args.at.is_some() {
                    return Err(UnfError::InvalidArgument(
                        "--session cannot be combined with --at".to_string(),
                    ));
                }
                cli::restore::run_session(
                    &root,
                    session_num,
                    args.since.as_deref(),
                    args.file.as_deref(),
                    args.dry_run,
                    args.yes,
                    format,
                )
            } else if let Some(ref at) = args.at {
                cli::restore::run(
                    &root,
                    at,
                    args.file.as_deref(),
                    args.dry_run,
                    args.yes,
                    format,
                )
            } else {
                Err(UnfError::InvalidArgument(
                    "Either --at or --session is required.".to_string(),
                ))
            }
        }),

        Commands::Cat(args) => resolve_project_root(cli.project.as_deref()).and_then(|root| {
            cli::cat::run(&root, &args.file, args.at.as_deref(), args.snapshot, format)
        }),

        Commands::List(args) => cli::list::run(format, args.verbose),

        Commands::Prune(args) => resolve_project_root(cli.project.as_deref()).and_then(|root| {
            cli::prune::run(
                &root,
                &args.older_than,
                args.dry_run,
                args.all_projects,
                format,
            )
        }),

        Commands::Config(args) => {
            if let Some(ref dest) = args.move_storage {
                cli::config::run_move_storage(&dest.to_string_lossy(), args.force, format)
            } else {
                cli::config::run(format)
            }
        }

        Commands::Recap(args) => {
            if args.global {
                cli::recap::run_global(
                    &args.include_project,
                    &args.exclude_project,
                    args.since.as_deref(),
                    format,
                )
            } else if !args.include_project.is_empty() || !args.exclude_project.is_empty() {
                Err(UnfError::InvalidArgument(
                    "--include-project/--exclude-project require --global".to_string(),
                ))
            } else {
                resolve_project_root(cli.project.as_deref())
                    .and_then(|root| cli::recap::run(&root, args.since.as_deref(), format))
            }
        }

        Commands::Daemon(args) => {
            // Hidden daemon subcommand - runs the global multi-project watcher loop
            // If --root is provided (legacy init flow), register the project first
            if let Some(ref root) = args.root {
                if let Err(e) = unfudged::registry::register_project(root) {
                    eprintln!("Warning: Failed to register project: {}", e);
                }
            }
            watcher::run_daemon()
        }

        Commands::Boot => {
            // Hidden boot subcommand - spawns daemons for all registered projects
            cli::boot::run()
        }

        Commands::Sentinel => {
            // Hidden sentinel subcommand - watchdog for daemon health and intent reconciliation
            unfudged::sentinel::run_sentinel()
        }
    };

    if let Err(e) = result {
        match &e {
            UnfError::NoResults(msg) => {
                if format == OutputFormat::Json {
                    // In JSON mode, NoResults exit code 4 without additional output
                    process::exit(ExitCode::from(&e) as i32);
                } else if !msg.is_empty() {
                    eprintln!("{}", msg);
                }
                process::exit(ExitCode::from(&e) as i32);
            }
            UnfError::NotInitialized => {
                if format == OutputFormat::Json {
                    let code = ExitCode::from(&e) as i32;
                    let err_json = serde_json::json!({
                        "error": e.to_string(),
                        "code": code,
                    });
                    eprintln!("{}", err_json);
                } else {
                    cli::output::print_error("not watching", Some("Run `unf watch` to start."));
                }
                process::exit(ExitCode::from(&e) as i32);
            }
            _ => {
                if format == OutputFormat::Json {
                    let code = ExitCode::from(&e) as i32;
                    let err_json = serde_json::json!({
                        "error": e.to_string(),
                        "code": code,
                    });
                    eprintln!("{}", err_json);
                } else {
                    cli::output::print_error(&e.to_string(), None);
                }
                process::exit(ExitCode::from(&e) as i32);
            }
        }
    }
}
