<script lang="ts">
  import { onMount } from "svelte";
  import type { Snippet } from "../lib/bindings/Snippet";
  import { t } from "../lib/i18n";
  import { body, firstLine, groupSnippets } from "../lib/snippets";
  import { isSnippetsReadOnly, library } from "../lib/stores/library.svelte";
  import { snippets } from "../lib/stores/snippets.svelte";
  import Pill from "./ui/Pill.svelte";
  import TextField from "./ui/TextField.svelte";

  // Anchored above the status bar by the parent, which owns what picking does.
  let {
    connectionId,
    connectionName = "",
    selection,
    onpick,
    onnew,
    onclose,
  }: {
    connectionId: string | null;
    connectionName?: string;
    selection: string;
    onpick: (s: Snippet) => void;
    onnew: (text: string) => void;
    onclose: () => void;
  } = $props();

  const uid = $props.id();
  const listId = `${uid}-list`;
  const optId = (i: number) => `${uid}-opt-${i}`;

  let query = $state("");
  let inputEl = $state<HTMLInputElement>();
  let active = $state(0);

  const groups = $derived(
    groupSnippets(
      snippets.list,
      query,
      connectionId,
      new Set(library.connections.map((c) => c.id)),
    ),
  );
  const flat = $derived([...groups.own, ...groups.global]);
  const sections = $derived([
    { label: connectionName, items: groups.own, offset: 0 },
    {
      label: t("snippet.global"),
      items: groups.global,
      offset: groups.own.length,
    },
  ]);

  onMount(() => inputEl?.focus());

  $effect(() => {
    document
      .getElementById(optId(active))
      ?.scrollIntoView?.({ block: "nearest" });
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      active = Math.max(
        0,
        Math.min(active + (e.key === "ArrowDown" ? 1 : -1), flat.length - 1),
      );
    } else if (e.key === "Enter" && flat[active]) {
      e.preventDefault();
      onpick(flat[active]);
    }
  }
</script>

<div
  class="panel"
  role="dialog"
  tabindex="-1"
  aria-label={t("snippet.palette")}
  onkeydown={(e) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      onclose();
    }
  }}
>
  <TextField
    icon="search"
    label={t("snippet.search")}
    placeholder={t("snippet.search")}
    bind:value={query}
    bind:inputEl
    {onkeydown}
    oninput={() => (active = 0)}
    role="combobox"
    autocomplete="off"
    spellcheck={false}
    aria-autocomplete="list"
    aria-expanded="true"
    aria-controls={listId}
    aria-activedescendant={flat.length ? optId(active) : undefined}
  />
  <div id={listId} role="listbox" aria-label={t("snippet.palette")}>
    {#each sections as { label, items, offset }, i (i)}
      {#if items.length}
        <div role="group" aria-label={label}>
          <div class="head" aria-hidden="true">{label}</div>
          {#each items as s, j (s.id)}
            <!-- Keyboard use goes through the combobox input. -->
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              id={optId(offset + j)}
              role="option"
              tabindex="-1"
              aria-selected={offset + j === active}
              class="opt"
              class:active={offset + j === active}
              onmousedown={(e) => e.preventDefault()}
              onclick={() => onpick(s)}
            >
              <span class="name">{s.name}</span>
              {#if s.run}<Pill>{t("snippet.runs")}</Pill>{/if}
              <code>{firstLine(body(s.text))}</code>
            </div>
          {/each}
        </div>
      {/if}
    {/each}
    {#if !flat.length}
      <p class="muted">
        {t(snippets.list.length ? "snippet.noMatches" : "snippet.empty")}
      </p>
    {/if}
  </div>
  <footer>
    <button
      type="button"
      disabled={isSnippetsReadOnly()}
      onclick={() => onnew("")}>{t("snippet.new")}</button
    >
    {#if selection.trim()}
      <button
        type="button"
        disabled={isSnippetsReadOnly()}
        onclick={() => onnew(selection)}>{t("snippet.saveSelection")}</button
      >
    {/if}
  </footer>
</div>

<style>
  .panel {
    position: absolute;
    left: var(--space-3);
    bottom: calc(100% + var(--space-2));
    z-index: 5;
    width: min(520px, calc(100% - 2 * var(--space-3)));
    padding: var(--space-3) var(--space-4);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
  }
  [role="listbox"] {
    max-height: 40vh;
    overflow-y: auto;
    margin: var(--space-2) 0;
  }
  .head {
    padding: var(--space-1) var(--space-2);
    color: var(--fg-muted);
    font-size: 12px;
    font-weight: 600;
  }
  .opt {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .opt:hover,
  .opt.active {
    background: var(--surface-3);
  }
  .name {
    font-weight: 600;
    white-space: nowrap;
  }
  code {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--fg-muted);
  }
  .muted {
    margin: var(--space-2);
    color: var(--fg-muted);
  }
  footer {
    display: flex;
    gap: var(--space-2);
  }
</style>
