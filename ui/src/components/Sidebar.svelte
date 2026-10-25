<script lang="ts">
  import { api } from "../lib/api";
  import type { Connection } from "../lib/bindings/Connection";
  import type { Group } from "../lib/bindings/Group";
  import type { TagColor } from "../lib/bindings/TagColor";
  import { colorOf } from "../lib/appearance";
  import { t } from "../lib/i18n";
  import { modal } from "../lib/modal";
  import { getPref, setPref } from "../lib/prefs";
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
  import Avatar from "./ui/Avatar.svelte";
  import ColorPicker from "./ui/ColorPicker.svelte";
  import Icon from "./ui/Icon.svelte";
  import Pill from "./ui/Pill.svelte";
  import TextField from "./ui/TextField.svelte";

  let {
    openConnectionIds,
    onopen,
    onedit,
    onsettings,
    onimport,
  }: {
    openConnectionIds: Set<string>;
    onopen: (connectionId: string) => void;
    /** Called with the connection to edit, or undefined for a new one. */
    onedit: (connection?: Connection) => void;
    onsettings: () => void;
    onimport: () => void;
  } = $props();

  type Menu = {
    conn?: Connection;
    group?: Group;
    x: number;
    y: number;
    mode: "main" | "move";
  };

  let query = $state("");
  // "all", "none" (no group) or a group id.
  let filter = $state("all");
  const GROUPS_KEY = "pylentra.sidebar.groups";
  const COLLAPSED_KEY = "pylentra.sidebar.collapsed";
  // Ids of the groups folded shut ("" is the no-group section).
  const savedShut = getPref<unknown>(GROUPS_KEY, []);
  let shutGroups = $state<string[]>(
    Array.isArray(savedShut)
      ? savedShut.filter((x): x is string => typeof x === "string")
      : [],
  );
  // Wide windows fold to an avatar rail by preference; narrow ones always do and
  // open the full panel over the sessions on demand.
  let railPref = $state(getPref<unknown>(COLLAPSED_KEY, false) === true);
  let expanded = $state(false);
  const narrowQuery =
    typeof matchMedia === "function" ? matchMedia("(max-width: 900px)") : null;
  let narrow = $state(narrowQuery?.matches ?? false);
  $effect(() => {
    if (!narrowQuery) return;
    const update = () => (narrow = narrowQuery.matches);
    narrowQuery.addEventListener("change", update);
    return () => narrowQuery.removeEventListener("change", update);
  });
  const rail = $derived(narrow || railPref);
  const full = $derived(narrow ? expanded : !railPref);
  let current = $state<string | null>(null);
  let menu = $state<Menu | null>(null);
  let confirm = $state<{ conn: Connection; names: string[] } | null>(null);
  let confirmGroup = $state<Group | null>(null);
  // `id` is set when renaming; a new group gets its id on save.
  let groupDialog = $state<{
    id?: string;
    name: string;
    color: TagColor | null;
    error: string;
  } | null>(null);
  let listEl: HTMLElement | undefined = $state();
  let menuEl: HTMLElement | undefined = $state();
  let searchEl = $state<HTMLInputElement>();
  let lastRow: HTMLElement | null = null;

  const byName = (a: { name: string }, b: { name: string }) =>
    a.name.localeCompare(b.name);

  // "No group" comes last and also takes connections whose group no longer exists.
  const view = $derived(
    filter === "all" ||
      filter === "none" ||
      library.groups.some((g) => g.id === filter)
      ? filter
      : "all",
  );
  // A filter shows one flat list, without group headings.
  const flat = $derived(view !== "all");

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
    const shown = flat
      ? all.filter((s) => s.id === (view === "none" ? "" : view))
      : all;
    return shown.filter(
      (s) => s.conns.length || (!query.trim() && s.id !== ""),
    );
  });

  function toggleGroup(id: string) {
    shutGroups = shutGroups.includes(id)
      ? shutGroups.filter((x) => x !== id)
      : [...shutGroups, id];
    setPref(GROUPS_KEY, shutGroups);
  }

  function toggleSidebar() {
    if (narrow) {
      expanded = !expanded;
    } else {
      railPref = !railPref;
      setPref(COLLAPSED_KEY, railPref);
    }
  }

  function open(id: string) {
    expanded = false;
    onopen(id);
  }

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
      open(conn.id);
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
    groupDialog = {
      id: group?.id,
      name: group?.name ?? "",
      // The menu only carries id and name; the color comes from the library.
      color: library.groups.find((g) => g.id === group?.id)?.color ?? null,
      error: "",
    };
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
      const { color } = groupDialog;
      await saveGroup({
        id: groupDialog.id ?? crypto.randomUUID(),
        name,
        ...(color && { color }),
      });
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
    // Escape closes the narrow overlay unless something above it took the key.
    if (
      e.key === "Escape" &&
      narrow &&
      expanded &&
      !e.defaultPrevented &&
      !fromOtherModal(e)
    )
      expanded = false;
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

