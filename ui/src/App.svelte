<script lang="ts">
  import { onMount } from "svelte";
  import Banner from "./components/Banner.svelte";
  import ConnectionForm from "./components/ConnectionForm.svelte";
  import Settings from "./components/Settings.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import TerminalTab from "./components/TerminalTab.svelte";
  import Toasts from "./components/Toasts.svelte";
  import UpdateBanner from "./components/UpdateBanner.svelte";
  import Welcome from "./components/Welcome.svelte";
  import { colorOf } from "./lib/appearance";
  import type { Connection } from "./lib/bindings/Connection";
  import { t } from "./lib/i18n";
  import { library, refresh } from "./lib/stores/library.svelte";
  import { closeTab, openTab, sessions } from "./lib/stores/sessions.svelte";
  import { loadSettings } from "./lib/stores/settings.svelte";
  import { toastError } from "./lib/stores/toasts.svelte";

  const openConnectionIds = $derived(
    new Set(
      sessions.tabs
        .filter((tab) => tab.phase !== "closed")
        .map((tab) => tab.connectionId),
    ),
  );
  // `connection` undefined means a new connection.
  let editing = $state<{ connection?: Connection } | null>(null);
  let settingsOpen = $state(false);

  const connOf = (id: string) => library.connections.find((c) => c.id === id);
  const nameOf = (id: string) => connOf(id)?.name ?? id;
  const dotOf = (id: string) => {
    const c = connOf(id);
    return c ? colorOf(c, library.groups) : null;
  };

  function edit(connectionId: string) {
    const connection = library.connections.find((c) => c.id === connectionId);
    if (connection) editing = { connection };
  }

  onMount(() => {
    Promise.all([loadSettings(), refresh()]).catch(toastError);
  });
</script>

<div class="app">
  <Banner />
  <UpdateBanner />
  <div class="body">
    <Sidebar
      {openConnectionIds}
      onopen={openTab}
      onedit={(connection) => (editing = { connection })}
      onsettings={() => (settingsOpen = true)}
      onimport={() => {}}
    />
    <main>
      {#if sessions.tabs.length}
        <div class="topbar">
          <div class="tabbar" role="tablist" aria-label={t("tabs.label")}>
            {#each sessions.tabs as tab (tab.tabId)}
              <div class="tab" class:active={tab.tabId === sessions.active}>
                <button
                  type="button"
                  role="tab"
                  class="title"
                  class:closed={tab.phase === "closed"}
                  aria-selected={tab.tabId === sessions.active}
                  onclick={() => (sessions.active = tab.tabId)}
                >
                  <span class="dot tag-{dotOf(tab.connectionId) ?? 'none'}"
                  ></span>
                  {nameOf(tab.connectionId)}
                </button>
                <button
                  type="button"
                  class="close"
                  aria-label={t("tabs.close")}
                  title={t("tabs.close")}
                  onclick={() => closeTab(tab.tabId)}>×</button
                >
              </div>
            {/each}
          </div>
          <!-- Quick connect goes here. -->
          <div class="quick"></div>
        </div>
        <div class="tabs">
          {#each sessions.tabs as tab (tab.tabId)}
            <TerminalTab
              {tab}
              active={tab.tabId === sessions.active}
              onedit={edit}
            />
          {/each}
        </div>
      {:else}
        <div class="home"><Welcome onopen={openTab} /></div>
      {/if}
    </main>
  </div>
</div>

{#if editing}
  <ConnectionForm
    connection={editing.connection}
    onsaved={() => (editing = null)}
    oncancel={() => (editing = null)}
  />
{/if}
{#if settingsOpen}
  <Settings onclose={() => (settingsOpen = false)} />
{/if}
<Toasts />

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .body {
    flex: 1;
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-3);
    padding: var(--space-3);
    min-height: 0;
  }
  main {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-width: 0;
    min-height: 0;
  }
  .home {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    background: var(--surface-1);
    border-radius: var(--radius-lg);
  }
  .topbar {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  .quick {
    flex: 1;
  }
  .tabbar {
    display: flex;
    gap: var(--space-1);
    min-width: 0;
    padding: 5px;
    overflow-x: auto;
    background: var(--surface-1);
    border-radius: var(--radius-md);
  }
  .tab {
    display: flex;
    flex: none;
    align-items: center;
    height: 34px;
    border-radius: var(--radius-sm);
    color: var(--fg-muted);
  }
  .tab:hover {
    background: var(--surface-2);
  }
  .tab.active {
    background: var(--surface-3);
    color: var(--fg);
    font-weight: 600;
  }
  .tab button {
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
  }
  .title {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    max-width: 200px;
    padding-right: var(--space-1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title.closed {
    color: var(--fg-muted);
  }
  .title.closed .dot {
    opacity: 0.45;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--tag-solid);
  }
  .close {
    padding: 2px var(--space-2);
    /* Not visibility: hidden, which would take the button out of the tab order. */
    opacity: 0;
    pointer-events: none;
  }
  .tab.active .close,
  .tab:hover .close,
  .close:focus-visible {
    opacity: 1;
    pointer-events: auto;
  }
  .tabs {
    position: relative;
    flex: 1;
    min-height: 0;
  }
</style>
