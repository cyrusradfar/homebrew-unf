import { createHighlighter, type Highlighter } from "shiki";

let highlighterPromise: Promise<Highlighter> | null = null;
const loadedLanguages = new Set<string>();

/**
 * Get or create the singleton Shiki highlighter instance.
 * Initializes with github-light theme and lazy-loads languages on demand.
 */
async function getHighlighter(): Promise<Highlighter> {
	if (!highlighterPromise) {
		highlighterPromise = createHighlighter({
			themes: ["github-light"],
			langs: [], // Start empty, load on demand
		});
	}
	return highlighterPromise;
}

/**
 * Highlight a single line of code. Returns HTML string with inline styles.
 * Falls back to escaped plain text if language is not available or an error occurs.
 *
 * @param code - The line of code to highlight
 * @param lang - The Shiki language identifier (e.g., "typescript", "rust")
 * @returns HTML string with syntax highlighting or escaped plain text
 */
export async function highlightLine(code: string, lang: string): Promise<string> {
	try {
		const highlighter = await getHighlighter();

		// Lazy load the language if not already loaded
		if (!loadedLanguages.has(lang)) {
			try {
				await highlighter.loadLanguage(lang as any);
				loadedLanguages.add(lang);
			} catch {
				// Language not available, return escaped text
				return escapeHtml(code);
			}
		}

		// Use codeToHtml and strip the wrapper tags to get just the highlighted tokens
		const html = highlighter.codeToHtml(code, {
			lang,
			theme: "github-light",
		});

		// codeToHtml wraps in <pre><code>...</code></pre> with <span class="line">...</span>
		// Extract just the inner content
		const match = html.match(/<code[^>]*><span class="line">(.*?)<\/span><\/code>/s);
		return match ? match[1] : escapeHtml(code);
	} catch {
		return escapeHtml(code);
	}
}

/**
 * Highlight multiple lines at once (more efficient than line-by-line).
 * Preserves context across lines for accurate tokenization.
 *
 * @param lines - Array of code lines to highlight
 * @param lang - The Shiki language identifier
 * @returns Array of HTML strings, one per line, in the same order as input
 */
export async function highlightLines(lines: string[], lang: string): Promise<string[]> {
	try {
		const highlighter = await getHighlighter();

		if (!loadedLanguages.has(lang)) {
			try {
				await highlighter.loadLanguage(lang as any);
				loadedLanguages.add(lang);
			} catch {
				return lines.map(escapeHtml);
			}
		}

		// Join lines and highlight as a single block for accurate cross-line tokenization
		const fullCode = lines.join("\n");
		const html = highlighter.codeToHtml(fullCode, {
			lang,
			theme: "github-light",
		});

		// Extract each <span class="line">...</span>
		const lineMatches = html.match(/<span class="line">(.*?)<\/span>/gs);
		if (!lineMatches || lineMatches.length !== lines.length) {
			return lines.map(escapeHtml);
		}

		return lineMatches.map((m) => {
			const inner = m.match(/<span class="line">(.*)<\/span>/s);
			return inner ? inner[1] : "";
		});
	} catch {
		return lines.map(escapeHtml);
	}
}

/**
 * Escape HTML special characters.
 * Used for fallback when syntax highlighting is not available.
 */
function escapeHtml(text: string): string {
	return text
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;")
		.replace(/"/g, "&quot;");
}
