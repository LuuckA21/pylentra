<script lang="ts">
  import { t } from "../../lib/i18n";
  import { settings } from "../../lib/stores/settings.svelte";
  import { toast, toastError } from "../../lib/stores/toasts.svelte";
  import { checkForUpdate, updates } from "../../lib/stores/updates.svelte";
  import type { SectionProps } from "./props";
  import Toggle from "./Toggle.svelte";

  let { save, version }: SectionProps & { version: string } = $props();

  // An update found shows the app banner; here only the other outcomes speak.
  async function checkNow() {
    try {
      if (!(await checkForUpdate())) toast(t("update.upToDate"), "success");
    } catch (e) {
      toastError(e);
    }
  }
</script>

{#if version}
  <p class="version">
    {t("settings.installedVersion")}: <strong>{version}</strong>
  </p>
{/if}

<Toggle
  label={t("settings.checkUpdates")}
  checked={settings.check_updates}
  onchange={(v) => save({ check_updates: v })}
/>

<div>
  <button type="button" disabled={updates.checking} onclick={checkNow}
    >{t("update.checkNow")}</button
  >
</div>

<style>
  .version {
    margin: 0;
  }
</style>
