<script lang="ts">
import type { FilterCandidate } from "../lib/filterUtils";
import { scoreMatch } from "../lib/filterUtils";

interface Props {
	filters: string[];
	onFiltersChange: (filters: string[]) => void;
	onClearAll?: () => void;
	candidates: FilterCandidate[];
}

let { filters, onFiltersChange, onClearAll, candidates }: Props = $props();

let query = $state("");
let highlightedIndex = $state(0);
let dropdownOpen = $state(false);
let inputEl: HTMLInputElement | undefined = $state();

// Open dropdown when query changes (user typing)
$effect(() => {
	if (query.length > 0) {
		dropdownOpen = true;
		highlightedIndex = 0;
	}
});

/** Score and filter candidates by query, excluding already-selected filters */
let matchedCandidates = $derived.by(() => {
	if (!query) return [];

	const scored = candidates
		.filter((c) => !filters.includes(c.path)) // exclude already selected
		.map((c) => ({
			...c,
			score: scoreMatch(query, c.path),
		}))
		.filter((c) => c.score > 0) // only matches
		.sort((a, b) => {
			// Sort by: score desc, path length asc, alphabetical
			if (a.score !== b.score) return b.score - a.score;
			if (a.path.length !== b.path.length) return a.path.length - b.path.length;
			return a.path.localeCompare(b.path);
		})
		.slice(0, 10); // top 10 only

	return scored;
});

/** Add a filter and reset input. If adding a folder, subsume child filters. */
function addFilter(path: string) {
	if (!filters.includes(path)) {
		let newFilters: string[];
		if (path.endsWith("/")) {
			// Folder subsumption: remove existing filters that are children of this folder
			newFilters = filters.filter((f) => !f.startsWith(path));
			newFilters.push(path);
		} else {
			newFilters = [...filters, path];
		}
		onFiltersChange(newFilters);
	}
	query = "";
	highlightedIndex = 0;
	dropdownOpen = false;
	inputEl?.focus();
}

/** Remove a filter */
function removeFilter(path: string) {
	onFiltersChange(filters.filter((f) => f !== path));
}

/** Handle keyboard navigation and shortcuts */
function handleKeydown(e: KeyboardEvent) {
	// Dropdown open: handle navigation
	if (dropdownOpen && matchedCandidates.length > 0) {
		switch (e.key) {
			case "ArrowDown":
				e.preventDefault();
				highlightedIndex = (highlightedIndex + 1) % matchedCandidates.length;
				break;
			case "ArrowUp":
				e.preventDefault();
				highlightedIndex =
					(highlightedIndex - 1 + matchedCandidates.length) % matchedCandidates.length;
				break;
			case "Enter":
			case "Tab":
				e.preventDefault();
				addFilter(matchedCandidates[highlightedIndex].path);
				break;
			case "Escape":
				e.preventDefault();
				dropdownOpen = false;
				break;
			default:
				break;
		}
		return;
	}

	// Dropdown closed: handle global shortcuts
	switch (e.key) {
		case "Escape":
			if (query.length > 0) {
				e.preventDefault();
				query = "";
			} else if (onClearAll) {
				e.preventDefault();
				onClearAll();
			}
			break;
		case "Backspace":
			if (query.length === 0 && filters.length > 0) {
				e.preventDefault();
				removeFilter(filters[filters.length - 1]);
			}
			break;
		default:
			break;
	}
}

/** Close dropdown on blur with small delay to allow click handling */
function handleInputBlur() {
	setTimeout(() => {
		dropdownOpen = false;
	}, 100);
}

/** Format a candidate for display */
function formatCandidate(candidate: FilterCandidate): {
	basename: string;
	parentPath: string | null;
} {
	if (candidate.isFolder) {
		// For folders, show full path with trailing slash
		return { basename: candidate.path, parentPath: null };
	}

	// For files: extract basename and parent
	const parts = candidate.path.split("/");
	const basename = parts[parts.length - 1];
	const parentPath = parts.length > 1 ? parts.slice(0, -1).join("/") : null;
	return { basename, parentPath };
}
</script>

