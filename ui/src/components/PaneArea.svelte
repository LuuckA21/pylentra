<script lang="ts">
  import {
    clampRatio,
    count,
    MAX_PANES,
    rects,
    type Dir,
    type Divider,
  } from "../lib/layout";
  import { keyStep, startDrag } from "../lib/divider";
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

  // Smallest pane while dragging: 160 px wide, 120 px tall.
  const minPx = (d: Divider) => (d.dir === "row" ? 160 : 120);
  const pct = (d: Divider) => Math.round(d.ratio * 100);

  // r is the pointer's position across the whole view; the divider splits only
  // its own area of it.
  function drag(view: HTMLElement, tab: Tab, d: Divider, r: number) {
    const box = view.getBoundingClientRect();
    const row = d.dir === "row";
    const [offset, span] = row ? [d.area.x, d.area.w] : [d.area.y, d.area.h];
    const total = ((row ? box.width : box.height) * span) / 100;
    if (total <= 0) return;
    setTabRatio(
      tab.tabId,
      d.path,
      clampRatio((r * 100 - offset) / span, total, minPx(d)),
    );
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
            const view = e.currentTarget.parentElement!;
            startDrag(e, view, (r) => drag(view, tab, d, r), d.dir);
          }}
          ondblclick={() => setTabRatio(tab.tabId, d.path, 0.5)}
          onkeydown={(e) => {
            const next = keyStep(e, d.ratio);
            if (next !== null) setTabRatio(tab.tabId, d.path, next);
          }}
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
</style>
