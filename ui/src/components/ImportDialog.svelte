<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../lib/api";
  import type { Candidate } from "../lib/bindings/Candidate";
  import type { ImportNote } from "../lib/bindings/ImportNote";
  import type { ScanResult } from "../lib/bindings/ScanResult";
  import { t, type MessageKey } from "../lib/i18n";
  import {
    prepare,
    requiredBy,
    selectable,
    toggle,
  } from "../lib/importSelection";
  import { modal } from "../lib/modal";
  import {
    isReadOnly,
    library,
    refresh,
    saveGroup,
  } from "../lib/stores/library.svelte";
  import { quickDefaults } from "../lib/stores/settings.svelte";
  import { toast, toastError } from "../lib/stores/toasts.svelte";
  import Avatar from "./ui/Avatar.svelte";
  import Pill from "./ui/Pill.svelte";

  let { onclose }: { onclose: () => void } = $props();

  const NEW_GROUP = "__new__";

  let scan = $state<ScanResult | null>(null);
  let selected = $state(new Set<string>());
  let user = $state(quickDefaults().user ?? "");
  let groupId = $state("");
  let busy = $state(false);

  const candidates = $derived(scan?.candidates ?? []);
  const needsUser = $derived(
    candidates.some((c) => selected.has(c.connection.id) && c.needs_user),
  );
  const userMissing = $derived(needsUser && !user.trim());
  const count = $derived(selected.size);
  const groups = $derived(
    [...library.groups].sort((a, b) => a.name.localeCompare(b.name)),
  );
  const readOnly = $derived(isReadOnly());

  const noteText = (n: ImportNote) => {
    const { type, ...params } = n;
    return t(`import.note.${type}` as MessageKey, params);
  };
  // A selected host whose candidate jump host is not selected loses its jump.
  const jumpDropped = (c: Candidate) => {
    const via = c.connection.jump_via;
    return (
      !!via &&
      selected.has(c.connection.id) &&
      !selected.has(via) &&
      candidates.some((o) => o.connection.id === via)
    );
  };
  const target = (c: Candidate) => {
    const { username, host, port } = c.connection;
    return `${username ? `${username}@` : ""}${host}:${port}`;
  };

  onMount(async () => {
    try {
      scan = await api.sshConfigScan();
      selected = new Set(
        scan.candidates.filter(selectable).map((c) => c.connection.id),
      );
    } catch (e) {
      toastError(e);
    }
  });

  async function submit() {
    busy = true;
    try {
      if (groupId === NEW_GROUP) {
        const id = crypto.randomUUID();
        await saveGroup({ id, name: "ssh config" });
        // A retry after a failed import reuses this group.
        groupId = id;
      }
      const n = await api.connectionsImport(
        prepare(candidates, selected, user.trim()),
        groupId || null,
      );
      await refresh();
      toast(n === 1 ? t("import.doneOne") : t("import.done", { n }), "success");
      onclose();
    } catch (e) {
      toastError(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="backdrop">
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="import-title"
    use:modal={onclose}
  >
    <h2 id="import-title">{t("import.title")}</h2>

    {#if !scan}
      <p class="hint">{t("import.loading")}</p>
    {:else}
      {#each scan.skipped as s, i (i)}
        <p class="hint">
          {t("import.skipped", { line: s.line, note: noteText(s.note) })}
        </p>
      {/each}
      {#if scan.missing}
        <p class="hint">{t("import.missing", { path: scan.path })}</p>
      {:else if !candidates.length}
        <p class="hint">{t("import.none")}</p>
      {:else}
        <ul class="hosts" aria-label={t("import.hosts")}>
          {#each candidates as c (c.connection.id)}
            {@const id = c.connection.id}
            {@const needed = requiredBy(candidates, selected, id)}
            {@const dropped = jumpDropped(c)}
            {@const noteIds = [
              ...(c.error ? [`n-${id}-err`] : []),
              ...c.notes.map((_, i) => `n-${id}-${i}`),
              ...(selected.has(id) && needed.length ? [`n-${id}-req`] : []),
              ...(dropped ? [`n-${id}-drop`] : []),
            ]}
            <li>
              <label class="row">
                <input
                  type="checkbox"
                  checked={selected.has(id)}
                  disabled={!selectable(c)}
                  aria-describedby={noteIds.join(" ") || undefined}
                  onchange={() => (selected = toggle(selected, candidates, id))}
                />
                <span aria-hidden="true"
                  ><Avatar
                    name={c.connection.name}
                    color={null}
                    size={28}
                  /></span
                >
                <span class="main">
                  <span class="name">{c.connection.name}</span>
                  <span class="meta">
                    {target(c)} · {t(`form.auth.${c.connection.auth.type}`)}
                  </span>
                </span>
                {#if c.exists}<Pill>{t("import.exists")}</Pill>{/if}
              </label>
              {#if c.error}
                <p class="note error" id="n-{id}-err">{noteText(c.error)}</p>
              {/if}
              {#each c.notes as n, i (i)}
                <p class="note" id="n-{id}-{i}">{noteText(n)}</p>
              {/each}
              {#if selected.has(id) && needed.length}
                <p class="note" id="n-{id}-req">
                  {t("import.requiredBy", { names: needed.join(", ") })}
                </p>
              {/if}
              {#if dropped}
                <p class="note" id="n-{id}-drop">{t("import.jumpDropped")}</p>
              {/if}
            </li>
          {/each}
        </ul>

        {#if needsUser}
          <div class="field">
            <label for="import-user">{t("import.user")}</label>
            <input
              id="import-user"
              required
              aria-required="true"
              aria-invalid={userMissing ? "true" : undefined}
              aria-describedby={userMissing ? "import-user-err" : undefined}
              bind:value={user}
            />
            {#if userMissing}
              <p class="note error" id="import-user-err">
                {t("import.userRequired")}
              </p>
            {/if}
          </div>
        {/if}
        <div class="field">
          <label for="import-group">{t("import.group")}</label>
          <select id="import-group" bind:value={groupId}>
            <option value="">{t("sidebar.noGroup")}</option>
            {#each groups as g (g.id)}
              <option value={g.id}>{g.name}</option>
            {/each}
            <option value={NEW_GROUP}>{t("import.group.new")}</option>
          </select>
        </div>
      {/if}
    {/if}

    {#if readOnly}
      <p class="note error" role="status">
        {t("error.config.read_only")}
      </p>
    {/if}
    <div class="actions">
      <button type="button" onclick={onclose}>{t("common.cancel")}</button>
      <button
        type="button"
        class="primary"
        disabled={busy || count === 0 || readOnly || userMissing}
        onclick={submit}
        >{count === 1
          ? t("import.submitOne")
          : t("import.submit", { n: count })}</button
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
  .note {
    padding-left: 60px;
  }
  .field .note {
    padding-left: 0;
  }
  .note.error {
    color: var(--danger);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 8px;
  }
  .actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    margin-top: 12px;
  }
</style>
