import { get, writable } from "svelte/store";
import type {
	CatResponse,
	DensityBucket,
	DiffResponse,
	GroupedLogFile,
	LogEntry,
	ProjectEntry,
	StatusResponse,
	TabState,
} from "./types";
import { createDefaultTabState, GLOBAL_TAB } from "./types";

// Re-export for convenience
export { GLOBAL_TAB };

// ============================================================================
// GLOBAL STATE (shared across all tabs)
// ============================================================================

/** List of all projects available in the system */
export const projects = writable<ProjectEntry[]>([]);

/** Global error state (not per-tab) */
export const error = writable<string | null>(null);

// ============================================================================
// TAB MANAGEMENT
// ============================================================================

/** Ordered list of currently open tab identifiers (project paths or GLOBAL_TAB) */
export const openTabs = writable<string[]>([]);

/** The currently active/visible tab identifier, or null if no tab is open */
export const activeTab = writable<string | null>(null);

/**
 * Internal storage of all tab states.
 * Keyed by tab identifier (project path or GLOBAL_TAB).
 */
const tabStateStorage = new Map<string, TabState>();

// ============================================================================
// PER-PROJECT STORES (backwards-compatible writable stores)
// Existing components continue to work without changes.
// These are kept in sync with the active tab's state via save/restore functions.
// ============================================================================

/** Currently selected project path (synced from activeTab) */
export const selectedProject = writable<string | null>(null);

/** Project status for the active project (synced from active tab state) */
export const projectStatus = writable<StatusResponse | null>(null);

/** Timeline entries for the active project (synced from active tab state) */
export const timelineEntries = writable<LogEntry[]>([]);

/** Cursor for pagination of timeline entries (synced from active tab state) */
export const nextCursor = writable<string | null>(null);

/** Whether timeline is currently loading (synced from active tab state) */
export const timelineLoading = writable(false);

/** File tree grouped by file (synced from active tab state) */
export const fileTree = writable<GroupedLogFile[]>([]);

/** Current file filters applied to the timeline (synced from active tab state) */
export const fileFilters = writable<string[]>([]);

/** Currently selected entry in the timeline (synced from active tab state) */
export const selectedEntry = writable<LogEntry | null>(null);

/** Which view to show: diff or file content (synced from active tab state) */
export const viewMode = writable<"diff" | "content">("diff");

/** Diff data for the currently selected entry (synced from active tab state) */
export const diffData = writable<DiffResponse | null>(null);

/** File content data for the currently selected entry (synced from active tab state) */
export const contentData = writable<CatResponse | null>(null);

/** Whether content data is currently loading (synced from active tab state) */
export const contentLoading = writable(false);

/** Timeline display mode: grouped by file or flat chronological (synced from active tab state) */
export const timelineViewMode = writable<"grouped" | "flat">("grouped");

/** Density buckets for the timeline histogram (synced from active tab state) */
export const densityBuckets = writable<DensityBucket[]>([]);

/** Start time for histogram view in ISO format (synced from active tab state) */
export const histogramStart = writable<string | null>(null);

/** End time for histogram view in ISO format (synced from active tab state) */
export const histogramEnd = writable<string | null>(null);

/** Whether the current histogram range was set by clicking a session diamond */
export const histogramIsSession = writable<boolean>(false);

// ============================================================================
// TAB PERSISTENCE (localStorage)
// ============================================================================

const STORAGE_KEY = "unfudged_tabs";

function persistTabs(): void {
	try {
		const data = { openTabs: get(openTabs), activeTab: get(activeTab) };
		localStorage.setItem(STORAGE_KEY, JSON.stringify(data));
	} catch (_e) {
		// localStorage unavailable
	}
}

/** Load saved tabs. Call once on app mount, before project list loads. */
export function loadPersistedTabs(): { openTabs: string[]; activeTab: string | null } {
	try {
		const raw = localStorage.getItem(STORAGE_KEY);
		if (!raw) return { openTabs: [], activeTab: null };
		const data = JSON.parse(raw);
		return {
			openTabs: Array.isArray(data.openTabs) ? data.openTabs : [],
			activeTab: typeof data.activeTab === "string" ? data.activeTab : null,
		};
	} catch (_e) {
		return { openTabs: [], activeTab: null };
	}
}

