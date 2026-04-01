import { get } from "svelte/store";
import { getProjectStatus } from "./api";
import { projectStatus, selectedProject } from "./stores";

let intervalId: ReturnType<typeof setInterval> | null = null;
let lastNewest: string | null = null;

export type OnNewData = () => void;

/**
 * Start polling for data changes.
 * For project tabs: checks project status and only refreshes when newest changes.
 * For global tab (selectedProject is null): refreshes unconditionally each tick.
 * If forceFirstRefresh is true, the first tick always triggers onNewData (used when
 * showing cached tab data that may be stale).
 */
export function startPolling(onNewData: OnNewData, forceFirstRefresh = false): void {
  stopPolling();
  lastNewest = forceFirstRefresh ? null : (get(projectStatus)?.newest ?? null);

  intervalId = setInterval(async () => {
    if (!document.hasFocus()) return;

    // Global mode: no project status to check, refresh unconditionally
    if (!get(selectedProject)) {
      try { onNewData(); } catch (_e) { /* non-critical */ }
      return;
    }

    try {
      const status = await getProjectStatus();
      projectStatus.set(status);
      if (status.newest && status.newest !== lastNewest) {
        lastNewest = status.newest;
        onNewData();
      }
    } catch (_e) {
      // Polling failures are non-critical
    }
  }, 5000);
}

export function stopPolling(): void {
  if (intervalId !== null) {
    clearInterval(intervalId);
    intervalId = null;
  }
}
