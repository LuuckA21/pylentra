<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "../../lib/api";
  import type { KnownHostEntry } from "../../lib/bindings/KnownHostEntry";
  import { t } from "../../lib/i18n";
  import { settings } from "../../lib/stores/settings.svelte";
  import { toastError } from "../../lib/stores/toasts.svelte";
  import type { SectionProps } from "./props";
  import ConfirmDialog from "../ui/ConfirmDialog.svelte";
  import Toggle from "./Toggle.svelte";

  let { save }: SectionProps = $props();
  let hosts = $state<KnownHostEntry[] | null>(null);
  let clearing = $state(false);

  const load = () =>
    api
      .knownHostsList()
      .then((list) => (hosts = list))
      .catch(toastError);
  onMount(() => void load());

  const name = (h: string, port: number) => (port === 22 ? h : `${h}:${port}`);

  async function forget(h: string, port: number) {
    try {
      await api.knownHostForget(h, port);
      await load();
    } catch (e) {
      toastError(e);
    }
  }

  async function clearAll() {
    clearing = false;
    try {
      await api.knownHostsClear();
      await load();
    } catch (e) {
      toastError(e);
    }
  }
</script>

<Toggle
  label={t("settings.confirmClose")}
  hint={t("settings.confirmCloseHint")}
  checked={settings.confirm_close_active}
  onchange={(v) => save({ confirm_close_active: v })}
/>

<h4>{t("settings.knownHosts")}</h4>
{#if hosts?.length === 0}
  <p class="hint">{t("settings.knownHostsEmpty")}</p>
{:else if hosts}
  <ul>
    {#each hosts as h, i (i)}
      <li>
        <div class="entry">
          <strong>
            {h.host === null
              ? t("settings.knownHostsEncrypted")
              : name(h.host, h.port)}
          </strong>
          <span class="hint">{h.algo} · <code>{h.fingerprint}</code></span>
        </div>
        {#if h.host !== null}
          {@const host = h.host}
          <button
            type="button"
            class="danger"
            aria-label="{t('common.remove')} {name(host, h.port)}"
            onclick={() => forget(host, h.port)}>{t("common.remove")}</button
          >
        {/if}
      </li>
    {/each}
  </ul>
  <button type="button" class="danger clear" onclick={() => (clearing = true)}
    >{t("settings.knownHostsClear")}</button
  >
{/if}
<p class="hint">{t("settings.knownHostsNote")}</p>

{#if clearing}
  <ConfirmDialog
    title={t("settings.knownHostsClearTitle")}
    message={t("settings.knownHostsClearBody", { n: hosts?.length ?? 0 })}
    confirmLabel={t("settings.knownHostsClear")}
    danger
    onconfirm={clearAll}
    oncancel={() => (clearing = false)}
  />
{/if}

<style>
  h4 {
    margin: 0;
    font-size: 14px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    max-height: 220px;
    overflow-y: auto;
  }
  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    background: var(--surface-2);
    border-radius: var(--radius-md);
  }
  .clear {
    align-self: flex-start;
  }
  .entry {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  code {
    overflow-wrap: anywhere;
  }
  .hint {
    margin: 0;
    color: var(--fg-muted);
    font-size: 12px;
  }
</style>
