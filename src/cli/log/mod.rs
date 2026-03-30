//! Implementation of `unf log` command.
//!
//! Streams file change history with cursor-based pagination. Supports filtering
//! by file, directory, or all changes. Supports interactive pagination when
//! connected to a TTY.

mod display;
mod filters;
mod format;
mod session;
mod tests;

use std::io::IsTerminal;
use std::path::Path;

use crate::cli::filter::GlobFilter;
use crate::cli::OutputFormat;
use crate::engine::db::{HistoryCursor, HistoryScope};
use crate::engine::Engine;
use crate::error::UnfError;
use crate::storage;
use crate::types::Snapshot;

/// Parameters for the `unf log` command.
///
/// Bundles common parameters used throughout the log module to reduce
/// parameter passing and improve maintainability.
#[derive(Debug, Clone)]
pub struct LogParams {
    /// Optional file or directory path to filter by
    pub target: Option<String>,
    /// Optional time specification (e.g., "5m", "1h", "2d")
    pub since: Option<String>,
    /// Maximum entries to return (only used in JSON mode)
    pub limit: u32,
    /// Glob patterns to include (repeatable, OR'd)
    pub include: Vec<String>,
    /// Glob patterns to exclude (repeatable, OR'd)
    pub exclude: Vec<String>,
    /// Case-insensitive glob matching
    pub ignore_case: bool,
    /// Group output by file path (tree view)
    pub grouped: bool,
    /// Output format (human or JSON)
    pub format: OutputFormat,
    /// Return density histogram instead of entries (JSON only)
    pub density: bool,
    /// Number of buckets for density histogram
    pub num_buckets: u32,
    /// Optional cursor string for pagination (JSON only)
    pub cursor_str: Option<String>,
}

impl Default for LogParams {
    fn default() -> Self {
        LogParams {
            target: None,
            since: None,
            limit: 1000,
            include: Vec::new(),
            exclude: Vec::new(),
            ignore_case: false,
            grouped: false,
            format: OutputFormat::Human,
            density: false,
            num_buckets: 100,
            cursor_str: None,
        }
    }
}

/// Parameters for the global (cross-project) `unf log` command.
///
/// Bundles parameters used in global log operations.
#[derive(Debug, Clone)]
pub struct GlobalLogParams {
    /// Project paths to include (prefix-matched on canonical path)
    pub include_project: Vec<String>,
    /// Project paths to exclude (prefix-matched on canonical path)
    pub exclude_project: Vec<String>,
    /// Optional time specification (e.g., "5m", "1h", "2d")
    pub since: Option<String>,
    /// Maximum entries to return (only used in JSON mode)
    pub limit: u32,
    /// Glob patterns to include (repeatable, OR'd)
    pub include: Vec<String>,
    /// Glob patterns to exclude (repeatable, OR'd)
    pub exclude: Vec<String>,
    /// Case-insensitive glob matching
    pub ignore_case: bool,
    /// Group output by file path and project
    pub grouped: bool,
    /// Output format (human or JSON)
    pub format: OutputFormat,
}

impl Default for GlobalLogParams {
    fn default() -> Self {
        GlobalLogParams {
            include_project: Vec::new(),
            exclude_project: Vec::new(),
            since: None,
            limit: 1000,
            include: Vec::new(),
            exclude: Vec::new(),
            ignore_case: false,
            grouped: false,
            format: OutputFormat::Human,
        }
    }
}

/// Parameters for density histogram operations.
///
/// Bundles parameters used in density histogram computation.
#[derive(Debug, Clone)]
pub struct DensityParams {
    /// Optional time specification (e.g., "5m", "1h", "2d")
    pub since: Option<String>,
    /// Glob patterns to include (repeatable, OR'd)
    pub include: Vec<String>,
    /// Glob patterns to exclude (repeatable, OR'd)
    pub exclude: Vec<String>,
    /// Case-insensitive glob matching
    pub ignore_case: bool,
    /// Number of buckets for density histogram
    pub num_buckets: u32,
}

