<script lang="ts">
  import type { Snippet } from "svelte";
  import type { TagColor } from "../../lib/bindings/TagColor";

  let {
    color = null,
    pressed = false,
    onclick,
    children,
  }: {
    color?: TagColor | null;
    pressed?: boolean;
    onclick?: () => void;
    children?: Snippet;
  } = $props();
</script>

{#if onclick}
  <button
    type="button"
    class="pill tag-{color ?? 'none'}"
    aria-pressed={pressed}
    {onclick}
  >
    {@render children?.()}
  </button>
{:else}
  <span class="pill tag-{color ?? 'none'}">{@render children?.()}</span>
{/if}

<style>
  .pill {
    display: inline-flex;
    align-items: center;
    padding: 2px var(--space-2);
    border: 1px solid transparent;
    border-radius: 999px;
    background: var(--tag-bg);
    color: var(--tag-fg);
    font-size: 12px;
    font-weight: 600;
  }

  button.pill:hover {
    background: var(--tag-bg);
    border-color: var(--tag-solid);
  }

  button.pill[aria-pressed="true"] {
    border-color: var(--tag-fg);
  }
</style>
