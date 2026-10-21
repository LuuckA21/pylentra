<script lang="ts">
  import { onMount } from "svelte";
  import Banner from "./components/Banner.svelte";
  import ConnectionForm from "./components/ConnectionForm.svelte";
  import Sidebar from "./components/Sidebar.svelte";
  import Toasts from "./components/Toasts.svelte";
  import type { Connection } from "./lib/bindings/Connection";
  import { t } from "./lib/i18n";
  import { refresh } from "./lib/stores/library.svelte";
  import { loadSettings } from "./lib/stores/settings.svelte";
  import { toastError } from "./lib/stores/toasts.svelte";

  // Task 14's session store feeds this set; until then nothing is open.
  let openConnectionIds = $state(new Set<string>());
  // `connection` undefined means a new connection.
  let editing = $state<{ connection?: Connection } | null>(null);

  onMount(() => {
    Promise.all([loadSettings(), refresh()]).catch(toastError);
  });
</script>

<div class="app">
  <Banner />
  <div class="body">
    <Sidebar
      {openConnectionIds}
      onopen={() => {
        /* Task 14 opens a session tab here. */
      }}
      onedit={(connection) => (editing = { connection })}
    />
    <main>
      <!-- Task 14: tabs and terminal. -->
      <p class="placeholder">{t("main.placeholder")}</p>
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
    grid-template-columns: 280px 1fr;
    min-height: 0;
  }
  main {
    display: grid;
    place-items: center;
    min-width: 0;
  }
  .placeholder {
    color: var(--fg-muted);
  }
</style>
