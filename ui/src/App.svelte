<script lang="ts">
  import { onMount } from "svelte";
  import Banner from "./components/Banner.svelte";
  import ConnectionForm from "./components/ConnectionForm.svelte";
  import Settings from "./components/Settings.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import TerminalTab from "./components/TerminalTab.svelte";
  import Toasts from "./components/Toasts.svelte";
  import UpdateBanner from "./components/UpdateBanner.svelte";
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

  const nameOf = (id: string) =>
    library.connections.find((c) => c.id === id)?.name ?? id;

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
                >{nameOf(tab.connectionId)}</button
              >
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
        <p class="placeholder">{t("main.placeholder")}</p>
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
    min-height: 0;
  }
  main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .placeholder {
    margin: auto;
    color: var(--fg-muted);
  }
  .tabbar {
    display: flex;
    overflow-x: auto;
    background: var(--bg-alt);
    border-bottom: 1px solid var(--border);
  }
  .tab {
    display: flex;
    align-items: center;
    border-right: 1px solid var(--border);
  }
  .tab.active {
    background: var(--bg);
  }
  .tab button {
    border: none;
    border-radius: 0;
    background: transparent;
  }
  .title {
    max-width: 200px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title.closed {
    color: var(--fg-muted);
  }
  .close {
    padding: 4px 8px;
  }
  .tabs {
    position: relative;
    flex: 1;
    min-height: 0;
  }
</style>
