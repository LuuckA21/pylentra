<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { api } from "../lib/api";
  import type { Entry } from "../lib/bindings/Entry";
  import type { Listing } from "../lib/bindings/Listing";
  import { t } from "../lib/i18n";
  import { current } from "../lib/i18n/lang.svelte";
  import { byName } from "../lib/search";
  import type { PaneState } from "../lib/sessionReducer";
  import { saveSettings, settings } from "../lib/stores/settings.svelte";
  import { isApiError, toast, toastError } from "../lib/stores/toasts.svelte";
  import Toggle from "./settings/Toggle.svelte";
  import TransferList from "./TransferList.svelte";

  // Shown by TerminalPane only while the session is ready.
  let { pane }: { pane: PaneState } = $props();

  let listing = $state<Listing>();
  // A Set: a listing can hold 100 000 entries, all selected at once.
  let selected = $state.raw(new Set<string>());
  // The focused row, and where a Shift range starts.
  let cursor = $state(0);
  let anchor = 0;
  let list = $state<HTMLElement>();
  let root: HTMLElement;
  // Transfer id → the folder this panel uploads into, until the transfer ends.
  let uploads = $state<Record<number, string>>({});
  // Only the latest listing request counts: a slow one must not land late.
  let request = 0;

  const dir = $derived(listing?.dir ?? "");
  const rows = $derived(
    (listing?.entries ?? [])
      .filter((e) => settings.sftp_show_hidden || !e.name.startsWith("."))
      .sort(
        (a, b) =>
          Number(b.kind === "Dir") - Number(a.kind === "Dir") || byName(a, b),
      ),
  );
  // Hidden rows stay selected but out of reach while the switch is off.
  const chosen = $derived(rows.filter((e) => selected.has(e.name)));
  // The row reached with Tab, kept in range when the list shrinks.
  const tabRow = $derived(Math.min(cursor, rows.length - 1));

  // Remote paths always use "/", whatever the server's system.
  const join = (d: string, name: string) =>
    d.endsWith("/") ? d + name : `${d}/${name}`;
  const parent = (d: string) => d.slice(0, d.lastIndexOf("/")) || "/";

  async function load(path: string) {
    const sessionId = pane.sessionId;
    if (!sessionId) return;
    const mine = ++request;
    const hadFocus = !!list?.contains(document.activeElement);
    try {
      const l = await api.sftpList(sessionId, path);
      if (mine !== request) return;
      listing = l;
      selected = new Set();
      cursor = anchor = 0;
      if (hadFocus) {
        await tick();
        rowAt(0)?.focus();
      }
    } catch (e) {
      toastError(e);
    }
  }

  // App finds the panel under a drop and hands it the drop's id.
  onMount(() => {
    void load("");
    const ondrop = (e: Event) => {
      const { dropId } = (e as CustomEvent<{ dropId: number }>).detail;
      if (pane.sessionId && dir)
        started(api.sftpUploadDrop(pane.sessionId, dropId, dir), dir, true);
    };
    root.addEventListener("sftp-drop", ondrop);
    return () => root.removeEventListener("sftp-drop", ondrop);
  });

  // An upload into the shown folder that copied something refreshes it.
  $effect(() => {
    for (const [id, target] of Object.entries(uploads)) {
      const s = pane.transfers[Number(id)]?.state;
      if (
        s?.type !== "Finished" &&
        s?.type !== "Failed" &&
        s?.type !== "Cancelled"
      )
        continue;
      delete uploads[Number(id)];
      if ((s.type === "Finished" || s.files > 0) && target === dir)
        void untrack(() => load(dir));
    }
  });

  const rowAt = (i: number) =>
    list?.querySelectorAll<HTMLElement>("[role=option]")[i];

  function range(to: number) {
    const [lo, hi] = anchor < to ? [anchor, to] : [to, anchor];
    selected = new Set(rows.slice(lo, hi + 1).map((e) => e.name));
  }

  function toggle(i: number) {
    const name = rows[i].name;
    selected = selected.has(name)
      ? new Set([...selected].filter((n) => n !== name))
      : new Set([...selected, name]);
    anchor = i;
  }

  function click(e: MouseEvent, i: number) {
    cursor = i;
    if (e.shiftKey) return range(i);
    if (e.ctrlKey || e.metaKey) return toggle(i);
    selected = new Set([rows[i].name]);
    anchor = i;
  }

  function open(e: Entry) {
    // A link may point to a folder; the server tells if it does not.
    if (e.kind === "Dir" || e.kind === "Link") void load(join(dir, e.name));
  }

  function up() {
    if (dir && dir !== "/") void load(parent(dir));
  }

  function onkey(e: KeyboardEvent, i: number) {
    const to = (
      {
        ArrowDown: i + 1,
        ArrowUp: i - 1,
        Home: 0,
        End: rows.length - 1,
      } as Record<string, number>
    )[e.key];
    if (to !== undefined) {
      e.preventDefault();
      cursor = Math.max(0, Math.min(rows.length - 1, to));
      rowAt(cursor)?.focus();
      if (e.shiftKey) range(cursor);
      return;
    }
    if (e.key === " ") toggle(i);
    else if (e.key === "Enter") open(rows[i]);
    else if (e.key === "Backspace") up();
    else return;
    e.preventDefault();
  }

  const failed = (e: unknown) => void toastError(e);

  function download() {
    const items = chosen.map((e) => join(dir, e.name));
    if (pane.sessionId && items.length)
      api.sftpDownload(pane.sessionId, items).catch(failed);
  }

  function started(call: Promise<number | null>, target: string, drop = false) {
    call.then(
      (id) => {
        if (id !== null) uploads[id] = target;
      },
      // A drop is used once, even by a failed upload: never try it again.
      (e) =>
        drop && isApiError(e) && e.params.what === "drop"
          ? toast(t("sftp.dropExpired"))
          : toastError(e),
    );
  }

  function upload(folder: boolean) {
    if (pane.sessionId && dir)
      started(api.sftpUploadPick(pane.sessionId, dir, folder), dir);
  }

  const UNITS = ["B", "KiB", "MiB", "GiB", "TiB"];
  function size(e: Entry): string {
    if (e.kind === "Dir") return t("sftp.folder");
    if (e.kind === "Link") return t("sftp.link");
    if (e.size === null) return "";
    let n = e.size;
    let u = 0;
    for (; n >= 1024 && u < UNITS.length - 1; u++) n /= 1024;
    return u ? `${n.toFixed(1)} ${UNITS[u]}` : `${n} B`;
  }

  const date = (s: number | null) =>
    s === null
      ? ""
      : new Date(s * 1000).toLocaleString(current.lang, {
          dateStyle: "short",
          timeStyle: "short",
        });
