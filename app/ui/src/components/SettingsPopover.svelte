<script lang="ts">
  import { tick } from "svelte";
  import { getConfig, moveStorage } from "../lib/api";
  import { open } from "@tauri-apps/plugin-dialog";
  import type { ConfigResponse } from "../lib/types";

  interface Props {
    visible: boolean;
    onClose: () => void;
  }

  let { visible, onClose }: Props = $props();

  let state = $state('idle');
  let config = $state<ConfigResponse | null>(null);
  let selectedPath = $state('');
  let errorMessage = $state('');
  let backupPath = $state('');
  let migratingStep = $state('');
  let loading = $state(false);

  // Load config when popover becomes visible
  $effect(() => {
    if (visible && state === 'idle') {
      loadConfig();
    }
  });

  async function loadConfig() {
    loading = true;
    try {
      config = await getConfig();
    } catch (err) {
      config = null;
    }
    loading = false;
  }

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
  }

  async function handleChangeLocation() {
    try {
      const selected = await open({ directory: true, title: "Choose new storage location" });
      if (selected && typeof selected === 'string') {
        selectedPath = selected;
        state = 'confirm';
      }
    } catch (err) {
      // User cancelled
    }
  }

  function handleCancel() {
    state = 'idle';
    selectedPath = '';
  }

  async function handleMoveStorage() {
    state = 'migrating';
    migratingStep = `Copying ${config ? formatBytes(config.disk_usage_bytes) : 'data'}...`;
    // Yield so Svelte updates the DOM before the async IPC call
    await tick();
    try {
      const result = await moveStorage(selectedPath);
      if (result && typeof result === 'object') {
        backupPath = (result as any).backup_path || '';
      }
      state = 'success';
      // Reload config to reflect new state
      await loadConfig();
      // Auto-dismiss success after 10s
      setTimeout(() => {
        if (state === 'success') {
          state = 'idle';
        }
      }, 10000);
    } catch (err: any) {
      state = 'error';
      errorMessage = err?.message || String(err) || 'Migration failed';
    }
  }

  function handleDismissError() {
    state = 'idle';
    errorMessage = '';
    loadConfig();
  }

  function handleClose() {
    if (state === 'migrating') return; // Can't close during migration
    onClose();
  }

  // Handle Escape key
  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && visible && state !== 'migrating') {
      handleClose();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if visible}
  <!-- Backdrop (not during migration) -->
  {#if state !== 'migrating'}
    <div class="popover-backdrop" role="button" tabindex="0" onclick={handleClose} onkeydown={(e) => e.key === 'Enter' && handleClose()}></div>
  {/if}

  <div class="settings-popover" class:locked={state === 'migrating'}>
    <!-- STATE: IDLE -->
    {#if state === 'idle'}
      <div class="popover-header">
        <span class="popover-title">Settings</span>
      </div>
      <div class="popover-body">
        {#if loading}
          <div class="loading">Loading...</div>
        {:else if config}
          <div class="config-section">
            <div class="config-label">Storage</div>
            <div class="config-value" title={config.storage_dir_display}>
              {config.storage_dir_display}
              {#if config.is_default}
                <span class="badge">default</span>
              {/if}
            </div>
            <div class="config-detail">
              {formatBytes(config.disk_usage_bytes)} across {config.project_count} project{config.project_count !== 1 ? 's' : ''}
            </div>
          </div>
          <button class="change-btn" onclick={handleChangeLocation}>
            Change Location...
          </button>
        {:else}
          <div class="config-error">Could not load configuration</div>
        {/if}
      </div>

    <!-- STATE: CONFIRM -->
    {:else if state === 'confirm'}
      <div class="popover-header">
        <span class="popover-title">Move Storage</span>
      </div>
      <div class="popover-body">
        <div class="confirm-detail">
          <p>Move storage to:</p>
          <code class="path-display">{selectedPath}</code>
        </div>
        {#if config}
          <div class="confirm-info">
            <p>This will:</p>
            <ul>
              <li>Pause recording</li>
              <li>Copy {formatBytes(config.disk_usage_bytes)}</li>
              <li>Restart recording</li>
            </ul>
            <p class="reassurance">Your original data will be kept as a backup.</p>
          </div>
        {/if}
        <div class="confirm-actions">
          <button class="btn-secondary" onclick={handleCancel}>Cancel</button>
          <button class="btn-primary" onclick={handleMoveStorage}>Move Storage</button>
        </div>
      </div>

    <!-- STATE: MIGRATING -->
    {:else if state === 'migrating'}
      <div class="popover-header">
        <span class="popover-title">Moving Storage...</span>
      </div>
      <div class="popover-body">
        <div class="progress-section">
          <div class="progress-bar">
            <div class="progress-fill indeterminate"></div>
          </div>
          <p class="progress-step">{migratingStep}</p>
          <p class="progress-text">Recording is paused during this operation.</p>
        </div>
      </div>

    <!-- STATE: SUCCESS -->
    {:else if state === 'success'}
      <div class="popover-header">
        <span class="popover-title success-title">Moved Successfully</span>
      </div>
      <div class="popover-body">
        <p class="success-text">Recording resumed.</p>
        <div class="success-detail">
          <span class="success-label">New location</span>
          <code class="path-display">{selectedPath}</code>
        </div>
        {#if backupPath}
          <p class="backup-text">Backup kept at <code>{backupPath}</code></p>
        {/if}
      </div>

    <!-- STATE: ERROR -->
    {:else if state === 'error'}
      <div class="popover-header">
        <span class="popover-title error-title">Storage Move Failed</span>
      </div>
      <div class="popover-body">
        <p class="error-text">{errorMessage}</p>
        <p class="reassurance">Recording has resumed at the original location. No data was lost.</p>
        <button class="btn-secondary" onclick={handleDismissError}>Dismiss</button>
      </div>
    {/if}
  </div>
{/if}

<style>
  .popover-backdrop {
    position: fixed;
    inset: 0;
    z-index: 999;
  }

  .settings-popover {
    position: fixed;
    top: 40px;
    right: 8px;
    width: 300px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.12);
    z-index: 1001;
    overflow: hidden;
  }

  .settings-popover.locked {
    pointer-events: auto;
  }

  .popover-header {
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
  }

  .popover-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text-primary);
  }

  .success-title {
    color: var(--addition);
  }

  .error-title {
    color: var(--deletion);
  }

  .popover-body {
    padding: 12px 14px;
  }

  .loading {
    color: var(--text-muted);
    font-size: var(--text-sm);
  }

  .config-section {
    margin-bottom: 12px;
  }

  .config-label {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-muted);
    margin-bottom: 4px;
  }

  .config-value {
    font-size: var(--text-sm);
    color: var(--text-primary);
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .badge {
    font-size: 10px;
    padding: 1px 5px;
    border-radius: 3px;
    background: var(--accent-bg);
    color: var(--accent);
    font-family: var(--font-sans);
    font-weight: 500;
  }

  .config-detail {
    font-size: var(--text-xs);
    color: var(--text-secondary);
    margin-top: 2px;
  }

  .config-error {
    color: var(--text-muted);
    font-size: var(--text-sm);
  }

  .change-btn {
    width: 100%;
    padding: 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    color: var(--text-primary);
    font-family: var(--font-sans);
    font-size: var(--text-sm);
    cursor: pointer;
    transition: background 0.15s;
  }

  .change-btn:hover {
    background: var(--accent-bg);
  }

  .confirm-detail {
    margin-bottom: 10px;
  }

  .confirm-detail p {
    margin: 0 0 4px;
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .path-display {
    display: block;
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    background: var(--bg);
    padding: 6px 8px;
    border-radius: 4px;
    word-break: break-all;
  }

  .confirm-info {
    font-size: var(--text-sm);
    color: var(--text-secondary);
    margin-bottom: 12px;
  }

  .confirm-info p {
    margin: 0 0 4px;
  }

  .confirm-info ul {
    margin: 0 0 8px;
    padding-left: 18px;
  }

  .confirm-info li {
    margin: 2px 0;
  }

  .reassurance {
    font-size: var(--text-xs);
    color: var(--text-muted);
    font-style: italic;
  }

  .confirm-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }

  .btn-secondary {
    padding: 6px 14px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    color: var(--text-primary);
    font-family: var(--font-sans);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .btn-secondary:hover {
    background: var(--bg);
  }

  .btn-primary {
    padding: 6px 14px;
    border: none;
    border-radius: 6px;
    background: var(--accent);
    color: white;
    font-family: var(--font-sans);
    font-size: var(--text-sm);
    font-weight: 500;
    cursor: pointer;
  }

  .btn-primary:hover {
    opacity: 0.9;
  }

  .progress-section {
    text-align: center;
  }

  .progress-bar {
    height: 4px;
    background: var(--border);
    border-radius: 2px;
    overflow: hidden;
    margin-bottom: 10px;
  }

  .progress-fill.indeterminate {
    height: 100%;
    width: 40%;
    background: var(--accent);
    border-radius: 2px;
    animation: indeterminate 1.5s ease-in-out infinite;
  }

  @keyframes indeterminate {
    0% { transform: translateX(-100%); }
    100% { transform: translateX(350%); }
  }

  .progress-step {
    font-size: var(--text-sm);
    color: var(--text-primary);
    margin: 0 0 4px;
  }

  .progress-text {
    font-size: var(--text-xs);
    color: var(--text-muted);
    margin: 0;
  }

  .success-text {
    font-size: var(--text-sm);
    color: var(--addition);
    margin: 0 0 8px;
  }

  .success-detail {
    margin-bottom: 8px;
  }

  .success-label {
    font-size: var(--text-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-muted);
    display: block;
    margin-bottom: 4px;
  }

  .backup-text {
    font-size: var(--text-xs);
    color: var(--text-muted);
    margin: 0;
  }

  .backup-text code {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
  }

  .error-text {
    font-size: var(--text-sm);
    color: var(--deletion);
    margin: 0 0 8px;
  }
</style>
