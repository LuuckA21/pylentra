<script lang="ts">
  import type { TagColor } from "../../lib/bindings/TagColor";
  import { TAG_COLORS } from "../../lib/appearance";
  import { t } from "../../lib/i18n";
  import { radioKeydown } from "../../lib/radioKeys";

  let {
    value = $bindable(),
    noneLabel,
    label,
  }: { value: TagColor | null; noneLabel: string; label: string } = $props();

  const options: (TagColor | null)[] = [null, ...TAG_COLORS];
</script>

<div role="radiogroup" aria-label={label} class="picker">
  {#each options as c (c ?? "none")}
    <button
      type="button"
      role="radio"
      aria-checked={value === c}
      aria-label={c ? t(`color.${c}`) : noneLabel}
      tabindex={value === c ? 0 : -1}
      class="swatch tag-{c ?? 'none'}"
      class:empty={!c}
      onclick={() => (value = c)}
      onkeydown={radioKeydown}
    ></button>
  {/each}
</div>

<style>
  .picker {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .swatch {
    width: 28px;
    height: 28px;
    padding: 0;
    border: 2px solid transparent;
    border-radius: 999px;
    background: var(--tag-solid);
    box-shadow: inset 0 0 0 2px var(--surface-1);
  }
  .swatch.empty {
    background: transparent;
    border: 2px dashed var(--tag-solid);
    box-shadow: none;
  }
  .swatch[aria-checked="true"] {
    border: 2px solid var(--fg);
  }
</style>
