// TypeScript interfaces mirroring `unf --json` output exactly.
// The Tauri backend passes JSON through as serde_json::Value.
// These types are the frontend's interpretation of that JSON.

export interface ProjectEntry {
  path: string;
  status: "watching" | "stopped" | "crashed" | "orphaned" | "error";
  snapshots: number | null;
  store_bytes: number | null;
  tracked_files: number | null;
  recording_since: string | null;
  last_activity: string | null;
}

export interface ProjectListResponse {
  projects: ProjectEntry[];
}

export interface StatusResponse {
  recording: boolean;
  since: string | null;
  newest: string | null;
  snapshots: number | null;
  files_tracked: number | null;
  store_bytes: number | null;
  auto_restart: boolean;
}

export interface LogEntry {
  id: number;
  file: string;
  event: "created" | "modified" | "deleted";
  bytes: number;
  size_human: string;
  timestamp: string;
  hash: string;
  lines: number;
  lines_added: number;
  lines_removed: number;
  /** Present in global (cross-project) mode */
  project?: string;
}

export interface PaginatedLogResponse {
  entries: LogEntry[];
  next_cursor: string | null;
}

export interface GroupedLogFile {
  path: string;
  change_count: number;
  entries: LogEntry[];
  /** Present in global (cross-project) mode */
  project?: string;
}

export interface GroupedLogResponse {
  files: GroupedLogFile[];
  summary: {
    total_changes: number;
    total_files: number;
  };
}

/** Sentinel value for the global (all projects) tab */
export const GLOBAL_TAB = "__global__";

export interface GlobalGroupedProject {
  project: string;
  files: GroupedLogFile[];
}

export interface GlobalGroupedLogResponse {
  projects: GlobalGroupedProject[];
  summary: {
    total_changes: number;
    total_files: number;
    total_projects: number;
  };
}

export interface DiffLine {
  op: "insert" | "delete" | "equal";
  content: string;
}

export interface DiffHunkLine {
  op: "insert" | "delete" | "equal";
  content: string;
  old_num?: number;
  new_num?: number;
}

export interface DiffHunk {
  old_start: number;
  old_count: number;
  new_start: number;
  new_count: number;
  lines: DiffHunkLine[];
}

export interface DiffChange {
  file: string;
  status: "modified" | "created" | "deleted";
  diff: DiffLine[] | null;        // OLD: backward compat
  hunks: DiffHunk[] | null;       // NEW: contextual format
}

export interface DiffResponse {
  from: string;
  to: string;
  changes: DiffChange[];
}

export interface CatResponse {
  file: string;
  content: string;
  snapshot_id: number;
  timestamp: string;
  bytes: number;
}

export interface DensityBucket {
  start: string;
  end: string;
  count: number;
}

export interface DensityResponse {
  buckets: DensityBucket[];
  total: number;
  from: string;
  to: string;
}

/**
 * Complete state for a single project tab.
 * This captures all per-project data that should be isolated when switching between tabs.
 */
export interface TabState {
  projectPath: string;
  projectStatus: StatusResponse | null;
  timelineEntries: LogEntry[];
  nextCursor: string | null;
  timelineLoading: boolean;
  fileTree: GroupedLogFile[];
  fileFilters: string[];
  selectedEntry: LogEntry | null;
  viewMode: "diff" | "content";
  diffData: DiffResponse | null;
  contentData: CatResponse | null;
  contentLoading: boolean;
  timelineViewMode: "grouped" | "flat";
  densityBuckets: DensityBucket[];
  histogramStart: string | null;
  histogramEnd: string | null;
  histogramIsSession: boolean;
}

/**
 * Creates a default/empty TabState for a given project path.
 * Used when opening a new tab.
 */
export function createDefaultTabState(projectPath: string): TabState {
  return {
    projectPath,
    projectStatus: null,
    timelineEntries: [],
    nextCursor: null,
    timelineLoading: false,
    fileTree: [],
    fileFilters: [],
    selectedEntry: null,
    viewMode: "diff",
    diffData: null,
    contentData: null,
    contentLoading: false,
    timelineViewMode: "grouped",
    densityBuckets: [],
    histogramStart: null,
    histogramEnd: null,
    histogramIsSession: false,
  };
}
