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
  import { paneOf, setTabRatio, type Tab } from "../lib/stores/sessions.svelte";
  import TerminalPane from "./TerminalPane.svelte";

  let {
    tab,
    visible,
    onedit,
    onsave,
    onclose,
    onsplit,
  }: {
    tab: Tab;
    visible: boolean;
    onedit: (connectionId: string) => void;
    onsave: (paneId: string) => void;
    onclose: (paneId: string) => void;
    onsplit: (paneId: string, dir: Dir) => void;
  } = $props();

  const geometry = $derived(rects(tab.root));
  const layout = $derived(geometry.panes);
  const n = $derived(count(tab.root));
  const canSplit = $derived(n < MAX_PANES);

  const STEP = 0.05;
  // Smallest pane while dragging: 160 px wide, 120 px tall.
  const minPx = (d: Divider) => (d.dir === "row" ? 160 : 120);
  const pct = (d: Divider) => Math.round(d.ratio * 100);
  let dragging = $state<Divider | null>(null);

  function drag(e: PointerEvent, d: Divider) {
    if (dragging?.path.join() !== d.path.join()) return;
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

  function key(e: KeyboardEvent, d: Divider) {
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
<!-- A flat list keyed by pane: a split or a close moves a pane without
     recreating it, so its terminal and session survive. -->
<div class="view" hidden={!visible}>
  {#each geometry.dividers as d (d.path.join())}
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
      style:left={row ? `${d.area.x + d.area.w * d.ratio}%` : `${d.area.x}%`}
      style:top={row ? `${d.area.y}%` : `${d.area.y + d.area.h * d.ratio}%`}
      style:width={row ? undefined : `${d.area.w}%`}
      style:height={row ? `${d.area.h}%` : undefined}
      onmousedown={(e) => e.preventDefault()}
      onpointerdown={(e) => {
        dragging = d;
        (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
      }}
      onpointermove={(e) => drag(e, d)}
      onpointerup={(e) => {
        dragging = null;
        (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
      }}
      onlostpointercapture={() => (dragging = null)}
      ondblclick={() => setTabRatio(tab.tabId, d.path, 0.5)}
      onkeydown={(e) => key(e, d)}
    ></div>
  {/each}
  {#each Object.keys(layout) as id (id)}
    {@const pane = paneOf(id)}
    {@const r = layout[id]}
    {#if pane}
      <div
        class="slot"
        style:left="{r.x}%"
        style:top="{r.y}%"
        style:width="{r.w}%"
        style:height="{r.h}%"
      >
        <TerminalPane
          {pane}
          {visible}
          focused={id === tab.focus}
          {onedit}
          {onsave}
          {onclose}
          {canSplit}
          multi={n > 1}
          {onsplit}
        />
      </div>
    {/if}
  {/each}
</div>

<style>
  .view,
  .slot {
    position: absolute;
  }
  .view {
    inset: 0;
  }
  .divider {
    position: absolute;
    z-index: 1;
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
