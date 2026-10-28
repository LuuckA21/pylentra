<script lang="ts">
  import { count, MAX_PANES, rects, type Dir } from "../lib/layout";
  import { paneOf, type Tab } from "../lib/stores/sessions.svelte";
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

  const layout = $derived(rects(tab.root).panes);
  const n = $derived(count(tab.root));
  const canSplit = $derived(n < MAX_PANES);
</script>

<!-- A flat list keyed by pane: a split or a close moves a pane without
     recreating it, so its terminal and session survive. -->
<div class="view" hidden={!visible}>
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
</style>
