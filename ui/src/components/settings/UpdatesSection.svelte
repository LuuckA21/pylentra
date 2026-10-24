<script lang="ts">
  import { t } from "../../lib/i18n";
  import { settings } from "../../lib/stores/settings.svelte";
  import type { SectionProps } from "./props";
  import Toggle from "./Toggle.svelte";

  let { save, version }: SectionProps & { version: string } = $props();
</script>

<p class="version">
  {t("settings.installedVersion")}: <strong>{version}</strong>
</p>

<Toggle
  label={t("settings.checkUpdates")}
  checked={settings.check_updates}
  onchange={(v) => save({ check_updates: v })}
/>

<!-- Task 19: the "Check now" button goes here. -->

<style>
  .version {
    margin: 0;
  }
</style>
