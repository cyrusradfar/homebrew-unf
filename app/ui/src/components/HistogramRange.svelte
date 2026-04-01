<script lang="ts">
import {
	initialState,
	type RangeState,
	mouseDown as rsMouseDown,
	mouseMove as rsMouseMove,
	mouseUp as rsMouseUp,
	selectSession as rsSelectSession,
	shouldClearRange,
} from "../lib/rangeSelection";
import { histogramEnd, histogramIsSession, histogramStart } from "../lib/stores";
import type { DensityBucket } from "../lib/types";

let barRowEl: HTMLDivElement | undefined = $state();
let maxLog = $derived(Math.max(1, ...$densityBuckets.map((b) => Math.log1p(b.count))));

// Range state managed by the pure state machine
let rs = $state<RangeState>(initialState());

// Convenience aliases for the template
let rangeStart = $derived(rs.rangeStart);
let rangeEnd = $derived(rs.rangeEnd);
let dragMode = $derived(rs.dragMode);
let hasRange = $derived(rangeStart !== null && rangeEnd !== null);

// Convert fraction position to bucket index
function fractionToIdx(frac: number): number {
	const n = $densityBuckets.length;
	return Math.min(Math.max(0, Math.floor(frac * n)), n - 1);
}

// Get fraction from mouse event relative to bar row
function getFraction(e: MouseEvent): number {
	if (!barRowEl) return 0;
	const rect = barRowEl.getBoundingClientRect();
	return Math.min(Math.max(0, (e.clientX - rect.left) / rect.width), 1);
}

// Sync stores from range fractions
function syncStores() {
	if (rangeStart === null || rangeEnd === null) {
		histogramStart.set(null);
		histogramEnd.set(null);
		return;
	}
	const buckets = $densityBuckets;
	const startIdx = fractionToIdx(rangeStart);
	const endIdx = fractionToIdx(rangeEnd);
	const lo = Math.min(startIdx, endIdx);
	const hi = Math.max(startIdx, endIdx);
	if (buckets[lo] && buckets[hi]) {
		histogramStart.set(buckets[lo].start);
		histogramEnd.set(buckets[hi].end);
	}
}

function isInRange(idx: number): boolean {
	if (rangeStart === null || rangeEnd === null) return true;
	const n = $densityBuckets.length;
	const lo = Math.min(rangeStart, rangeEnd) * n;
	const hi = Math.max(rangeStart, rangeEnd) * n;
	return idx >= Math.floor(lo) && idx <= Math.floor(hi);
}

function handleBarMouseDown(e: MouseEvent) {
	if (!barRowEl) return;
	rs = rsMouseDown(rs, getFraction(e));
}

function handleMouseMove(e: MouseEvent) {
	if (dragMode === "none") return;
	rs = rsMouseMove(rs, getFraction(e));
}

function handleMouseUp() {
	if (dragMode === "none") return;
	rs = rsMouseUp(rs);
	histogramIsSession.set(false);
	syncStores();
}

// Double-click to clear range
function handleDblClick() {
	rs = initialState();
	histogramIsSession.set(false);
	syncStores();
}

// Sync local range state when stores are cleared externally (e.g. "clear all" button).
// Guarded by dragMode === "none" to prevent clearing a range mid-creation.
$effect(() => {
	if (shouldClearRange($histogramStart === null, rs)) {
		rs = initialState();
	}
});

// Derived: left/right percentage for the highlight overlay
let highlightLeft = $derived(hasRange ? Math.min(rangeStart!, rangeEnd!) * 100 : 0);
let highlightWidth = $derived(hasRange ? Math.abs(rangeEnd! - rangeStart!) * 100 : 0);

