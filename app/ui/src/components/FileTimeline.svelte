<script lang="ts">
  import {
    timelineEntries,
    timelineLoading,
    nextCursor,
    selectedEntry,
    fileFilters,
    fileTree,
    projectStatus,
    timelineViewMode,
    projects,
    selectedProject,
    activeTab,
    histogramStart,
    histogramEnd,
    histogramIsSession,
    GLOBAL_TAB,
  } from "../lib/stores";
  import HistogramRange from "./HistogramRange.svelte";
  import FilterAutocomplete from "./FilterAutocomplete.svelte";
  import { formatTimeRange } from "../lib/timeFormat";
  import { extractCandidates } from "../lib/filterUtils";
  import type { LogEntry } from "../lib/types";

  interface Props {
    onLoadMore: () => void;
  }
  let { onLoadMore }: Props = $props();

  let listEl: HTMLDivElement | undefined = $state();
  let expandedFiles = $state<Set<string>>(new Set());
  let pulsing = $state(false);
  let prevSnapshots = $state<number | null>(null);

  let isGlobal = $derived($activeTab === GLOBAL_TAB);

  // Pulse animation when snapshot count changes
  $effect(() => {
    const snapshots = totalSnapshots;
    if (prevSnapshots !== null && snapshots > 0 && snapshots !== prevSnapshots) {
      pulsing = true;
      setTimeout(() => { pulsing = false; }, 600);
    }
    prevSnapshots = snapshots;
  });

  function handleScroll() {
    if (!listEl || $timelineLoading || !$nextCursor) return;
    const { scrollTop, scrollHeight, clientHeight } = listEl;
    if (scrollHeight - scrollTop - clientHeight < 200) {
      onLoadMore();
    }
  }

  function selectEntry(entry: LogEntry) {
    selectedEntry.set(entry);
  }

  function toggleFile(key: string) {
    expandedFiles = new Set(expandedFiles);
    if (expandedFiles.has(key)) {
      expandedFiles.delete(key);
    } else {
      expandedFiles.add(key);
    }
  }

  function formatTime(ts: string): string {
    const d = new Date(ts);
    const now = new Date();
    const isToday = d.toDateString() === now.toDateString();
    const timePart = d.toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
    if (isToday) {
      return timePart;
    }
    return d.toLocaleDateString(undefined, { month: "short", day: "numeric" }) + " " + timePart;
  }

  function eventColor(event: string): string {
    switch (event) {
      case "created": return "var(--addition)";
      case "deleted": return "var(--deletion)";
      default: return "var(--text-secondary)";
    }
  }

  function truncatePath(path: string): string {
    if (path.length <= 40) return path;
    const parts = path.split("/");
    if (parts.length <= 2) return path;
    return "..." + parts.slice(-2).join("/");
  }

  function formatFileName(path: string): string {
    return path.split("/").pop() ?? path;
  }

  /** Get short project name from full path */
  function projectShortName(projectPath: string): string {
    return projectPath.split("/").pop() ?? projectPath;
  }

  /** Remove consecutive entries with the same hash (watcher debounce duplicates) */
  function deduplicateConsecutive(entries: LogEntry[]): LogEntry[] {
    return entries.filter((e, i) => i === 0 || e.hash !== entries[i - 1].hash);
  }

  let isGrouped = $derived($timelineViewMode === "grouped");

  // Fall back to project list data when status doesn't include counts (daemon stopped)
  let selectedProjectData = $derived(
    $projects.find((p) => p.path === $selectedProject)
  );

  // In global mode, compute totals from the file tree data
  let totalSnapshots = $derived(
    isGlobal
      ? $fileTree.reduce((sum, g) => sum + g.entries.length, 0)
      : ($projectStatus?.snapshots ?? selectedProjectData?.snapshots ?? 0)
  );
  let totalFiles = $derived(
    isGlobal
      ? $fileTree.length
      : ($projectStatus?.files_tracked ?? selectedProjectData?.tracked_files ?? 0)
  );

  // Count unique projects in global mode
  let totalProjects = $derived(
    isGlobal
      ? new Set($fileTree.map((g) => g.project).filter(Boolean)).size
      : 0
  );

  // Filtered counts from fileTree (already filtered by backend when filter/time is active)
  let filteredSnapshotCount = $derived(
    $fileTree.reduce((sum, g) => sum + g.entries.length, 0)
  );
  let filteredFileCount = $derived($fileTree.length);
  let isFiltered = $derived($fileFilters.length > 0 || $histogramStart !== null);

  // In global mode, group files by project first, then by directory within each project
  let sortedFiles = $derived(
    [...$fileTree]
      .map((g) => ({ ...g, entries: deduplicateConsecutive(g.entries) }))
      .sort((a, b) => {
        // In global mode, sort by project first, then by activity
        if (isGlobal && a.project !== b.project) {
          return (a.project ?? "").localeCompare(b.project ?? "");
        }
        const aTime = a.entries.length > 0 ? new Date(a.entries[0].timestamp).getTime() : 0;
        const bTime = b.entries.length > 0 ? new Date(b.entries[0].timestamp).getTime() : 0;
        return bTime - aTime;
      })
  );

  /** Extract parent directory from a file path */
  function parentDir(path: string): string {
    const parts = path.split("/");
    return parts.length > 1 ? parts.slice(0, -1).join("/") + "/" : "";
  }

  /** Group sorted files by parent directory (or project + directory in global mode) */
  let dirGroups = $derived.by(() => {
    const dirMap = new Map<string, typeof sortedFiles[number][]>();

    for (const file of sortedFiles) {
      let dir: string;
      if (isGlobal && file.project) {
        // In global mode, prefix dir with project short name
        const projName = projectShortName(file.project);
        const fileDir = parentDir(file.path);
        dir = fileDir ? `${projName}/${fileDir}` : `${projName}/`;
      } else {
        dir = parentDir(file.path);
      }
      if (!dirMap.has(dir)) {
        dirMap.set(dir, []);
      }
      dirMap.get(dir)!.push(file);
    }

    return Array.from(dirMap.entries())
      .map(([dir, files]) => ({ dir, files }))
      .sort((a, b) => {
        // Root files (empty dir) always first
        if (!a.dir && b.dir) return -1;
        if (a.dir && !b.dir) return 1;
        // Shallowest directories first, then alphabetical
        const aDepth = a.dir.split("/").length;
        const bDepth = b.dir.split("/").length;
        if (aDepth !== bDepth) return aDepth - bDepth;
        return a.dir.localeCompare(b.dir);
      });
  });

  /** Human-friendly time range description for the filter status bar */
  let timeRange = $derived(
    $histogramStart && $histogramEnd
      ? formatTimeRange($histogramStart, $histogramEnd, $histogramIsSession)
      : null
  );

  /** Autocomplete candidates derived from file tree */
  let candidates = $derived(extractCandidates($fileTree));

  /** Handle filter changes from autocomplete component */
  function handleFiltersChange(newFilters: string[]) {
    fileFilters.set(newFilters);
  }

  /** Remove a single file filter */
  function removeFileFilter(path: string) {
    fileFilters.set($fileFilters.filter((f) => f !== path));
  }

  /** Clear the time range filter */
  function clearTimeRange() {
    histogramStart.set(null);
    histogramEnd.set(null);
    histogramIsSession.set(false);
  }

  /** Clear all active filters */
  function clearAllFilters() {
    fileFilters.set([]);
    clearTimeRange();
  }

  /** Unique key for file in accordion (includes project in global mode) */
  function fileKey(group: { path: string; project?: string }): string {
    return group.project ? `${group.project}:${group.path}` : group.path;
  }
