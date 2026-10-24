<script lang="ts">
  import Icon, { type UiIcon } from "./Icon.svelte";

  let {
    icon,
    label,
    placeholder,
    value = $bindable(),
    onkeydown,
    inputEl = $bindable(),
  }: {
    icon: UiIcon;
    label: string;
    placeholder?: string;
    value: string;
    onkeydown?: (e: KeyboardEvent) => void;
    inputEl?: HTMLInputElement;
  } = $props();
</script>

<div class="field">
  <Icon name={icon} />
  <input
    bind:this={inputEl}
    bind:value
    aria-label={label}
    {placeholder}
    {onkeydown}
  />
</div>

<style>
  .field {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-3);
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-md);
    color: var(--fg-muted);
  }

  .field:focus-within {
    border-color: var(--accent);
  }

  input {
    flex: 1;
    min-width: 0;
    padding: var(--space-2) 0;
    background: none;
    border: 0;
    color: var(--fg);
  }

  input:focus-visible {
    outline: none;
  }
</style>
