<script lang="ts">
  import type { Snippet } from "svelte";
  import { colorOf } from "../lib/appearance";
  import { t } from "../lib/i18n";
  import { recentIds } from "../lib/recent";
  import { library } from "../lib/stores/library.svelte";
  import Avatar from "./ui/Avatar.svelte";

  let { onopen, quick }: { onopen: (id: string) => void; quick?: Snippet } =
    $props();

  const recent = $derived.by(() => {
    const byId = new Map(library.connections.map((c) => [c.id, c]));
    return recentIds(new Set(byId.keys())).map((id) => byId.get(id)!);
  });
</script>

<section class="welcome">
  <h1>{t("welcome.title")}</h1>
  <p class="hint">{t("main.placeholder")}</p>
  {@render quick?.()}
  {#if recent.length}
    <h2>{t("welcome.recent")}</h2>
    <ul>
      {#each recent as c (c.id)}
        <li>
          <button type="button" onclick={() => onopen(c.id)}>
            <Avatar
              name={c.name}
              icon={c.icon}
              color={colorOf(c, library.groups)}
            />
            <span class="name">{c.name}</span>
            <span class="addr">{c.username}@{c.host}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .welcome {
    margin: auto;
    width: min(440px, 100%);
    padding: var(--space-6);
    text-align: center;
  }
  h1 {
    margin: 0 0 var(--space-1);
    font-size: 22px;
  }
  h2 {
    margin: var(--space-6) 0 var(--space-2);
    font-size: 13px;
    color: var(--fg-muted);
    text-align: left;
  }
  .hint {
    margin: 0 0 var(--space-4);
    color: var(--fg-muted);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li button {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-2) var(--space-3);
    border: none;
    border-radius: var(--radius-md);
    background: var(--surface-1);
    text-align: left;
  }
  li button:hover {
    background: var(--surface-2);
  }
  .name {
    font-weight: 600;
  }
  .addr {
    margin-left: auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--fg-muted);
  }
</style>