impl DensityParams {
    /// Creates a new `DensityParams` with provided values.
    pub fn new(
        since: Option<String>,
        include: Vec<String>,
        exclude: Vec<String>,
        ignore_case: bool,
        num_buckets: u32,
    ) -> Self {
        DensityParams {
            since,
            include,
            exclude,
            ignore_case,
            num_buckets,
        }
    }
}

impl Default for DensityParams {
    fn default() -> Self {
        DensityParams {
            since: None,
            include: Vec::new(),
            exclude: Vec::new(),
            ignore_case: false,
            num_buckets: 100,
        }
    }
}

/// Parameters for global density histogram operations.
///
/// Bundles parameters used in global density histogram computation.
#[derive(Debug, Clone)]
pub struct GlobalDensityParams {
    /// Project paths to include (prefix-matched on canonical path)
    pub include_project: Vec<String>,
    /// Project paths to exclude (prefix-matched on canonical path)
    pub exclude_project: Vec<String>,
    /// Optional time specification (e.g., "5m", "1h", "2d")
    pub since: Option<String>,
    /// Glob patterns to include (repeatable, OR'd)
    pub include: Vec<String>,
    /// Glob patterns to exclude (repeatable, OR'd)
    pub exclude: Vec<String>,
    /// Case-insensitive glob matching
    pub ignore_case: bool,
    /// Number of buckets for density histogram
    pub num_buckets: u32,
}

impl GlobalDensityParams {
    /// Creates a new `GlobalDensityParams` with provided values.
    pub fn new(
        include_project: Vec<String>,
        exclude_project: Vec<String>,
        since: Option<String>,
        include: Vec<String>,
        exclude: Vec<String>,
        ignore_case: bool,
        num_buckets: u32,
    ) -> Self {
        GlobalDensityParams {
            include_project,
            exclude_project,
            since,
            include,
            exclude,
            ignore_case,
            num_buckets,
        }
    }
}

impl Default for GlobalDensityParams {
    fn default() -> Self {
        GlobalDensityParams {
            include_project: Vec::new(),
            exclude_project: Vec::new(),
            since: None,
            include: Vec::new(),
            exclude: Vec::new(),
            ignore_case: false,
            num_buckets: 100,
        }
    }
}

pub use self::filters::{
    determine_scope_kind, group_by_file, resolve_global_projects, FileGroup, ScopeKind,
};
pub use self::format::{
    compute_density_buckets, format_cursor, format_session_duration, parse_cursor,
};
pub use self::session::{run_global_sessions, run_sessions};

use display::{
    render_global_flat_human, render_global_flat_json, render_global_grouped_human,
    render_global_grouped_json, render_grouped_human, render_grouped_json,
};
use format::{cursor_from_page, format_snapshot_line};

/// JSON output for a single log entry.
#[derive(serde::Serialize)]
pub struct LogEntry {
    pub id: i64,
    pub file: String,
    pub event: String,
    pub bytes: u64,
    pub size_human: String,
    pub timestamp: String,
    pub hash: String,
    pub lines: u64,
    pub lines_added: u64,
    pub lines_removed: u64,
}

/// JSON output wrapping log entries with cursor pagination.
#[derive(serde::Serialize)]
pub(super) struct PaginatedLogOutput {
    pub entries: Vec<LogEntry>,
    pub next_cursor: Option<String>,
}

/// JSON output for density histogram.
#[derive(serde::Serialize)]
pub struct DensityOutput {
    pub buckets: Vec<DensityBucket>,
    pub total: u64,
    pub from: String,
    pub to: String,
}

/// A single bucket in the density histogram.
#[derive(serde::Serialize, Debug, PartialEq)]
pub struct DensityBucket {
    pub start: String,
    pub end: String,
    pub count: u64,
}

/// JSON output for grouped log view.
#[derive(serde::Serialize)]
pub(super) struct GroupedLogOutput {
    pub files: Vec<GroupedFileEntry>,
    pub summary: GroupedSummary,
}

