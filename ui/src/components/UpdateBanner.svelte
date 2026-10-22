<script lang="ts">
  import { onMount } from "svelte";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import { t } from "../lib/i18n";

  let update = $state<Update | null>(null);
  let installing = $state(false);

  // Updater failures (offline, endpoint down) are never shown to the user.
  onMount(() => {
    check()
      .then((u) => (update = u))
      .catch((e) => console.warn("update check failed", e));
  });

  async function install() {
    if (!update) return;
    installing = true;
    try {
      await update.downloadAndInstall();
      await relaunch();
    } catch (e) {
      console.warn("update install failed", e);
      installing = false;
    }
  }
</script>

{#if update}
  <div
    class="update"
    role="region"
    aria-label={t("update.available", { version: update.version })}
  >
    <span>{t("update.available", { version: update.version })}</span>
    <button type="button" disabled={installing} onclick={install}
      >{installing ? t("update.installing") : t("update.install")}</button
    >
  </div>
{/if}

<style>
  .update {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 12px;
    background: var(--bg-alt);
    border-bottom: 1px solid var(--border);
  }
</style>
