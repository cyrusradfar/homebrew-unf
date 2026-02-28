<script lang="ts">
  import { projects, openTabs, activeTab, GLOBAL_TAB } from "../lib/stores";
  import { closeTab, switchTab } from "../lib/stores";
  import { listProjects, removeProject } from "../lib/api";
  import type { ProjectEntry } from "../lib/types";

  interface Props {
    onProjectOpened: (path: string) => void;
  }

  let { onProjectOpened }: Props = $props();

  let dropdownOpen = $state(false);
  let dropdownButtonRef: HTMLButtonElement | undefined = $state();
  let dropdownRef: HTMLDivElement | undefined = $state();

  // Projects not yet open as tabs (available to add)
  const availableProjects = $derived(
    $projects.filter((p) => !$openTabs.includes(p.path))
  );

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

  // Helper: check if project is selectable (not orphaned/crashed/error)
  function isSelectable(status: ProjectEntry["status"]): boolean {
    return status === "watching" || status === "stopped";
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

  // Handle removing a project (orphaned/crashed/error)
  async function handleRemove(e: MouseEvent, path: string) {
    e.stopPropagation();
    try {
      await removeProject(path);
      const result = await listProjects();
      projects.set(result.projects);
    } catch (_e) {
      // Will show in toast
    }
  }

  // Toggle dropdown
  function toggleDropdown() {
    dropdownOpen = !dropdownOpen;
  }

  /** Get display label for a tab */
  function tabLabel(tabId: string): string {
    if (tabId === GLOBAL_TAB) return "All Projects";
    return tabId.split("/").pop() ?? tabId;
  }
</script>

<!-- Click outside detector -->
<svelte:window onmousedown={handleClickOutside} />

<div class="topbar">
  <!-- Left: Project Dropdown -->
  <div class="dropdown-container">
    <button
      bind:this={dropdownButtonRef}
      class="dropdown-button"
      onclick={toggleDropdown}
      title="Open project list"
    >
      <span class="dropdown-label">Select project</span>
      <span class="chevron">{dropdownOpen ? "▾" : "▸"}</span>
    </button>

    {#if dropdownOpen}
      <div bind:this={dropdownRef} class="dropdown-panel">
        {#if availableProjects.length === 0}
          <div class="dropdown-empty">
            <p>{$projects.length === 0 ? "No projects available" : "All projects are open"}</p>
          </div>
        {:else}
          <ul class="project-dropdown-list">
            {#each availableProjects as project (project.path)}
              <li>
                {#if isSelectable(project.status)}
                  <button
                    class="dropdown-item selectable"
                    onclick={() => handleSelectProject(project.path)}
                  >
                    <span class="status-dot" style="background: {statusColor(project.status)}"></span>
                    <div class="project-info">
                      <span class="project-name">{project.path.split("/").pop()}</span>
                      <span class="project-meta">
                        {project.status}
                        {#if project.snapshots !== null}
                          &middot; {project.snapshots} snapshots
                        {/if}
                      </span>
                    </div>
                  </button>
                {:else}
                  <div class="dropdown-item disabled">
                    <span class="status-dot" style="background: {statusColor(project.status)}"></span>
                    <div class="project-info">
                      <span class="project-name">{project.path.split("/").pop()}</span>
                      <span class="project-meta">{project.status}</span>
                    </div>
                    <button
                      class="remove-btn"
                      title="Remove from list"
                      onclick={(e) => handleRemove(e, project.path)}
                    >
                      ×
                    </button>
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {/if}
  </div>

  <!-- Center: Open Tabs -->
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
          <button
            class="tab-close"
            onclick={() => closeTab(tabPath)}
            title="Close tab"
          >
            ×
          </button>
        </div>
      {/key}
    {/each}
  </div>
</div>

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
    border-right: 1px solid var(--border);
    flex-shrink: 0;
  }

  .dropdown-button {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 12px;
    height: 36px;
    border: none;
    background: none;
    color: var(--text-primary);
    font-family: var(--font-sans);
    font-size: var(--text-sm);
    cursor: pointer;
    white-space: nowrap;
  }

  .dropdown-button:hover {
    background: var(--accent-bg);
  }

  .dropdown-label {
    max-width: 150px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
    min-width: 280px;
    max-height: 400px;
    overflow-y: auto;
    z-index: 1000;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
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
  }

  .dropdown-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 8px 12px;
    border: none;
    background: none;
    color: var(--text-primary);
    font-family: var(--font-sans);
    font-size: var(--text-sm);
    text-align: left;
    cursor: pointer;
  }

  .dropdown-item.selectable:hover {
    background: var(--accent-bg);
  }

  .dropdown-item.disabled {
    opacity: 0.6;
    cursor: default;
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

  .remove-btn {
    flex-shrink: 0;
    width: 24px;
    height: 24px;
    border: none;
    background: var(--deletion-bg);
    color: var(--deletion);
    border-radius: 4px;
    cursor: pointer;
    font-size: 12px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background 0.2s, color 0.2s;
  }

  .remove-btn:hover {
    background: var(--deletion);
    color: white;
  }

  /* ===== TABS SECTION ===== */
  .tabs-container {
    display: flex;
    align-items: center;
    flex: 1;
    overflow-x: auto;
    overflow-y: hidden;
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
</style>
