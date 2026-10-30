<script lang="ts">
  import { t } from "../../lib/i18n";
  import { exportLibrary } from "../../lib/libraryTransfer";
  import { isReadOnly } from "../../lib/stores/library.svelte";

  let { onimportlibrary }: { onimportlibrary: () => void } = $props();
  const readOnly = $derived(isReadOnly());
</script>

<p class="hint">{t("library.hint")}</p>
<div class="actions">
  <button type="button" onclick={exportLibrary}>{t("library.export")}</button>
  <button
    type="button"
    disabled={readOnly}
    aria-describedby={readOnly ? "library-ro" : undefined}
    onclick={onimportlibrary}>{t("library.import")}</button
  >
</div>
{#if readOnly}
  <p class="hint" id="library-ro">{t("error.config.read_only")}</p>
{/if}

<style>
  .hint {
    margin: 0;
    color: var(--fg-muted);
    font-size: 12px;
  }
  .actions {
    display: flex;
    gap: var(--space-3);
  }
</style>
