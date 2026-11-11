<script lang="ts">
  import { api } from "../lib/api";
  import type { SkipReason } from "../lib/bindings/SkipReason";
  import type { Skipped } from "../lib/bindings/Skipped";
  import { errorMessage, t } from "../lib/i18n";
  import type { PaneState } from "../lib/sessionReducer";
  import { dismissTransfer } from "../lib/stores/sessions.svelte";
  import { toastError } from "../lib/stores/toasts.svelte";

  let { pane }: { pane: PaneState } = $props();

  // Integer keys come out in id order: the oldest first.
  const rows = $derived(Object.entries(pane.transfers));

  function cancel(id: number) {
    if (pane.sessionId)
      api.sftpCancel(pane.sessionId, id).catch((e) => void toastError(e));
  }

  function summary(files: number, skipped: number): string {
    const f = files === 1 ? t("sftp.filesOne") : t("sftp.files", { n: files });
    if (!skipped) return f;
    const s =
      skipped === 1 ? t("sftp.skippedOne") : t("sftp.skipped", { n: skipped });
    return `${f}, ${s}`;
  }

  const reason = (r: SkipReason) =>
    r.type === "Error"
      ? t("sftp.skip.Error", { error: errorMessage(r.error) })
      : t(`sftp.skip.${r.type}` as const);
</script>

{#snippet outcome(files: number, skipped: Skipped[])}
  {#if skipped.length}
    <details>
      <summary>{summary(files, skipped.length)}</summary>
      <ul class="skipped">
        {#each skipped as x, i (i)}
          <li>{x.path}: {reason(x.reason)}</li>
        {/each}
      </ul>
    </details>
  {:else}
    <span>{summary(files, 0)}</span>
  {/if}
{/snippet}

{#if rows.length}
  <ul class="transfers" aria-label={t("sftp.transfers")}>
    {#each rows as [key, tr] (key)}
      {@const id = Number(key)}
      {@const s = tr.state}
      <li>
        <div class="head">
          <span class="name" title={tr.name}>{tr.name}</span>
          {#if s.type === "Queued" || s.type === "Running" || s.type === "Conflict"}
            <button type="button" class="small" onclick={() => cancel(id)}
              >{t("common.cancel")}</button
            >
          {:else}
            <button
              type="button"
              class="small"
              aria-label={t("common.dismiss")}
              title={t("common.dismiss")}
              onclick={() => dismissTransfer(pane.paneId, id)}>×</button
            >
          {/if}
        </div>
        {#if s.type === "Queued"}
          <span class="note">{t("sftp.queued")}</span>
        {:else if s.type === "Conflict"}
          <span class="note">{t("sftp.waiting")}</span>
        {:else if s.type === "Running"}
          <progress
            aria-label={tr.name}
            value={s.done_bytes}
            max={s.total_bytes || 1}
          ></progress>
          {#if s.current !== tr.name}
            <span class="note" title={s.current}>{s.current}</span>
          {/if}
        {:else if s.type === "Finished"}
          {@render outcome(s.files, s.skipped)}
        {:else if s.type === "Failed"}
          <span class="error">{errorMessage(s.error)}</span>
          {@render outcome(s.files, s.skipped)}
        {:else}
          <span>{t("sftp.cancelled")} · {summary(s.files, 0)}</span>
        {/if}
      </li>
    {/each}
  </ul>
{/if}

<style>
  .transfers {
    flex: none;
    max-height: 40%;
    overflow-y: auto;
    margin: 0;
    padding: 0;
    list-style: none;
    border-top: 1px solid var(--border);
  }
  .transfers > li {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-2) 0;
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .name,
  .note {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name {
    flex: 1;
    min-width: 0;
    font-weight: 600;
  }
  .small {
    flex: none;
    padding: 0 var(--space-2);
  }
  .note {
    color: var(--fg-muted);
  }
  .error {
    color: var(--danger);
  }
  progress {
    width: 100%;
    accent-color: var(--accent);
  }
  .skipped {
    margin: var(--space-1) 0 0;
    padding-left: var(--space-4);
    color: var(--fg-muted);
    overflow-wrap: anywhere;
  }
</style>