</script>

<section
  class="files"
  bind:this={root}
  data-sftp-pane={pane.paneId}
  aria-label={t("sftp.title")}
>
  <div class="bar">
    <button
      type="button"
      class="icon"
      aria-label={t("sftp.up")}
      title={t("sftp.up")}
      disabled={!dir || dir === "/"}
      onclick={up}>↑</button
    >
    <span class="path" title={dir}>{dir}</span>
    <button
      type="button"
      class="icon"
      aria-label={t("sftp.refresh")}
      title={t("sftp.refresh")}
      disabled={!dir}
      onclick={() => load(dir)}>↻</button
    >
  </div>
  <div class="opts">
    <Toggle
      label={t("sftp.showHidden")}
      checked={settings.sftp_show_hidden}
      onchange={(v) => saveSettings({ sftp_show_hidden: v }).catch(failed)}
    />
  </div>
  {#if listing?.truncated}
    <p class="note" role="status">{t("sftp.truncated")}</p>
  {/if}
  <ul
    class="list"
    role="listbox"
    aria-label={t("sftp.list")}
    aria-multiselectable="true"
    bind:this={list}
  >
    {#each rows as e, i (e.name)}
      <li
        role="option"
        aria-selected={selected.has(e.name)}
        tabindex={i === tabRow ? 0 : -1}
        class:selected={selected.has(e.name)}
        onfocus={() => (cursor = i)}
        onclick={(ev) => click(ev, i)}
        ondblclick={() => open(e)}
        onkeydown={(ev) => onkey(ev, i)}
      >
        <span class="name" class:dir={e.kind === "Dir"} title={e.name}
          >{e.name}</span
        >
        <span class="size">{size(e)}</span>
        <span class="date">{date(e.modified)}</span>
      </li>
    {/each}
  </ul>
  {#if listing && !rows.length}
    <p class="note">{t("sftp.empty")}</p>
  {/if}
  <div class="actions">
    <button type="button" disabled={!dir} onclick={() => upload(false)}
      >{t("sftp.uploadFiles")}</button
    >
    <button type="button" disabled={!dir} onclick={() => upload(true)}
      >{t("sftp.uploadFolder")}</button
    >
    <button
      type="button"
      class="primary"
      disabled={!chosen.length}
      onclick={download}>{t("sftp.download")}</button
    >
  </div>
  <TransferList {pane} />
</section>

<style>
  .files {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
    gap: var(--space-2);
    padding: var(--space-3);
    background: var(--surface-2);
    font-size: 12.5px;
  }
  /* Set by App while files are dragged over the panel. */
  .files:global([data-drop]) {
    outline: 2px dashed var(--accent);
    outline-offset: -4px;
  }
  .bar,
  .actions {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .actions {
    flex-wrap: wrap;
  }
  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: monospace;
  }
  .icon {
    flex: none;
    padding: 2px var(--space-2);
  }
  .note {
    margin: 0;
    color: var(--fg-muted);
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .list li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    gap: var(--space-3);
    padding: 3px var(--space-2);
    border-radius: var(--radius-sm);
    cursor: default;
    user-select: none;
  }
  .list li.selected {
    background: var(--surface-3);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .list li:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name.dir {
    font-weight: 600;
  }
  .size,
  .date {
    color: var(--fg-muted);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
</style>
