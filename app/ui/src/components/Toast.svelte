<script lang="ts">
import { error } from "../lib/stores";

let visible = $state(false);
let message = $state("");
let timer: ReturnType<typeof setTimeout> | null = null;

error.subscribe((val) => {
	if (val) {
		message = val;
		visible = true;
		if (timer) clearTimeout(timer);
		timer = setTimeout(() => {
			visible = false;
			error.set(null);
		}, 5000);
	}
});

function dismiss() {
	visible = false;
	error.set(null);
	if (timer) clearTimeout(timer);
}
</script>

{#if visible}
  <div class="toast" role="alert">
    <span class="toast-message">{message}</span>
    <button class="toast-close" onclick={dismiss}>&times;</button>
  </div>
{/if}

<style>
  .toast {
    position: fixed;
    bottom: 24px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--text-primary);
    color: var(--bg);
    padding: 10px 16px;
    border-radius: 8px;
    font-size: var(--text-sm);
    display: flex;
    align-items: center;
    gap: 12px;
    max-width: 500px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
    z-index: 100;
    animation: slideUp 200ms ease-out;
  }
  @keyframes slideUp {
    from { transform: translateX(-50%) translateY(20px); opacity: 0; }
    to { transform: translateX(-50%) translateY(0); opacity: 1; }
  }
  .toast-message {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .toast-close {
    background: none;
    border: none;
    color: var(--bg);
    cursor: pointer;
    font-size: 16px;
    padding: 0;
    opacity: 0.7;
  }
  .toast-close:hover { opacity: 1; }
</style>
