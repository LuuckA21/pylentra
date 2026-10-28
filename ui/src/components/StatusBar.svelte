<script lang="ts">
  import type { Connection } from "../lib/bindings/Connection";
  import type { Group } from "../lib/bindings/Group";
  import { t } from "../lib/i18n";
  import type { Dir } from "../lib/layout";
  import type { PaneState } from "../lib/sessionReducer";
  import Icon from "./ui/Icon.svelte";
  import Pill from "./ui/Pill.svelte";

  let {
    pane,
    connection,
    groups,
    onforwards,
    forwardsOpen = false,
    onsnippets,
    snippetsOpen = false,
    onsave,
    onsplit,
    canSplit = false,
  }: {
    pane: PaneState;
    connection: Connection | undefined;
    groups: Group[];
    onforwards: () => void;
    forwardsOpen?: boolean;
    onsnippets?: () => void;
    snippetsOpen?: boolean;
    /** Only for quick (unsaved) tabs. */
    onsave?: () => void;
    onsplit?: (dir: Dir) => void;
    // False when the tab holds as many panes as it can.
    canSplit?: boolean;
  } = $props();

  const group = $derived(groups.find((g) => g.id === connection?.group_id));
  const active = $derived(
    Object.entries(pane.forwards).filter(([, f]) => f.state.type === "Active"),
  );

  // The duration only changes by the minute, so one tick a minute is enough.
  let now = $state(Date.now());
  $effect(() => {
    const id = setInterval(() => (now = Date.now()), 60_000);
    return () => clearInterval(id);
  });

  const connected = $derived.by(() => {
    if (pane.phase !== "ready" || pane.readyAt === undefined) return null;
    const min = Math.max(0, Math.floor((now - pane.readyAt) / 60_000));
    if (min < 1) return t("status.lessThanMinute");
    return min < 60
      ? t("status.minutes", { n: min })
      : t("status.hours", { n: Math.floor(min / 60) });
  });
</script>

<div class="bar">
  {#if group}<Pill color={group.color ?? null}>{group.name}</Pill>{/if}
  {#each active as [id, f] (id)}
    {#if f.state.type === "Active"}
      <Pill color="teal" pressed={forwardsOpen} onclick={onforwards}
        >⇄ {f.rule.type === "Dynamic"
          ? t("fwd.dynamicPill", { port: f.state.bound_port })
          : `${f.state.bound_port} → ${f.rule.dest_host}:${f.rule.dest_port}`}</Pill
      >
    {/if}
  {/each}
  <button
    type="button"
    class="fwd"
    disabled={pane.phase !== "ready"}
    aria-expanded={forwardsOpen}
    onclick={onforwards}>{t("status.forward")}</button
  >
  {#if onsnippets}
    <button
      type="button"
      class="fwd"
      disabled={pane.phase !== "ready"}
      aria-expanded={snippetsOpen}
      onclick={onsnippets}>{t("snippet.button")}</button
    >
  {/if}
  {#if onsave}
    <button type="button" class="fwd" onclick={onsave}>{t("quick.save")}</button
    >
  {/if}
  {#if onsplit}
    <!-- Icons: a pane can be a quarter of the tab, the bar has little room. -->
    <button
      type="button"
      class="fwd split"
      disabled={!canSplit}
      aria-label={t("pane.splitRight")}
      title={t("pane.splitRight")}
      onclick={() => onsplit("row")}
      ><Icon name="split-right" size={14} /></button
    >
    <button
      type="button"
      class="fwd split"
      disabled={!canSplit}
      aria-label={t("pane.splitDown")}
      title={t("pane.splitDown")}
      onclick={() => onsplit("column")}
      ><Icon name="split-down" size={14} /></button
    >
  {/if}
  <span class="gap"></span>
  {#if connected}
    <span class="time">{t("status.connectedFor")} <span>{connected}</span></span
    >
  {/if}
</div>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-4);
    background: var(--surface-2);
    color: var(--fg-muted);
    font-size: 12.5px;
  }
  .gap {
    flex: 1;
  }
  .fwd {
    flex: none;
    white-space: nowrap;
    padding: 2px var(--space-3);
    border-radius: 999px;
    font-size: 12px;
    color: var(--fg-muted);
  }
  .split {
    display: grid;
    padding: 3px var(--space-2);
  }
  .time {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
