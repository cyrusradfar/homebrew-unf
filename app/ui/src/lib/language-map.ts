/**
 * Extension-to-language mapping for syntax highlighting.
 * Maps file extensions to Shiki language identifiers.
 */
const EXTENSION_MAP: Record<string, string> = {
	".ts": "typescript",
	".tsx": "tsx",
	".js": "javascript",
	".jsx": "jsx",
	".mjs": "javascript",
	".rs": "rust",
	".py": "python",
	".go": "go",
	".java": "java",
	".c": "c",
	".h": "c",
	".cpp": "cpp",
	".hpp": "cpp",
	".cc": "cpp",
	".rb": "ruby",
	".rake": "ruby",
	".css": "css",
	".scss": "scss",
	".html": "html",
	".json": "json",
	".md": "markdown",
	".toml": "toml",
	".yaml": "yaml",
	".yml": "yaml",
	".sh": "shellscript",
	".bash": "shellscript",
	".zsh": "shellscript",
	".sql": "sql",
	".svelte": "svelte",
	".vue": "vue",
	".xml": "xml",
	".swift": "swift",
	".kt": "kotlin",
	".cs": "csharp",
	".php": "php",
	".lua": "lua",
	".dart": "dart",
	".zig": "zig",
};

/**
 * Detect the language for a given file path based on its extension.
 * Returns the Shiki language identifier, or null if not found.
 */
export function detectLanguage(filePath: string): string | null {
	const ext = `.${filePath.split(".").pop()?.toLowerCase()}`;
	return EXTENSION_MAP[ext] ?? null;
}

/**
 * Function/class definition patterns per language.
 * Used to detect enclosing scope for context expansion.
 * Inspired by Git's userdiff.c patterns.
 */
const FUNCTION_PATTERNS: Record<string, RegExp> = {
	typescript:
		/^\s*(export\s+)?(default\s+)?(async\s+)?(function\*?\s+\w+|class\s+\w+|const\s+\w+\s*=\s*(async\s+)?[(]|interface\s+\w+|type\s+\w+|enum\s+\w+)/,
	javascript:
		/^\s*(export\s+)?(default\s+)?(async\s+)?(function\*?\s+\w+|class\s+\w+|const\s+\w+\s*=\s*(async\s+)?[(])/,
	tsx: /^\s*(export\s+)?(default\s+)?(async\s+)?(function\*?\s+\w+|class\s+\w+|const\s+\w+\s*=\s*(async\s+)?[(])/,
	jsx: /^\s*(export\s+)?(default\s+)?(async\s+)?(function\*?\s+\w+|class\s+\w+|const\s+\w+\s*=\s*(async\s+)?[(])/,
	rust: /^\s*(pub(\([^)]+\))?\s+)?((async|const|unsafe|extern)\s+)*(fn|struct|enum|trait|impl|mod|macro_rules!)\s+\w+/,
	python: /^\s*(async\s+)?(def|class)\s+\w+/,
	go: /^\s*(func\s+(\(\w+\s+\*?\w+\)\s+)?\w+|type\s+\w+\s+(struct|interface))/,
	ruby: /^\s*(def|class|module)\s+\w+/,
	java: /^\s*(public|private|protected)?\s*(static\s+)?(abstract\s+)?(class|interface|enum|void|int|long|String|boolean)\s+\w+/,
	c: /^[A-Za-z_]\w*(\s+\*?\w+)+\s*\(/,
	cpp: /^[A-Za-z_]\w*(\s+\*?\w+)+\s*\(|^\s*(class|struct|namespace)\s+\w+/,
	csharp:
		/^\s*(public|private|protected|internal)?\s*(static\s+)?(class|interface|enum|void|int|string|bool|async)\s+\w+/,
	kotlin: /^\s*(fun|class|object|interface)\s+\w+/,
	swift: /^\s*(func|class|struct|enum|protocol)\s+\w+/,
	php: /^\s*(public|private|protected)?\s*(static\s+)?(function|class)\s+\w+/,
	shellscript: /^\s*(\w+\s*\(\)|function\s+\w+)/,
	lua: /^\s*(local\s+)?function\s+[\w.:]+/,
	dart: /^\s*(class|void|int|String|bool|Future|Stream)\s+\w+|^\s*\w+\s+\w+\s*\(/,
	zig: /^\s*(pub\s+)?(fn|const)\s+\w+/,
};

/**
 * Get the function detection pattern for a language.
 * Returns null if no pattern is defined.
 */
export function getFunctionPattern(lang: string): RegExp | null {
	return FUNCTION_PATTERNS[lang] ?? null;
}

/**
 * Extract a short function/scope name from a line that matches a function pattern.
 * Returns null if no name can be extracted.
 */
export function extractFunctionName(line: string, lang: string): string | null {
	const pattern = FUNCTION_PATTERNS[lang];
	if (!pattern) return null;

	const match = line.match(pattern);
	if (!match) return null;

	// Try to extract a meaningful name from the matched portion
	// Look for common patterns: "fn name", "def name", "function name", "class name", etc.
	const nameMatch = match[0].match(
		/(?:fn|def|function\*?|class|struct|enum|trait|impl|mod|type|interface|module|object|protocol|macro_rules!)\s+(\w+)/
	);
	if (nameMatch) return nameMatch[1];

	// For "const name = ..." patterns
	const constMatch = match[0].match(/(?:const|let|var)\s+(\w+)/);
	if (constMatch) return constMatch[1];

	return null;
}
