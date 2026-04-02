<script lang="ts">
import { getDiff, getFileContent } from "../lib/api";
import {
	buildRenderItems,
	type CollapsedRegion,
	findWordDiffPairs,
	formatHunkHeader,
} from "../lib/diff-helpers";
import { highlightLines } from "../lib/highlight";
import { detectLanguage } from "../lib/language-map";
import {
	contentData,
	contentLoading,
	diffData,
	error,
	selectedEntry,
	viewMode,
} from "../lib/stores";
import type { WordSegment } from "../lib/word-diff";

// Track which entry is currently loaded to avoid redundant fetches
let currentEntryId = $state<number | null>(null);
let currentViewMode = $state<"diff" | "content" | null>(null);

// Unified effect: fetch diff or content based on viewMode and selected entry
$effect(() => {
	const entry = $selectedEntry;
	const mode = $viewMode;

	if (!entry) return;

	// Reset tracking if entry or mode changed
	if (entry.id !== currentEntryId || mode !== currentViewMode) {
		currentEntryId = entry.id;
		currentViewMode = mode;
		contentLoading.set(true);

		if (mode === "content") {
			// Fetch file content (pass project for global mode entries)
			getFileContent({ file: entry.file, at: entry.timestamp, project: entry.project })
				.then((result) => {
					contentData.set(result);
				})
				.catch((e) => {
					error.set(`Failed to load content: ${e}`);
					contentData.set(null);
				})
				.finally(() => {
					contentLoading.set(false);
				});
		} else {
			// Fetch diff (pass project for global mode entries)
			getDiff({ snapshot: entry.id, project: entry.project })
				.then((result) => {
					diffData.set(result);
				})
				.catch((e) => {
					error.set(`Failed to load diff: ${e}`);
					diffData.set(null);
				})
				.finally(() => {
					contentLoading.set(false);
				});
		}
	}
});

// Map from line content to highlighted HTML (used in both diff and raw modes)
let highlightedMap = $state(new Map<string, string>());

// Highlight all lines when diff data changes or in raw content mode
// NOTE: Do NOT use highlightedMap.clear() here — reading the $state variable
// inside the effect creates a dependency, and the async reassignment below
// would re-trigger this effect infinitely. Assigning a new Map avoids the read.
$effect(() => {
	highlightedMap = new Map();

	// Skip highlighting for large diffs unless user opted in
	if (totalDiffLines > DIFF_LINE_LIMIT && !showFullDiff) return;

	if ($viewMode === "diff") {
		const data = $diffData;
		if (!data || data.changes.length === 0) {
			return;
		}

		const change = data.changes[0];
		if (!change.hunks) {
			return;
		}

		const lang = detectLanguage(change.file);
		if (!lang) {
			return;
		}

		// Collect all unique line contents from diff hunks
		const allLines = change.hunks.flatMap((h) => h.lines.map((l) => l.content));

		highlightLines(allLines, lang).then((htmls) => {
			const map = new Map<string, string>();
			allLines.forEach((line, i) => {
				if (!map.has(line)) {
					map.set(line, htmls[i]);
				}
			});
			highlightedMap = map;
		});
	} else if ($viewMode === "content") {
		const data = $contentData;
		if (!data?.content) {
			return;
		}

		const lang = detectLanguage(data.file);
		if (!lang) {
			return;
		}

		// Collect all lines from raw content
		const allLines = data.content.split("\n");

		highlightLines(allLines, lang).then((htmls) => {
			const map = new Map<string, string>();
			allLines.forEach((line, i) => {
				if (!map.has(line)) {
					map.set(line, htmls[i]);
				}
			});
			highlightedMap = map;
		});
	}
});

// Large diff protection: count total lines across all hunks
const DIFF_LINE_LIMIT = 5000;
let totalDiffLines = $derived(
	($diffData?.changes[0]?.hunks ?? []).reduce((sum, h) => sum + h.lines.length, 0)
);
let isDiffTruncated = $state(false);
let showFullDiff = $state(false);

