<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { get } from "svelte/store";
  import { listProjects, selectProject, getLog, getGlobalLog, getDensity, getGlobalDensity } from "./lib/api";
  import {
    projects,
    projectStatus,
    timelineEntries,
    nextCursor,
    timelineLoading,
    fileTree,
    fileFilters,
    selectedEntry,
    densityBuckets,
    error,
    activeTab,
    openTab,
    histogramStart,
    histogramEnd,
    loadPersistedTabs,
  } from "./lib/stores";
  import { GLOBAL_TAB as GLOBAL_TAB_CONST } from "./lib/types";
  import type { GlobalGroupedLogResponse, GroupedLogFile } from "./lib/types";
  import { filtersToGlobs } from "./lib/filterUtils";
  import { startPolling, stopPolling } from "./lib/polling";
  import { handleGlobalKeydown } from "./lib/keyboard";
  import TopBar from "./components/TopBar.svelte";
  import FileTimeline from "./components/FileTimeline.svelte";
  import ContextualDiffView from "./components/ContextualDiffView.svelte";
  import Toast from "./components/Toast.svelte";
  import "./lib/layout.css";

  let sidebarWidth = $state(320);
  let isDragging = $state(false);
  let layoutEl: HTMLDivElement | undefined = $state();

  function handleDragStart(e: MouseEvent) {
    e.preventDefault();
    isDragging = true;
  }

  function handleDragMove(e: MouseEvent) {
    if (!isDragging || !layoutEl) return;
    const rect = layoutEl.getBoundingClientRect();
    const newWidth = Math.min(Math.max(200, e.clientX - rect.left), 600);
    sidebarWidth = newWidth;
  }

  function handleDragEnd() {
    isDragging = false;
  }

  onDestroy(() => {
    stopPolling();
  });

  onMount(async () => {
    try {
      const result = await listProjects();
      projects.set(result.projects);
      const knownPaths = new Set(result.projects.map((p) => p.path));

      // Restore persisted tabs, filtering out any that no longer exist
      const saved = loadPersistedTabs();
      // Global tab is always valid; project tabs must exist
      const validTabs = saved.openTabs.filter(
        (p) => p === GLOBAL_TAB_CONST || knownPaths.has(p)
      );

      // Ensure global tab is always present as first tab
      if (!validTabs.includes(GLOBAL_TAB_CONST)) {
        validTabs.unshift(GLOBAL_TAB_CONST);
      }

      if (validTabs.length > 0) {
        const active = validTabs.includes(saved.activeTab ?? "")
          ? saved.activeTab!
          : validTabs[0];
        // Register all tabs; $effect handles data loading for the active one
        for (const tab of validTabs) {
          openTab(tab);
        }
        if (active !== validTabs[validTabs.length - 1]) {
          openTab(active);
        }
      }
    } catch (e) {
      error.set(`Failed to load projects: ${e}`);
    }
  });

  function handleSelectProject(path: string) {
    openTab(path);
    // activateProject is triggered by the activeTab $effect
  }

  /** Select a project on the backend, load all data, start polling. */
  async function activateProject(path: string) {
    stopPolling(); // Stop old polling FIRST to prevent stale data races
    error.set(null);
    try {
      const status = await selectProject(path);
      // RACE: User may switch tabs during the await above. Check if we're still the active tab.
      if (get(activeTab) !== path) return;
      projectStatus.set(status);
      await refreshAllData();
      // RACE: User may switch tabs during refreshAllData. Verify before polling starts.
      if (get(activeTab) !== path) return;
      startPolling(() => refreshAllData());
    } catch (e) {
      // RACE: Only set error if we're still the active tab. Stale errors from old activations
      // should not overwrite new tab's state.
      if (get(activeTab) === path) {
        error.set(`Failed to activate project: ${e}`);
      }
    }
  }

  /** Activate the global (cross-project) view. */
  async function activateGlobal() {
    stopPolling();
    error.set(null);
    projectStatus.set(null);
    try {
      await refreshGlobalData();
      // RACE: User may switch away from global tab during refreshGlobalData.
      // Only keep polling if we're still on the global tab. activateProject will
      // handle polling restart if user switches to a project tab.
      if (get(activeTab) === GLOBAL_TAB_CONST) {
        startPolling(() => refreshGlobalData());
      }
    } catch (e) {
      // RACE: Only set error if we're still on global tab.
      if (get(activeTab) === GLOBAL_TAB_CONST) {
        error.set(`Failed to load global view: ${e}`);
      }
    }
  }

  /** Reload timeline and file tree for the global view. */
  async function refreshGlobalData() {
    const globs = filtersToGlobs(get(fileFilters));
    const include = globs.length > 0 ? globs : undefined;
    const histStart = get(histogramStart);
    const histEnd = get(histogramEnd);
    await Promise.all([
      loadGlobalTimeline(include, histStart, histEnd),
      loadGlobalFileTree(include, histStart, histEnd),
      loadGlobalDensity(include),
    ]);
  }

  /** Reload timeline, file tree, and density using current store values. */
  async function refreshAllData() {
    const globs = filtersToGlobs(get(fileFilters));
    const include = globs.length > 0 ? globs : undefined;
    const histStart = get(histogramStart);
    const histEnd = get(histogramEnd);
    await Promise.all([
      loadTimeline(undefined, include, histStart, histEnd),
      loadFileTree(include, histStart, histEnd),
      loadDensity(include),
    ]);
  }

  async function loadGlobalTimeline(include?: string[], since?: string | null, until?: string | null) {
    timelineLoading.set(true);
    try {
      const result = await getGlobalLog({
        limit: 200,
        include,
        since: since ?? undefined,
      });
      let entries = result.entries;
      if (until) {
        entries = entries.filter((e) => e.timestamp <= until);
      }
      timelineEntries.set(entries);
      nextCursor.set(null); // Global mode doesn't support cursor pagination
    } catch (e) {
      error.set(`Failed to load global timeline: ${e}`);
    } finally {
      timelineLoading.set(false);
    }
  }

  async function loadGlobalFileTree(include?: string[], since?: string | null, until?: string | null) {
    try {
      const result = await getGlobalLog({
        groupByFile: true,
        include,
        since: since ?? undefined,
        limit: 100000,
      }) as GlobalGroupedLogResponse;
      // Convert global grouped response to flat GroupedLogFile[] with project field
      const files: GroupedLogFile[] = [];
      for (const proj of result.projects) {
        for (const file of proj.files) {
          // Tag each entry with its project
          const taggedEntries = file.entries.map((e) => ({ ...e, project: proj.project }));
          let entries = taggedEntries;
          if (until) {
            entries = entries.filter((e) => e.timestamp <= until);
          }
          if (entries.length > 0) {
            files.push({
              ...file,
              entries,
              change_count: entries.length,
              project: proj.project,
            });
          }
        }
      }
      fileTree.set(files);
    } catch (_e) {
      // Non-critical
    }
  }

  async function loadTimeline(cursor?: string, include?: string[], since?: string | null, until?: string | null) {
    timelineLoading.set(true);
    try {
      const result = await getLog({
        limit: 50,
        cursor: cursor ?? undefined,
        include,
        since: since ?? undefined,
      });
      let entries = result.entries;
      // Client-side upper-bound filter (backend only supports --since, not --until)
      if (until) {
        entries = entries.filter((e) => e.timestamp <= until);
      }
      if (cursor) {
        timelineEntries.update((prev) => [...prev, ...entries]);
      } else {
        timelineEntries.set(entries);
      }
      nextCursor.set(result.next_cursor);
    } catch (e) {
      error.set(`Failed to load timeline: ${e}`);
    } finally {
      timelineLoading.set(false);
    }
  }

  async function loadFileTree(include?: string[], since?: string | null, until?: string | null) {
    try {
      const result = await getLog({
        groupByFile: true,
        include,
        since: since ?? undefined,
        limit: 100000, // Override default 1000 cap for accurate counts
      });
      let files = result.files;
      // Client-side upper-bound filter (backend only supports --since, not --until)
      if (until) {
        files = files
          .map((f) => {
            const filtered = f.entries.filter((e) => e.timestamp <= until);
            return { ...f, entries: filtered, change_count: filtered.length };
          })
          .filter((f) => f.entries.length > 0);
      }
      fileTree.set(files);
    } catch (_e) {
      // Non-critical
    }
  }

  async function loadDensity(include?: string[]) {
    try {
      const result = await getDensity({
        buckets: 100,
        include,
      });
      densityBuckets.set(result.buckets);
    } catch (_e) {
      // Non-critical
    }
  }

  async function loadGlobalDensity(include?: string[]) {
    try {
      const result = await getGlobalDensity({
        buckets: 100,
        include,
      });
      densityBuckets.set(result.buckets);
    } catch (_e) {
      // Non-critical — show empty histogram
      densityBuckets.set([]);
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    handleGlobalKeydown(e);
  }

  /** Determine if the current tab is global */
  function isGlobal(): boolean {
    return get(activeTab) === GLOBAL_TAB_CONST;
  }

  // Re-load data when file filters change (skip initial mount)
  let filterInitialized = false;
  let prevFilterVal: string = "";
  $effect(() => {
    const currentFilters = $fileFilters;
    const serialized = JSON.stringify(currentFilters);
    if (!filterInitialized) {
      filterInitialized = true;
      prevFilterVal = serialized;
      return;
    }
    if (serialized === prevFilterVal) return;
    prevFilterVal = serialized;
    if (get(activeTab)) {
      const globs = filtersToGlobs(currentFilters);
      const include = globs.length > 0 ? globs : undefined;
      const since = get(histogramStart);
      const until = get(histogramEnd);
      // RACE: Multiple filter changes can trigger rapid-fire concurrent requests.
      // If user types in filter box repeatedly, previous requests may complete after
      // newer ones, overwriting fileTree/timelineEntries with stale data.
      // Mitigation: Use timelineLoading guard in pagination, but filter changes bypass this.
      // Consider: Add request ID or cancellation token for filter/histogram effects.
      if (isGlobal()) {
        loadGlobalTimeline(include, since, until);
        loadGlobalFileTree(include, since, until);
        loadGlobalDensity(include);
      } else {
        loadTimeline(undefined, include, since, until);
        loadDensity(include);
        loadFileTree(include, since, until);
      }
    }
  });

  // Re-load data when histogram range changes
  let histInitialized = false;
  let prevHistStart: string | null = null;
  let prevHistEnd: string | null = null;
  $effect(() => {
    const start = $histogramStart;
    const end = $histogramEnd;
    if (!histInitialized) {
      histInitialized = true;
      prevHistStart = start;
      prevHistEnd = end;
      return;
    }
    if (start === prevHistStart && end === prevHistEnd) return;
    prevHistStart = start;
    prevHistEnd = end;
    if (get(activeTab)) {
      const globs = filtersToGlobs(get(fileFilters));
      const include = globs.length > 0 ? globs : undefined;
      // RACE: Histogram range drags can fire multiple updates quickly. Concurrent
      // loadTimeline/loadFileTree calls could complete out of order, corrupting the
      // timeline/fileTree state with stale data.
      // Mitigation: Consider debouncing histogram changes or using request IDs.
      if (isGlobal()) {
        loadGlobalTimeline(include, start, end);
        loadGlobalFileTree(include, start, end);
      } else {
        loadTimeline(undefined, include, start, end);
        loadFileTree(include, start, end);
      }
    }
  });

  // When active tab changes, select project on backend and load data
  let prevActiveTab: string | null = null;
  $effect(() => {
    const current = $activeTab;
    if (current === prevActiveTab) return;
    prevActiveTab = current;
    if (current === GLOBAL_TAB_CONST) {
      activateGlobal();
    } else if (current) {
      activateProject(current);
    } else {
      stopPolling();
    }
  });
</script>

<svelte:window on:keydown={handleKeydown} on:mousemove={handleDragMove} on:mouseup={handleDragEnd} />

<div
  class="app-layout"
  class:resizing={isDragging}
  style="grid-template-columns: {sidebarWidth}px 1fr"
  bind:this={layoutEl}
>
  <div class="topbar">
    <TopBar onProjectOpened={handleSelectProject} />
  </div>

  <div class="timeline">
    {#if $activeTab}
      <FileTimeline onLoadMore={() => {
        if ($activeTab === GLOBAL_TAB_CONST) return; // No pagination in global mode
        // RACE: Prevent multiple concurrent pagination requests. User clicking "Load More"
        // while a previous pagination is in flight could cause out-of-order state updates
        // (timelineEntries.update() concatenates, so order matters).
        if ($timelineLoading) return;
        const cursor = $nextCursor;
        if (cursor) {
          const globs = filtersToGlobs(get(fileFilters));
          const include = globs.length > 0 ? globs : undefined;
          loadTimeline(cursor, include, get(histogramStart), get(histogramEnd));
        }
      }} />
    {:else}
      <div class="empty-state">
        <p>Select a project to view its timeline.</p>
      </div>
    {/if}
  </div>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="resize-handle" onmousedown={handleDragStart}></div>

  <div class="content">
    {#if $selectedEntry}
      <ContextualDiffView />
    {:else}
      <div class="empty-state">
        <p>Select a change from the timeline to view its diff.</p>
      </div>
    {/if}
  </div>
</div>

<Toast />

<style>
  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-muted);
    text-align: center;
    padding: 24px;
    gap: 8px;
  }
  .resize-handle {
    grid-row: 2;
    grid-column: 2;
    width: 6px;
    margin-left: -3px;
    cursor: col-resize;
    z-index: 5;
    position: relative;
  }
  .resize-handle:hover,
  .resizing .resize-handle {
    background: var(--accent);
    opacity: 0.3;
  }
  .resizing {
    cursor: col-resize;
    user-select: none;
  }
</style>
