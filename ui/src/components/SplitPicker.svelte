<script lang="ts">
  import { onMount } from "svelte";
  import { t } from "../lib/i18n";
  import { modal } from "../lib/modal";
  import type { QuickTarget } from "../lib/quickTarget";
  import QuickConnect from "./QuickConnect.svelte";

  let {
    onpick,
    oncancel,
  }: {
    // A saved connection's id, or a typed target.
    onpick: (target: string | QuickTarget) => void;
    oncancel: () => void;
  } = $props();

  const uid = $props.id();
  let quick: QuickConnect;

  onMount(() => quick.focus());
</script>

<!-- A press outside the dialog cancels; the keyboard has Escape. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="backdrop"
  onmousedown={(e) => e.target === e.currentTarget && oncancel()}
>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="{uid}-title"
    tabindex="-1"
    use:modal
    onkeydown={(e) => {
      // Delegated, so it runs after the field's own handler: an Escape that
      // closed the suggestions does not also close the dialog.
      if (e.key === "Escape" && !e.defaultPrevented) {
        e.stopPropagation();
        oncancel();
      }
    }}
  >
    <h2 id="{uid}-title">{t("pane.splitTitle")}</h2>
    <QuickConnect bind:this={quick} onopen={onpick} onquick={onpick} />
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
    width: min(440px, calc(100vw - 32px));
    padding: var(--space-6);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-xl);
    box-shadow: var(--shadow);
  }
  h2 {
    margin: 0 0 12px;
    font-size: 16px;
  }
</style>