// Reset truncation state when diff data changes
$effect(() => {
	if ($diffData) {
		isDiffTruncated = totalDiffLines > DIFF_LINE_LIMIT;
		showFullDiff = false;
	}
});

// Only compute word diffs if under the limit (or user opted in)
let wordDiffsByHunk = $derived(
	totalDiffLines > DIFF_LINE_LIMIT && !showFullDiff
		? []
		: ($diffData?.changes[0]?.hunks ?? []).map((hunk) => findWordDiffPairs(hunk.lines))
);

// Track which collapsed regions are expanded
let expandedRegions = $state(new Set<number>());

function toggleCollapsedRegion(regionIndex: number) {
	if (expandedRegions.has(regionIndex)) {
		expandedRegions.delete(regionIndex);
	} else {
		expandedRegions.add(regionIndex);
	}
}
</script>

<div class="diff-container">
  <div class="toolbar">
    <div class="tabs">
      <button class="tab {$viewMode === 'diff' ? 'active' : ''}" onclick={() => viewMode.set("diff")}>
        <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor"><path d="M8 4a.5.5 0 0 1 .5.5v3h3a.5.5 0 0 1 0 1h-3v3a.5.5 0 0 1-1 0v-3h-3a.5.5 0 0 1 0-1h3v-3A.5.5 0 0 1 8 4z"/><path d="M2 1a1 1 0 0 0-1 1v12a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V2a1 1 0 0 0-1-1H2zm0-1h12a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H2a2 2 0 0 1-2-2V2a2 2 0 0 1 2-2z"/></svg>
        Diff
      </button>
      <button class="tab {$viewMode === 'content' ? 'active' : ''}" onclick={() => viewMode.set("content")}>
        <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor"><path d="M5.5 7a.5.5 0 0 0 0 1h5a.5.5 0 0 0 0-1h-5zM5 9.5a.5.5 0 0 1 .5-.5h5a.5.5 0 0 1 0 1h-5a.5.5 0 0 1-.5-.5zm0 2a.5.5 0 0 1 .5-.5h2a.5.5 0 0 1 0 1h-2a.5.5 0 0 1-.5-.5z"/><path d="M9.5 0H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V4.5L9.5 0zM4 1h5v3.5A1.5 1.5 0 0 0 10.5 6H13v8a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V2a1 1 0 0 1 1-1z"/></svg>
        Raw
      </button>
    </div>
    {#if $selectedEntry}
      <span class="file-info">
        {#if $selectedEntry.project}
          <span class="project-badge">{$selectedEntry.project.split("/").pop()}</span>
        {/if}
        {$selectedEntry.file} @ {new Date($selectedEntry.timestamp).toLocaleTimeString()}
      </span>
    {/if}
  </div>

  {#if $contentLoading}
    <div class="loading">Loading {$viewMode === "diff" ? "diff" : "content"}...</div>
  {:else if $viewMode === "diff"}
    {#if $diffData && $diffData.changes.length > 0}
      <div class="diff-content">
        {#each $diffData.changes as change (change.file)}
          <div class="change-header">
            {#if change.status === "created"}
              <span class="created">File created</span>
            {:else if change.status === "deleted"}
              <span class="deleted">File deleted</span>
            {:else}
              <span>--- a/{change.file}</span>
              <span>+++ b/{change.file}</span>
            {/if}
          </div>

          {#if change.hunks && change.hunks.length > 0}
            {#if isDiffTruncated && !showFullDiff}
              <div class="diff-truncated">
                <p>{totalDiffLines.toLocaleString()} lines changed — too large to render safely.</p>
                <button class="show-btn" onclick={() => showFullDiff = true}>
                  Show full diff ({totalDiffLines.toLocaleString()} lines)
                </button>
              </div>
            {:else}
            {@const lang = detectLanguage(change.file)}
            {#each buildRenderItems(change.hunks, lang) as item (item)}
              {#if item.type === "region"}
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div
                  class="collapsed-region"
                  role="button"
                  tabindex="0"
                  onclick={() => toggleCollapsedRegion(item.data.regionIndex)}
                  onkeydown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      toggleCollapsedRegion(item.data.regionIndex);
                    }
                  }}
                >
                  <span class="collapsed-divider">
                    ··· {item.data.lineCount} lines hidden
                    {#if item.data.functionName}
                      <span class="fn-name">(fn {item.data.functionName})</span>
                    {/if}
                    ···
                  </span>
                  <button class="show-btn" type="button">Show</button>
                </div>
              {:else if item.type === "hunk-header"}
                <div class="hunk-header">{formatHunkHeader(item.data)}</div>
              {:else if item.type === "line"}
                {@const line = item.data}
                {@const wordDiffSegments =
                  item.hunkIndex !== undefined && item.lineIndex !== undefined
                    ? wordDiffsByHunk[item.hunkIndex]?.get(item.lineIndex)
                    : undefined}
                <div class="diff-line {line.op}">
                  <span class="gutter old-num">
                    {line.old_num !== undefined ? String(line.old_num) : ""}
                  </span>
                  <span class="gutter new-num">
                    {line.new_num !== undefined ? String(line.new_num) : ""}
                  </span>
                  <span class="gutter op-indicator">
                    {line.op === "insert" ? "+" : line.op === "delete" ? "-" : " "}
                  </span>
                  <span class="line-content">
                    {#if wordDiffSegments}
                      {#each wordDiffSegments as segment}
                        {#if segment.changed}
                          <span class="word-change">{segment.text}</span>
                        {:else}
                          {segment.text}
                        {/if}
                      {/each}
                    {:else if highlightedMap.has(line.content)}
                      {@html highlightedMap.get(line.content)}
                    {:else}
                      {line.content}
                    {/if}
                  </span>
                </div>
              {/if}
            {/each}
            {/if}
          {:else if change.status === "created" || change.status === "deleted"}
            <!-- File created or deleted; no hunks to show -->
          {/if}
        {/each}
      </div>
    {:else if $diffData}
      <div class="empty">No changes found for this entry.</div>
    {:else}
      <div class="empty">
        Select a change from the timeline to view its diff.
      </div>
    {/if}
  {:else if $viewMode === "content"}
    {#if $contentData && $contentData.content}
      {@const rawLines = $contentData.content.split("\n")}
      {#if rawLines.length > DIFF_LINE_LIMIT && !showFullDiff}
        <div class="diff-truncated">
          <p>{rawLines.length.toLocaleString()} lines — too large to render safely.</p>
          <button class="show-btn" onclick={() => showFullDiff = true}>
            Show full file ({rawLines.length.toLocaleString()} lines)
          </button>
        </div>
      {:else}
        <div class="raw-content">
          {#each rawLines as line, i}
            <div class="raw-line">
              <span class="gutter new-num">
                {i + 1}
              </span>
              <span class="line-content">
                {#if highlightedMap.has(line)}
                  {@html highlightedMap.get(line)}
                {:else}
                  {line}
                {/if}
              </span>
            </div>
          {/each}
        </div>
      {/if}
    {:else if $contentData}
      <div class="empty">No content found for this entry.</div>
    {:else}
      <div class="empty">
        Select a change from the timeline to view its content.
      </div>
    {/if}
  {/if}
</div>

<style>
  .diff-truncated {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 48px 24px;
    color: var(--text-secondary);
    text-align: center;
  }
  .diff-truncated p {
    font-size: var(--text-sm);
  }

  .diff-container {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 16px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
    flex-shrink: 0;
  }

  .tabs {
    display: flex;
    gap: 4px;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 4px 12px;
    border: 1px solid var(--border);
    background: none;
    border-radius: 4px;
    cursor: pointer;
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .tab.active {
    background: var(--accent-bg);
    color: var(--accent);
    border-color: var(--accent);
  }

  .file-info {
    font-size: var(--text-sm);
    color: var(--text-secondary);
    font-family: var(--font-mono);
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .project-badge {
    background: var(--accent-bg);
    color: var(--accent);
    padding: 1px 6px;
    border-radius: 3px;
    font-weight: 600;
    font-size: var(--text-xs);
  }

  .diff-content,
  .raw-content {
    flex: 1;
    overflow: auto;
    padding: 0;
  }

  .change-header {
    padding: 8px 16px;
    background: var(--bg);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--text-secondary);
    display: flex;
    flex-direction: column;
    border-bottom: 1px solid var(--border);
  }

  .created {
    color: var(--addition);
  }

  .deleted {
    color: var(--deletion);
  }

  .hunk-header {
    padding: 4px 16px;
    background: var(--surface);
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--accent);
    border-bottom: 1px solid var(--border);
    font-weight: 500;
  }

  .diff-line {
    display: grid;
    grid-template-columns: 48px 48px 20px 1fr;
    min-width: 100%;
    padding: 0;
    white-space: pre;
    font-family: var(--font-mono);
    font-size: var(--text-base);
    line-height: 1.5;
    border-bottom: 1px solid var(--border);
  }

  .diff-line.insert {
    background: var(--addition-bg);
  }

  .diff-line.delete {
    background: var(--deletion-bg);
  }

  .diff-line.equal {
    background: transparent;
    color: var(--text-primary);
  }

  .gutter {
    display: flex;
    align-items: center;
    padding: 0 8px;
    text-align: right;
    font-size: var(--text-sm);
    color: var(--text-muted);
    user-select: none;
    font-variant-numeric: tabular-nums;
    border-right: 1px solid var(--border);
  }

  .gutter.old-num {
    border-right: none;
  }

  .gutter.op-indicator {
    text-align: center;
    border-right: 1px solid var(--border);
  }

  .line-content {
    padding: 0 8px;
    white-space: pre;
  }

  .collapsed-region {
    display: flex;
    align-items: center;
    padding: 8px 16px;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    cursor: pointer;
    user-select: none;
    gap: 12px;
  }

  .collapsed-region:hover {
    background: var(--bg);
  }

  .collapsed-divider {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--text-muted);
    flex: 1;
    text-align: center;
  }

  .fn-name {
    color: var(--accent);
    font-style: italic;
  }

  .show-btn {
    padding: 4px 12px;
    border: 1px solid var(--border);
    background: var(--surface);
    border-radius: 4px;
    cursor: pointer;
    font-size: var(--text-sm);
    color: var(--text-secondary);
    flex-shrink: 0;
  }

  .show-btn:hover {
    background: var(--accent-bg);
    border-color: var(--accent);
    color: var(--accent);
  }

  .loading,
  .empty {
    display: flex;
    align-items: center;
    justify-content: center;
    flex: 1;
    color: var(--text-muted);
  }

  .diff-line.insert .word-change {
    background: var(--addition-strong);
    border-radius: 2px;
  }

  .diff-line.delete .word-change {
    background: var(--deletion-strong);
    border-radius: 2px;
  }

  .raw-line {
    display: grid;
    grid-template-columns: 48px max-content;
    min-width: 100%;
    padding: 0;
    white-space: pre;
    font-family: var(--font-mono);
    font-size: var(--text-base);
    line-height: 1.5;
    border-bottom: 1px solid var(--border);
    background: transparent;
    color: var(--text-primary);
  }

  .raw-line .gutter {
    display: flex;
    align-items: center;
    padding: 0 8px;
    text-align: right;
    font-size: var(--text-sm);
    color: var(--text-muted);
    user-select: none;
    font-variant-numeric: tabular-nums;
    border-right: 1px solid var(--border);
  }

  .raw-line .line-content {
    padding: 0 8px;
    white-space: pre;
  }
</style>
