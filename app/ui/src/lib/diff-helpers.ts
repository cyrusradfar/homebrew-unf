/**
 * Pure helper functions for diff rendering.
 * Extracted from ContextualDiffView to keep component focused on presentation.
 */
import { extractFunctionName, getFunctionPattern } from "./language-map";
import type { DiffHunk, DiffHunkLine } from "./types";
import { computeWordDiff, type WordSegment } from "./word-diff";

export interface CollapsedRegion {
	startLine: number;
	endLine: number;
	lineCount: number;
	regionIndex: number;
	functionName: string | null;
}

/**
 * A single entry in the flat diff render list.
 *
 * Discriminated on `type` so that narrowing on `item.type` also narrows
 * `item.data` (and the index fields) to the shape that variant really carries.
 * The variants mirror exactly what `buildRenderItems` constructs: only "line"
 * items know which hunk and line they came from.
 */
export type RenderItem =
	| { type: "region"; data: CollapsedRegion }
	| { type: "hunk-header"; data: DiffHunk }
	| { type: "line"; data: DiffHunkLine; hunkIndex: number; lineIndex: number };

/**
 * Find word-diff pairs in a sequence of hunk lines.
 * Groups consecutive deletes followed by inserts and computes word-level diffs.
 */
export function findWordDiffPairs(lines: DiffHunkLine[]): Map<number, WordSegment[]> {
	const result = new Map<number, WordSegment[]>();
	let i = 0;

	while (i < lines.length) {
		const deleteStart = i;
		while (i < lines.length && lines[i].op === "delete") i++;
		const deleteEnd = i;

		const insertStart = i;
		while (i < lines.length && lines[i].op === "insert") i++;
		const insertEnd = i;

		const pairCount = Math.min(deleteEnd - deleteStart, insertEnd - insertStart);
		for (let p = 0; p < pairCount; p++) {
			const { deleted, inserted } = computeWordDiff(
				lines[deleteStart + p].content,
				lines[insertStart + p].content
			);
			result.set(deleteStart + p, deleted);
			result.set(insertStart + p, inserted);
		}

		if (deleteEnd === deleteStart && insertEnd === insertStart) i++;
	}

	return result;
}

function findEnclosingFunction(
	hunks: DiffHunk[],
	regionIndex: number,
	lang: string | null
): string | null {
	if (!lang) return null;
	const pattern = getFunctionPattern(lang);
	if (!pattern) return null;

	for (let h = regionIndex; h >= 0; h--) {
		if (h >= hunks.length) continue;
		const hunk = hunks[h];
		for (let i = hunk.lines.length - 1; i >= 0 && i >= hunk.lines.length - 10; i--) {
			if (pattern.test(hunk.lines[i].content)) {
				return extractFunctionName(hunk.lines[i].content, lang);
			}
		}
	}
	return null;
}

function calculateCollapsedRegions(hunks: DiffHunk[], lang: string | null): CollapsedRegion[] {
	const regions: CollapsedRegion[] = [];
	if (!hunks || hunks.length === 0) return regions;
	let regionIndex = 0;

	if (hunks[0].old_start > 1) {
		regions.push({
			startLine: 1,
			endLine: hunks[0].old_start - 1,
			lineCount: hunks[0].old_start - 1,
			regionIndex: regionIndex++,
			functionName: findEnclosingFunction(hunks, -1, lang),
		});
	}

	for (let i = 0; i < hunks.length - 1; i++) {
		const gapStart = hunks[i].old_start + hunks[i].old_count;
		const gapEnd = hunks[i + 1].old_start - 1;
		if (gapEnd >= gapStart) {
			regions.push({
				startLine: gapStart,
				endLine: gapEnd,
				lineCount: gapEnd - gapStart + 1,
				regionIndex: regionIndex++,
				functionName: findEnclosingFunction(hunks, i, lang),
			});
		}
	}
	return regions;
}

/** Build a flat list of render items interleaving hunks and collapsed regions. */
export function buildRenderItems(hunks: DiffHunk[], lang: string | null): RenderItem[] {
	const items: RenderItem[] = [];
	if (!hunks || hunks.length === 0) return items;

	const regions = calculateCollapsedRegions(hunks, lang);
	const regionMap = new Map(regions.map((r) => [r.regionIndex, r]));
	let regionIdx = 0;

	/** Push the region at the current index, if one exists, and advance past it. */
	const pushRegionIfPresent = () => {
		const region = regionMap.get(regionIdx);
		if (!region) return;
		items.push({ type: "region", data: region });
		regionIdx++;
	};

	pushRegionIfPresent();

	for (let hunkIndex = 0; hunkIndex < hunks.length; hunkIndex++) {
		const hunk = hunks[hunkIndex];
		items.push({ type: "hunk-header", data: hunk });

		for (let lineIndex = 0; lineIndex < hunk.lines.length; lineIndex++) {
			items.push({ type: "line", data: hunk.lines[lineIndex], hunkIndex, lineIndex });
		}

		pushRegionIfPresent();
	}
	return items;
}

export function formatHunkHeader(hunk: DiffHunk): string {
	return `@@ -${hunk.old_start},${hunk.old_count} +${hunk.new_start},${hunk.new_count} @@`;
}