/// A single file's history in grouped JSON output.
#[derive(serde::Serialize)]
pub struct GroupedFileEntry {
    pub path: String,
    pub change_count: usize,
    pub entries: Vec<LogEntry>,
}

/// Summary statistics for grouped JSON output.
#[derive(serde::Serialize)]
pub(super) struct GroupedSummary {
    pub total_files: usize,
    pub total_changes: usize,
}

/// Page size for history pagination.
pub(super) const PAGE_SIZE: u32 = 50;

/// Page size for grouped output (number of complete file groups per page).
pub(super) const GROUPED_PAGE_SIZE: usize = 20;

use super::output::use_color;

/// Streams file change history with interactive pagination.
///
/// Stats are always shown from snapshot fields (no computation needed).
///
/// # Arguments
/// * `project_root` - Root directory of the project
/// * `params` - Parameters bundling target, time range, filters, and output options
///
/// # Returns
/// `Ok(())` on success, or `UnfError` if querying history fails.
#[allow(clippy::cognitive_complexity)]
// TODO(v0.18): reduce complexity
pub fn run(project_root: &Path, params: &LogParams) -> Result<(), UnfError> {
    // Validate JSON-only flags
    if params.density && params.format != OutputFormat::Json {
        return Err(UnfError::InvalidArgument(
            "--density requires --json".to_string(),
        ));
    }
    if params.cursor_str.is_some() && params.format != OutputFormat::Json {
        return Err(UnfError::InvalidArgument(
            "--cursor requires --json".to_string(),
        ));
    }

    let storage_dir = storage::resolve_storage_dir(project_root)?;
    let engine = Engine::open(project_root, &storage_dir)?;

    // Create glob filter from include/exclude patterns
    let filter = GlobFilter::new(&params.include, &params.exclude, params.ignore_case)?;

    // Parse since parameter if provided
    let since_time = if let Some(spec) = &params.since {
        Some(super::parse_time_spec(spec)?)
    } else {
        None
    };

    // Density mode: compute histogram and return early
    if params.density {
        return run_density(&engine, since_time, &filter, params.num_buckets);
    }

    // Determine the scope kind and canonical target string.
    // The owned String lives here so HistoryScope can borrow it each iteration.
    let (scope_kind, scope_target) = determine_scope_kind(project_root, params.target.as_deref());

    // Parse cursor if provided
    let initial_cursor = match &params.cursor_str {
        Some(s) => Some(parse_cursor(s)?),
        None => None,
    };

    // JSON mode: collect all results up to limit
    if params.format == OutputFormat::Json {
        let mut all_snapshots = Vec::new();
        let mut cursor: Option<HistoryCursor> = initial_cursor;
        let mut remaining = params.limit;
        let mut has_more = false;

        loop {
            let scope = match scope_kind {
                ScopeKind::All => HistoryScope::All,
                ScopeKind::File => HistoryScope::File(&scope_target),
                ScopeKind::Directory => HistoryScope::Directory(&scope_target),
            };

            let page_size = std::cmp::min(PAGE_SIZE, remaining);
            let mut page =
                engine.get_history_page(scope, cursor.as_ref(), page_size, since_time)?;

            // Capture raw page info before filtering for cursor advancement
            let raw_page_len = page.len();
            cursor = cursor_from_page(&page);

            // Apply glob filter to the page
            page.retain(|s| filter.matches(&s.file_path));

            if page.is_empty() {
                // If DB returned no results (or fewer than requested), no more data exists
                if raw_page_len == 0 || raw_page_len < page_size as usize {
                    break;
                }
                // Filter removed all results but more pages may have matches
                remaining = params.limit - all_snapshots.len() as u32;
                if remaining == 0 {
                    has_more = true;
                    break;
                }
                continue;
            }

            for snapshot in page {
                if all_snapshots.len() >= params.limit as usize {
                    has_more = true;
                    break;
                }
                all_snapshots.push(snapshot);
            }

            if all_snapshots.len() >= params.limit as usize {
                has_more = true;
                break;
            }

            remaining = params.limit - all_snapshots.len() as u32;

            if raw_page_len < PAGE_SIZE as usize {
                break;
            }
        }

        let is_empty = all_snapshots.is_empty();

        // Compute next_cursor from the last collected snapshot
        let next_cursor = if has_more {
            all_snapshots.last().map(|s| {
                format_cursor(&HistoryCursor {
                    timestamp: s.timestamp,
                    id: s.id,
                })
            })
        } else {
            None
        };

        if params.grouped {
            // Grouped JSON output (no cursor support)
            let groups = group_by_file(all_snapshots);
            let output = render_grouped_json(&engine, groups);
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        } else {
            // Paginated flat JSON output
            let entries: Vec<LogEntry> = all_snapshots
                .iter()
                .map(|snapshot| LogEntry {
                    id: snapshot.id.0,
                    file: snapshot.file_path.clone(),
                    event: snapshot.event_type.to_string(),
                    bytes: snapshot.size_bytes,
                    size_human: super::format_size(snapshot.size_bytes),
                    timestamp: snapshot.timestamp.to_rfc3339(),
                    hash: snapshot.content_hash.0.clone(),
                    lines: snapshot.line_count,
                    lines_added: snapshot.lines_added,
                    lines_removed: snapshot.lines_removed,
                })
                .collect();
            let output = PaginatedLogOutput {
                entries,
                next_cursor,
            };
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }

        if is_empty {
            return Err(UnfError::NoResults(String::new()));
        }

        return Ok(());
    }

    // Human mode with grouping: collect all results and render grouped view
    if params.grouped {
        let mut all_snapshots = Vec::new();
        let mut cursor: Option<HistoryCursor> = None;

        // Collect all snapshots (with a reasonable limit to prevent unbounded memory)
        let max_snapshots = 10000;

        loop {
            let scope = match scope_kind {
                ScopeKind::All => HistoryScope::All,
                ScopeKind::File => HistoryScope::File(&scope_target),
                ScopeKind::Directory => HistoryScope::Directory(&scope_target),
            };

            let page_size = PAGE_SIZE;
            let mut page =
                engine.get_history_page(scope, cursor.as_ref(), page_size, since_time)?;

            // Capture raw page info before filtering for cursor advancement
            let raw_page_len = page.len();
            cursor = cursor_from_page(&page);

            // Apply glob filter to the page
            page.retain(|s| filter.matches(&s.file_path));

            if page.is_empty() {
                // If DB returned no results or fewer than requested, no more data
                if raw_page_len == 0 || raw_page_len < page_size as usize {
                    if all_snapshots.is_empty() {
                        let target_display = if scope_target.is_empty() {
                            "all files".to_string()
                        } else {
                            format!("\"{}\"", scope_target)
                        };
                        return Err(UnfError::NoResults(format!(
                            "No history for {}.",
                            target_display
                        )));
                    }
                    break;
                }
                // Filter removed all results but more pages may have matches
                continue;
            }

            for snapshot in page {
                if all_snapshots.len() >= max_snapshots {
                    break;
                }
                all_snapshots.push(snapshot);
            }

            if all_snapshots.len() >= max_snapshots {
                break;
            }

            // If raw DB page was not full, we've reached the end
            if raw_page_len < PAGE_SIZE as usize {
                break;
            }
        }

        // Group and render
        let groups = group_by_file(all_snapshots);
        let is_tty = std::io::stdout().is_terminal();
        render_grouped_human(&engine, groups, use_color(), is_tty)?;

        return Ok(());
    }

    // Human mode: interactive pagination (flat view)
    let is_tty = std::io::stdout().is_terminal();
    let colored = use_color();

    // Streaming pagination loop
    let mut cursor: Option<HistoryCursor> = None;
    let mut displayed_any = false;

    loop {
        let scope = match scope_kind {
            ScopeKind::All => HistoryScope::All,
            ScopeKind::File => HistoryScope::File(&scope_target),
            ScopeKind::Directory => HistoryScope::Directory(&scope_target),
        };
        let mut page = engine.get_history_page(scope, cursor.as_ref(), PAGE_SIZE, since_time)?;

        // Capture raw page info before filtering for cursor advancement
        let raw_page_len = page.len();
        cursor = cursor_from_page(&page);

        // Apply glob filter to the page
        page.retain(|s| filter.matches(&s.file_path));

        if page.is_empty() {
            // If DB returned no results or fewer than requested, no more data
            if raw_page_len == 0 || raw_page_len < PAGE_SIZE as usize {
                if !displayed_any {
                    let target_display = if scope_target.is_empty() {
                        "all files".to_string()
                    } else {
                        format!("\"{}\"", scope_target)
                    };
                    return Err(UnfError::NoResults(format!(
                        "No history for {}.",
                        target_display
                    )));
                }
                println!("-- end --");
                break;
            }
            // Filter removed all results but more pages may have matches
            continue;
        }

        displayed_any = true;

        // Print each snapshot
        for snapshot in &page {
            let line = format_snapshot_line(snapshot, colored);
            println!("{}", line);
        }

        // Check if DB had more pages
        if raw_page_len < PAGE_SIZE as usize {
            println!("-- end --");
            break;
        }

        // If connected to a TTY, prompt for continuation
        if is_tty {
            println!("-- press Enter for more, q to quit --");
            let mut input = String::new();
            std::io::stdin()
                .read_line(&mut input)
                .map_err(|e| UnfError::InvalidArgument(format!("Failed to read input: {}", e)))?;

            if input.trim().eq_ignore_ascii_case("q") {
                break;
            }
        }
    }

    Ok(())
}

