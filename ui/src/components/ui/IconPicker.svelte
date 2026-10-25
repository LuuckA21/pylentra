<script lang="ts">
  import type { ConnIcon } from "../../lib/bindings/ConnIcon";
  import { CONN_ICONS } from "../../lib/appearance";
  import { t } from "../../lib/i18n";
  import { radioKeydown } from "../../lib/radioKeys";
  import Icon from "./Icon.svelte";

  let {
    value = $bindable(),
    label,
  }: { value: ConnIcon | null; label: string } = $props();

  const options: (ConnIcon | null)[] = [null, ...CONN_ICONS];
</script>

<div role="radiogroup" aria-label={label} class="picker">
  {#each options as i (i ?? "initials")}
    <button
      type="button"
      role="radio"
      aria-checked={value === i}
      aria-label={i ? t(`icon.${i}`) : t("form.icon.initials")}
      tabindex={value === i ? 0 : -1}
      class="choice"
      onclick={() => (value = i)}
      onkeydown={radioKeydown}
    >
      {#if i}<Icon name={i} size={18} />{:else}Ab{/if}
    </button>
  {/each}
</div>

<style>
  .picker {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .choice {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    padding: 0;
    font-size: 13px;
    font-weight: 700;
  }
  .choice[aria-checked="true"] {
    border-color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent);
  }
</style>
