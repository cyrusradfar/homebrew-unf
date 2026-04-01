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
    openTabs,
    openTab,
    histogramStart,
    histogramEnd,
    loadPersistedTabs,
    hasTabData,
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

  // Monotonic counter to discard stale async responses.
  // Each data-loading function captures the current value; if it has
  // changed by the time the response arrives, the result is discarded.
  let requestGen = 0;

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
        // Set all tabs at once to avoid N sequential activations.
        // Only openTab(active) triggers the $effect → activateProject.
        openTabs.set(validTabs);
        openTab(active);
      }
    } catch (e) {
      error.set(`Failed to load projects: ${e}`);
    }
  });

  function handleSelectProject(path: string) {
    openTab(path);
    // activateProject is triggered by the activeTab $effect
  }

  /** Select a project on the backend, load all data, start polling.
   *  If cached is true, skip the full data reload (tab already has data from restoreTabState). */
  async function activateProject(path: string, cached = false) {
    const gen = ++requestGen;
    stopPolling();
    error.set(null);
    try {
      const status = await selectProject(path);
      if (gen !== requestGen) return;
      projectStatus.set(status);
      if (!cached) {
        await refreshAllData(gen);
        if (gen !== requestGen) return;
      }
      startPolling(() => refreshAllData(requestGen), cached);
    } catch (e) {
      if (gen === requestGen) {
        error.set(`Failed to activate project: ${e}`);
      }
    }
  }

  /** Activate the global (cross-project) view.
   *  If cached is true, skip the full data reload (tab already has data from restoreTabState). */
  async function activateGlobal(cached = false) {
    const gen = ++requestGen;
    stopPolling();
    error.set(null);
    projectStatus.set(null);
    try {
      if (!cached) {
        await refreshGlobalData(gen);
        if (gen !== requestGen) return;
      }
      startPolling(() => refreshGlobalData(requestGen));
    } catch (e) {
      if (gen === requestGen) {
        error.set(`Failed to load global view: ${e}`);
      }
    }
  }

  /** Reload timeline and file tree for the global view. */
  async function refreshGlobalData(gen: number) {
    const globs = filtersToGlobs(get(fileFilters));
    const include = globs.length > 0 ? globs : undefined;
    const histStart = get(histogramStart);
    const histEnd = get(histogramEnd);
    await Promise.all([
      loadGlobalTimeline(gen, include, histStart, histEnd),
      loadGlobalFileTree(gen, include, histStart, histEnd),
      loadGlobalDensity(gen, include),
    ]);
  }

  /** Reload timeline, file tree, and density using current store values. */
  async function refreshAllData(gen: number) {
    const globs = filtersToGlobs(get(fileFilters));
    const include = globs.length > 0 ? globs : undefined;
    const histStart = get(histogramStart);
    const histEnd = get(histogramEnd);
    await Promise.all([
      loadTimeline(gen, undefined, include, histStart, histEnd),
      loadFileTree(gen, include, histStart, histEnd),
      loadDensity(gen, include),
    ]);
  }

  async function loadGlobalTimeline(gen: number, include?: string[], since?: string | null, until?: string | null) {
    timelineLoading.set(true);
    try {
      const result = await getGlobalLog({ limit: 200, include, since: since ?? undefined });
      if (gen !== requestGen) return;
      let entries = result.entries;
      if (until) entries = entries.filter((e) => e.timestamp <= until);
      timelineEntries.set(entries);
      nextCursor.set(null);
    } catch (e) {
      if (gen === requestGen) error.set(`Failed to load global timeline: ${e}`);
    } finally {
      if (gen === requestGen) timelineLoading.set(false);
    }
  }

  async function loadGlobalFileTree(gen: number, include?: string[], since?: string | null, until?: string | null) {
    try {
      const result = await getGlobalLog({
        groupByFile: true, include, since: since ?? undefined, limit: 100000,
      }) as GlobalGroupedLogResponse;
      if (gen !== requestGen) return;
      const files: GroupedLogFile[] = [];
      for (const proj of result.projects) {
        for (const file of proj.files) {
          const taggedEntries = file.entries.map((e) => ({ ...e, project: proj.project }));
          let entries = taggedEntries;
          if (until) entries = entries.filter((e) => e.timestamp <= until);
          if (entries.length > 0) {
            files.push({ ...file, entries, change_count: entries.length, project: proj.project });
          }
        }
      }
      fileTree.set(files);
    } catch (_e) {
      // Non-critical
    }
  }

  async function loadTimeline(gen: number, cursor?: string, include?: string[], since?: string | null, until?: string | null) {
    timelineLoading.set(true);
    try {
      const result = await getLog({
        limit: 50, cursor: cursor ?? undefined, include, since: since ?? undefined,
      });
      if (gen !== requestGen) return;
      let entries = result.entries;
      if (until) entries = entries.filter((e) => e.timestamp <= until);
      if (cursor) {
        timelineEntries.update((prev) => [...prev, ...entries]);
      } else {
        timelineEntries.set(entries);
      }
      nextCursor.set(result.next_cursor);
    } catch (e) {
      if (gen === requestGen) error.set(`Failed to load timeline: ${e}`);
    } finally {
      if (gen === requestGen) timelineLoading.set(false);
    }
  }

  async function loadFileTree(gen: number, include?: string[], since?: string | null, until?: string | null) {
    try {
      const result = await getLog({
        groupByFile: true, include, since: since ?? undefined, limit: 100000,
      });
      if (gen !== requestGen) return;
      let files = result.files;
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

  async function loadDensity(gen: number, include?: string[]) {
    try {
      const result = await getDensity({ buckets: 100, include });
      if (gen !== requestGen) return;
      densityBuckets.set(result.buckets);
    } catch (_e) {
      // Non-critical
    }
  }

  async function loadGlobalDensity(gen: number, include?: string[]) {
    try {
      const result = await getGlobalDensity({ buckets: 100, include });
      if (gen !== requestGen) return;
      densityBuckets.set(result.buckets);
    } catch (_e) {
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

  // Re-load data when file filters change (debounced 150ms, skip initial mount)
  let filterInitialized = false;
  let prevFilterVal: string = "";
  let filterDebounceTimer: ReturnType<typeof setTimeout> | null = null;
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
    if (filterDebounceTimer) clearTimeout(filterDebounceTimer);
    filterDebounceTimer = setTimeout(() => {
      if (!get(activeTab)) return;
      const gen = ++requestGen;
      const globs = filtersToGlobs(currentFilters);
      const include = globs.length > 0 ? globs : undefined;
      const since = get(histogramStart);
      const until = get(histogramEnd);
      if (isGlobal()) {
        loadGlobalTimeline(gen, include, since, until);
        loadGlobalFileTree(gen, include, since, until);
        loadGlobalDensity(gen, include);
      } else {
        loadTimeline(gen, undefined, include, since, until);
        loadDensity(gen, include);
        loadFileTree(gen, include, since, until);
      }
    }, 150);
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
      const gen = ++requestGen;
      const globs = filtersToGlobs(get(fileFilters));
      const include = globs.length > 0 ? globs : undefined;
      if (isGlobal()) {
        loadGlobalTimeline(gen, include, start, end);
        loadGlobalFileTree(gen, include, start, end);
      } else {
        loadTimeline(gen, undefined, include, start, end);
        loadFileTree(gen, include, start, end);
      }
    }
  });

  // When active tab changes, select project on backend and load data.
  // If the tab has cached data from a previous visit, show it immediately
  // and let the poller handle incremental updates.
  let prevActiveTab: string | null = null;
  $effect(() => {
    const current = $activeTab;
    if (current === prevActiveTab) return;
    prevActiveTab = current;
    if (current === GLOBAL_TAB_CONST) {
      activateGlobal(hasTabData(GLOBAL_TAB_CONST));
    } else if (current) {
      activateProject(current, hasTabData(current));
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
        if ($timelineLoading) return;
        const cursor = $nextCursor;
        if (cursor) {
          const globs = filtersToGlobs(get(fileFilters));
          const include = globs.length > 0 ? globs : undefined;
          loadTimeline(requestGen, cursor, include, get(histogramStart), get(histogramEnd));
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
