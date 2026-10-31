<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import { api } from "../lib/api";
  import type { ImportPick } from "../lib/bindings/ImportPick";
  import { errorMessage, t } from "../lib/i18n";
  import { jumpText } from "../lib/libraryTransfer";
  import { modal } from "../lib/modal";
  import { refresh } from "../lib/stores/library.svelte";
  import { loadSnippets } from "../lib/stores/snippets.svelte";
  import { isApiError, toast, toastError } from "../lib/stores/toasts.svelte";
  import Avatar from "./ui/Avatar.svelte";
  import Pill from "./ui/Pill.svelte";

  let { pick, onclose }: { pick: ImportPick; onclose: () => void } = $props();

  const p = $derived(pick.preview);
  // Duplicates start unchecked; the rest of the file is imported by default.
  // svelte-ignore state_referenced_locally
  const selected = new SvelteSet(
    pick.preview.rows.filter((r) => !r.duplicate).map((r) => r.id),
  );
  let busy = $state(false);
  let error = $state("");

  const toggle = (id: string) => {
    if (!selected.delete(id)) selected.add(id);
  };

  async function submit() {
    busy = true;
    error = "";
    try {
      // File order, so the new connections keep it.
      const c = await api.libraryImport(
        p.rows.filter((r) => selected.has(r.id)).map((r) => r.id),
      );
      toast(t("library.imported", c), "success");
    } catch (e) {
      const reason = isApiError(e) ? errorMessage(e) : String(e);
      const done = isApiError(e) ? e.params.connections_imported : undefined;
      if (done === undefined) {
        error = reason;
        busy = false;
        return;
      }
      // The connections are in and the backend forgot the file: retrying would duplicate them.
      toast(t("library.partial", { connections: done, reason }));
    }
    await Promise.all([refresh(), loadSnippets()]).catch(toastError);
    onclose();
  }
</script>

<div class="backdrop">
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="library-import-title"
    use:modal={() => {
      if (!busy) onclose();
    }}
  >
    <h2 id="library-import-title">{t("library.importTitle")}</h2>
    <p class="hint path">{pick.path}</p>

    <ul class="hosts" aria-label={t("library.importTitle")}>
      {#each p.rows as r (r.id)}
        {@const jump = jumpText(r, selected)}
        {@const noteIds = [
          ...(r.missing_key ? [`n-${r.id}-key`] : []),
          ...(jump ? [`n-${r.id}-jump`] : []),
        ]}
        <li>
          <label class="row">
            <input
              type="checkbox"
              checked={selected.has(r.id)}
              aria-describedby={noteIds.join(" ") || undefined}
              onchange={() => toggle(r.id)}
            />
            <span aria-hidden="true"
              ><Avatar name={r.name} color={null} size={28} /></span
            >
            <span class="main">
              <span class="name">{r.name}</span>
              <span class="meta">
                {r.target}{r.group ? ` · ${r.group}` : ""}
              </span>
            </span>
            {#if r.duplicate}<Pill>{t("library.duplicate")}</Pill>{/if}
          </label>
          {#if r.missing_key}
            <p class="note error" id="n-{r.id}-key">
              {t("library.missingKey", { path: r.missing_key })}
            </p>
          {/if}
          {#if jump}
            <p class="note" id="n-{r.id}-jump">{jump}</p>
          {/if}
        </li>
      {/each}
    </ul>

    <p class="hint">
      {t("library.summary", {
        new: p.groups_new,
        reused: p.groups_reused,
        global: p.snippets_global,
        perConnection: p.snippets_per_connection,
      })}
    </p>

    {#if error}
      <p class="note error" role="alert">{error}</p>
    {/if}
    <div class="actions">
      <button type="button" disabled={busy} onclick={onclose}
        >{t("common.cancel")}</button
      >
      <button
        type="button"
        class="primary"
        disabled={busy || selected.size === 0}
        onclick={submit}>{t("library.importN", { n: selected.size })}</button
      >
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: var(--backdrop);
    z-index: 60;
  }
  .dialog {
    width: min(620px, calc(100vw - 32px));
    max-height: calc(100vh - 32px);
    overflow-y: auto;
    padding: var(--space-6);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-xl);
    box-shadow: var(--shadow);
  }
  h2 {
    margin: 0 0 8px;
    font-size: 16px;
  }
  .hosts {
    margin: 0 0 12px;
    padding: 0;
    list-style: none;
  }
  li {
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--border);
  }
  .row {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }
  .main {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }
  .name {
    font-weight: 600;
  }
  .meta,
  .note,
  .hint {
    color: var(--fg-muted);
    font-size: 12px;
  }
  .note,
  .hint {
    margin: 2px 0 0;
  }
  .path {
    overflow-wrap: anywhere;
  }
  .note {
    padding-left: 60px;
  }
  .note.error {
    color: var(--danger);
  }
  .actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    margin-top: 12px;
  }
</style>
