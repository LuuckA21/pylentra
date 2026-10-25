<script lang="ts">
  import type { Settings } from "../../lib/bindings/Settings";
  import { t } from "../../lib/i18n";
  import { settings } from "../../lib/stores/settings.svelte";
  import type { SectionProps } from "./props";

  let { save }: SectionProps = $props();
  const uid = $props.id();
</script>

<div class="field">
  <label for="{uid}-theme">{t("settings.theme")}</label>
  <select
    id="{uid}-theme"
    value={settings.theme}
    onchange={(e) =>
      save({ theme: e.currentTarget.value as Settings["theme"] })}
  >
    {#each ["System", "Light", "Dark"] as const as v (v)}
      <option value={v}>{t(`settings.theme.${v}`)}</option>
    {/each}
  </select>
</div>

<div class="field">
  <label for="{uid}-lang">{t("settings.language")}</label>
  <select
    id="{uid}-lang"
    value={settings.language ?? ""}
    onchange={(e) => save({ language: e.currentTarget.value || null })}
  >
    <option value="">{t("settings.language.system")}</option>
    <option value="en">{t("settings.language.en")}</option>
    <option value="it">{t("settings.language.it")}</option>
  </select>
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    max-width: 280px;
  }
</style>
