<script lang="ts">
  import type { Connection } from "../lib/bindings/Connection";
  import type { Group } from "../lib/bindings/Group";
  import { t } from "../lib/i18n";
  import type { TabState } from "../lib/sessionReducer";
  import Pill from "./ui/Pill.svelte";

  let {
    tab,
    connection,
    groups,
    onforwards,
  }: {
    tab: TabState;
    connection: Connection | undefined;
    groups: Group[];
    onforwards: () => void;
  } = $props();

  const group = $derived(groups.find((g) => g.id === connection?.group_id));
  const active = $derived(
    Object.entries(tab.forwards).filter(([, f]) => f.state.type === "Active"),
  );

  // The duration only changes by the minute, so one tick a minute is enough.
  let now = $state(Date.now());
  $effect(() => {
    const id = setInterval(() => (now = Date.now()), 60_000);
    return () => clearInterval(id);
  });

  const connected = $derived.by(() => {
    if (tab.phase !== "ready" || tab.readyAt === undefined) return null;
    const min = Math.max(0, Math.floor((now - tab.readyAt) / 60_000));
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
      <Pill color="teal" onclick={onforwards}
        >⇄ {f.state.bound_port} → {f.rule.dest_host}:{f.rule.dest_port}</Pill
      >
    {/if}
  {/each}
  <button
    type="button"
    class="fwd"
    disabled={tab.phase !== "ready"}
    onclick={onforwards}>{t("status.forward")}</button
  >
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
    padding: 2px var(--space-3);
    border-radius: 999px;
    font-size: 12px;
    color: var(--fg-muted);
  }
  .time {
    white-space: nowrap;
  }
</style>
