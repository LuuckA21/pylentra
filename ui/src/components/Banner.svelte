<script lang="ts">
  import { errorMessage, t } from "../lib/i18n";
  import { library } from "../lib/stores/library.svelte";
</script>

{#if library.warnings.length}
  <div class="banner" role="region" aria-label={t("banner.title")}>
    {#each library.warnings as w, i (i)}
      <p>{errorMessage(w)}</p>
    {/each}
  </div>
{/if}

<style>
  .banner {
    padding: 6px 12px;
    background: var(--warning-bg);
    border-bottom: 1px solid var(--border);
  }
  p {
    margin: 2px 0;
  }
</style>