/// Runs density histogram mode.
///
/// Collects all snapshot timestamps, applies glob filtering, computes buckets,
/// and outputs the DensityOutput JSON.
fn run_density(
    engine: &Engine,
    since_time: Option<chrono::DateTime<chrono::Utc>>,
    filter: &GlobFilter,
    num_buckets: u32,
) -> Result<(), UnfError> {
    let all_timestamps = engine.get_all_snapshot_timestamps(since_time)?;

    // Apply glob filter
    let filtered_timestamps: Vec<chrono::DateTime<chrono::Utc>> = all_timestamps
        .into_iter()
        .filter(|(_, path)| filter.matches(path))
        .map(|(ts, _)| ts)
        .collect();

    if filtered_timestamps.is_empty() {
        let output = DensityOutput {
            buckets: vec![],
            total: 0,
            from: String::new(),
            to: String::new(),
        };
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
        return Err(UnfError::NoResults(String::new()));
    }

    let from = *filtered_timestamps.iter().min().unwrap();
    let to = *filtered_timestamps.iter().max().unwrap();
    let buckets = compute_density_buckets(&filtered_timestamps, from, to, num_buckets);

    let output = DensityOutput {
        buckets,
        total: filtered_timestamps.len() as u64,
        from: from.to_rfc3339(),
        to: to.to_rfc3339(),
    };
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
    Ok(())
}

