<script lang="ts">
  import type { Snippet } from "../../lib/bindings/Snippet";
  import { colorOf } from "../../lib/appearance";
  import { t } from "../../lib/i18n";
  import { library, isSnippetsReadOnly } from "../../lib/stores/library.svelte";
  import { deleteSnippet, snippets } from "../../lib/stores/snippets.svelte";
  import { toastError } from "../../lib/stores/toasts.svelte";
  import { fold } from "../../lib/search";
  import { firstLine } from "../../lib/snippets";
  import SnippetForm from "../SnippetForm.svelte";
  import Avatar from "../ui/Avatar.svelte";
  import ConfirmDialog from "../ui/ConfirmDialog.svelte";
  import Pill from "../ui/Pill.svelte";
  import TextField from "../ui/TextField.svelte";

  const uid = $props.id();
  let query = $state("");
  // `undefined` = closed, `null` = new snippet.
  let editing = $state<Snippet | null | undefined>();
  let deleting = $state<Snippet | null>(null);

  const readOnly = $derived(isSnippetsReadOnly());
  const byName = (a: { name: string }, b: { name: string }) =>
    a.name.localeCompare(b.name);

  // "Global" first, then one group per connection that has snippets. A
  // snippet whose connection is gone stays reachable under "Global".
  const groups = $derived.by(() => {
    const q = fold(query.trim());
    const found = snippets.list
      .filter((s) => fold(s.name).includes(q) || fold(s.text).includes(q))
      .sort(byName);
    const known = new Set(library.connections.map((c) => c.id));
    const own = (id: string) => found.filter((s) => s.connection_id === id);
    return [
      {
        id: "",
        title: t("snippet.global"),
        conn: undefined,
        items: found.filter(
          (s) => !s.connection_id || !known.has(s.connection_id),
        ),
      },
      ...[...library.connections].sort(byName).map((c) => ({
        id: c.id,
        title: c.name,
        conn: c,
        items: own(c.id),
      })),
    ].filter((g) => g.items.length);
  });

  async function confirmDelete() {
    const s = deleting;
    deleting = null;
    if (!s) return;
    try {
      await deleteSnippet(s.id);
    } catch (e) {
      toastError(e);
    }
  }
</script>

<div class="bar">
  <div class="search">
    <TextField
      icon="search"
      label={t("snippet.search")}
      placeholder={t("snippet.search")}
      bind:value={query}
    />
  </div>
  <button
    type="button"
    class="primary"
    disabled={readOnly}
    onclick={() => (editing = null)}>{t("snippet.new")}</button
  >
</div>
{#if readOnly}<p class="hint">{t("snippet.readOnly")}</p>{/if}

{#each groups as g (g.id)}
  <section aria-labelledby="{uid}-{g.id || 'global'}">
    <h4 id="{uid}-{g.id || 'global'}">
      {#if g.conn}
        <span aria-hidden="true"
          ><Avatar
            name={g.conn.name}
            icon={g.conn.icon}
            color={colorOf(g.conn, library.groups)}
            size={28}
          /></span
        >
      {/if}
      {g.title}
    </h4>
    <ul>
      {#each g.items as s (s.id)}
        <li>
          <span class="name">{s.name}</span>
          <span class="preview">{firstLine(s.text)}</span>
          <span class="tags">
            {#if s.run}<Pill>{t("snippet.runs")}</Pill>{/if}
            <button
              type="button"
              disabled={readOnly}
              onclick={() => (editing = s)}>{t("menu.edit")}</button
            >
            <button
              type="button"
              class="danger"
              disabled={readOnly}
              onclick={() => (deleting = s)}>{t("common.delete")}</button
            >
          </span>
        </li>
      {/each}
    </ul>
  </section>
{:else}
  <p class="hint">
    {snippets.list.length ? t("snippet.noMatches") : t("snippet.empty")}
  </p>
{/each}

{#if editing !== undefined}
  <SnippetForm
    initial={editing ?? undefined}
    onclose={() => (editing = undefined)}
  />
{/if}
{#if deleting}
  <ConfirmDialog
    title={t("snippet.deleteTitle")}
    message={t("snippet.deleteBody", { name: deleting.name })}
    confirmLabel={t("common.delete")}
    danger
    onconfirm={confirmDelete}
    oncancel={() => (deleting = null)}
  />
{/if}

<style>
  .bar {
    display: flex;
    gap: var(--space-3);
  }
  .search {
    flex: 1;
  }
  .hint {
    margin: 0;
    color: var(--fg-muted);
    font-size: 12px;
  }
  section h4 {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin: 0 0 var(--space-1);
    font-size: 14px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    display: grid;
    grid-template-columns: 1fr auto;
    column-gap: var(--space-3);
    align-items: center;
    padding: var(--space-2);
    border-radius: var(--radius-md);
  }
  li:hover {
    background: var(--surface-2);
  }
  .name {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .preview {
    grid-column: 1;
    color: var(--fg-muted);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tags {
    grid-column: 2;
    grid-row: 1 / span 2;
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
</style>
