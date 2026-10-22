<script lang="ts">
  import { api } from "../lib/api";
  import type { Connection } from "../lib/bindings/Connection";
  import type { Group } from "../lib/bindings/Group";
  import { t } from "../lib/i18n";
  import { modal } from "../lib/modal";
  import { matches } from "../lib/search";
  import { fromOtherModal, shortcutFor } from "../lib/shortcuts";
  import {
    deleteConnection,
    deleteGroup,
    library,
    saveConnection,
    saveGroup,
  } from "../lib/stores/library.svelte";
  import { toastError } from "../lib/stores/toasts.svelte";

  let {
    openConnectionIds,
    onopen,
    onedit,
    onsettings,
  }: {
    openConnectionIds: Set<string>;
    onopen: (connectionId: string) => void;
    /** Called with the connection to edit, or undefined for a new one. */
    onedit: (connection?: Connection) => void;
    onsettings: () => void;
  } = $props();

  type Menu = {
    conn?: Connection;
    group?: Group;
    x: number;
    y: number;
    mode: "main" | "move";
  };

  let query = $state("");
  let collapsed = $state<Record<string, boolean>>({});
  let current = $state<string | null>(null);
  let menu = $state<Menu | null>(null);
  let confirm = $state<{ conn: Connection; names: string[] } | null>(null);
  let confirmGroup = $state<Group | null>(null);
  // `id` is set when renaming; a new group gets its id on save.
  let groupDialog = $state<{ id?: string; name: string; error: string } | null>(
    null,
  );
  let listEl: HTMLElement | undefined = $state();
  let menuEl: HTMLElement | undefined = $state();
  let searchEl: HTMLInputElement | undefined = $state();
  let lastRow: HTMLElement | null = null;

  const byName = (a: { name: string }, b: { name: string }) =>
    a.name.localeCompare(b.name);

  // "No group" comes last and also takes connections whose group no longer exists.
  const sections = $derived.by(() => {
    const known = new Set(library.groups.map((g) => g.id));
    const visible = library.connections.filter((c) => matches(c, query));
    const of = (pred: (c: Connection) => boolean) =>
      visible.filter(pred).sort(byName);
    const all = [
      ...[...library.groups].sort(byName).map((g) => ({
        id: g.id,
        title: g.name,
        conns: of((c) => c.group_id === g.id),
      })),
      {
        id: "",
        title: t("sidebar.noGroup"),
        conns: of((c) => !c.group_id || !known.has(c.group_id)),
      },
    ];
    return all.filter((s) => s.conns.length || (!query.trim() && s.id !== ""));
  });

  const rows = () => [
    ...(listEl?.querySelectorAll<HTMLElement>("[data-row]") ?? []),
  ];

  function moveFocus(from: HTMLElement | null, delta: number) {
    const all = rows();
    const i = from ? all.indexOf(from) : -1;
    all[Math.min(Math.max(i + delta, 0), all.length - 1)]?.focus();
  }

  function onrowkeydown(e: KeyboardEvent, conn: Connection) {
    const row = e.currentTarget as HTMLElement;
    if (e.key === "Enter") {
      e.preventDefault();
      onopen(conn.id);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      moveFocus(row, e.key === "ArrowDown" ? 1 : -1);
    } else if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
      e.preventDefault();
      const r = row.getBoundingClientRect();
      openMenu({ conn }, row, r.left + 24, r.bottom);
    }
  }

  function openMenu(
    target: Pick<Menu, "conn" | "group">,
    row: HTMLElement,
    x: number,
    y: number,
  ) {
    lastRow = row;
    menu = { ...target, x, y, mode: "main" };
  }

  function closeMenu(refocus = true) {
    menu = null;
    if (refocus && lastRow?.isConnected) lastRow.focus();
  }

  // Focus the first item whenever the menu opens or switches to the group list;
  // the mode switch replaces the focused button, so `mode` must be read here.
  $effect(() => {
    if (menu && menu.mode)
      menuEl?.querySelector<HTMLElement>("button")?.focus();
  });

  function onmenukeydown(e: KeyboardEvent) {
    const items = [...(menuEl?.querySelectorAll<HTMLElement>("button") ?? [])];
    const i = items.indexOf(document.activeElement as HTMLElement);
    if (e.key === "Escape") {
      e.preventDefault();
      closeMenu();
    } else if (e.key === "Tab") {
      closeMenu(false);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next =
        (i + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
      items[next]?.focus();
    }
  }

  async function run(op: Promise<unknown>) {
    try {
      await op;
    } catch (e) {
      toastError(e);
    }
  }

  function duplicate(conn: Connection) {
    closeMenu();
    const copy: Connection = {
      ...$state.snapshot(conn),
      id: crypto.randomUUID(),
      name: conn.name + t("sidebar.copySuffix"),
    };
    return run(saveConnection(copy));
  }

  function moveTo(conn: Connection, groupId: string | null) {
    closeMenu();
    return run(saveConnection({ ...$state.snapshot(conn), group_id: groupId }));
  }

  async function askDelete(conn: Connection) {
    closeMenu(false);
    let ids: string[] = [];
    try {
      ids = await api.connectionDependents(conn.id);
    } catch (e) {
      return toastError(e);
    }
    const name = (id: string) =>
      library.connections.find((c) => c.id === id)?.name ?? id;
    confirm = { conn, names: ids.map(name) };
  }

  function cancelDelete() {
    confirm = null;
    lastRow?.focus();
  }

  async function doDelete() {
    if (!confirm) return;
    const { id } = confirm.conn;
    confirm = null;
    await run(deleteConnection(id));
  }

  function onheaderkeydown(e: KeyboardEvent, group: Group) {
    if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
      e.preventDefault();
      const el = e.currentTarget as HTMLElement;
      const r = el.getBoundingClientRect();
      openMenu({ group }, el, r.left + 24, r.bottom);
    }
  }

  function startGroupDialog(group?: Group) {
    closeMenu(false);
    if (!group) lastRow = null; // closing returns focus to the New group button
    groupDialog = { id: group?.id, name: group?.name ?? "", error: "" };
  }

  function closeGroupDialog() {
    groupDialog = null;
    if (lastRow?.isConnected) lastRow.focus();
  }

  async function submitGroup(e: SubmitEvent) {
    e.preventDefault();
    if (!groupDialog) return;
    const name = groupDialog.name.trim();
    if (!name) {
      groupDialog.error = t("form.error.required");
      return;
    }
    try {
      await saveGroup({ id: groupDialog.id ?? crypto.randomUUID(), name });
      closeGroupDialog();
    } catch (err) {
      toastError(err);
    }
  }

  function askDeleteGroup(group: Group) {
    closeMenu(false);
    confirmGroup = group;
  }

  function cancelDeleteGroup() {
    confirmGroup = null;
    if (lastRow?.isConnected) lastRow.focus();
  }

  async function doDeleteGroup() {
    if (!confirmGroup) return;
    const { id } = confirmGroup;
    confirmGroup = null;
    await run(deleteGroup(id));
  }
