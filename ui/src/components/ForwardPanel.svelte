<script lang="ts">
  import { untrack } from "svelte";
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
      r.type !== "Remote" ? r.bind_host : "",
      r.bind_port,
      r.type === "Dynamic" ? "" : r.dest_host,
      r.type === "Dynamic" ? "" : r.dest_port,
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
      : r.type === "Remote"
        ? `R ${r.bind_port} → ${r.dest_host}:${r.dest_port}`
        : t("fwd.dynamicRow", { bind: `${r.bind_host}:${r.bind_port}` });

  // Where a client connects: a wildcard bind is reached through loopback.
  const proxyHost = (h: string) =>
    h === "0.0.0.0" || h === "::"
      ? "127.0.0.1"
      : h.includes(":")
        ? `[${h}]`
        : h;

  const usable = $derived(!!tab.sessionId && tab.phase === "ready");

  // Opened by the user: focus moves in, and back to the opener on close. Opened by
  // a failing forward: focus stays where the user is typing.
  let panel = $state<HTMLElement>();
  let autoOpened = false;
  let returnTo: HTMLElement | null = null;
  $effect(() => {
    if (!open || !panel) return;
    if (!autoOpened && document.activeElement instanceof HTMLElement) {
      returnTo = document.activeElement;
      panel.focus();
    }
    autoOpened = false;
  });
  function close() {
    onclose();
    if (returnTo?.isConnected) returnTo.focus();
    returnTo = null;
  }

  // A forward that newly fails opens the panel once; closing it stays closed.
  // Plain Set on purpose: a reactive one would re-run the effect that fills it.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity
  const seenFailed = new Set<number>();
  $effect(() => {
    let fresh = false;
    for (const r of rows)
      if (
        r.state.type === "Failed" &&
        r.id !== undefined &&
        !seenFailed.has(r.id)
      ) {
        seenFailed.add(r.id);
        fresh = true;
      }
    if (fresh && !untrack(() => open)) {
      autoOpened = true;
      open = true;
    }
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
    type: "Local" as ForwardRule["type"],
    bind_host: "127.0.0.1",
    bind_port: null as number | null,
    dest_host: "localhost",
    dest_port: null as number | null,
  });
  let adding = $state(false);
  let draft = $state(blank());
  // Same defaults as the form's "+ SOCKS" row.
  function setType(type: ForwardRule["type"]) {
    draft.type = type;
    if (type === "Dynamic" && draft.bind_port === null) draft.bind_port = 1080;
  }
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
    if (d.type !== "Dynamic" && !d.dest_host.trim())
      found.dest_host = "form.error.required";
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
        d.type === "Dynamic"
          ? {
              type: "Dynamic",
              bind_host: d.bind_host.trim(),
              bind_port: d.bind_port!,
              auto_start: false,
            }
          : d.type === "Local"
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
    bind:this={panel}
    onkeydown={(e) => e.key === "Escape" && close()}
  >
    <header>
      <h2>{t("fwd.title")} ({rows.length})</h2>
      <button
        type="button"
        class="x"
        aria-label={t("common.close")}
        title={t("common.close")}
        onclick={close}><Icon name="close" size={14} /></button
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
          {#if row.rule.type === "Dynamic" && row.state.type === "Active"}
            <p class="muted hint">
              {t("fwd.dynamicHint", {
                host: proxyHost(row.rule.bind_host),
                port: row.state.bound_port,
              })}
            </p>
          {/if}
        </li>
      {/each}
    </ul>

    {#if adding}
      <form onsubmit={submit} novalidate>
        <div class="field">
          <label for="{uid}-type">{t("fwd.type")}</label>
          <select
            id="{uid}-type"
            value={draft.type}
            onchange={(e) =>
              setType(e.currentTarget.value as ForwardRule["type"])}
          >
            <option value="Local">{t("fwd.local")}</option>
            <option value="Remote">{t("fwd.remote")}</option>
            <option value="Dynamic">{t("fwd.dynamic")}</option>
          </select>
        </div>
        {#if draft.type !== "Remote"}
          {@render field("bind_host", "form.bindHost", false)}
        {/if}
        {@render field("bind_port", "form.bindPort", true)}
        {#if draft.type !== "Dynamic"}
          {@render field("dest_host", "form.destHost", false)}
          {@render field("dest_port", "form.destPort", true)}
        {/if}
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
    flex-wrap: wrap;
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
  .hint {
    flex-basis: 100%;
    margin: 0 0 4px;
    font-size: 12px;
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
