<script lang="ts">
  import type { Snippet } from "../lib/bindings/Snippet";
  import { errorMessage, t } from "../lib/i18n";
  import { modal } from "../lib/modal";
  import { library } from "../lib/stores/library.svelte";
  import { settings } from "../lib/stores/settings.svelte";
  import { saveSnippet } from "../lib/stores/snippets.svelte";
  import { isApiError, toastError } from "../lib/stores/toasts.svelte";
  import { body } from "../lib/snippets";
  import Toggle from "./settings/Toggle.svelte";

  const NAME_MAX = 80;
  const TEXT_MAX_BYTES = 64 * 1024;

  let {
    initial,
    preset,
    onclose,
    onsaved,
  }: {
    initial?: Snippet;
    preset?: { text?: string; connectionId: string | null };
    onclose: () => void;
    onsaved?: () => void;
  } = $props();

  // The form edits a copy; the props are only read when the dialog opens.
  // svelte-ignore state_referenced_locally
  const id = initial?.id ?? crypto.randomUUID();
  // svelte-ignore state_referenced_locally
  const start = { ...preset, ...initial };
  let name = $state(start.name ?? "");
  let text = $state(start.text ?? "");
  let run = $state(start.run ?? false);
  // An orphan (its connection was deleted) turns global when saved.
  const wanted = start.connection_id ?? start.connectionId ?? "";
  let scope = $state(
    library.connections.some((c) => c.id === wanted) ? wanted : "",
  );
  let errors = $state<Record<string, string>>({});
  let saving = $state(false);
  let formEl: HTMLFormElement | undefined = $state();
  const uid = $props.id();

  const connections = $derived(
    [...library.connections].sort((a, b) => a.name.localeCompare(b.name)),
  );
  const eid = (field: string) => `${uid}-err-${field}`;
  const focusField = (field: string) =>
    formEl?.querySelector<HTMLElement>(`[data-field="${field}"]`);

  function validate(): Record<string, string> {
    const found: Record<string, string> = {};
    if (!name.trim()) found.name = t("snippet.required");
    else if ([...name.trim()].length > NAME_MAX)
      found.name = t("snippet.tooLong", { max: NAME_MAX });
    if (!text.trim()) found.text = t("snippet.required");
    else if (new TextEncoder().encode(body(text)).length > TEXT_MAX_BYTES)
      found.text = t("snippet.textTooLong");
    return found;
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    errors = validate();
    const [first] = Object.keys(errors);
    if (first) return focusField(first)?.focus();

    saving = true;
    try {
      await saveSnippet({
        id,
        name: name.trim(),
        text: body(text),
        run,
        ...(scope && { connection_id: scope }),
      });
      onsaved?.();
      onclose();
    } catch (err) {
      const field =
        isApiError(err) && err.code === "config.invalid"
          ? String(err.params.field)
          : "";
      const el = ["name", "text", "connection_id"].includes(field)
        ? focusField(field)
        : null;
      if (el && isApiError(err)) {
        errors = { [field]: errorMessage(err) };
        el.focus();
      } else toastError(err);
    } finally {
      saving = false;
    }
  }
</script>

{#snippet error(field: string)}
  {#if errors[field]}
    <p class="error" id={eid(field)}>{errors[field]}</p>
  {/if}
{/snippet}

<div class="backdrop">
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="{uid}-title"
    use:modal={onclose}
  >
    <h2 id="{uid}-title">{initial ? t("snippet.edit") : t("snippet.new")}</h2>
    <form bind:this={formEl} onsubmit={submit} novalidate>
      <div class="field">
        <label for="{uid}-name">{t("snippet.name")}</label>
        <input
          id="{uid}-name"
          data-field="name"
          bind:value={name}
          aria-invalid={errors.name ? "true" : undefined}
          aria-describedby={errors.name ? eid("name") : undefined}
        />
        {@render error("name")}
      </div>
      <div class="field">
        <label for="{uid}-text">{t("snippet.text")}</label>
        <textarea
          id="{uid}-text"
          data-field="text"
          rows="6"
          spellcheck="false"
          style:font-family={settings.font_family}
          bind:value={text}
          aria-invalid={errors.text ? "true" : undefined}
          aria-describedby={errors.text ? eid("text") : undefined}></textarea>
        {@render error("text")}
      </div>
      <div class="field">
        <label for="{uid}-scope">{t("snippet.scope")}</label>
        <select
          id="{uid}-scope"
          data-field="connection_id"
          bind:value={scope}
          aria-invalid={errors.connection_id ? "true" : undefined}
          aria-describedby={errors.connection_id
            ? eid("connection_id")
            : undefined}
        >
          <option value="">{t("snippet.scopeAll")}</option>
          {#each connections as c (c.id)}
            <option value={c.id}>{c.name}</option>
          {/each}
        </select>
        {@render error("connection_id")}
      </div>
      <Toggle
        label={t("snippet.run")}
        hint={t("snippet.runHint")}
        checked={run}
        onchange={(v) => (run = v)}
      />
      <div class="actions">
        <button type="button" onclick={onclose}>{t("common.cancel")}</button>
        <button type="submit" class="primary" disabled={saving}
          >{t("common.save")}</button
        >
      </div>
    </form>
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
    width: min(560px, calc(100vw - 32px));
    max-height: calc(100vh - 32px);
    overflow-y: auto;
    padding: var(--space-6);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-xl);
    box-shadow: var(--shadow);
  }
  h2 {
    margin: 0 0 8px;
    font-size: 16px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 8px;
  }
  textarea {
    resize: vertical;
    padding: var(--space-2) var(--space-3);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    color: var(--fg);
  }
  textarea[aria-invalid="true"] {
    border-color: var(--danger);
  }
  .error {
    margin: 0;
    color: var(--danger);
    font-size: 12px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: var(--space-4);
  }
</style>