</script>

<svelte:window
  onkeydown={(e) => {
    // Ctrl+Shift+F, also from the terminal (its key handler lets it bubble here),
    // but never from inside a dialog.
    if (shortcutFor(e) === "search" && !fromOtherModal(e)) {
      e.preventDefault();
      searchEl?.focus();
      searchEl?.select();
    }
  }}
  onpointerdown={(e) => {
    if (menu && !menuEl?.contains(e.target as Node)) closeMenu(false);
  }}
/>

<nav class="sidebar" aria-label={t("sidebar.title")}>
  <div class="toolbar">
    <input
      type="search"
      bind:this={searchEl}
      aria-label={t("sidebar.search")}
      placeholder={t("sidebar.search")}
      bind:value={query}
      onkeydown={(e) => {
        if (e.key === "ArrowDown") {
          e.preventDefault();
          moveFocus(null, 1);
        }
      }}
    />
    <div class="buttons">
      <button type="button" class="primary" onclick={() => onedit()}>
        {t("sidebar.new")}
      </button>
      <button type="button" onclick={() => startGroupDialog()}>
        {t("sidebar.newGroup")}
      </button>
    </div>
  </div>

  <div class="list" bind:this={listEl}>
    {#each sections as section (section.id)}
      {@const shut = collapsed[section.id] && !query.trim()}
      <section>
        <button
          type="button"
          class="group"
          id="grp-{section.id || 'none'}"
          aria-expanded={!shut}
          onclick={() => (collapsed[section.id] = !collapsed[section.id])}
          oncontextmenu={(e) => {
            if (!section.id) return;
            e.preventDefault();
            openMenu(
              { group: { id: section.id, name: section.title } },
              e.currentTarget,
              e.clientX,
              e.clientY,
            );
          }}
          onkeydown={(e) =>
            section.id &&
            onheaderkeydown(e, { id: section.id, name: section.title })}
        >
          <span aria-hidden="true">{shut ? "▸" : "▾"}</span>
          {section.title}
          <span class="count">{section.conns.length}</span>
        </button>
        {#if !shut}
          <ul role="listbox" aria-labelledby="grp-{section.id || 'none'}">
            {#each section.conns as conn (conn.id)}
              <li
                role="option"
                aria-selected={current === conn.id}
                tabindex="0"
                data-row
                class:selected={current === conn.id}
                onfocus={() => (current = conn.id)}
                ondblclick={() => onopen(conn.id)}
                onkeydown={(e) => onrowkeydown(e, conn)}
                oncontextmenu={(e) => {
                  e.preventDefault();
                  openMenu({ conn }, e.currentTarget, e.clientX, e.clientY);
                }}
              >
                <span class="name">{conn.name}</span>
                <span class="host">{conn.username}@{conn.host}</span>
                {#if openConnectionIds.has(conn.id)}
                  <span class="dot" role="img" aria-label={t("sidebar.open")}
                    >●</span
                  >
                {/if}
              </li>
            {/each}
          </ul>
        {/if}
      </section>
    {/each}
    {#if !library.connections.length}
      <p class="empty">{t("sidebar.empty")}</p>
    {:else if !sections.length}
      <p class="empty">{t("sidebar.noMatches")}</p>
    {/if}
  </div>

  <div class="foot">
    <button type="button" onclick={onsettings}>{t("settings.open")}</button>
  </div>
</nav>

{#if menu}
  {@const m = menu}
  <div
    class="menu"
    role="menu"
    tabindex="-1"
    bind:this={menuEl}
    style="left: {Math.min(m.x, window.innerWidth - 220)}px; top: {Math.min(
      m.y,
      window.innerHeight - 180,
    )}px"
    onkeydown={onmenukeydown}
  >
    {#if m.group}
      {@const g = m.group}
      <button type="button" role="menuitem" onclick={() => startGroupDialog(g)}>
        {t("menu.rename")}
      </button>
      <button
        type="button"
        role="menuitem"
        class="danger"
        onclick={() => askDeleteGroup(g)}
      >
        {t("menu.delete")}
      </button>
    {:else if m.conn}
      {@const c = m.conn}
      {#if m.mode === "main"}
        <button
          type="button"
          role="menuitem"
          onclick={() => {
            closeMenu(false);
            onedit(c);
          }}
        >
          {t("menu.edit")}
        </button>
        <button type="button" role="menuitem" onclick={() => duplicate(c)}>
          {t("menu.duplicate")}
        </button>
        <button type="button" role="menuitem" onclick={() => (m.mode = "move")}>
          {t("menu.move")}
        </button>
        <button
          type="button"
          role="menuitem"
          class="danger"
          onclick={() => askDelete(c)}
        >
          {t("menu.delete")}
        </button>
      {:else}
        {#each [...library.groups].sort(byName) as g (g.id)}
          <button type="button" role="menuitem" onclick={() => moveTo(c, g.id)}>
            {g.name}
          </button>
        {/each}
        <button type="button" role="menuitem" onclick={() => moveTo(c, null)}>
          {t("sidebar.noGroup")}
        </button>
      {/if}
    {/if}
  </div>
{/if}

{#if confirm}
  <div class="backdrop">
    <div
      class="dialog"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="del-title"
      aria-describedby="del-body"
      use:modal={cancelDelete}
    >
      <h2 id="del-title">{t("delete.title")}</h2>
      <div id="del-body">
        <p>{t("delete.confirm", { name: confirm.conn.name })}</p>
        {#if confirm.names.length}
          <p>{t("delete.dependents", { names: confirm.names.join(", ") })}</p>
        {/if}
      </div>
      <div class="actions">
        <button type="button" data-autofocus onclick={cancelDelete}
          >{t("common.cancel")}</button
        >
        <button type="button" class="danger" onclick={doDelete}
          >{t("common.delete")}</button
        >
      </div>
    </div>
  </div>
{/if}

{#if groupDialog}
  <div class="backdrop">
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="grp-title"
      use:modal={closeGroupDialog}
    >
      <h2 id="grp-title">
        {groupDialog.id ? t("group.titleRename") : t("group.titleNew")}
      </h2>
      <form onsubmit={submitGroup} novalidate>
        <div class="field">
          <label for="grp-name">{t("group.name")}</label>
          <input
            id="grp-name"
            data-autofocus
            bind:value={groupDialog.name}
            aria-invalid={groupDialog.error ? "true" : undefined}
            aria-describedby={groupDialog.error ? "grp-name-err" : undefined}
          />
          {#if groupDialog.error}
            <p class="error" id="grp-name-err">{groupDialog.error}</p>
          {/if}
        </div>
        <div class="actions">
          <button type="button" onclick={closeGroupDialog}
            >{t("common.cancel")}</button
          >
          <button type="submit" class="primary">{t("common.save")}</button>
        </div>
      </form>
    </div>
  </div>
{/if}

{#if confirmGroup}
  <div class="backdrop">
    <div
      class="dialog"
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="delgrp-title"
      aria-describedby="delgrp-body"
      use:modal={cancelDeleteGroup}
    >
      <h2 id="delgrp-title">{t("group.deleteTitle")}</h2>
      <p id="delgrp-body">
        {t("group.deleteConfirm", {
          name: confirmGroup.name,
          noGroup: t("sidebar.noGroup"),
        })}
      </p>
      <div class="actions">
        <button type="button" data-autofocus onclick={cancelDeleteGroup}
          >{t("common.cancel")}</button
        >
        <button type="button" class="danger" onclick={doDeleteGroup}
          >{t("common.delete")}</button
        >
      </div>
    </div>
  </div>
{/if}

<style>
  .sidebar {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg-alt);
    border-right: 1px solid var(--border);
  }
  .toolbar {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px;
  }
  .buttons {
    display: flex;
    gap: 6px;
  }
  .buttons button {
    flex: 1;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 12px;
  }
  .error {
    margin: 0;
    color: var(--danger);
    font-size: 12px;
  }
  .foot {
    padding: 8px;
    border-top: 1px solid var(--border);
  }
  .list {
    flex: 1;
    overflow-y: auto;
    padding: 0 4px 8px;
  }
  .group {
    display: flex;
    gap: 6px;
    width: 100%;
    border: 0;
    background: none;
    text-align: left;
    font-weight: 600;
    padding: 4px;
  }
  .count {
    margin-left: auto;
    color: var(--fg-muted);
    font-weight: normal;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    display: grid;
    grid-template-columns: 1fr auto;
    column-gap: 8px;
    padding: 4px 8px 4px 20px;
    border-radius: 4px;
    cursor: default;
  }
  li:hover {
    background: var(--bg-hover);
  }
  li.selected {
    background: var(--bg-selected);
  }
  .host {
    grid-column: 1;
    color: var(--fg-muted);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dot {
    grid-column: 2;
    grid-row: 1 / span 2;
    align-self: center;
    color: var(--ok);
  }
  .empty {
    color: var(--fg-muted);
    padding: 8px;
  }
  .menu {
    position: fixed;
    z-index: 50;
    display: flex;
    flex-direction: column;
    min-width: 180px;
    padding: 4px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: var(--shadow);
  }
  .menu button {
    border: 0;
    background: none;
    text-align: left;
  }
  .menu button:hover,
  .menu button:focus-visible {
    background: var(--bg-hover);
  }
  .backdrop {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: var(--backdrop);
    z-index: 60;
  }
  .dialog {
    max-width: min(440px, calc(100vw - 32px));
    padding: 16px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
  }
  h2 {
    margin: 0 0 8px;
    font-size: 16px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
