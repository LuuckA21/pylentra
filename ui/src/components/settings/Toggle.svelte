<script lang="ts">
  let {
    label,
    hint,
    checked,
    onchange,
  }: {
    label: string;
    hint?: string;
    checked: boolean;
    onchange: (v: boolean) => void;
  } = $props();

  const uid = $props.id();
</script>

<div class="row">
  <div>
    <span id="{uid}-label">{label}</span>
    {#if hint}<p class="hint" id="{uid}-hint">{hint}</p>{/if}
  </div>
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    aria-labelledby="{uid}-label"
    aria-describedby={hint ? `${uid}-hint` : undefined}
    onclick={() => onchange(!checked)}
  ></button>
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
  }
  .hint {
    margin: 0;
    color: var(--fg-muted);
    font-size: 12px;
  }
  [role="switch"] {
    flex: none;
  }
</style>