function formatAxisLabel(bucket: DensityBucket, total: number): string {
	const d = new Date(bucket.start);
	if (total <= 48) {
		return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
	}
	return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

let labelIndices = $derived.by(() => {
	const n = $densityBuckets.length;
	if (n === 0) return [];
	const step = Math.max(1, Math.floor(n / 5));
	const indices: number[] = [];
	for (let i = 0; i < n; i += step) {
		indices.push(i);
	}
	return indices;
});

// Session detection: group non-empty buckets separated by significant gaps
interface Session {
	startIdx: number;
	endIdx: number;
	start: string;
	end: string;
	centerPct: number;
	startPct: number;
	widthPct: number;
}

let sessions = $derived.by((): Session[] => {
	const buckets = $densityBuckets;
	if (buckets.length === 0) return [];

	const active = buckets.map((b, i) => ({ idx: i, bucket: b })).filter((x) => x.bucket.count > 0);
	if (active.length === 0) return [];

	// Adaptive gap threshold based on total histogram span.
	// For short spans (hours), gaps of 5-10 min are real session boundaries.
	// For long spans (weeks), we need 30-90 min thresholds.
	const bucketMs = new Date(buckets[0].end).getTime() - new Date(buckets[0].start).getTime();
	const totalSpanMs = bucketMs * buckets.length;
	const spanHours = totalSpanMs / (60 * 60 * 1000);

	// Scale minimum: 5 min for <6h spans, ramp to 30 min for 48h+ spans
	const minGapMs =
		spanHours < 6
			? 5 * 60 * 1000
			: spanHours < 48
				? Math.round((5 + (25 * (spanHours - 6)) / 42) * 60 * 1000)
				: 30 * 60 * 1000;
	const gapThreshold = Math.max(minGapMs, Math.min(bucketMs * 3, 90 * 60 * 1000));

	const result: Session[] = [];
	let start = active[0];
	let end = active[0];

	for (let i = 1; i < active.length; i++) {
		const prev = active[i - 1];
		const curr = active[i];
		const gap =
			new Date(buckets[curr.idx].start).getTime() - new Date(buckets[prev.idx].end).getTime();

		if (gap > gapThreshold) {
			pushSession(result, buckets, start, end);
			start = curr;
		}
		end = curr;
	}
	pushSession(result, buckets, start, end);

	return result;
});

function pushSession(
	result: Session[],
	buckets: DensityBucket[],
	start: { idx: number },
	end: { idx: number }
) {
	const n = buckets.length;
	const centerPct = (((start.idx + end.idx) / 2 + 0.5) / n) * 100;
	const startPct = (start.idx / n) * 100;
	const widthPct = ((end.idx - start.idx + 1) / n) * 100;
	result.push({
		startIdx: start.idx,
		endIdx: end.idx,
		start: buckets[start.idx].start,
		end: buckets[end.idx].end,
		centerPct,
		startPct,
		widthPct,
	});
}

function selectSessionHandler(session: Session) {
	rs = rsSelectSession(rs, session.startIdx, session.endIdx, $densityBuckets.length);
	histogramIsSession.set(true);
	syncStores();
}

function formatSessionTooltip(session: Session): string {
	const s = new Date(session.start);
	const e = new Date(session.end);
	const fmt = (d: Date) =>
		d.toLocaleString(undefined, {
			month: "short",
			day: "numeric",
			hour: "2-digit",
			minute: "2-digit",
		});
	return `${fmt(s)} — ${fmt(e)}`;
}
</script>

<svelte:window on:mousemove={handleMouseMove} on:mouseup={handleMouseUp} />

<div class="histogram-range" class:dragging={dragMode !== "none"}>
  {#if $densityBuckets.length === 0}
    <div class="no-data">No activity data</div>
  {:else}
    {#if sessions.length > 1}
      <div class="session-markers">
        {#each sessions as session}
          <button
            class="session-diamond"
            style="left: {session.centerPct}%"
            title={formatSessionTooltip(session)}
            onclick={() => selectSessionHandler(session)}
          >
            <svg width="10" height="10" viewBox="0 0 10 10">
              <rect x="1.5" y="1.5" width="7" height="7" rx="1" transform="rotate(45 5 5)" />
            </svg>
          </button>
        {/each}
      </div>
    {/if}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="bar-row"
      bind:this={barRowEl}
      onmousedown={handleBarMouseDown}
      ondblclick={handleDblClick}
    >
      {#each $densityBuckets as bucket, i}
        <div
          class="bar-wrapper"
          class:dimmed={hasRange && !isInRange(i)}
          title="{bucket.count} changes"
        >
          <div
            class="bar"
            style="height: {Math.max(2, (Math.log1p(bucket.count) / maxLog) * 100)}%"
          ></div>
        </div>
      {/each}

      {#if hasRange}
        <div
          class="range-highlight"
          style="left: {highlightLeft}%; width: {highlightWidth}%"
        ></div>
        <div
          class="range-handle left"
          style="left: {highlightLeft}%"
        ></div>
        <div
          class="range-handle right"
          style="left: {highlightLeft + highlightWidth}%"
        ></div>
      {/if}
    </div>
    <div class="time-axis">
      {#each labelIndices as idx}
        <span
          class="tick"
          style="left: {(idx / $densityBuckets.length) * 100}%"
        >
          {formatAxisLabel($densityBuckets[idx], $densityBuckets.length)}
        </span>
      {/each}
    </div>
    {#if hasRange}
      <div class="range-label">
        Double-click to clear
      </div>
    {/if}
  {/if}
</div>

<style>
  .histogram-range {
    padding: 8px 12px 4px;
    flex-shrink: 0;
  }

  .histogram-range.dragging {
    cursor: col-resize;
    user-select: none;
  }

  .no-data {
    text-align: center;
    color: var(--text-muted);
    font-size: var(--text-sm);
    padding: 8px 0;
  }

  .bar-row {
    display: flex;
    align-items: flex-end;
    gap: 1px;
    height: 60px;
    position: relative;
    cursor: crosshair;
  }

  .bar-wrapper {
    flex: 1;
    height: 100%;
    display: flex;
    align-items: flex-end;
    position: relative;
  }

  .bar-wrapper .bar {
    width: 100%;
    background: var(--accent);
    opacity: 0.5;
    border-radius: 1px 1px 0 0;
    min-height: 2px;
    transition: opacity 150ms;
  }

  .bar-wrapper:hover .bar {
    opacity: 0.7;
  }

  .bar-wrapper.dimmed .bar {
    opacity: 0.1;
  }

  .bar-wrapper.dimmed:hover .bar {
    opacity: 0.2;
  }

  /* Range highlight overlay */
  .range-highlight {
    position: absolute;
    top: 0;
    bottom: 0;
    background: var(--accent);
    opacity: 0.08;
    pointer-events: none;
    border-radius: 2px;
  }

  /* Draggable handles */
  .range-handle {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 3px;
    margin-left: -1.5px;
    background: var(--accent);
    opacity: 0.6;
    cursor: col-resize;
    border-radius: 1px;
    transition: opacity 100ms;
  }

  .range-handle:hover {
    opacity: 1;
    width: 5px;
    margin-left: -2.5px;
  }

  .range-label {
    text-align: center;
    font-size: 9px;
    color: var(--text-muted);
    margin-top: 2px;
  }

  .time-axis {
    position: relative;
    height: 16px;
    margin-top: 2px;
  }

  .tick {
    position: absolute;
    font-size: 9px;
    color: var(--text-muted);
    transform: translateX(-50%);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  .session-markers {
    position: relative;
    height: 14px;
  }

  .session-diamond {
    position: absolute;
    transform: translateX(-50%);
    background: none;
    border: none;
    padding: 2px;
    cursor: pointer;
    color: var(--text-muted);
    transition: color 100ms;
  }

  .session-diamond:hover {
    color: var(--accent);
  }

  .session-diamond svg {
    display: block;
  }

  .session-diamond rect {
    fill: none;
    stroke: currentColor;
    stroke-width: 1.5;
  }

  .session-diamond:hover rect {
    fill: currentColor;
    fill-opacity: 0.2;
  }
</style>
