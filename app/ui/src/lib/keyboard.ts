import { get } from "svelte/store";
import { activeTab, closeTab, selectedEntry, timelineEntries, viewMode } from "./stores";

export function handleGlobalKeydown(e: KeyboardEvent): void {
	const mod = e.metaKey || e.ctrlKey;
	const target = e.target as HTMLElement;
	const isInput = target.tagName === "INPUT" || target.tagName === "TEXTAREA";

	// Cmd+W: close active tab
	if (mod && e.key === "w") {
		e.preventDefault();
		const active = get(activeTab);
		if (active) closeTab(active);
		return;
	}

	// Cmd+F: focus file filter
	if (mod && e.key === "f") {
		e.preventDefault();
		const filterInput = document.querySelector<HTMLInputElement>(
			".filter-autocomplete-container input"
		);
		filterInput?.focus();
		return;
	}

	// Don't handle other shortcuts in input fields
	if (isInput) return;

	// Space: toggle diff/content
	if (e.key === " ") {
		e.preventDefault();
		viewMode.update((m) => (m === "diff" ? "content" : "diff"));
		return;
	}

	// Escape: clear selection
	if (e.key === "Escape") {
		selectedEntry.set(null);
		return;
	}

	// Up/Down: navigate timeline
	if (e.key === "ArrowUp" || e.key === "ArrowDown") {
		e.preventDefault();
		const entries = get(timelineEntries);
		const current = get(selectedEntry);
		if (entries.length === 0) return;

		const currentIdx = current ? entries.findIndex((en) => en.id === current.id) : -1;
		const nextIdx =
			e.key === "ArrowDown"
				? Math.min(currentIdx + 1, entries.length - 1)
				: Math.max(currentIdx - 1, 0);
		selectedEntry.set(entries[nextIdx]);
		return;
	}
}
