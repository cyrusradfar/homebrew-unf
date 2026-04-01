/**
 * Pure state machine for histogram range selection.
 *
 * Extracted from HistogramRange.svelte so the drag/edit/clear logic
 * can be unit-tested without a Svelte rendering context.
 */

export type DragMode = "none" | "left" | "right" | "create" | "slide";

export interface RangeState {
	rangeStart: number | null;
	rangeEnd: number | null;
	dragMode: DragMode;
	slideAnchor: number;
	slideStartRange: [number, number];
}

/** Hit-zone around each handle edge (fraction of total width). */
export const HANDLE_ZONE = 0.015;

/** Ranges smaller than this are treated as a click (cleared on mouseup). */
export const MIN_RANGE = 0.005;

export function initialState(): RangeState {
	return {
		rangeStart: null,
		rangeEnd: null,
		dragMode: "none",
		slideAnchor: 0,
		slideStartRange: [0, 0],
	};
}

/**
 * Handle mousedown on the bar row.
 *
 * If an existing range is present, detect whether the click lands on a
 * handle (left/right resize) or inside the range (slide).  Otherwise
 * start creating a brand-new range.
 */
export function mouseDown(state: RangeState, frac: number): RangeState {
	const { rangeStart, rangeEnd } = state;
	const hasRange = rangeStart !== null && rangeEnd !== null;

	if (hasRange) {
		const lo = Math.min(rangeStart!, rangeEnd!);
		const hi = Math.max(rangeStart!, rangeEnd!);

		if (Math.abs(frac - lo) < HANDLE_ZONE) {
			return { ...state, dragMode: "left" };
		}
		if (Math.abs(frac - hi) < HANDLE_ZONE) {
			return { ...state, dragMode: "right" };
		}
		if (frac > lo && frac < hi) {
			return {
				...state,
				dragMode: "slide",
				slideAnchor: frac,
				slideStartRange: [lo, hi],
			};
		}
	}

	return {
		...state,
		dragMode: "create",
		rangeStart: frac,
		rangeEnd: frac,
	};
}

/** Handle mousemove during an active drag. */
export function mouseMove(state: RangeState, frac: number): RangeState {
	if (state.dragMode === "none") return state;

	if (state.dragMode === "create") {
		return { ...state, rangeEnd: frac };
	}
	if (state.dragMode === "left") {
		return { ...state, rangeStart: Math.min(frac, state.rangeEnd!) };
	}
	if (state.dragMode === "right") {
		return { ...state, rangeEnd: Math.max(frac, state.rangeStart!) };
	}
	if (state.dragMode === "slide") {
		const delta = frac - state.slideAnchor;
		let newLo = state.slideStartRange[0] + delta;
		let newHi = state.slideStartRange[1] + delta;
		if (newLo < 0) {
			newHi -= newLo;
			newLo = 0;
		}
		if (newHi > 1) {
			newLo -= newHi - 1;
			newHi = 1;
		}
		return {
			...state,
			rangeStart: Math.max(0, newLo),
			rangeEnd: Math.min(1, newHi),
		};
	}
	return state;
}

/** Handle mouseup — normalize, clear micro-ranges, end drag. */
export function mouseUp(state: RangeState): RangeState {
	if (state.dragMode === "none") return state;

	let { rangeStart, rangeEnd } = state;

	// Normalize so start < end
	if (rangeStart !== null && rangeEnd !== null && rangeStart > rangeEnd) {
		[rangeStart, rangeEnd] = [rangeEnd, rangeStart];
	}

	// Clear tiny ranges (click without meaningful drag)
	if (rangeStart !== null && rangeEnd !== null && Math.abs(rangeEnd - rangeStart) < MIN_RANGE) {
		rangeStart = null;
		rangeEnd = null;
	}

	return { ...state, rangeStart, rangeEnd, dragMode: "none" };
}

/**
 * Whether the local range should be cleared because external stores
 * were reset (e.g. the user clicked "clear all" in another component).
 *
 * IMPORTANT: must NOT fire while a drag is in progress, otherwise it
 * destroys a range that the user is actively creating — this was the
 * root cause of the "drag doesn't start an editable range" bug.
 */
export function shouldClearRange(histogramStartIsNull: boolean, state: RangeState): boolean {
	return (
		histogramStartIsNull &&
		state.dragMode === "none" &&
		(state.rangeStart !== null || state.rangeEnd !== null)
	);
}

/** Apply a session-diamond click: snap range to session boundaries. */
export function selectSession(
	state: RangeState,
	startIdx: number,
	endIdx: number,
	bucketCount: number
): RangeState {
	return {
		...state,
		rangeStart: startIdx / bucketCount,
		rangeEnd: (endIdx + 1) / bucketCount,
	};
}
