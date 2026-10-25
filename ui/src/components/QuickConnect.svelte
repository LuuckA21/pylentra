<script lang="ts">
  import { t } from "../lib/i18n";
  import { parseTarget } from "../lib/quickTarget";
  import { matches } from "../lib/search";
  import { fromOtherModal, shortcutFor } from "../lib/shortcuts";
  import { library } from "../lib/stores/library.svelte";
  import { openQuickTab } from "../lib/stores/sessions.svelte";
  import { quickDefaults } from "../lib/stores/settings.svelte";
  import TextField from "./ui/TextField.svelte";

  let { onopen }: { onopen: (connectionId: string) => void } = $props();

  const uid = $props.id();
  const listId = `${uid}-list`;
  const errId = `${uid}-err`;
  const optId = (i: number) => `${uid}-opt-${i}`;

  let value = $state("");
  let inputEl = $state<HTMLInputElement>();
  let dismissed = $state(false);
  let active = $state(-1);
  let error = $state<string | null>(null);

  const text = $derived(value.trim());
  const suggestions = $derived(
    text ? library.connections.filter((c) => matches(c, text)).slice(0, 6) : [],
  );
  // The last option is "Connect to <text>".
  const count = $derived(suggestions.length + 1);
  const open = $derived(!!text && !dismissed);

  export function focus() {
    inputEl?.focus();
    inputEl?.select();
  }

  function reset() {
    value = "";
    active = -1;
    error = null;
  }

  function connectTyped() {
    const r = parseTarget(text, quickDefaults());
    if (!r.ok) return void (error = t(r.error));
    openQuickTab(r.target);
    reset();
  }

  function choose(i: number) {
    if (i < suggestions.length) {
      onopen(suggestions[i].id);
      reset();
    } else connectTyped();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      dismissed = false;
      // -1 is "no selection" (Enter then connects to the typed text).
      active =
        e.key === "ArrowDown"
          ? Math.min(active + 1, count - 1)
          : Math.max(active - 1, -1);
    } else if (e.key === "Enter" && text) {
      e.preventDefault();
      choose(active < 0 ? suggestions.length : active);
    } else if (e.key === "Escape" && open) {
      e.preventDefault();
      dismissed = true;
      active = -1;
    }
  }
</script>

<svelte:window
  onkeydown={(e) => {
    if (shortcutFor(e) === "quickConnect" && !fromOtherModal(e)) {
      e.preventDefault();
      focus();
    }
  }}
/>

<div
  class="quick"
  onfocusout={(e) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node)) dismissed = true;
  }}
>
  <TextField
    icon="bolt"
    label={t("quick.label")}
    placeholder={t("quick.placeholder")}
    bind:value
    bind:inputEl
    {onkeydown}
    oninput={() => {
      dismissed = false;
      active = -1;
      error = null;
    }}
    role="combobox"
    autocomplete="off"
    spellcheck={false}
    aria-autocomplete="list"
    aria-expanded={open}
    aria-controls={listId}
    aria-activedescendant={open && active >= 0 ? optId(active) : undefined}
    aria-invalid={error ? "true" : undefined}
    aria-describedby={error ? errId : undefined}
  />
  {#if error}<p class="error" id={errId} role="alert">{error}</p>{/if}
  <!-- The listbox stays in the DOM (hidden) so aria-controls always resolves. -->
  <ul id={listId} role="listbox" aria-label={t("quick.label")} hidden={!open}>
    {#if open}
      {#each [...suggestions, null] as c, i (c?.id ?? "typed")}
        <!-- Keyboard use goes through the combobox input. -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          id={optId(i)}
          role="option"
          aria-selected={i === active}
          class:active={i === active}
          onmousedown={(e) => e.preventDefault()}
          onclick={() => choose(i)}
        >
          {#if c}
            <span class="name">{c.name}</span>
            <span class="addr">{c.username}@{c.host}</span>
          {:else}
            {t("quick.connectTo", { text })}
          {/if}
        </li>
      {/each}
    {/if}
  </ul>
</div>

<style>
  .quick {
    position: relative;
    width: 100%;
    max-width: 360px;
    text-align: left;
  }
  .error {
    margin: var(--space-1) 0 0;
    color: var(--danger);
    font-size: 12px;
  }
  ul {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    z-index: 20;
    margin: var(--space-1) 0 0;
    padding: var(--space-1);
    list-style: none;
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow);
  }
  ul[hidden] {
    display: none;
  }
  li {
    display: flex;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  li:hover,
  li.active {
    background: var(--surface-3);
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
