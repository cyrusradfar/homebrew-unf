import { describe, expect, it } from "vitest";
import {
	initialState,
	mouseDown,
	mouseMove,
	mouseUp,
	selectSession,
	shouldClearRange,
} from "../rangeSelection";

// ---------------------------------------------------------------------------
// Bug reproduction: drag-to-create range gets cleared by $effect
// ---------------------------------------------------------------------------

describe("drag-to-create range", () => {
	it("creates a range via click-drag-release", () => {
		let s = initialState();

		s = mouseDown(s, 0.2);
		expect(s.dragMode).toBe("create");
		expect(s.rangeStart).toBe(0.2);

		s = mouseMove(s, 0.6);
		expect(s.rangeEnd).toBe(0.6);

		s = mouseUp(s);
		expect(s.dragMode).toBe("none");
		expect(s.rangeStart).toBe(0.2);
		expect(s.rangeEnd).toBe(0.6);
	});

	it("clears range on click without drag (micro-range)", () => {
		let s = initialState();

		s = mouseDown(s, 0.5);
		// No meaningful mousemove
		s = mouseUp(s);

		expect(s.rangeStart).toBeNull();
		expect(s.rangeEnd).toBeNull();
	});

	it("BUG FIX: range must NOT be cleared mid-drag even when histogramStart is null", () => {
		// This is the exact scenario that was broken:
		// 1. User mousedown → rangeStart/rangeEnd are set, dragMode = "create"
		// 2. $effect sees histogramStart is null + range is non-null → CLEARED the range
		// 3. User could never create a range by dragging
		let s = initialState();
		s = mouseDown(s, 0.3);

		// At this point histogramStart is still null (syncStores only runs on mouseup).
		// The $effect guard must NOT clear the range during an active drag.
		expect(shouldClearRange(true, s)).toBe(false);

		// Continue the drag
		s = mouseMove(s, 0.7);
		expect(shouldClearRange(true, s)).toBe(false);
		expect(s.rangeEnd).toBe(0.7);

		// Release — now syncStores would set histogramStart to a timestamp
		s = mouseUp(s);
		expect(s.rangeStart).toBe(0.3);
		expect(s.rangeEnd).toBe(0.7);
	});
});

// ---------------------------------------------------------------------------
// shouldClearRange — the fixed guard logic
// ---------------------------------------------------------------------------

describe("shouldClearRange", () => {
	it("clears range when stores are cleared externally and not dragging", () => {
		const s = { ...initialState(), rangeStart: 0.2, rangeEnd: 0.6 };
		expect(shouldClearRange(true, s)).toBe(true);
	});

	it("does NOT clear during create drag", () => {
		const s = { ...initialState(), rangeStart: 0.3, rangeEnd: 0.3, dragMode: "create" as const };
		expect(shouldClearRange(true, s)).toBe(false);
	});

	it("does NOT clear during left-handle drag", () => {
		const s = { ...initialState(), rangeStart: 0.2, rangeEnd: 0.6, dragMode: "left" as const };
		expect(shouldClearRange(true, s)).toBe(false);
	});

	it("does NOT clear during right-handle drag", () => {
		const s = { ...initialState(), rangeStart: 0.2, rangeEnd: 0.6, dragMode: "right" as const };
		expect(shouldClearRange(true, s)).toBe(false);
	});

	it("does NOT clear during slide drag", () => {
		const s = { ...initialState(), rangeStart: 0.2, rangeEnd: 0.6, dragMode: "slide" as const };
		expect(shouldClearRange(true, s)).toBe(false);
	});

	it("does NOT clear when histogramStart is set (non-null)", () => {
		const s = { ...initialState(), rangeStart: 0.2, rangeEnd: 0.6 };
		expect(shouldClearRange(false, s)).toBe(false);
	});

	it("does NOT clear when no local range exists", () => {
		expect(shouldClearRange(true, initialState())).toBe(false);
	});
});

// ---------------------------------------------------------------------------
// Session (segment) selection
// ---------------------------------------------------------------------------

