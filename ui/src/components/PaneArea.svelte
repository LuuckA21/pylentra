<script lang="ts">
  import {
    clampRatio,
    count,
    MAX_PANES,
    rects,
    type Dir,
    type Divider,
  } from "../lib/layout";
  import { t } from "../lib/i18n";
  import {
    paneOf,
    sessions,
    setTabRatio,
    type Tab,
  } from "../lib/stores/sessions.svelte";
  import TerminalPane from "./TerminalPane.svelte";

  let {
    onedit,
    onsave,
    onclose,
    onsplit,
  }: {
    onedit: (connectionId: string) => void;
    onsave: (paneId: string) => void;
    onclose: (paneId: string) => void;
    onsplit: (paneId: string, dir: Dir) => void;
  } = $props();

  // Every tab's layout. The panes of all tabs are one flat list keyed by pane,
  // so moving a pane to another tab (or splitting, closing) never recreates it:
  // its terminal and session survive. Only the active tab shows.
  const views = $derived(
    sessions.tabs.map((tab) => ({
      tab,
      geometry: rects(tab.root),
      n: count(tab.root),
      shown: tab.tabId === sessions.active,
    })),
  );
  const slots = $derived(
    views.flatMap((v) =>
      Object.entries(v.geometry.panes).map(([id, r]) => ({ id, r, v })),
    ),
  );

  const STEP = 0.05;
  // Smallest pane while dragging: 160 px wide, 120 px tall.
  const minPx = (d: Divider) => (d.dir === "row" ? 160 : 120);
  const pct = (d: Divider) => Math.round(d.ratio * 100);
  // The path of the divider being dragged.
  let dragging = $state<string | null>(null);

  function drag(e: PointerEvent, tab: Tab, d: Divider) {
    if (dragging !== d.path.join()) return;
    const view = (e.currentTarget as HTMLElement).parentElement!;
    const box = view.getBoundingClientRect();
    const row = d.dir === "row";
    const total =
      (row ? box.width : box.height) * ((row ? d.area.w : d.area.h) / 100);
    if (total <= 0) return;
    const start =
      (row ? box.left : box.top) +
      ((row ? box.width : box.height) * (row ? d.area.x : d.area.y)) / 100;
    const pos = (row ? e.clientX : e.clientY) - start;
    setTabRatio(tab.tabId, d.path, clampRatio(pos / total, total, minPx(d)));
  }

  function key(e: KeyboardEvent, tab: Tab, d: Divider) {
    // Alt+arrow is the window's pane-focus shortcut, not a resize.
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    const back = d.dir === "row" ? "ArrowLeft" : "ArrowUp";
    const fwd = d.dir === "row" ? "ArrowRight" : "ArrowDown";
    const next =
      e.key === back
        ? d.ratio - STEP
        : e.key === fwd
          ? d.ratio + STEP
          : e.key === "Enter"
            ? 0.5
            : null;
    if (next === null) return;
    e.preventDefault();
    setTabRatio(tab.tabId, d.path, next);
  }
</script>

<!-- Dividers come first in the DOM so Tab from outside the terminals reaches
     them (a terminal swallows Tab); z-index keeps them over the panes. -->
<div class="area">
  {#each views as v (v.tab.tabId)}
    {@const tab = v.tab}
    <div class="view" hidden={!v.shown}>
      {#each v.geometry.dividers as d (d.path.join())}
        {@const row = d.dir === "row"}
        <!-- A focusable separator is a widget (ARIA window splitter). -->
        <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
        <div
          class="divider"
          class:row
          role="separator"
          tabindex="0"
          aria-label={t("pane.divider")}
          aria-orientation={row ? "vertical" : "horizontal"}
          aria-valuenow={pct(d)}
          aria-valuemin="10"
          aria-valuemax="90"
          style:left={row
            ? `${d.area.x + d.area.w * d.ratio}%`
            : `${d.area.x}%`}
          style:top={row ? `${d.area.y}%` : `${d.area.y + d.area.h * d.ratio}%`}
          style:width={row ? undefined : `${d.area.w}%`}
          style:height={row ? `${d.area.h}%` : undefined}
          onmousedown={(e) => e.preventDefault()}
          onpointerdown={(e) => {
            dragging = d.path.join();
            (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
          }}
          onpointermove={(e) => drag(e, tab, d)}
          onpointerup={(e) => {
            dragging = null;
            (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
          }}
          onlostpointercapture={() => (dragging = null)}
          ondblclick={() => setTabRatio(tab.tabId, d.path, 0.5)}
          onkeydown={(e) => key(e, tab, d)}
        ></div>
      {/each}
    </div>
  {/each}
  {#each slots as { id, r, v } (id)}
    {@const pane = paneOf(id)}
    {#if pane}
      <div
        class="slot"
        hidden={!v.shown}
        style:left="{r.x}%"
        style:top="{r.y}%"
        style:width="{r.w}%"
        style:height="{r.h}%"
      >
        <TerminalPane
          {pane}
          visible={v.shown}
          focused={id === v.tab.focus}
          {onedit}
          {onsave}
          {onclose}
          canSplit={v.n < MAX_PANES}
          multi={v.n > 1}
          {onsplit}
        />
      </div>
    {/if}
  {/each}
</div>

<style>
  .area,
  .view,
  .slot {
    position: absolute;
  }
  .area,
  .view {
    inset: 0;
  }
  /* The dividers' layer covers the panes: only the dividers take the pointer. */
  .view {
    z-index: 1;
    pointer-events: none;
  }
  .view[hidden],
  .slot[hidden] {
    display: none;
  }
  .divider {
    position: absolute;
    pointer-events: auto;
    /* Centered on the line, over the panes' edges. */
    transform: translateX(-3px);
    width: 6px;
    background: transparent;
    cursor: col-resize;
    touch-action: none;
  }
  .divider:not(.row) {
    transform: translateY(-3px);
    height: 6px;
    cursor: row-resize;
  }
  .divider:hover,
  .divider:focus-visible,
  .divider:active {
    background: var(--accent);
    outline: none;
  }
</style>