<div class="filter-autocomplete-container">
  <!-- Input with search icon -->
  <div class="input-wrapper">
    <input
      type="text"
      placeholder="Filter by file or folder..."
      bind:value={query}
      bind:this={inputEl}
      onkeydown={handleKeydown}
      onfocus={() => {
        if (query.length > 0) {
          dropdownOpen = true;
        }
      }}
      onblur={handleInputBlur}
      role="combobox"
      aria-expanded={dropdownOpen && query.length > 0}
      aria-autocomplete="list"
      aria-controls="filter-listbox"
      autocomplete="off"
      autocorrect="off"
      autocapitalize="off"
      spellcheck="false"
    />
  </div>

  <!-- Autocomplete dropdown -->
  {#if dropdownOpen && query.length > 0}
    <div class="dropdown" role="listbox" id="filter-listbox" tabindex="-1" onmousedown={(e) => e.preventDefault()}>
      {#if matchedCandidates.length === 0}
        <div class="dropdown-empty">No matching files</div>
      {:else}
        {#each matchedCandidates as candidate, index (candidate.path)}
          {@const formatted = formatCandidate(candidate)}
          {@const isHighlighted = index === highlightedIndex}
          <button
            class="dropdown-item"
            class:highlighted={isHighlighted}
            onclick={() => addFilter(candidate.path)}
            role="option"
            aria-selected={isHighlighted}
          >
            <span class="item-icon">
              {#if candidate.isFolder}
                <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor"
                  ><path
                    d="M.54 3.87.5 3a2 2 0 0 1 2-2h3.672a2 2 0 0 1 1.414.586l.828.828A2 2 0 0 0 9.828 3H13.5a2 2 0 0 1 2 2v.287a2 2 0 0 1-.213.896l-.915 1.83A2 2 0 0 0 14 9v.5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 9.5V7a2 2 0 0 0-.372-1.162z"
                  /></svg
                >
              {:else}
                <svg width="14" height="14" viewBox="0 0 16 16" fill="currentColor"
                  ><path
                    d="M4 0a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2V4.5L9.5 0H4zm5 0v3.5A1.5 1.5 0 0 0 10.5 5H14v9a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V2a1 1 0 0 1 1-1h5z"
                  /></svg
                >
              {/if}
            </span>
            {#if candidate.isFolder}
              <span class="item-text-folder">{candidate.path}</span>
            {:else}
              <span class="item-text-file">
                <strong>{formatted.basename}</strong>
                {#if formatted.parentPath}
                  <span class="item-path">{formatted.parentPath}/</span>
                {/if}
              </span>
            {/if}
          </button>
        {/each}
      {/if}
    </div>
  {/if}

  <!-- Screen reader announcements -->
  <div class="sr-only" aria-live="polite" aria-atomic="true">
    {#if filters.length > 0}
      Filtering by {filters.length} {filters.length === 1 ? 'item' : 'items'}: {filters.join(', ')}
    {/if}
  </div>
</div>

<style>
  .filter-autocomplete-container {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  /* Input wrapper */
  .input-wrapper {
    position: relative;
  }
  input {
    width: 100%;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: 4px;
    font-size: var(--text-sm);
    font-family: var(--font-sans);
    background: var(--bg);
    color: var(--text-primary);
    outline: none;
  }
  input:focus {
    border-color: var(--accent);
  }
  input::placeholder {
    color: var(--text-muted);
  }

  /* Dropdown */
  .dropdown {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    z-index: 10;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 4px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.12);
    margin-top: 4px;
    max-height: 280px;
    overflow-y: auto;
  }

  .dropdown-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 8px;
    border: none;
    background: none;
    color: var(--text-primary);
    text-align: left;
    cursor: pointer;
    font-family: var(--font-sans);
    font-size: var(--text-sm);
    transition: background 100ms;
  }
  .dropdown-item:hover {
    background: var(--accent-bg);
  }
  .dropdown-item.highlighted {
    background: var(--accent-bg);
  }
  .dropdown-item + .dropdown-item {
    border-top: 1px solid var(--border);
  }

  .item-icon {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    min-width: 16px;
    height: 16px;
    color: var(--text-secondary);
  }

  .item-text-folder {
    flex: 1;
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .item-text-file {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .item-text-file strong {
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .item-path {
    font-size: var(--text-xs);
    color: var(--text-muted);
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dropdown-empty {
    padding: 8px 12px;
    color: var(--text-muted);
    font-size: var(--text-sm);
    font-style: italic;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }
</style>
