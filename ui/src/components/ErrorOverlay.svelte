<script lang="ts">
  import { api } from "../lib/api";
  import type { ApiError } from "../lib/bindings/ApiError";
  import { errorMessage, t } from "../lib/i18n";
  import { modal } from "../lib/modal";
  import { keygenHint, type TabState } from "../lib/sessionReducer";
  import { toastError } from "../lib/stores/toasts.svelte";

  let {
    error,
    mismatch,
    onretry,
    onedit,
  }: {
    error: ApiError;
    mismatch: TabState["mismatch"];
    onretry: () => void;
    onedit: () => void;
  } = $props();

  const uid = $props.id();

  async function forget(host: string, port: number) {
    try {
      await api.knownHostForget(host, port);
      onretry();
    } catch (e) {
      toastError(e);
    }
  }
</script>

<div class="tab-overlay">
  <div
    class="tab-panel"
    role="alertdialog"
    aria-modal="true"
    aria-labelledby="{uid}-title"
    aria-describedby="{uid}-body"
    use:modal={() => {}}
  >
    <h2 id="{uid}-title">{t("session.errorTitle")}</h2>
    <p id="{uid}-body">{errorMessage(error)}</p>
    {#if mismatch}
      <dl>
        <dt>{t("session.expected")}</dt>
        <dd><code>{mismatch.expected}</code></dd>
        <dt>{t("session.got")}</dt>
        <dd><code>{mismatch.got}</code></dd>
      </dl>
      {#if mismatch.source === "OpenSsh"}
        <p>{t("session.opensshHint")}</p>
        <pre><code>{keygenHint(mismatch.host, mismatch.port)}</code></pre>
      {/if}
    {/if}
    <div class="actions">
      <button type="button" onclick={onedit}>{t("session.edit")}</button>
      {#if mismatch?.source === "Pylentra"}
        <button
          type="button"
          class="danger"
          onclick={() => forget(mismatch.host, mismatch.port)}
          >{t("session.forget")}</button
        >
      {/if}
      <button type="button" class="primary" data-autofocus onclick={onretry}
        >{t("session.retry")}</button
      >
    </div>
  </div>
</div>

<style>
  dd {
    margin: 0 0 8px;
  }
  dt {
    color: var(--fg-muted);
  }
  pre {
    margin: 0;
    padding: var(--space-3);
    background: var(--surface-2);
    border-radius: var(--radius-sm);
    white-space: pre-wrap;
    user-select: all;
  }
</style>
