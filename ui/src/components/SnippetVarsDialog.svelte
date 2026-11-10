<script lang="ts">
  import type { Snippet } from "../lib/bindings/Snippet";
  import { t } from "../lib/i18n";
  import { modal } from "../lib/modal";
  import { recallVar } from "../lib/snippets";

  let {
    snippet,
    names,
    onsubmit,
    oncancel,
  }: {
    snippet: Snippet;
    names: string[];
    onsubmit: (values: Record<string, string>) => void;
    oncancel: () => void;
  } = $props();

  const uid = $props.id();

  // The dialog opens once per pick, so the props are read once.
  // svelte-ignore state_referenced_locally
  let values = $state(names.map((n) => recallVar(snippet.id, n)));

  function submit(e: SubmitEvent) {
    e.preventDefault();
    // fromEntries makes own properties even for a name like __proto__.
    onsubmit(Object.fromEntries(names.map((n, i) => [n, values[i]])));
  }
</script>

<div class="backdrop">
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="{uid}-title"
    use:modal={oncancel}
  >
    <h2 id="{uid}-title">{t("snippet.varsTitle", { name: snippet.name })}</h2>
    <form onsubmit={submit}>
      {#each names as n, i (n)}
        <div class="field">
          <label for="{uid}-{i}">{n}</label>
          <input id="{uid}-{i}" bind:value={values[i]} autocomplete="off" />
        </div>
      {/each}
      <div class="actions">
        <button type="button" onclick={oncancel}>{t("common.cancel")}</button>
        <button type="submit" class="primary">{t("snippet.varsSend")}</button>
      </div>
    </form>
  </div>
</div>

<style>
  .dialog {
    width: min(440px, calc(100vw - 32px));
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 8px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: var(--space-4);
  }
</style>