/// Runs density histogram mode across all registered projects.
///
/// Opens engines for all matching projects, collects snapshot timestamps,
/// applies glob filtering, merges them, and passes the unified list to
/// `compute_density_buckets`.
///
/// # Arguments
/// * `params` - Parameters bundling project filters, time range, file filters, and density options
pub fn run_global_density(params: &GlobalDensityParams) -> Result<(), UnfError> {
    let filter = GlobFilter::new(&params.include, &params.exclude, params.ignore_case)?;
    let since_time = if let Some(spec) = &params.since {
        Some(super::parse_time_spec(spec)?)
    } else {
        None
    };

    let projects = resolve_global_projects(&params.include_project, &params.exclude_project)?;

    let mut all_timestamps: Vec<chrono::DateTime<chrono::Utc>> = Vec::new();
    for (project_path, storage_dir) in &projects {
        match Engine::open(project_path, storage_dir) {
            Ok(engine) => match engine.get_all_snapshot_timestamps(since_time) {
                Ok(timestamps) => {
                    let filtered = timestamps
                        .into_iter()
                        .filter(|(_, path)| filter.matches(path))
                        .map(|(ts, _)| ts);
                    all_timestamps.extend(filtered);
                }
                Err(_) => continue,
            },
            Err(_) => continue,
        }
    }

    if all_timestamps.is_empty() {
        let output = DensityOutput {
            buckets: vec![],
            total: 0,
            from: String::new(),
            to: String::new(),
        };
        println!("{}", serde_json::to_string_pretty(&output).unwrap());
        return Err(UnfError::NoResults(String::new()));
    }

    let from = *all_timestamps.iter().min().unwrap();
    let to = *all_timestamps.iter().max().unwrap();
    let buckets = compute_density_buckets(&all_timestamps, from, to, params.num_buckets);

    let output = DensityOutput {
        buckets,
        total: all_timestamps.len() as u64,
        from: from.to_rfc3339(),
        to: to.to_rfc3339(),
    };
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
    Ok(())
}

