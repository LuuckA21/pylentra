<script lang="ts">
  import { errorMessage, t } from "../lib/i18n";
  import { library } from "../lib/stores/library.svelte";
</script>

{#if library.warnings.length}
  <div class="banner" role="region" aria-label={t("banner.title")}>
    {#each library.warnings as w, i (i)}
      <p>
        {w.params.file
          ? `${w.params.file}: ${errorMessage(w)}`
          : errorMessage(w)}
      </p>
    {/each}
  </div>
{/if}

<style>
  .banner {
    margin: var(--space-2) var(--space-3) 0;
    padding: var(--space-2) var(--space-4);
    background: var(--warning-bg);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
  }
  p {
    margin: 2px 0;
  }
</style>
