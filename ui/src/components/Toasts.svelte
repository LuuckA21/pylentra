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
    right: var(--space-4);
    bottom: var(--space-4);
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: min(420px, calc(100vw - 32px));
    z-index: 100;
  }
  .toast {
    display: flex;
    gap: 8px;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-2) var(--space-4);
    border-radius: 999px;
    border: 1px solid var(--danger);
    background: var(--danger-bg);
    box-shadow: var(--shadow);
  }
  .toast.success {
    border-color: var(--ok);
    background: var(--surface-2);
  }
  .toast.warning {
    border-color: var(--border);
    background: var(--warning-bg);
  }
  .toast button {
    border: 0;
    background: none;
    padding: 0 var(--space-1);
  }
</style>