<div class="shell" class:rail>
  {#if rail}
    <nav class="sidebar slim" aria-label={t("sidebar.title")} inert={full}>
      <button
        type="button"
        class="icon-btn"
        aria-label={t("sidebar.expand")}
        title={t("sidebar.expand")}
        onclick={toggleSidebar}
      >
        <Icon name="chevron" />
      </button>
      <div class="slim-list">
        {#each [...library.connections].sort(byName) as conn (conn.id)}
          <button
            type="button"
            class="slim-item"
            aria-label={conn.name +
              (openConnectionIds.has(conn.id) ? ` (${t("sidebar.open")})` : "")}
            title={conn.name}
            onclick={() => open(conn.id)}
          >
            <Avatar
              name={conn.name}
              icon={conn.icon}
              color={colorOf(conn, library.groups)}
            />
            {#if openConnectionIds.has(conn.id)}
              <span class="live-dot" aria-hidden="true"></span>
            {/if}
          </button>
        {/each}
      </div>
      <button
        type="button"
        class="icon-btn"
        aria-label={t("settings.open")}
        title={t("settings.open")}
        onclick={onsettings}
      >
        <Icon name="settings" />
      </button>
    </nav>
  {/if}

  {#if full}
    <nav class="sidebar" class:overlay={rail} aria-label={t("sidebar.title")}>
      <header>
        <h1>{t("app.title")}</h1>
        <button
          type="button"
          class="icon-btn collapse"
          aria-label={t("sidebar.collapse")}
          title={t("sidebar.collapse")}
          onclick={toggleSidebar}
        >
          <Icon name="chevron" />
        </button>
        <button
          type="button"
          class="icon-btn"
          aria-label={t("settings.open")}
          title={t("settings.open")}
          onclick={onsettings}
        >
          <Icon name="settings" />
        </button>
      </header>

      <div>
        <TextField
          icon="search"
          label={t("sidebar.search")}
          placeholder={t("sidebar.search")}
          bind:value={query}
          bind:inputEl={searchEl}
          onkeydown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              moveFocus(null, 1);
            }
          }}
        />
      </div>

      <div class="filters" role="group" aria-label={t("sidebar.filters")}>
        <Pill pressed={view === "all"} onclick={() => (filter = "all")}>
          {t("sidebar.all")}
        </Pill>
        {#each [...library.groups].sort(byName) as g (g.id)}
          <Pill
            color={g.color ?? null}
            pressed={view === g.id}
            onclick={() => (filter = g.id)}
          >
            {g.name}
          </Pill>
        {/each}
        <Pill pressed={view === "none"} onclick={() => (filter = "none")}>
          {t("sidebar.noGroup")}
        </Pill>
      </div>

      <div class="list" bind:this={listEl}>
        {#each sections as section (section.id)}
          {@const shut =
            !flat && shutGroups.includes(section.id) && !query.trim()}
          <section>
            {#if !flat}
              <button
                type="button"
                class="group"
                id="grp-{section.id || 'none'}"
                aria-expanded={!shut}
                onclick={() => toggleGroup(section.id)}
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
            {/if}
            {#if !shut}
              <ul
                role="listbox"
                aria-label={flat ? section.title : undefined}
                aria-labelledby={flat
                  ? undefined
                  : `grp-${section.id || "none"}`}
              >
                {#each section.conns as conn (conn.id)}
                  <li
                    role="option"
                    aria-selected={current === conn.id}
                    tabindex="0"
                    data-row
                    class:selected={current === conn.id}
                    onfocus={() => (current = conn.id)}
                    ondblclick={() => open(conn.id)}
                    onkeydown={(e) => onrowkeydown(e, conn)}
                    oncontextmenu={(e) => {
                      e.preventDefault();
                      openMenu({ conn }, e.currentTarget, e.clientX, e.clientY);
                    }}
                  >
                    <span aria-hidden="true"
                      ><Avatar
                        name={conn.name}
                        icon={conn.icon}
                        color={colorOf(conn, library.groups)}
                      /></span
                    >
                    <span class="name">{conn.name}</span>
                    <span class="host">{conn.username}@{conn.host}</span>
                    {#if openConnectionIds.has(conn.id)}
                      <span class="live"
                        ><span aria-hidden="true">●</span>
                        {t("sidebar.open")}</span
                      >
                    {/if}
                  </li>
                {/each}
              </ul>
            {/if}
          </section>
        {/each}
        {#if !library.connections.length}
          <div class="empty">
            <p>{t("sidebar.empty")}</p>
            <div class="empty-actions">
              <button type="button" class="primary" onclick={() => onedit()}>
                {t("sidebar.new")}
              </button>
              <button type="button" onclick={onimport}>
                {t("sidebar.import")}
              </button>
            </div>
          </div>
        {:else if !sections.length || (flat && !sections[0]?.conns.length)}
          <p class="empty">{t("sidebar.noMatches")}</p>
        {/if}
      </div>

      <div class="foot">
        <button type="button" class="primary new" onclick={() => onedit()}>
          <Icon name="plus" />
          {t("sidebar.new")}
        </button>
        <button
          type="button"
          class="more"
          aria-label={t("sidebar.more")}
          title={t("sidebar.more")}
          aria-haspopup="menu"
          aria-expanded={!!menu && !menu.conn && !menu.group}
          onclick={(e) => {
            const el = e.currentTarget;
            const r = el.getBoundingClientRect();
            openMenu({}, el, r.left, Math.max(8, r.top - 88));
          }}
        >
          <Icon name="more" />
        </button>
      </div>
    </nav>
  {/if}
</div>

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
            // `c` derives from `menu`: read it before closing the menu nulls it.
            onedit(c);
            closeMenu(false);
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
    {:else}
      <button type="button" role="menuitem" onclick={() => startGroupDialog()}>
        {t("sidebar.newGroup")}
      </button>
      <button
        type="button"
        role="menuitem"
        onclick={() => {
          closeMenu(false);
          onimport();
        }}
      >
        {t("sidebar.import")}
      </button>
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
        <div class="field">
          <span>{t("group.color")}</span>
          <ColorPicker
            bind:value={groupDialog.color}
            noneLabel={t("common.none")}
            label={t("group.color")}
          />
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
  .shell {
    position: relative;
    flex: none;
    width: 272px;
    height: 100%;
  }
  .shell.rail {
    width: 64px;
  }
  .sidebar {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    height: 100%;
    padding: var(--space-4);
    background: var(--surface-1);
    border-radius: var(--radius-lg);
  }
  .sidebar.overlay {
    position: absolute;
    inset: 0 auto 0 0;
    width: 272px;
    z-index: 30;
    box-shadow: var(--shadow);
  }
  .slim {
    align-items: center;
    padding: var(--space-3) var(--space-2);
  }
  .slim-list {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    overflow-y: auto;
    min-height: 0;
  }
  .slim-item {
    position: relative;
    display: flex;
    padding: var(--space-1);
    background: none;
    border-color: transparent;
  }
  .live-dot {
    position: absolute;
    top: 2px;
    right: 2px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--ok);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  h1 {
    flex: 1;
    margin: 0;
    font-size: 18px;
    font-weight: 700;
  }
  .icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    padding: 0;
    border-radius: var(--radius-md);
    border: 0;
    background: var(--surface-2);
    color: var(--fg-muted);
  }
  .icon-btn:hover {
    background: var(--surface-3);
    color: var(--fg);
  }
  /* The chevron points right (expand); turn it to collapse. */
  .collapse :global(svg) {
    transform: rotate(180deg);
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    max-height: 96px;
    overflow-y: auto;
  }
  .filters :global(button.pill[aria-pressed="true"]) {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-fg);
  }
  .foot {
    display: flex;
    gap: var(--space-2);
  }
  .foot button {
    height: 44px;
    border-radius: var(--radius-md);
  }
  .new {
    flex: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    font-weight: 600;
    white-space: nowrap;
  }
  .more {
    width: 44px;
    padding: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
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
  .list {
    flex: 1;
    overflow-y: auto;
    min-height: 0;
  }
  .group {
    display: flex;
    gap: 6px;
    width: 100%;
    border: 0;
    background: none;
    text-align: left;
    font-size: 12px;
    font-weight: 600;
    color: var(--fg-muted);
    padding: var(--space-1);
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
    grid-template-columns: auto 1fr auto;
    column-gap: var(--space-3);
    align-items: center;
    padding: var(--space-2);
    border-radius: var(--radius-md);
    cursor: default;
  }
  li :global(.avatar) {
    grid-row: 1 / span 2;
  }
  li:hover {
    background: var(--surface-2);
  }
  /* Same surface as hover, so the accent bar is what marks the selection. */
  li.selected {
    background: var(--surface-2);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .name {
    grid-column: 2;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .host {
    grid-column: 2;
    color: var(--fg-muted);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .live {
    grid-column: 3;
    grid-row: 1 / span 2;
    color: var(--ok);
    font-size: 12px;
  }
  .empty {
    color: var(--fg-muted);
    padding: var(--space-2);
  }
  .empty-actions {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }
  .menu {
    position: fixed;
    z-index: 50;
    display: flex;
    flex-direction: column;
    min-width: 180px;
    padding: 4px;
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow);
  }
  .menu button {
    border: 0;
    background: none;
    text-align: left;
  }
  .menu button:hover,
  .menu button:focus-visible {
    background: var(--surface-3);
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
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 20px;
  }
</style>
