<script lang="ts">
  import type { ForwardRule } from "../lib/bindings/ForwardRule";
  import type { ForwardState } from "../lib/bindings/ForwardState";
  import { errorMessage, t, type MessageKey } from "../lib/i18n";
  import type { TabState } from "../lib/sessionReducer";
  import { api } from "../lib/api";
  import { library } from "../lib/stores/library.svelte";
  import { isApiError, toastError } from "../lib/stores/toasts.svelte";
  import { validate } from "../lib/validate";
  import Icon from "./ui/Icon.svelte";

  // Anchored above the status bar by the parent; closes itself on a failed start.
  let {
    tab,
    open = $bindable(false),
    onclose,
  }: { tab: TabState; open?: boolean; onclose: () => void } = $props();

  type Row = { rule: ForwardRule; id?: number; state: ForwardState };
  const STOPPED: ForwardState = { type: "Stopped" };

  // auto_start is a saved-connection setting: it does not tell tunnels apart.
  const key = (r: ForwardRule) =>
    [
      r.type,
      r.type === "Local" ? r.bind_host : "",
      r.bind_port,
      r.dest_host,
      r.dest_port,
    ].join("|");

  // Saved rules first, then tunnels added at runtime. A rule that was started,
  // stopped and started again has several ids: the highest (latest) one counts.
  const rows = $derived.by(() => {
    const known: Record<string, Row> = {};
    for (const [id, f] of Object.entries(tab.forwards))
      known[key(f.rule)] = { rule: f.rule, id: Number(id), state: f.state };
    const saved =
      library.connections.find((c) => c.id === tab.connectionId)?.forwards ??
      [];
    const savedKeys = saved.map(key);
    return [
      ...saved.map((rule): Row => known[key(rule)] ?? { rule, state: STOPPED }),
      ...Object.entries(known)
        .filter(([k]) => !savedKeys.includes(k))
        .map(([, row]) => row),
    ];
  });
  const label = (r: ForwardRule) =>
    r.type === "Local"
      ? `L ${r.bind_host}:${r.bind_port} → ${r.dest_host}:${r.dest_port}`
      : `R ${r.bind_port} → ${r.dest_host}:${r.dest_port}`;

  const usable = $derived(!!tab.sessionId && tab.phase === "ready");

  // A failed (auto-)start must not hide in a closed panel.
  $effect(() => {
    if (rows.some((r) => r.state.type === "Failed")) open = true;
  });

  // A failed start is already shown on its row (the core sends a Forward event).
  const failed = (e: unknown) =>
    !(isApiError(e) && e.code.startsWith("forward.")) && toastError(e);

  async function toggle(row: Row) {
    if (!tab.sessionId) return;
    try {
      if (row.state.type === "Active" && row.id !== undefined)
        await api.forwardStop(tab.sessionId, row.id);
      else await api.forwardStart(tab.sessionId, row.rule);
    } catch (e) {
      failed(e);
    }
  }

  // The "Add tunnel" mini form. Tunnels live only in the session: the
  // connection is never saved from here.
  const blank = () => ({
    type: "Local" as "Local" | "Remote",
    bind_host: "127.0.0.1",
    bind_port: null as number | null,
    dest_host: "localhost",
    dest_port: null as number | null,
  });
  let adding = $state(false);
  let draft = $state(blank());
  let errors = $state<Record<string, string>>({});
  const num = (v: string) => (v === "" ? null : Number(v));
  const uid = $props.id();

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    const d = $state.snapshot(draft);
    const found: Record<string, MessageKey> = {};
    for (const [k, v] of Object.entries(
      validate({
        name: "-",
        host: "-",
        username: "-",
        port: 22,
        forwards: [d],
      }),
    ))
      found[k.replace("forwards[0].", "")] = v;
    if (!d.dest_host.trim()) found.dest_host = "form.error.required";
    errors = Object.fromEntries(
      Object.entries(found).map(([k, v]) => [k, t(v)]),
    );
    if (Object.keys(found).length || !tab.sessionId) return;

    const common = {
      bind_port: d.bind_port!,
      dest_host: d.dest_host.trim(),
      dest_port: d.dest_port!,
      auto_start: false,
    };
    try {
      await api.forwardStart(
        tab.sessionId,
        d.type === "Local"
          ? { type: "Local", bind_host: d.bind_host.trim(), ...common }
          : { type: "Remote", ...common },
      );
      adding = false;
      draft = blank();
    } catch (err) {
      failed(err);
    }
  }
