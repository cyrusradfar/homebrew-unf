<script lang="ts">
import { onDestroy, onMount } from "svelte";
import { get } from "svelte/store";
import ContextualDiffView from "./components/ContextualDiffView.svelte";
import FileTimeline from "./components/FileTimeline.svelte";
import Toast from "./components/Toast.svelte";
import TopBar from "./components/TopBar.svelte";
import { listProjects } from "./lib/api";
import {
	activateGlobal,
	activateProject,
	triggerFilteredReload,
	triggerLoadMore,
	triggerTimeRangeReload,
} from "./lib/data-loader";
import { handleGlobalKeydown } from "./lib/keyboard";
import { stopPolling } from "./lib/polling";
import {
	activeTab,
	error,
	fileFilters,
	hasTabData,
	histogramEnd,
	histogramStart,
	loadPersistedTabs,
	nextCursor,
	openTab,
	openTabs,
	projects,
	selectedEntry,
	timelineLoading,
} from "./lib/stores";
import { GLOBAL_TAB as GLOBAL_TAB_CONST } from "./lib/types";
import "./lib/reset.css";
import "./lib/fonts.css";
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
	sidebarWidth = Math.min(Math.max(200, e.clientX - rect.left), 600);
}

function handleDragEnd() {
	isDragging = false;
}

onDestroy(() => stopPolling());

onMount(async () => {
	try {
		const result = await listProjects();
		projects.set(result.projects);
		const knownPaths = new Set(result.projects.map((p) => p.path));

		const saved = loadPersistedTabs();
		const validTabs = saved.openTabs.filter((p) => p === GLOBAL_TAB_CONST || knownPaths.has(p));
		if (!validTabs.includes(GLOBAL_TAB_CONST)) validTabs.unshift(GLOBAL_TAB_CONST);

		if (validTabs.length > 0) {
			const active = validTabs.includes(saved.activeTab ?? "") ? saved.activeTab! : validTabs[0];
			openTabs.set(validTabs);
			openTab(active);
		}
	} catch (e) {
		error.set(`Failed to load projects: ${e}`);
	}
});

// Re-load data when file filters change (debounced 150ms)
let filterInitialized = false;
let prevFilterVal = "";
let filterTimer: ReturnType<typeof setTimeout> | null = null;
$effect(() => {
	const serialized = JSON.stringify($fileFilters);
	if (!filterInitialized) {
		filterInitialized = true;
		prevFilterVal = serialized;
		return;
	}
	if (serialized === prevFilterVal) return;
	prevFilterVal = serialized;
	if (filterTimer) clearTimeout(filterTimer);
	filterTimer = setTimeout(() => {
		if (get(activeTab)) triggerFilteredReload();
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
	if (get(activeTab)) triggerTimeRangeReload();
});

// When active tab changes, activate on backend
let prevActiveTab: string | null = null;
$effect(() => {
	const current = $activeTab;
	if (current === prevActiveTab) return;
	prevActiveTab = current;
	if (current === GLOBAL_TAB_CONST) activateGlobal(hasTabData(GLOBAL_TAB_CONST));
	else if (current) activateProject(current, hasTabData(current));
	else stopPolling();
});
</script>

<svelte:window on:keydown={(e) => handleGlobalKeydown(e)} on:mousemove={handleDragMove} on:mouseup={handleDragEnd} />

<div
  class="app-layout"
  class:resizing={isDragging}
  style="grid-template-columns: {sidebarWidth}px 1fr"
  bind:this={layoutEl}
>
  <div class="topbar">
    <TopBar onProjectOpened={(path) => openTab(path)} />
  </div>

  <div class="timeline">
    {#if $activeTab}
      <FileTimeline onLoadMore={() => {
        if ($activeTab === GLOBAL_TAB_CONST || $timelineLoading) return;
        const cursor = $nextCursor;
        if (cursor) triggerLoadMore(cursor);
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
