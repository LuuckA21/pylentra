<script lang="ts">
  import { t } from "../lib/i18n";
  import { dismiss, toasts } from "../lib/stores/toasts.svelte";
</script>

<div class="toasts" role="status" aria-live="polite">
  {#each toasts as toast (toast.id)}
    <div class="toast {toast.kind}">
      <span>{toast.message}</span>
      <button
        type="button"
        aria-label={t("common.dismiss")}
        onclick={() => dismiss(toast.id)}>×</button
      >
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 16px;
    bottom: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: min(420px, calc(100vw - 32px));
    z-index: 100;
  }
  .toast {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    justify-content: space-between;
    padding: 8px 12px;
    border-radius: 6px;
    border: 1px solid var(--danger);
    background: var(--danger-bg);
    box-shadow: var(--shadow);
  }
  .toast.warning {
    border-color: var(--border);
    background: var(--warning-bg);
  }
  .toast button {
    border: 0;
    background: none;
    padding: 0 4px;
  }
</style>
