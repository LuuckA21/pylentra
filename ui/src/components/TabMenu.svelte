<script lang="ts">
  import { onMount } from "svelte";
  import { t } from "../lib/i18n";
  import { count, MAX_PANES } from "../lib/layout";
  import {
    detachPane,
    mergeTab,
    sessions,
    type Tab,
  } from "../lib/stores/sessions.svelte";

  let {
    tab,
    x,
    y,
    titleOf,
    onclose,
  }: {
    tab: Tab;
    x: number;
    y: number;
    // The name shown for a pane, as on the tab strip.
    titleOf: (paneId: string) => string;
    // `refocus`: give the focus back to the tab (Escape) rather than leave it
    // where the action put it.
    onclose: (refocus: boolean) => void;
  } = $props();

  let menuEl: HTMLElement;
  const others = $derived(sessions.tabs.filter((o) => o !== tab));
  const n = $derived(count(tab.root));

  const items = () => [
    ...menuEl.querySelectorAll<HTMLElement>("button:not(:disabled)"),
  ];

  onMount(() => (items()[0] ?? menuEl).focus());

  function onkeydown(e: KeyboardEvent) {
    const list = items();
    const i = list.indexOf(document.activeElement as HTMLElement);
    if (e.key === "Escape") {
      e.preventDefault();
      onclose(true);
    } else if (e.key === "Tab") {
      onclose(false);
    } else if (list.length && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      list[(i + step + list.length) % list.length].focus();
    }
  }
</script>

<svelte:window
  onpointerdown={(e) => {
    if (!menuEl.contains(e.target as Node)) onclose(false);
  }}
/>

<div
  class="menu"
  role="menu"
  tabindex="-1"
  aria-label={t("tabs.menu")}
  bind:this={menuEl}
  style:left="{Math.min(x, window.innerWidth - 260)}px"
  style:top="{Math.min(y, window.innerHeight - 200)}px"
  {onkeydown}
>
  {#each others as o (o.tabId)}
    {@const full = n + count(o.root) > MAX_PANES}
    <button
      type="button"
      role="menuitem"
      disabled={full}
      title={full ? t("tabs.mergeFull") : undefined}
      onclick={() => {
        mergeTab(tab.tabId, o.tabId);
        onclose(false);
      }}
    >
      {t("tabs.mergeInto", { name: titleOf(o.focus) })}
    </button>
  {/each}
  {#if n > 1}
    <button
      type="button"
      role="menuitem"
      onclick={() => {
        detachPane(tab.focus);
        onclose(false);
      }}
    >
      {t("tabs.detach", { name: titleOf(tab.focus) })}
    </button>
  {/if}
  {#if !others.length && n === 1}
    <button type="button" role="menuitem" disabled>{t("tabs.nothing")}</button>
  {/if}
</div>

<style>
  .menu {
    position: fixed;
    z-index: 50;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    padding: 4px;
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow);
  }
  button {
    border: 0;
    background: none;
    text-align: left;
  }
  button:hover:not(:disabled),
  button:focus-visible {
    background: var(--surface-3);
  }
  button:disabled {
    color: var(--fg-muted);
  }
</style>
