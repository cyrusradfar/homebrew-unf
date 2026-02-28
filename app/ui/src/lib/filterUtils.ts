import type { GroupedLogFile } from "./types";

export interface FilterCandidate {
  path: string;      // e.g. "src/components/" or "src/main.rs"
  isFolder: boolean;
}

/**
 * Convert user-selected filter entries into glob patterns for --include CLI flag.
 * - Directory "src/components/" -> "src/components/"+"**"
 * - Bare filename "main.rs" -> "**"+"/main.rs"
 * - Path with file "src/main.rs" -> "src/main.rs" (exact)
 * - Empty array -> empty array
 */
export function filtersToGlobs(filters: string[]): string[] {
  return filters
    .map((f) => f.trim())
    .filter((f) => f.length > 0)
    .map((filter) => {
      if (filter.endsWith("/")) {
        // Directory: "src/components/" → "src/components/**"
        return `${filter}**`;
      } else if (filter.includes("/")) {
        // Path with file: "src/main.rs" → "src/main.rs" (exact)
        return filter;
      } else {
        // Bare filename: "main.rs" → "**/main.rs"
        return `**/${filter}`;
      }
    });
}

/**
 * Extract unique file and folder candidates from the file tree.
 * Files come from group paths. Folders are derived by walking path segments.
 * Returns sorted by path.
 */
export function extractCandidates(fileTree: GroupedLogFile[]): FilterCandidate[] {
  const candidateMap = new Map<string, FilterCandidate>();

  for (const fileGroup of fileTree) {
    const path = fileGroup.path;

    // Add the file itself
    candidateMap.set(path, { path, isFolder: false });

    // Extract all folder segments from this file's path
    const segments = path.split("/");
    for (let i = 1; i < segments.length; i++) {
      const folderPath = segments.slice(0, i).join("/") + "/";
      if (!candidateMap.has(folderPath)) {
        candidateMap.set(folderPath, { path: folderPath, isFolder: true });
      }
    }
  }

  // Convert to array and sort by path
  const candidates = Array.from(candidateMap.values());
  candidates.sort((a, b) => a.path.localeCompare(b.path));
  return candidates;
}

/**
 * Score a query against a candidate path. Higher = better match.
 * All comparisons case-insensitive, strict substring (no fuzzy).
 *
 * Scoring tiers:
 * - 100: exact filename match (query === basename)
 * - 80: filename starts with query
 * - 60: full path starts with query
 * - 40: filename contains query as substring
 * - 20: full path contains query as substring
 * - 0: no match
 */
export function scoreMatch(query: string, candidate: string): number {
  const lowerQuery = query.toLowerCase();
  const lowerCandidate = candidate.toLowerCase();

  // Extract basename (filename without folder path)
  const basename = candidate.includes("/")
    ? candidate.split("/").pop() || ""
    : candidate;
  const lowerBasename = basename.toLowerCase();

  // Tier 100: exact filename match
  if (lowerBasename === lowerQuery) {
    return 100;
  }

  // Tier 80: filename starts with query
  if (lowerBasename.startsWith(lowerQuery)) {
    return 80;
  }

  // Tier 60: full path starts with query
  if (lowerCandidate.startsWith(lowerQuery)) {
    return 60;
  }

  // Tier 40: filename contains query as substring
  if (lowerBasename.includes(lowerQuery)) {
    return 40;
  }

  // Tier 20: full path contains query as substring
  if (lowerCandidate.includes(lowerQuery)) {
    return 20;
  }

  // No match
  return 0;
}
