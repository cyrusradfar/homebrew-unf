/**
 * Word-level diffing for highlighting specific changed tokens within a line.
 * Uses a simple LCS (Longest Common Subsequence) algorithm to identify which
 * word tokens changed between two adjacent lines.
 */

/**
 * Represents a contiguous segment of text that either changed or remained the same.
 */
export interface WordSegment {
	text: string;
	changed: boolean;
}

/**
 * Compute word-level diff between a deleted line and an inserted line.
 * Returns segments for each line marking which parts changed.
 *
 * @param deletedLine - The original (deleted) line content
 * @param insertedLine - The new (inserted) line content
 * @returns An object with `deleted` and `inserted` arrays of word segments
 */
export function computeWordDiff(
	deletedLine: string,
	insertedLine: string
): { deleted: WordSegment[]; inserted: WordSegment[] } {
	// Split into words (preserving whitespace as separate tokens)
	const oldWords = tokenize(deletedLine);
	const newWords = tokenize(insertedLine);

	// Simple LCS-based diff on word tokens
	const lcs = computeLCS(oldWords, newWords);

	// Build segments from LCS result
	const deleted = buildSegments(oldWords, lcs.oldIndices);
	const inserted = buildSegments(newWords, lcs.newIndices);

	return { deleted, inserted };
}

/**
 * Tokenize a line into words and whitespace for diffing.
 * Splits on word boundaries but preserves whitespace as separate tokens.
 *
 * @param text - The text to tokenize
 * @returns An array of tokens (words and whitespace)
 */
function tokenize(text: string): string[] {
	// Split into groups of: non-whitespace sequences or whitespace sequences
	return text.match(/\S+|\s+/g) ?? [text];
}

/**
 * Compute Longest Common Subsequence (LCS) using dynamic programming.
 * Returns the indices of matched tokens in both sequences.
 *
 * @param a - First token sequence
 * @param b - Second token sequence
 * @returns Object with oldIndices and newIndices sets containing matched positions
 */
function computeLCS(
	a: string[],
	b: string[]
): { oldIndices: Set<number>; newIndices: Set<number> } {
	const m = a.length;
	const n = b.length;

	// DP table
	const dp: number[][] = Array.from({ length: m + 1 }, () => Array(n + 1).fill(0));

	// Fill DP table
	for (let i = 1; i <= m; i++) {
		for (let j = 1; j <= n; j++) {
			if (a[i - 1] === b[j - 1]) {
				dp[i][j] = dp[i - 1][j - 1] + 1;
			} else {
				dp[i][j] = Math.max(dp[i - 1][j], dp[i][j - 1]);
			}
		}
	}

	// Backtrack to find matched indices
	const oldIndices = new Set<number>();
	const newIndices = new Set<number>();
	let i = m,
		j = n;
	while (i > 0 && j > 0) {
		if (a[i - 1] === b[j - 1]) {
			oldIndices.add(i - 1);
			newIndices.add(j - 1);
			i--;
			j--;
		} else if (dp[i - 1][j] > dp[i][j - 1]) {
			i--;
		} else {
			j--;
		}
	}

	return { oldIndices, newIndices };
}

/**
 * Build word segments from tokens, marking which ones changed (not in LCS).
 *
 * @param tokens - The tokens to segment
 * @param matchedIndices - Set of indices that matched (LCS)
 * @returns Array of word segments with changed flag
 */
function buildSegments(tokens: string[], matchedIndices: Set<number>): WordSegment[] {
	const segments: WordSegment[] = [];
	let currentText = "";
	let currentChanged: boolean | null = null;

	for (let i = 0; i < tokens.length; i++) {
		const changed = !matchedIndices.has(i);
		if (currentChanged !== null && changed !== currentChanged) {
			// State change: flush current segment and start new one
			segments.push({ text: currentText, changed: currentChanged });
			currentText = "";
		}
		currentText += tokens[i];
		currentChanged = changed;
	}

	// Flush final segment
	if (currentText && currentChanged !== null) {
		segments.push({ text: currentText, changed: currentChanged });
	}

	return segments;
}