</script>

<div class="timeline-container">
  <div class="summary" class:pulsing>
    <div class="metrics">
      {#if isGlobal}
        <span class="metric">
          <span class="metric-value">{totalSnapshots.toLocaleString()}</span>
          <span class="metric-label">snapshots</span>
        </span>
        <span class="metric-sep">&middot;</span>
        <span class="metric">
          <span class="metric-value">{totalFiles.toLocaleString()}</span>
          <span class="metric-label">files</span>
        </span>
        {#if totalProjects > 0}
          <span class="metric-sep">&middot;</span>
          <span class="metric">
            <span class="metric-value">{totalProjects}</span>
            <span class="metric-label">projects</span>
          </span>
        {/if}
      {:else}
        <span class="metric">
          {#if isFiltered}
            <span class="metric-value">{filteredSnapshotCount.toLocaleString()}</span>
            <span class="metric-of">/ {totalSnapshots.toLocaleString()}</span>
          {:else}
            <span class="metric-value">{totalSnapshots.toLocaleString()}</span>
          {/if}
          <span class="metric-label">snapshots</span>
        </span>
        <span class="metric-sep">&middot;</span>
        <span class="metric">
          {#if isFiltered}
            <span class="metric-value">{filteredFileCount.toLocaleString()}</span>
            <span class="metric-of">/ {totalFiles.toLocaleString()}</span>
          {:else}
            <span class="metric-value">{totalFiles.toLocaleString()}</span>
          {/if}
          <span class="metric-label">files</span>
        </span>
      {/if}
    </div>
    <div class="summary-row">
      <div class="view-toggle">
        <button
          class="toggle-btn"
          class:active={isGrouped}
          title="Group by file"
          onclick={() => timelineViewMode.set("grouped")}
        >
          <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor"><path d="M1 2.5A1.5 1.5 0 0 1 2.5 1h3A1.5 1.5 0 0 1 7 2.5v3A1.5 1.5 0 0 1 5.5 7h-3A1.5 1.5 0 0 1 1 5.5v-3zm8 0A1.5 1.5 0 0 1 10.5 1h3A1.5 1.5 0 0 1 15 2.5v3A1.5 1.5 0 0 1 13.5 7h-3A1.5 1.5 0 0 1 9 5.5v-3zm-8 8A1.5 1.5 0 0 1 2.5 9h3A1.5 1.5 0 0 1 7 10.5v3A1.5 1.5 0 0 1 5.5 15h-3A1.5 1.5 0 0 1 1 13.5v-3zm8 0A1.5 1.5 0 0 1 10.5 9h3a1.5 1.5 0 0 1 1.5 1.5v3a1.5 1.5 0 0 1-1.5 1.5h-3A1.5 1.5 0 0 1 9 13.5v-3z"/></svg>
        </button>
        <button
          class="toggle-btn"
          class:active={!isGrouped}
          title="Flat timeline"
          onclick={() => timelineViewMode.set("flat")}
        >
          <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor"><path fill-rule="evenodd" d="M2.5 12a.5.5 0 0 1 .5-.5h10a.5.5 0 0 1 0 1H3a.5.5 0 0 1-.5-.5zm0-4a.5.5 0 0 1 .5-.5h10a.5.5 0 0 1 0 1H3a.5.5 0 0 1-.5-.5zm0-4a.5.5 0 0 1 .5-.5h10a.5.5 0 0 1 0 1H3a.5.5 0 0 1-.5-.5z"/></svg>
        </button>
      </div>
    </div>
  </div>

  <!-- Filter area: input + chips -->
  <div class="filter-area">
    <div class="filter-input-row">
      <FilterAutocomplete
        filters={$fileFilters}
        onFiltersChange={handleFiltersChange}
        onClearAll={clearAllFilters}
        {candidates}
      />
      {#if isFiltered}
        <button class="clear-all" onclick={clearAllFilters}>clear all</button>
      {/if}
    </div>
    {#if $fileFilters.length > 0 || timeRange}
      <div class="filter-chips">
        {#each $fileFilters as f (f)}
          <div class="filter-chip" title={f}>
            <span class="chip-text">{f}</span>
            <button class="chip-x" onclick={() => removeFileFilter(f)} aria-label="Remove filter: {f}">&times;</button>
          </div>
        {/each}
        {#if timeRange}
          <div class="filter-chip" title={timeRange.tooltip}>
            <span class="chip-text">{timeRange.text}</span>
            <button class="chip-x" onclick={clearTimeRange} aria-label="Clear time range">&times;</button>
          </div>
        {/if}
      </div>
    {/if}
  </div>

  <HistogramRange />

  {#if isGrouped}
    <!-- Grouped by file accordion -->
    <div class="entry-list" bind:this={listEl} onscroll={handleScroll}>
      {#if sortedFiles.length === 0 && !$timelineLoading}
        <div class="empty">No changes recorded yet.</div>
      {/if}

      {#each dirGroups as dirGroup (dirGroup.dir)}
        {#if dirGroup.dir}
          <div class="dir-separator">{dirGroup.dir}</div>
        {/if}
        {#each dirGroup.files as group (fileKey(group))}
          <div class="file-group">
            <button class="file-group-header" onclick={() => toggleFile(fileKey(group))}>
              <span class="chevron" class:expanded={expandedFiles.has(fileKey(group))}>&#9654;</span>
              <span class="group-count">{group.entries.length}</span>
              <span class="group-name">{formatFileName(group.path)}</span>
            </button>

            {#if expandedFiles.has(fileKey(group))}
              <div class="group-entries">
                {#each group.entries as entry (entry.id)}
                  <button
                    class="entry nested"
                    class:selected={$selectedEntry?.id === entry.id}
                    onclick={() => selectEntry(entry)}
                  >
                    <span class="timestamp">{formatTime(entry.timestamp)}</span>
                    <span class="event-badge" style="color: {eventColor(entry.event)}">{entry.event}</span>
                    {#if entry.event === "modified"}
                      <span class="delta">
                        <span class="add">+{entry.lines_added}</span>
                        <span class="del">-{entry.lines_removed}</span>
                      </span>
                    {/if}
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/each}
      {/each}

      {#if $timelineLoading}
        <div class="loading">Loading...</div>
      {/if}
    </div>
  {:else}
    <!-- Flat timeline -->
    <div class="entry-list" bind:this={listEl} onscroll={handleScroll}>
      {#if $timelineEntries.length === 0 && !$timelineLoading}
        <div class="empty">No changes recorded yet.</div>
      {/if}

      {#each $timelineEntries as entry (entry.id)}
        <button
          class="entry"
          class:selected={$selectedEntry?.id === entry.id}
          onclick={() => selectEntry(entry)}
        >
          <div class="entry-top">
            <span class="timestamp">{formatTime(entry.timestamp)}</span>
            {#if entry.event === "modified"}
              <span class="delta">
                <span class="add">+{entry.lines_added}</span>
                <span class="del">-{entry.lines_removed}</span>
              </span>
            {/if}
          </div>
          <div class="entry-bottom">
            <span class="filepath">
              {#if isGlobal && entry.project}
                <span class="project-label">{projectShortName(entry.project)}/</span>
              {/if}
              {truncatePath(entry.file)}
            </span>
            <span class="event-badge" style="color: {eventColor(entry.event)}">{entry.event}</span>
          </div>
        </button>
      {/each}

      {#if $timelineLoading}
        <div class="loading">Loading...</div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .timeline-container {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  /* Filter area */
  .filter-area {
    padding: 4px 12px 8px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .filter-input-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .filter-input-row :global(.filter-autocomplete-container) {
    flex: 1;
    min-width: 0;
  }
  .clear-all {
    background: none;
    border: none;
    color: var(--accent);
    cursor: pointer;
    font-size: var(--text-xs);
    font-family: var(--font-sans);
    text-decoration: underline;
    padding: 0;
    flex-shrink: 0;
    white-space: nowrap;
  }
  .clear-all:hover {
    opacity: 0.8;
  }
  .filter-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .filter-chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 6px;
    background: var(--accent-bg);
    color: var(--accent);
    border-radius: 3px;
    font-size: var(--text-sm);
    border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
  }
  .chip-text {
    font-family: var(--font-mono);
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }
  .chip-x {
    background: none;
    border: none;
    color: var(--accent);
    cursor: pointer;
    padding: 0;
    font-size: 14px;
    line-height: 1;
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    border-radius: 2px;
  }
  .chip-x:hover {
    background: rgba(0, 0, 0, 0.1);
  }

  /* Summary / Metrics */
  .summary {
    padding: 12px 16px 8px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .summary.pulsing .metric-value {
    animation: pulse 600ms ease;
  }
  @keyframes pulse {
    0% { transform: scale(1); color: var(--text-primary); }
    40% { transform: scale(1.15); color: var(--accent); }
    100% { transform: scale(1); color: var(--text-primary); }
  }
  .metrics {
    display: flex;
    align-items: baseline;
    gap: 6px;
    margin-bottom: 6px;
  }
  .metric {
    display: flex;
    align-items: baseline;
    gap: 4px;
  }
  .metric-value {
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
    font-variant-numeric: tabular-nums;
    display: inline-block;
  }
  .metric-label {
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }
  .metric-of {
    font-size: var(--text-sm);
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }
  .metric-sep {
    color: var(--text-muted);
  }
  .summary-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .view-toggle {
    display: flex;
    gap: 2px;
    margin-left: auto;
  }
  .toggle-btn {
    width: 28px;
    height: 24px;
    border: 1px solid var(--border);
    background: none;
    border-radius: 4px;
    cursor: pointer;
    color: var(--text-muted);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .toggle-btn.active {
    background: var(--accent-bg);
    color: var(--accent);
    border-color: var(--accent);
  }

  /* Shared */
  .empty {
    padding: 24px 16px;
    color: var(--text-muted);
    text-align: center;
  }
  .entry-list {
    flex: 1;
    overflow-y: auto;
  }
  .loading {
    padding: 16px;
    text-align: center;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }

  /* Grouped view */
  .file-group {
    position: relative;
  }
  .file-group-header {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 10px 16px;
    border: none;
    background: none;
    cursor: pointer;
    text-align: left;
    font-family: var(--font-sans);
  }
  .file-group-header:hover {
    background: var(--accent-bg);
  }
  .chevron {
    font-size: 8px;
    transition: transform 150ms;
    color: var(--text-muted);
    flex-shrink: 0;
  }
  .chevron.expanded {
    transform: rotate(90deg);
  }
  .group-name {
    font-weight: 600;
    font-size: var(--text-base);
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dir-separator {
    padding: 6px 16px 2px;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-muted);
    border-top: 1px solid var(--border);
    background: var(--bg);
    position: sticky;
    top: 0;
    z-index: 1;
  }
  .dir-separator:first-child {
    border-top: none;
  }
  .group-count {
    flex-shrink: 0;
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--text-secondary);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 1px 6px;
    font-variant-numeric: tabular-nums;
    min-width: 20px;
    text-align: center;
  }
  .group-entries {
    border-top: 1px solid var(--border);
    background: var(--surface);
  }
  .entry.nested {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 16px 6px 32px;
    border: none;
    border-left: 3px solid transparent;
    background: none;
    cursor: pointer;
    text-align: left;
    font-family: var(--font-sans);
  }
  .entry.nested:hover {
    background: var(--accent-bg);
  }
  .entry.nested.selected {
    border-left-color: var(--accent);
    background: var(--accent-bg);
  }
  .entry.nested + .entry.nested {
    border-top: 1px solid var(--border);
  }

  /* Flat view */
  .entry:not(.nested) {
    display: block;
    width: 100%;
    padding: 10px 16px;
    border: none;
    border-left: 3px solid transparent;
    background: none;
    cursor: pointer;
    text-align: left;
    font-family: var(--font-sans);
    transition: border-color 150ms;
  }
  .entry:not(.nested):hover {
    background: var(--accent-bg);
  }
  .entry:not(.nested).selected {
    border-left-color: var(--accent);
    background: var(--accent-bg);
  }
  .entry:not(.nested) + .entry:not(.nested) {
    border-top: 1px solid var(--border);
  }
  .entry-top {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 2px;
  }
  .timestamp {
    font-size: var(--text-sm);
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
  }
  .delta {
    font-size: var(--text-sm);
    font-family: var(--font-mono);
  }
  .add { color: var(--addition); }
  .del { color: var(--deletion); margin-left: 4px; }
  .entry-bottom {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .filepath {
    font-family: var(--font-mono);
    font-size: var(--text-base);
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .project-label {
    color: var(--accent);
    font-weight: 600;
  }
  .event-badge {
    font-size: var(--text-xs);
    flex-shrink: 0;
    margin-left: 8px;
  }
</style>
