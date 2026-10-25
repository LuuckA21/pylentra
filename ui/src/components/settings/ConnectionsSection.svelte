<script lang="ts">
  import { t } from "../../lib/i18n";
  import { settings } from "../../lib/stores/settings.svelte";
  import NumberField from "./NumberField.svelte";
  import type { SectionProps } from "./props";
  import Toggle from "./Toggle.svelte";

  let { save, errors }: SectionProps = $props();
  const uid = $props.id();
</script>

<div class="field">
  <label for="{uid}-user">{t("settings.defaultUser")}</label>
  <input
    id="{uid}-user"
    value={settings.default_user ?? ""}
    aria-invalid={errors.default_user ? "true" : undefined}
    aria-describedby="{uid}-user-note"
    onchange={(e) =>
      save({ default_user: e.currentTarget.value.trim() || null })}
  />
  <p class={errors.default_user ? "error" : "hint"} id="{uid}-user-note">
    {errors.default_user ?? t("settings.defaultUserHint")}
  </p>
</div>

<div class="grid">
  <NumberField
    label={t("settings.defaultPort")}
    value={settings.default_port}
    min={1}
    max={65535}
    error={errors.default_port}
    save={(n) => save({ default_port: n })}
  />
  <NumberField
    label={t("settings.connectTimeout")}
    value={settings.connect_timeout_secs}
    min={5}
    max={120}
    hint={t("settings.connectTimeoutHint")}
    error={errors.connect_timeout_secs}
    save={(n) => save({ connect_timeout_secs: n })}
  />
  <NumberField
    label={t("settings.keepalive")}
    value={settings.keepalive_secs}
    min={5}
    max={300}
    zeroOff
    hint={t("settings.keepaliveHint")}
    error={errors.keepalive_secs}
    save={(n) => save({ keepalive_secs: n })}
  />
</div>

<Toggle
  label={t("settings.autoReconnect")}
  hint={t("settings.autoReconnectHint")}
  checked={settings.auto_reconnect}
  onchange={(v) => save({ auto_reconnect: v })}
/>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    max-width: 280px;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--space-4);
  }
  .error,
  .hint {
    margin: 0;
    font-size: 12px;
  }
  .error {
    color: var(--danger);
  }
  .hint {
    color: var(--fg-muted);
  }
</style>
