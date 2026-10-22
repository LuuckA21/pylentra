<script lang="ts">
  import { openPath } from "@tauri-apps/plugin-opener";
  import { api } from "../lib/api";
  import type { Settings } from "../lib/bindings/Settings";
  import { t } from "../lib/i18n";
  import { modal } from "../lib/modal";
  import { saveSettings, settings } from "../lib/stores/settings.svelte";
  import { toastError } from "../lib/stores/toasts.svelte";

  let { onclose }: { onclose: () => void } = $props();

  const uid = $props.id();
  let sizeError = $state(false);

  // Every control saves on change; the theme and language apply right away.
  const save = (patch: Partial<Settings>) =>
    saveSettings(patch).catch(toastError);

  function saveSize(raw: string) {
    const n = raw.trim() === "" ? NaN : Number(raw);
    sizeError = !(Number.isInteger(n) && n >= 8 && n <= 32);
    if (!sizeError) save({ font_size: n });
  }

  function saveFamily(raw: string) {
    if (raw.trim()) save({ font_family: raw.trim() });
  }

  const openLogs = async () => {
    try {
      await openPath(await api.logDir());
    } catch (e) {
      toastError(e);
    }
  };
</script>

<div class="backdrop">
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="{uid}-title"
    use:modal={onclose}
  >
    <h2 id="{uid}-title">{t("settings.title")}</h2>

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
      <label for="{uid}-family">{t("settings.fontFamily")}</label>
      <input
        id="{uid}-family"
        value={settings.font_family}
        onchange={(e) => saveFamily(e.currentTarget.value)}
      />
    </div>

    <div class="field">
      <label for="{uid}-size">{t("settings.fontSize")}</label>
      <input
        id="{uid}-size"
        type="number"
        value={settings.font_size}
        aria-invalid={sizeError ? "true" : undefined}
        aria-describedby={sizeError ? `${uid}-size-err` : undefined}
        onchange={(e) => saveSize(e.currentTarget.value)}
      />
      {#if sizeError}
        <p class="error" id="{uid}-size-err">{t("settings.fontSizeError")}</p>
      {/if}
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

    <div class="actions">
      <button type="button" onclick={openLogs}>{t("settings.openLogs")}</button>
      <button type="button" class="primary" onclick={onclose}
        >{t("common.close")}</button
      >
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: var(--backdrop);
    z-index: 60;
  }
  .dialog {
    width: min(420px, calc(100vw - 32px));
    padding: 16px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
  }
  h2 {
    margin: 0 0 12px;
    font-size: 16px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 10px;
  }
  .error {
    margin: 0;
    color: var(--danger);
    font-size: 12px;
  }
  .actions {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    margin-top: 16px;
  }
</style>
