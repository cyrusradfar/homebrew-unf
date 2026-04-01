/**
 * Human-friendly relative time formatting for filter status bar.
 *
 * Returns a vague-but-contextual description (text) and exact timestamps (tooltip).
 */

interface TimeRangeResult {
	text: string;
	tooltip: string;
}

type TimeOfDay = "morning" | "afternoon" | "evening" | "night";

function timeOfDay(d: Date): TimeOfDay {
	const h = d.getHours();
	if (h >= 5 && h < 12) return "morning";
	if (h >= 12 && h < 17) return "afternoon";
	if (h >= 17 && h < 21) return "evening";
	return "night";
}

function formatDuration(ms: number): string {
	const mins = Math.round(ms / 60_000);
	if (mins < 2) return "1 min";
	if (mins < 60) return `${mins} min`;
	const hrs = Math.round(ms / 3_600_000);
	if (hrs < 24) return `${hrs} hr${hrs === 1 ? "" : "s"}`;
	const days = Math.round(ms / 86_400_000);
	return `${days} day${days === 1 ? "" : "s"}`;
}

function formatRelativeDay(d: Date, now: Date): string {
	const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate());
	const startOfTarget = new Date(d.getFullYear(), d.getMonth(), d.getDate());
	const dayDiff = Math.floor((startOfToday.getTime() - startOfTarget.getTime()) / 86_400_000);

	if (dayDiff === 0) return "today";
	if (dayDiff === 1) return "yesterday";
	if (dayDiff < 7) {
		const dayName = d.toLocaleDateString(undefined, { weekday: "short" });
		return `last ${dayName}`;
	}
	return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

function formatExact(d: Date): string {
	return d.toLocaleString(undefined, {
		month: "short",
		day: "numeric",
		hour: "numeric",
		minute: "2-digit",
		second: "2-digit",
	});
}

export function formatTimeRange(
	startIso: string,
	endIso: string,
	isSession: boolean
): TimeRangeResult {
	const start = new Date(startIso);
	const end = new Date(endIso);
	const now = new Date();
	const durationMs = end.getTime() - start.getTime();
	const agoMs = now.getTime() - end.getTime();

	const duration = formatDuration(durationMs);
	const relDay = formatRelativeDay(start, now);
	const tod = timeOfDay(start);

	let text: string;

	// Recent + short: "2 hrs ago for 45 min"
	if (agoMs < 24 * 3_600_000 && durationMs < 2 * 3_600_000) {
		const agoStr = formatDuration(agoMs);
		text = `${agoStr} ago for ${duration}`;
	} else if (relDay === "today" || relDay === "yesterday") {
		// Today/yesterday: "today morning for 1 hr"
		text = `${relDay} ${tod} for ${duration}`;
	} else {
		// Older: "last Thu afternoon for 3 hrs" or "Feb 5 afternoon for 3 hrs"
		text = `${relDay} ${tod} for ${duration}`;
	}

	if (isSession) {
		text = `session ${text}`;
	}

	const tooltip = `${formatExact(start)} → ${formatExact(end)}`;

	return { text, tooltip };
}
