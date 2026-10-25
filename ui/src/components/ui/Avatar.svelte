<script lang="ts">
  import type { ConnIcon } from "../../lib/bindings/ConnIcon";
  import type { TagColor } from "../../lib/bindings/TagColor";
  import { initialsOf } from "../../lib/appearance";
  import Icon from "./Icon.svelte";

  let {
    name,
    icon,
    color,
    size = 36,
  }: {
    name: string;
    icon?: ConnIcon;
    color: TagColor | null;
    size?: 28 | 36 | 44;
  } = $props();
</script>

<span
  class="avatar tag-{color ?? 'none'}"
  style:width="{size}px"
  style:height="{size}px"
  style:font-size="{Math.round(size * 0.38)}px"
>
  {#if icon}
    <Icon name={icon} size={Math.round(size * 0.55)} />
  {:else}
    {initialsOf(name)}
  {/if}
</span>

<style>
  .avatar {
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-sm);
    background: var(--tag-bg);
    color: var(--tag-fg);
    font-weight: 700;
    line-height: 1;
  }
</style>
