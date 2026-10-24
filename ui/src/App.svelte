<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { warn } from "@tauri-apps/plugin-log";
  import QuickConnect from "./components/QuickConnect.svelte";
  import Banner from "./components/Banner.svelte";
  import ConnectionForm from "./components/ConnectionForm.svelte";
  import ImportDialog from "./components/ImportDialog.svelte";
  import Settings from "./components/Settings.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import TerminalTab from "./components/TerminalTab.svelte";
  import Toasts from "./components/Toasts.svelte";
  import UpdateBanner from "./components/UpdateBanner.svelte";
  import ConfirmDialog from "./components/ui/ConfirmDialog.svelte";
  import Welcome from "./components/Welcome.svelte";
  import { colorOf } from "./lib/appearance";
  import type { Connection } from "./lib/bindings/Connection";
  import { t } from "./lib/i18n";
  import { library, refresh } from "./lib/stores/library.svelte";
  import {
    closeTab,
    linkQuickTab,
    openTab,
    sessions,
  } from "./lib/stores/sessions.svelte";
  import { loadSettings, settings } from "./lib/stores/settings.svelte";
  import { toastError } from "./lib/stores/toasts.svelte";
  import { checkForUpdate } from "./lib/stores/updates.svelte";

  const openConnectionIds = $derived(
    new Set(
      sessions.tabs
        .filter((tab) => tab.phase !== "closed")
        .map((tab) => tab.connectionId),
    ),
  );
  // `connection` undefined means a new connection. `quickTabId`: the form
  // saves that quick tab's connection (pre-filled, created as new).
  let editing = $state<{ connection?: Connection; quickTabId?: string } | null>(
    null,
  );
  let settingsOpen = $state(false);
  let importOpen = $state(false);

  // The one confirmation on screen; `action` runs if the user confirms.
  let confirming = $state<{
    title: string;
    message: string;
    action: () => void;
  } | null>(null);

  const liveCount = () =>
    sessions.tabs.filter((tab) => tab.phase === "ready").length;

  // Only a connected tab is worth asking about.
  function requestClose(tabId: string) {
    const tab = sessions.tabs.find((x) => x.tabId === tabId);
    if (!tab || !settings.confirm_close_active || tab.phase !== "ready")
      return closeTab(tabId);
    confirming = {
      title: t("confirm.tab.title"),
      message: t("confirm.tab.body", {
        name: tab.quick?.name ?? nameOf(tab.connectionId),
      }),
      action: () => closeTab(tabId),
    };
  }

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

  // Opens the new-connection form pre-filled from a quick tab, named after its host.
  function saveQuick(tabId: string) {
    const quick = sessions.tabs.find((tab) => tab.tabId === tabId)?.quick;
    if (quick)
      editing = {
        connection: { ...quick, name: quick.host },
        quickTabId: tabId,
      };
  }

  onMount(() => {
    // Updater failures (offline, endpoint down) are never shown at startup.
    Promise.all([
      loadSettings().then(() => {
        if (settings.check_updates)
          return checkForUpdate().catch((e) =>
            warn(`update check failed: ${e}`),
          );
      }),
      refresh(),
    ]).catch(toastError);

    const unlisten = getCurrentWindow().onCloseRequested((e) => {
      const n = liveCount();
      if (!settings.confirm_close_active || !n) return;
      e.preventDefault();
      confirming = {
        title: t("confirm.window.title"),
        message:
          n === 1
            ? t("confirm.window.bodyOne")
            : t("confirm.window.body", { n }),
        action: () => void getCurrentWindow().destroy().catch(toastError),
      };
    });
    return () => void unlisten.then((f) => f());
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
      onimport={() => (importOpen = true)}
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
                  {tab.quick?.name ?? nameOf(tab.connectionId)}
                </button>
                <button
                  type="button"
                  class="close"
                  aria-label={t("tabs.close")}
                  title={t("tabs.close")}
                  onclick={() => requestClose(tab.tabId)}>×</button
                >
              </div>
            {/each}
          </div>
          <div class="quick"><QuickConnect onopen={openTab} /></div>
        </div>
        <div class="tabs">
          {#each sessions.tabs as tab (tab.tabId)}
            <TerminalTab
              {tab}
              active={tab.tabId === sessions.active}
              onedit={(id) => (tab.quick ? saveQuick(tab.tabId) : edit(id))}
              onsave={saveQuick}
              onclose={requestClose}
            />
          {/each}
        </div>
      {:else}
        <div class="home">
          <Welcome onopen={openTab}>
            {#snippet quick()}<QuickConnect onopen={openTab} />{/snippet}
          </Welcome>
        </div>
      {/if}
    </main>
  </div>
</div>

{#if editing}
  <ConnectionForm
    connection={editing.connection}
    isNew={!!editing.quickTabId}
    onsaved={(saved) => {
      if (editing?.quickTabId) linkQuickTab(editing.quickTabId, saved);
      editing = null;
    }}
    oncancel={() => (editing = null)}
  />
{/if}
{#if importOpen}
  <ImportDialog onclose={() => (importOpen = false)} />
{/if}
{#if settingsOpen}
  <Settings onclose={() => (settingsOpen = false)} />
{/if}
{#if confirming}
  <ConfirmDialog
    title={confirming.title}
    message={confirming.message}
    confirmLabel={t("common.close")}
    danger
    onconfirm={() => {
      // Dismissed first, so a throwing action cannot leave it on screen.
      const action = confirming?.action;
      confirming = null;
      action?.();
    }}
    oncancel={() => (confirming = null)}
  />
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
    display: flex;
    flex: 1;
    justify-content: flex-end;
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
