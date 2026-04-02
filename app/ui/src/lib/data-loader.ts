/**
 * Data-loading functions for the App.
 * All functions accept a `gen` counter for stale-response detection.
 * Caller must pass `isStale` check (gen !== current requestGen).
 */
import { get } from "svelte/store";
import { getDensity, getGlobalDensity, getGlobalLog, getLog, selectProject } from "./api";
import { filtersToGlobs } from "./filterUtils";
import { startPolling, stopPolling } from "./polling";
import {
	activeTab,
	densityBuckets,
	error,
	fileFilters,
	fileTree,
	histogramEnd,
	histogramStart,
	nextCursor,
	projectStatus,
	timelineEntries,
	timelineLoading,
} from "./stores";
import type { GlobalGroupedLogResponse, GroupedLogFile } from "./types";
import { GLOBAL_TAB } from "./types";

/** Shared mutable generation counter. Increment before each data session. */
export let requestGen = 0;
export function nextGen(): number {
	return ++requestGen;
}

export async function activateProject(path: string, cached = false): Promise<void> {
	const gen = nextGen();
	stopPolling();
	error.set(null);
	try {
		const status = await selectProject(path);
		if (gen !== requestGen) return;
		projectStatus.set(status);
		if (!cached) {
			await refreshAllData(gen);
			if (gen !== requestGen) return;
		}
		startPolling(() => refreshAllData(requestGen), cached);
	} catch (e) {
		if (gen === requestGen) error.set(`Failed to activate project: ${e}`);
	}
}

export async function activateGlobal(cached = false): Promise<void> {
	const gen = nextGen();
	stopPolling();
	error.set(null);
	projectStatus.set(null);
	try {
		if (!cached) {
			await refreshGlobalData(gen);
			if (gen !== requestGen) return;
		}
		startPolling(() => refreshGlobalData(requestGen));
	} catch (e) {
		if (gen === requestGen) error.set(`Failed to load global view: ${e}`);
	}
}

export async function refreshGlobalData(gen: number): Promise<void> {
	const include = getInclude();
	const [since, until] = getTimeRange();
	await Promise.all([
		loadGlobalTimeline(gen, include, since, until),
		loadGlobalFileTree(gen, include, since, until),
		loadGlobalDensity(gen, include),
	]);
}

export async function refreshAllData(gen: number): Promise<void> {
	const include = getInclude();
	const [since, until] = getTimeRange();
	await Promise.all([
		loadTimeline(gen, undefined, include, since, until),
		loadFileTree(gen, include, since, until),
		loadDensity(gen, include),
	]);
}

export function triggerFilteredReload(): void {
	const gen = nextGen();
	const include = getInclude();
	const [since, until] = getTimeRange();
	if (isGlobal()) {
		loadGlobalTimeline(gen, include, since, until);
		loadGlobalFileTree(gen, include, since, until);
		loadGlobalDensity(gen, include);
	} else {
		loadTimeline(gen, undefined, include, since, until);
		loadDensity(gen, include);
		loadFileTree(gen, include, since, until);
	}
}

export function triggerTimeRangeReload(): void {
	const gen = nextGen();
	const include = getInclude();
	const [since, until] = getTimeRange();
	if (isGlobal()) {
		loadGlobalTimeline(gen, include, since, until);
		loadGlobalFileTree(gen, include, since, until);
	} else {
		loadTimeline(gen, undefined, include, since, until);
		loadFileTree(gen, include, since, until);
	}
}

export function triggerLoadMore(cursor: string): void {
	const include = getInclude();
	const [since, until] = getTimeRange();
	loadTimeline(requestGen, cursor, include, since, until);
}

// -- Helpers --

function getInclude(): string[] | undefined {
	const globs = filtersToGlobs(get(fileFilters));
	return globs.length > 0 ? globs : undefined;
}

function getTimeRange(): [string | null, string | null] {
	return [get(histogramStart), get(histogramEnd)];
}

function isGlobal(): boolean {
	return get(activeTab) === GLOBAL_TAB;
}

// -- Individual loaders --

async function loadGlobalTimeline(
	gen: number,
	include?: string[],
	since?: string | null,
	until?: string | null
): Promise<void> {
	timelineLoading.set(true);
	try {
		const result = await getGlobalLog({
			limit: 200,
			include,
			since: since ?? undefined,
			until: until ?? undefined,
		});
		if (gen !== requestGen) return;
		timelineEntries.set(result.entries);
		nextCursor.set(null);
	} catch (e) {
		if (gen === requestGen) error.set(`Failed to load global timeline: ${e}`);
	} finally {
		if (gen === requestGen) timelineLoading.set(false);
	}
}

async function loadGlobalFileTree(
	gen: number,
	include?: string[],
	since?: string | null,
	until?: string | null
): Promise<void> {
	try {
		const result = (await getGlobalLog({
			groupByFile: true,
			include,
			since: since ?? undefined,
			until: until ?? undefined,
			limit: 5000,
		})) as GlobalGroupedLogResponse;
		if (gen !== requestGen) return;
		const files: GroupedLogFile[] = [];
		for (const proj of result.projects) {
			for (const file of proj.files) {
				const taggedEntries = file.entries.map((e) => ({ ...e, project: proj.project }));
				if (taggedEntries.length > 0) {
					files.push({
						...file,
						entries: taggedEntries,
						change_count: taggedEntries.length,
						project: proj.project,
					});
				}
			}
		}
		fileTree.set(files);
	} catch (_e) {
		// Non-critical
	}
}

async function loadTimeline(
	gen: number,
	cursor?: string,
	include?: string[],
	since?: string | null,
	until?: string | null
): Promise<void> {
	timelineLoading.set(true);
	try {
		const result = await getLog({
			limit: 50,
			cursor: cursor ?? undefined,
			include,
			since: since ?? undefined,
			until: until ?? undefined,
		});
		if (gen !== requestGen) return;
		if (cursor) {
			timelineEntries.update((prev) => [...prev, ...result.entries]);
		} else {
			timelineEntries.set(result.entries);
		}
		nextCursor.set(result.next_cursor);
	} catch (e) {
		if (gen === requestGen) error.set(`Failed to load timeline: ${e}`);
	} finally {
		if (gen === requestGen) timelineLoading.set(false);
	}
}

async function loadFileTree(
	gen: number,
	include?: string[],
	since?: string | null,
	until?: string | null
): Promise<void> {
	try {
		const result = await getLog({
			groupByFile: true,
			include,
			since: since ?? undefined,
			until: until ?? undefined,
			limit: 5000,
		});
		if (gen !== requestGen) return;
		fileTree.set(result.files);
	} catch (_e) {
		// Non-critical
	}
}

async function loadDensity(gen: number, include?: string[]): Promise<void> {
	try {
		const result = await getDensity({ buckets: 100, include });
		if (gen !== requestGen) return;
		densityBuckets.set(result.buckets);
	} catch (_e) {
		// Non-critical
	}
}

async function loadGlobalDensity(gen: number, include?: string[]): Promise<void> {
	try {
		const result = await getGlobalDensity({ buckets: 100, include });
		if (gen !== requestGen) return;
		densityBuckets.set(result.buckets);
	} catch (_e) {
		densityBuckets.set([]);
	}
}