// --- Global (cross-project) log types and functions ---

/// A snapshot annotated with its project path for cross-project output.
pub(super) struct ProjectSnapshot {
    pub project_path: String,
    pub snapshot: Snapshot,
}

/// A lazy stream of snapshots from a single project, fetched page-by-page.
pub(super) struct ProjectStream {
    pub project_path: String,
    pub engine: Engine,
    pub buffer: Vec<Snapshot>,
    pub index: usize,
    pub cursor: Option<HistoryCursor>,
    pub exhausted: bool,
    pub since: Option<chrono::DateTime<chrono::Utc>>,
    pub filter: GlobFilter,
}

impl ProjectStream {
    /// Creates a new stream for the given project.
    pub fn new(
        project_path: String,
        engine: Engine,
        since: Option<chrono::DateTime<chrono::Utc>>,
        filter: GlobFilter,
    ) -> Self {
        ProjectStream {
            project_path,
            engine,
            buffer: Vec::new(),
            index: 0,
            cursor: None,
            exhausted: false,
            since,
            filter,
        }
    }

    /// Peeks at the current snapshot without advancing.
    pub fn peek(&mut self) -> Result<Option<&Snapshot>, UnfError> {
        // If we have buffered data, return next item
        if self.index < self.buffer.len() {
            return Ok(Some(&self.buffer[self.index]));
        }

        // If exhausted, nothing left
        if self.exhausted {
            return Ok(None);
        }

        // Fetch next page
        self.fetch_next_page()?;

        if self.index < self.buffer.len() {
            Ok(Some(&self.buffer[self.index]))
        } else {
            Ok(None)
        }
    }

    /// Advances past the current snapshot.
    pub fn advance(&mut self) {
        if self.index < self.buffer.len() {
            self.index += 1;
        }
    }

    /// Fetches the next page of results, applying glob filter.
    pub fn fetch_next_page(&mut self) -> Result<(), UnfError> {
        loop {
            let page = self.engine.get_history_page(
                HistoryScope::All,
                self.cursor.as_ref(),
                PAGE_SIZE,
                self.since,
            )?;

            let raw_len = page.len();
            self.cursor = cursor_from_page(&page);

            let filtered: Vec<Snapshot> = page
                .into_iter()
                .filter(|s| self.filter.matches(&s.file_path))
                .collect();

            if !filtered.is_empty() {
                self.buffer = filtered;
                self.index = 0;
                return Ok(());
            }

            // No matches in this page
            if raw_len < PAGE_SIZE as usize {
                self.exhausted = true;
                self.buffer.clear();
                self.index = 0;
                return Ok(());
            }
            // Try next page
        }
    }
}

/// JSON output for a single global log entry.
#[derive(serde::Serialize)]
pub(super) struct GlobalLogEntry {
    pub project: String,
    #[serde(flatten)]
    pub entry: LogEntry,
}

