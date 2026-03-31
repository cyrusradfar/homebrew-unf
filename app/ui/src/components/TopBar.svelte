<script lang="ts">
  import { projects, openTabs, activeTab, GLOBAL_TAB, error } from "../lib/stores";
  import { closeTab, switchTab } from "../lib/stores";
  import { listProjects, removeProject, watchProject, unwatchProject } from "../lib/api";
  import { open } from "@tauri-apps/plugin-dialog";
  import type { ProjectEntry } from "../lib/types";
  import SettingsPopover from "./SettingsPopover.svelte";

  interface Props {
    onProjectOpened: (path: string) => void;
  }

  let { onProjectOpened }: Props = $props();

  let dropdownOpen = $state(false);
  let dropdownButtonRef: HTMLButtonElement | undefined = $state();
  let dropdownRef: HTMLDivElement | undefined = $state();
  let settingsOpen = $state(false);

  // Helper: status color for indicator dot
  function statusColor(status: ProjectEntry["status"]): string {
    switch (status) {
      case "watching": return "var(--addition)";
      case "stopped": return "var(--text-muted)";
      case "crashed":
      case "orphaned":
      case "error": return "var(--deletion)";
      default: return "var(--text-muted)";
    }
  }

  // Helper: get action button label for a project status
  function actionLabel(status: ProjectEntry["status"]): string {
    switch (status) {
      case "watching": return "Stop";
      case "stopped": return "Start";
      case "crashed": return "Restart";
      case "orphaned":
      case "error": return "Remove";
      default: return "";
    }
  }

  // Helper: get action button style class for a project status
  function actionStyle(status: ProjectEntry["status"]): string {
    switch (status) {
      case "watching": return "action-muted";
      case "stopped": return "action-accent";
      case "crashed": return "action-warning";
      case "orphaned":
      case "error": return "action-danger";
      default: return "";
    }
  }

  // Close dropdown when clicking outside
  function handleClickOutside(event: MouseEvent) {
    if (!dropdownOpen) return;
    const target = event.target as HTMLElement;
    if (dropdownRef && !dropdownRef.contains(target) &&
        dropdownButtonRef && !dropdownButtonRef.contains(target)) {
      dropdownOpen = false;
    }
  }

  // Handle selecting a project from the dropdown
  function handleSelectProject(path: string) {
    dropdownOpen = false;
    onProjectOpened(path);
  }

  // Handle per-project action buttons (Start/Stop/Restart/Remove)
  async function handleAction(e: MouseEvent, project: ProjectEntry) {
    e.stopPropagation();
    const path = project.path;
    try {
      switch (project.status) {
        case "watching":
          await unwatchProject(path);
          break;
        case "stopped":
        case "crashed":
          await watchProject(path);
          break;
        case "orphaned":
        case "error":
          await removeProject(path);
          break;
      }
      // Refresh project list
      const result = await listProjects();
      projects.set(result.projects);
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      error.set(`Failed to ${getActionVerb(project.status)}: ${errorMessage}`);
      console.error("Action failed:", err);
    }
  }

  // Helper: get action verb for error message
  function getActionVerb(status: ProjectEntry["status"]): string {
    switch (status) {
      case "watching": return "stop watching";
      case "stopped": return "start watching";
      case "crashed": return "restart";
      case "orphaned":
      case "error": return "remove";
      default: return "perform action on";
    }
  }

  // Handle watch new folder button
  async function handleWatchNewFolder() {
    try {
      const selected = await open({ directory: true, title: "Select folder to watch" });
      if (selected && typeof selected === "string") {
        await watchProject(selected);
        const result = await listProjects();
        projects.set(result.projects);
      }
    } catch (err) {
      const errorMessage = err instanceof Error ? err.message : String(err);
      error.set(`Failed to watch folder: ${errorMessage}`);
      console.error("Watch folder failed:", err);
    }
  }

  // Toggle dropdown
  function toggleDropdown() {
    dropdownOpen = !dropdownOpen;
  }

  // Toggle settings popover
  function toggleSettings() {
    settingsOpen = !settingsOpen;
  }

  /** Get display label for a tab */
  function tabLabel(tabId: string): string {
    if (tabId === GLOBAL_TAB) return "All";
    return tabId.split("/").pop() ?? tabId;
  }