</script>

{#snippet field(
  name: "bind_host" | "bind_port" | "dest_host" | "dest_port",
  text: MessageKey,
  numeric: boolean,
)}
  <div class="field">
    <label for="{uid}-{name}">{t(text)}</label>
    <input
      id="{uid}-{name}"
      type={numeric ? "number" : "text"}
      value={draft[name] ?? ""}
      aria-invalid={errors[name] ? "true" : undefined}
      aria-describedby={errors[name] ? `${uid}-${name}-err` : undefined}
      oninput={(e) => {
        const v = e.currentTarget.value;
        if (name === "bind_port" || name === "dest_port") draft[name] = num(v);
        else draft[name] = v;
      }}
    />
    {#if errors[name]}
      <p class="error" id="{uid}-{name}-err">{errors[name]}</p>
    {/if}
  </div>
{/snippet}

{#if open}
  <div
    class="panel"
    role="dialog"
    tabindex="-1"
    aria-label={t("fwd.title")}
    onkeydown={(e) => e.key === "Escape" && onclose()}
  >
    <header>
      <h2>{t("fwd.title")} ({rows.length})</h2>
      <button
        type="button"
        class="x"
        aria-label={t("common.close")}
        title={t("common.close")}
        onclick={onclose}><Icon name="close" size={14} /></button
      >
    </header>
    {#if !rows.length}<p class="muted">{t("fwd.empty")}</p>{/if}
    <ul>
      {#each rows as row, i (i)}
        <li>
          <code>{label(row.rule)}</code>
          <span
            class="state"
            class:ok={row.state.type === "Active"}
            class:bad={row.state.type === "Failed"}
          >
            {#if row.state.type === "Active"}
              {t("fwd.active", { port: row.state.bound_port })}
            {:else if row.state.type === "Failed"}
              {errorMessage(row.state.error)}
            {:else}
              {t("fwd.stopped")}
            {/if}
          </span>
          <button type="button" disabled={!usable} onclick={() => toggle(row)}>
            {t(row.state.type === "Active" ? "fwd.stop" : "fwd.start")}
          </button>
        </li>
      {/each}
    </ul>

    {#if adding}
      <form onsubmit={submit} novalidate>
        <div class="field">
          <label for="{uid}-type">{t("fwd.type")}</label>
          <select id="{uid}-type" bind:value={draft.type}>
            <option value="Local">{t("fwd.local")}</option>
            <option value="Remote">{t("fwd.remote")}</option>
          </select>
        </div>
        {#if draft.type === "Local"}
          {@render field("bind_host", "form.bindHost", false)}
        {/if}
        {@render field("bind_port", "form.bindPort", true)}
        {@render field("dest_host", "form.destHost", false)}
        {@render field("dest_port", "form.destPort", true)}
        <p class="muted">{t("fwd.sessionOnly")}</p>
        <div class="actions">
          <button type="button" onclick={() => (adding = false)}
            >{t("common.cancel")}</button
          >
          <button type="submit" class="primary" disabled={!usable}
            >{t("fwd.create")}</button
          >
        </div>
      </form>
    {:else}
      <button type="button" onclick={() => (adding = true)}
        >{t("fwd.add")}</button
      >
    {/if}
  </div>
{/if}

<style>
  .panel {
    position: absolute;
    left: var(--space-3);
    bottom: calc(100% + var(--space-2));
    z-index: 5;
    width: min(520px, calc(100% - 2 * var(--space-3)));
    max-height: 60vh;
    overflow-y: auto;
    padding: var(--space-3) var(--space-4);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2 {
    margin: 0;
    font-size: 14px;
  }
  .x {
    padding: var(--space-1);
    border: none;
    background: transparent;
    line-height: 0;
  }
  ul {
    margin: 4px 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 2px 0;
  }
  li code {
    flex: 1;
  }
  .state.ok {
    color: var(--ok);
  }
  .state.bad {
    color: var(--danger);
  }
  .muted {
    margin: 4px 0;
    color: var(--fg-muted);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 6px;
  }
  .error {
    margin: 0;
    color: var(--danger);
    font-size: 12px;
  }
  .actions {
    display: flex;
    gap: 8px;
  }
</style>