/// JSON flat output for global log.
#[derive(serde::Serialize)]
pub(super) struct GlobalLogOutput {
    pub entries: Vec<GlobalLogEntry>,
}

/// JSON grouped output for global log (grouped by project, then by file).
#[derive(serde::Serialize)]
pub(super) struct GlobalGroupedProject {
    pub project: String,
    pub files: Vec<GroupedFileEntry>,
}

/// JSON grouped output wrapper with summary.
#[derive(serde::Serialize)]
pub(super) struct GlobalGroupedOutput {
    pub projects: Vec<GlobalGroupedProject>,
    pub summary: GlobalGroupedSummary,
}

/// Summary statistics for global grouped output.
#[derive(serde::Serialize)]
pub(super) struct GlobalGroupedSummary {
    pub total_projects: usize,
    pub total_files: usize,
    pub total_changes: usize,
}

/// Runs the global (cross-project) log command.
///
/// Opens engines for all matching projects, performs a k-way merge across
/// their history streams (sorted by timestamp descending), and renders
/// output in the requested format.
///
/// # Arguments
/// * `params` - Parameters bundling project filters, time range, file filters, and output options
#[allow(clippy::cognitive_complexity)]
// TODO(v0.18): reduce complexity
pub fn run_global(params: &GlobalLogParams) -> Result<(), UnfError> {
    let filter = GlobFilter::new(&params.include, &params.exclude, params.ignore_case)?;
    let since_time = if let Some(spec) = &params.since {
        Some(super::parse_time_spec(spec)?)
    } else {
        None
    };

    let projects = resolve_global_projects(&params.include_project, &params.exclude_project)?;

    // Open engines and create streams
    let mut streams: Vec<ProjectStream> = Vec::new();
    for (project_path, storage_dir) in &projects {
        match Engine::open(project_path, storage_dir) {
            Ok(engine) => {
                streams.push(ProjectStream::new(
                    project_path.to_string_lossy().to_string(),
                    engine,
                    since_time,
                    filter.clone(),
                ));
            }
            Err(_) => continue, // Skip projects that can't be opened
        }
    }

    if streams.is_empty() {
        return Err(UnfError::NoResults(
            "No accessible projects found.".to_string(),
        ));
    }

    // K-way merge: collect up to `limit` snapshots, newest first
    let mut collected: Vec<ProjectSnapshot> = Vec::new();
    let effective_limit = if params.format == OutputFormat::Json {
        params.limit as usize
    } else {
        10000 // Reasonable cap for human mode
    };

    loop {
        if collected.len() >= effective_limit {
            break;
        }

        // Find the stream with the newest next snapshot
        let mut best_idx: Option<usize> = None;
        let mut best_key: Option<(chrono::DateTime<chrono::Utc>, i64)> = None;

        for (i, stream) in streams.iter_mut().enumerate() {
            if let Some(snap) = stream.peek()? {
                let key = (snap.timestamp, snap.id.0);
                if best_key.is_none() || key > best_key.unwrap() {
                    best_key = Some(key);
                    best_idx = Some(i);
                }
            }
        }

        match best_idx {
            Some(idx) => {
                // We already peeked, so we know there's a snapshot
                let stream = &mut streams[idx];
                let snap = stream.peek()?.unwrap().clone();
                let project_path = stream.project_path.clone();
                stream.advance();

                collected.push(ProjectSnapshot {
                    project_path,
                    snapshot: snap,
                });
            }
            None => break, // All streams exhausted
        }
    }

    if collected.is_empty() {
        return Err(UnfError::NoResults(
            "No changes found across projects.".to_string(),
        ));
    }

    // Render output
    match params.format {
        OutputFormat::Json => {
            if params.grouped {
                render_global_grouped_json(&collected);
            } else {
                render_global_flat_json(&collected);
            }
        }
        OutputFormat::Human => {
            let colored = use_color();
            if params.grouped {
                render_global_grouped_human(&collected, colored)?;
            } else {
                render_global_flat_human(&collected, colored);
            }
        }
    }

    Ok(())
}