</script>

<!-- Click outside detector -->
<svelte:window onmousedown={handleClickOutside} />

<div class="topbar">
  <!-- Tabs + Add button (inline) -->
  <div class="tabs-container">
    {#each $openTabs as tabPath (tabPath)}
      {#key tabPath}
        <div class="tab" class:active={$activeTab === tabPath} class:global-tab={tabPath === GLOBAL_TAB}>
          <button
            class="tab-button"
            onclick={() => switchTab(tabPath)}
          >
            {tabLabel(tabPath)}
          </button>
          {#if tabPath !== GLOBAL_TAB}
            <button
              class="tab-close"
              onclick={() => closeTab(tabPath)}
              title="Close tab"
            >
              ×
            </button>
          {/if}
        </div>
      {/key}
    {/each}

    <!-- Add tab button (inline after last tab) -->
    <div class="dropdown-container">
      <button
        bind:this={dropdownButtonRef}
        class="add-tab-button"
        onclick={toggleDropdown}
        title="Add project tab"
      >
        +
      </button>

      {#if dropdownOpen}
        <div bind:this={dropdownRef} class="dropdown-panel">
          {#if $projects.length === 0}
            <div class="dropdown-empty">
              <p>No projects available</p>
            </div>
          {:else}
            <ul class="project-dropdown-list">
              {#each $projects as project (project.path)}
                <li>
                  <div class="dropdown-item" class:active-tab={$openTabs.includes(project.path)}>
                    <button
                      class="project-select-area"
                      onclick={() => handleSelectProject(project.path)}
                    >
                      <span class="status-dot" style="background: {statusColor(project.status)}"></span>
                      <div class="project-info">
                        <span class="project-name">{project.path.split("/").pop()}</span>
                        <span class="project-meta">
                          {project.status}
                          {#if project.snapshots !== null}
                            &middot; {project.snapshots.toLocaleString()} snapshots
                          {/if}
                        </span>
                      </div>
                    </button>
                    <button
                      class="action-btn {actionStyle(project.status)}"
                      onclick={(e) => handleAction(e, project)}
                    >
                      {actionLabel(project.status)}
                    </button>
                  </div>
                </li>
              {/each}
            </ul>
          {/if}
          <div class="dropdown-footer">
            <button class="watch-folder-btn" onclick={handleWatchNewFolder}>
              + Watch New Folder
            </button>
          </div>
        </div>
      {/if}
    </div>
  </div>

  <!-- Right: Settings -->
  <div class="settings-container">
    <button
      class="gear-button"
      class:active={settingsOpen}
      onclick={toggleSettings}
      title="Settings"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="12" cy="12" r="3"></circle>
        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
      </svg>
    </button>
  </div>
</div>

<SettingsPopover visible={settingsOpen} onClose={() => settingsOpen = false} />

<style>
  .topbar {
    display: flex;
    align-items: center;
    height: 36px;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    gap: 0;
  }

  /* ===== DROPDOWN SECTION ===== */
  .dropdown-container {
    position: relative;
    flex-shrink: 0;
  }

  .add-tab-button {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 36px;
    border: none;
    background: none;
    color: var(--text-muted);
    font-size: 16px;
    cursor: pointer;
    transition: color 0.15s, background 0.15s;
  }

  .add-tab-button:hover {
    color: var(--text-primary);
    background: var(--accent-bg);
  }

  .chevron {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 10px;
    color: var(--text-muted);
    width: 12px;
  }

  .dropdown-panel {
    position: absolute;
    top: 36px;
    left: 0;
    background: var(--surface);
    border: 1px solid var(--border);
    border-top: none;
    border-radius: 0 0 6px 6px;
    min-width: 340px;
    max-height: 400px;
    overflow-y: auto;
    z-index: 1000;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
    display: flex;
    flex-direction: column;
  }

  .dropdown-empty {
    padding: 12px;
    text-align: center;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }

  .project-dropdown-list {
    list-style: none;
    padding: 0;
    margin: 0;
    flex: 1;
    overflow-y: auto;
  }

  .dropdown-item {
    display: flex;
    align-items: center;
    gap: 0;
    width: 100%;
    padding: 0;
  }

  .dropdown-item.active-tab {
    background: var(--accent-bg);
  }

  .project-select-area {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    padding: 8px 8px 8px 12px;
    border: none;
    background: none;
    color: var(--text-primary);
    font-family: var(--font-sans);
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
    min-width: 0;
  }

  .project-select-area:hover {
    background: var(--accent-bg);
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .project-info {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }

  .project-name {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .project-meta {
    font-size: 11px;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .action-btn {
    flex-shrink: 0;
    padding: 4px 10px;
    margin-right: 8px;
    border: none;
    border-radius: 4px;
    font-family: var(--font-sans);
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: opacity 0.15s;
    white-space: nowrap;
  }

  .action-btn:hover {
    opacity: 0.8;
  }

  .action-muted {
    background: rgba(0, 0, 0, 0.05);
    color: var(--text-muted);
  }

  .action-accent {
    background: var(--accent-bg);
    color: var(--accent);
  }

  .action-warning {
    background: rgba(245, 158, 11, 0.15);
    color: #f59e0b;
  }

  .action-danger {
    background: var(--deletion-bg);
    color: var(--deletion);
  }

  .dropdown-footer {
    border-top: 1px solid var(--border);
    padding: 6px 8px;
    flex-shrink: 0;
  }

  .watch-folder-btn {
    width: 100%;
    padding: 8px;
    border: 1px dashed var(--border);
    border-radius: 4px;
    background: none;
    color: var(--text-secondary);
    font-family: var(--font-sans);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background 0.15s, color 0.15s, border-color 0.15s;
  }

  .watch-folder-btn:hover {
    background: var(--accent-bg);
    color: var(--accent);
    border-color: var(--accent);
  }

  /* ===== TABS SECTION ===== */
  .tabs-container {
    display: flex;
    align-items: center;
    flex: 1;
    gap: 0;
  }

  .tab {
    display: flex;
    align-items: center;
    height: 36px;
    border-right: 1px solid var(--border);
    flex-shrink: 0;
    transition: background 0.15s;
  }

  .tab.active {
    background: var(--accent-bg);
    border-bottom: 2px solid var(--accent);
  }

  .tab.global-tab .tab-button {
    font-weight: 600;
  }

  .tab-button {
    padding: 0 12px;
    height: 36px;
    border: none;
    background: none;
    color: var(--text-primary);
    font-family: var(--font-sans);
    font-size: var(--text-sm);
    cursor: pointer;
    white-space: nowrap;
    display: flex;
    align-items: center;
  }

  .tab-button:hover {
    background: var(--accent-bg);
  }

  .tab-close {
    padding: 0 4px 0 0;
    height: 36px;
    width: 28px;
    border: none;
    background: none;
    color: var(--text-muted);
    font-size: 14px;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0.6;
    transition: opacity 0.15s, color 0.15s;
    margin-right: 4px;
  }

  .tab-close:hover {
    opacity: 1;
    color: var(--text-primary);
  }

  /* ===== SETTINGS SECTION ===== */
  .settings-container {
    flex-shrink: 0;
    border-left: 1px solid var(--border);
  }

  .gear-button {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    border: none;
    background: none;
    color: var(--text-muted);
    cursor: pointer;
    transition: color 0.15s, background 0.15s;
  }

  .gear-button:hover {
    color: var(--text-primary);
    background: var(--accent-bg);
  }

  .gear-button.active {
    color: var(--accent);
    background: var(--accent-bg);
  }
</style>
