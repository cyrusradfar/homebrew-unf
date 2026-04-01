<script lang="ts">
  import { selectedEntry, diffData, contentData, contentLoading, viewMode, error } from "../lib/stores";
  import { getDiff, getFileContent } from "../lib/api";
  import type { DiffHunk, DiffHunkLine } from "../lib/types";
  import { highlightLines } from "../lib/highlight";
  import { detectLanguage, getFunctionPattern, extractFunctionName } from "../lib/language-map";
  import { computeWordDiff, type WordSegment } from "../lib/word-diff";

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
      if (!data || !data.content) {
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

  /**
   * Find word-diff pairs in a sequence of hunk lines.
   * Groups consecutive deletes followed by consecutive inserts and computes
   * word-level diffs for paired lines.
   *
   * @param lines - All lines in a hunk
   * @returns Map from line index to word segments
   */
  function findWordDiffPairs(
    lines: DiffHunkLine[]
  ): Map<number, WordSegment[]> {
    const result = new Map<number, WordSegment[]>();
    let i = 0;

    while (i < lines.length) {
      // Find a run of deletes
      const deleteStart = i;
      while (i < lines.length && lines[i].op === "delete") i++;
      const deleteEnd = i;

      // Find a run of inserts immediately after
      const insertStart = i;
      while (i < lines.length && lines[i].op === "insert") i++;
      const insertEnd = i;

      // Pair up deletes and inserts
      const deleteCount = deleteEnd - deleteStart;
      const insertCount = insertEnd - insertStart;
      const pairCount = Math.min(deleteCount, insertCount);

      for (let p = 0; p < pairCount; p++) {
        const delIdx = deleteStart + p;
        const insIdx = insertStart + p;
        const { deleted, inserted } = computeWordDiff(
          lines[delIdx].content,
          lines[insIdx].content
        );
        result.set(delIdx, deleted);
        result.set(insIdx, inserted);
      }

      // Skip past equal lines or anything else
      if (deleteCount === 0 && insertCount === 0) i++;
    }

    return result;
  }

  /**
   * Compute word diffs for all hunks.
   * Returns an array of Maps (one per hunk) from line index to word segments.
   */
  let wordDiffsByHunk = $derived(
    ($diffData?.changes[0]?.hunks ?? []).map((hunk) =>
      findWordDiffPairs(hunk.lines)
    )
  );

  /**
   * Represents a collapsed region between hunks.
   */
  interface CollapsedRegion {
    startLine: number;
    endLine: number;
    lineCount: number;
    regionIndex: number;
    functionName: string | null;
  }

  /**
   * Find the enclosing function name by scanning backward through preceding hunk context.
   * Looks at the last 10 lines of all preceding hunks for a function definition.
   */
  function findEnclosingFunction(
    hunks: DiffHunk[],
    regionIndex: number,
    lang: string | null
  ): string | null {
    if (!lang) return null;
    const pattern = getFunctionPattern(lang);
    if (!pattern) return null;

    // Scan backward through all preceding hunks, checking the last 10 lines of each
    for (let h = regionIndex; h >= 0; h--) {
      if (h >= hunks.length) continue;
      const hunk = hunks[h];
      // Scan lines of this hunk in reverse
      for (let i = hunk.lines.length - 1; i >= 0 && i >= hunk.lines.length - 10; i--) {
        const line = hunk.lines[i];
        if (pattern.test(line.content)) {
          return extractFunctionName(line.content, lang);
        }
      }
    }

    return null;
  }

  /**
   * Calculate which collapsed regions exist between hunks.
   * Returns an array of { startLine, endLine, lineCount, regionIndex, functionName }
   */
  function calculateCollapsedRegions(hunks: DiffHunk[], lang: string | null): CollapsedRegion[] {
    const regions: CollapsedRegion[] = [];
    let regionIndex = 0;

    if (!hunks || hunks.length === 0) return regions;

    // Region before the first hunk (if first hunk doesn't start at line 1)
    if (hunks[0].old_start > 1) {
      const lineCount = hunks[0].old_start - 1;
      regions.push({
        startLine: 1,
        endLine: hunks[0].old_start - 1,
        lineCount,
        regionIndex: regionIndex++,
        functionName: findEnclosingFunction(hunks, -1, lang),
      });
    }

    // Regions between consecutive hunks
    for (let i = 0; i < hunks.length - 1; i++) {
      const currentHunk = hunks[i];
      const nextHunk = hunks[i + 1];
      const gapStart = currentHunk.old_start + currentHunk.old_count;
      const gapEnd = nextHunk.old_start - 1;

      if (gapEnd >= gapStart) {
        const lineCount = gapEnd - gapStart + 1;
        regions.push({
          startLine: gapStart,
          endLine: gapEnd,
          lineCount,
          regionIndex: regionIndex++,
          functionName: findEnclosingFunction(hunks, i, lang),
        });
      }
    }

    return regions;
  }

  /**
   * Build a flat list of { type, data } items to render, interleaving hunks and collapsed regions.
   * Includes hunk index and line index for word-diff lookups.
   */
  function buildRenderItems(hunks: DiffHunk[], lang: string | null) {
    const items: Array<{
      type: "region" | "hunk-header" | "line";
      data: any;
      hunkIndex?: number;
      lineIndex?: number;
    }> = [];

    if (!hunks || hunks.length === 0) return items;

    const collapsedRegions = calculateCollapsedRegions(hunks, lang);
    const regionMap = new Map(collapsedRegions.map((r) => [r.regionIndex, r]));

    let regionIdx = 0;

    // Region before first hunk
    if (regionMap.has(regionIdx)) {
      const region = regionMap.get(regionIdx)!;
      items.push({ type: "region", data: region });
      regionIdx++;
    }

    // Each hunk and the gap after it
    for (let hunkIndex = 0; hunkIndex < hunks.length; hunkIndex++) {
      const hunk = hunks[hunkIndex];
      items.push({ type: "hunk-header", data: hunk });

      // All lines in this hunk
      for (let lineIndex = 0; lineIndex < hunk.lines.length; lineIndex++) {
        const line = hunk.lines[lineIndex];
        items.push({
          type: "line",
          data: line,
          hunkIndex,
          lineIndex,
        });
      }

      // Gap after this hunk
      if (regionMap.has(regionIdx)) {
        const region = regionMap.get(regionIdx)!;
        items.push({ type: "region", data: region });
        regionIdx++;
      }
    }

    return items;
  }

  // Track which collapsed regions are expanded
  let expandedRegions = $state(new Set<number>());

  function toggleCollapsedRegion(regionIndex: number) {
    if (expandedRegions.has(regionIndex)) {
      expandedRegions.delete(regionIndex);
    } else {
      expandedRegions.add(regionIndex);
    }
  }

  /**
   * Format a hunk header in unified diff format.
   */
  function formatHunkHeader(hunk: DiffHunk): string {
    return `@@ -${hunk.old_start},${hunk.old_count} +${hunk.new_start},${hunk.new_count} @@`;
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
      <div class="raw-content">
        {#each $contentData.content.split("\n") as line, i}
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
