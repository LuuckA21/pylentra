<script lang="ts">
  import { t } from "../lib/i18n";
  import { installUpdate, updates } from "../lib/stores/updates.svelte";

  const update = $derived(updates.available);
</script>

{#if update}
  <div
    class="update"
    role="region"
    aria-label={t("update.available", { version: update.version })}
  >
    <span>{t("update.available", { version: update.version })}</span>
    <button type="button" disabled={updates.installing} onclick={installUpdate}
      >{updates.installing
        ? t("update.installing")
        : t("update.install")}</button
    >
  </div>
{/if}

<style>
  .update {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin: var(--space-2) var(--space-3) 0;
    padding: var(--space-2) var(--space-4);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
  }
</style>