describe("session selection", () => {
	it("snaps range to session boundaries", () => {
		let s = initialState();
		s = selectSession(s, 10, 20, 100);

		expect(s.rangeStart).toBe(0.1);
		expect(s.rangeEnd).toBe(0.21);
	});

	it("selected session range is editable via left handle", () => {
		let s = initialState();
		s = selectSession(s, 10, 20, 100);

		// Click left handle
		const lo = Math.min(s.rangeStart!, s.rangeEnd!);
		s = mouseDown(s, lo);
		expect(s.dragMode).toBe("left");

		s = mouseMove(s, 0.05);
		expect(s.rangeStart).toBe(0.05);

		s = mouseUp(s);
		expect(s.rangeStart).toBe(0.05);
		expect(s.rangeEnd).toBe(0.21);
	});

	it("selected session range is editable via right handle", () => {
		let s = initialState();
		s = selectSession(s, 10, 20, 100);

		const hi = Math.max(s.rangeStart!, s.rangeEnd!);
		s = mouseDown(s, hi);
		expect(s.dragMode).toBe("right");

		s = mouseMove(s, 0.9);
		s = mouseUp(s);
		expect(s.rangeEnd).toBe(0.9);
	});

	it("session range is not cleared by shouldClearRange when stores are set", () => {
		let s = initialState();
		s = selectSession(s, 10, 20, 100);
		// After selectSession, syncStores runs synchronously → histogramStart is non-null
		expect(shouldClearRange(false, s)).toBe(false);
	});
});

// ---------------------------------------------------------------------------
// Editing a drag-created range (handles + slide)
// ---------------------------------------------------------------------------

describe("editing existing range", () => {
	function createRange(start: number, end: number) {
		let s = initialState();
		s = mouseDown(s, start);
		s = mouseMove(s, end);
		return mouseUp(s);
	}

	it("resizes via left handle", () => {
		let s = createRange(0.2, 0.6);

		s = mouseDown(s, 0.2);
		expect(s.dragMode).toBe("left");

		s = mouseMove(s, 0.1);
		s = mouseUp(s);
		expect(s.rangeStart).toBe(0.1);
		expect(s.rangeEnd).toBe(0.6);
	});

	it("resizes via right handle", () => {
		let s = createRange(0.2, 0.6);

		s = mouseDown(s, 0.6);
		expect(s.dragMode).toBe("right");

		s = mouseMove(s, 0.8);
		s = mouseUp(s);
		expect(s.rangeStart).toBe(0.2);
		expect(s.rangeEnd).toBe(0.8);
	});

	it("slides the entire range", () => {
		let s = createRange(0.2, 0.6);

		s = mouseDown(s, 0.4); // middle of range
		expect(s.dragMode).toBe("slide");

		s = mouseMove(s, 0.5); // drag right by 0.1
		s = mouseUp(s);
		expect(s.rangeStart).toBeCloseTo(0.3, 10);
		expect(s.rangeEnd).toBeCloseTo(0.7, 10);
	});

	it("clicking outside range starts a new range", () => {
		let s = createRange(0.2, 0.4);

		s = mouseDown(s, 0.8); // well outside
		expect(s.dragMode).toBe("create");
		expect(s.rangeStart).toBe(0.8);
	});

	it("slide clamps to [0, 1] when dragged past left edge", () => {
		let s = createRange(0.1, 0.3);

		s = mouseDown(s, 0.2);
		expect(s.dragMode).toBe("slide");

		s = mouseMove(s, 0.0); // drag left by 0.2
		s = mouseUp(s);
		expect(s.rangeStart).toBeCloseTo(0, 10);
		expect(s.rangeEnd).toBeCloseTo(0.2, 10);
	});

	it("slide clamps to [0, 1] when dragged past right edge", () => {
		let s = createRange(0.7, 0.9);

		s = mouseDown(s, 0.8);
		expect(s.dragMode).toBe("slide");

		s = mouseMove(s, 1.0); // drag right by 0.2
		s = mouseUp(s);
		expect(s.rangeStart).toBeCloseTo(0.8, 10);
		expect(s.rangeEnd).toBeCloseTo(1.0, 10);
	});
});

// ---------------------------------------------------------------------------
// Reverse drag (right-to-left) normalization
// ---------------------------------------------------------------------------

describe("reverse drag normalization", () => {
	it("normalizes range when dragging right to left", () => {
		let s = initialState();
		s = mouseDown(s, 0.7);
		s = mouseMove(s, 0.3);
		s = mouseUp(s);

		// mouseUp normalizes: start < end
		expect(s.rangeStart).toBe(0.3);
		expect(s.rangeEnd).toBe(0.7);
	});
});