// ============================================================================
// TAB STATE MANAGEMENT FUNCTIONS
// ============================================================================

/**
 * Save the current state of all per-project stores into the internal tabStateStorage
 * for the currently active tab. Call this before switching tabs.
 */
export function saveCurrentTabState(): void {
	const activeTabPath = get(activeTab);
	if (!activeTabPath) return;

	const tabState: TabState = {
		projectPath: activeTabPath,
		projectStatus: get(projectStatus),
		timelineEntries: get(timelineEntries),
		nextCursor: get(nextCursor),
		timelineLoading: get(timelineLoading),
		fileTree: get(fileTree),
		fileFilters: get(fileFilters),
		selectedEntry: get(selectedEntry),
		viewMode: get(viewMode),
		diffData: get(diffData),
		contentData: get(contentData),
		contentLoading: get(contentLoading),
		timelineViewMode: get(timelineViewMode),
		densityBuckets: get(densityBuckets),
		histogramStart: get(histogramStart),
		histogramEnd: get(histogramEnd),
		histogramIsSession: get(histogramIsSession),
	};

	tabStateStorage.set(activeTabPath, tabState);
}

/**
 * Restore all per-project stores from the internal tabStateStorage for a given tab identifier.
 * If no state exists for this tab, initializes it with defaults.
 */
export function restoreTabState(tabId: string): void {
	const tabState = tabStateStorage.get(tabId) || createDefaultTabState(tabId);

	projectStatus.set(tabState.projectStatus);
	timelineEntries.set(tabState.timelineEntries);
	nextCursor.set(tabState.nextCursor);
	timelineLoading.set(tabState.timelineLoading);
	fileTree.set(tabState.fileTree);
	fileFilters.set(Array.isArray(tabState.fileFilters) ? tabState.fileFilters : []);
	selectedEntry.set(tabState.selectedEntry);
	viewMode.set(tabState.viewMode);
	diffData.set(tabState.diffData);
	contentData.set(tabState.contentData);
	contentLoading.set(tabState.contentLoading);
	timelineViewMode.set(tabState.timelineViewMode);
	densityBuckets.set(tabState.densityBuckets);
	histogramStart.set(tabState.histogramStart);
	histogramEnd.set(tabState.histogramEnd);
	histogramIsSession.set(tabState.histogramIsSession);
}

/**
 * Open a new tab for the given identifier, or activate it if already open.
 * Automatically saves the current tab state and restores the target tab state.
 */
export function openTab(tabId: string): void {
	const currentTabs = get(openTabs);
	if (!currentTabs.includes(tabId)) {
		openTabs.set([...currentTabs, tabId]);
	}
	switchTab(tabId);
	persistTabs();
}

/**
 * Close a tab for the given identifier.
 * If closing the active tab, switches to an adjacent tab (or null if last tab).
 */
export function closeTab(tabId: string): void {
	// Global tab cannot be closed — it's the permanent default
	if (tabId === GLOBAL_TAB) return;

	const currentTabs = get(openTabs);
	const filteredTabs = currentTabs.filter((p) => p !== tabId);

	tabStateStorage.delete(tabId);
	openTabs.set(filteredTabs);

	const currentActive = get(activeTab);
	if (currentActive === tabId) {
		const nextTab = filteredTabs.length > 0 ? filteredTabs[0] : null;
		if (nextTab) {
			switchTab(nextTab);
		} else {
			activeTab.set(null);
			selectedProject.set(null);
		}
	}
	persistTabs();
}

/**
 * Switch to a different tab, saving the current tab state and restoring the target tab state.
 */
export function switchTab(tabId: string): void {
	// Save current tab state before switching
	saveCurrentTabState();

	// Switch to the target tab
	activeTab.set(tabId);
	selectedProject.set(tabId === GLOBAL_TAB ? null : tabId);

	// Restore the target tab's state
	restoreTabState(tabId);
	persistTabs();
}

/** Check if the given tab identifier represents the global view */
export function isGlobalTab(tabId: string | null): boolean {
	return tabId === GLOBAL_TAB;
}

/** Check if a tab has cached data (non-empty timeline or file tree) */
export function hasTabData(tabId: string): boolean {
	const state = tabStateStorage.get(tabId);
	return !!state && (state.timelineEntries.length > 0 || state.fileTree.length > 0);
}
