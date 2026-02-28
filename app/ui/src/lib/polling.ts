import { get } from "svelte/store";
import { getProjectStatus } from "./api";
import { projectStatus, selectedProject } from "./stores";

let intervalId: ReturnType<typeof setInterval> | null = null;
let lastNewest: string | null = null;

export type OnNewData = () => void;

export function startPolling(onNewData: OnNewData): void {
  stopPolling();
  lastNewest = get(projectStatus)?.newest ?? null;

  intervalId = setInterval(async () => {
    if (!document.hasFocus()) return;
    if (!get(selectedProject)) return;

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
