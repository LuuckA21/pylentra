<script lang="ts">
  import { t } from "../../lib/i18n";

  // Saves on change, and only a value the core would accept (same range).
  let {
    label,
    value,
    min,
    max,
    zeroOff = false,
    hint,
    error,
    save,
  }: {
    label: string;
    value: number;
    min: number;
    max: number;
    zeroOff?: boolean;
    hint?: string;
    error?: string;
    save: (n: number) => void;
  } = $props();

  const uid = $props.id();
  let bad = $state(false);

  function commit(raw: string) {
    const n = raw.trim() === "" ? NaN : Number(raw);
    bad = !(
      Number.isInteger(n) &&
      ((n >= min && n <= max) || (zeroOff && n === 0))
    );
    if (!bad) save(n);
  }

  const message = $derived(
    bad
      ? t(zeroOff ? "settings.rangeOffError" : "settings.rangeError", {
          min,
          max,
        })
      : error,
  );
</script>

<div class="field">
  <label for="{uid}-in">{label}</label>
  <input
    id="{uid}-in"
    type="number"
    {value}
    aria-invalid={message ? "true" : undefined}
    aria-describedby={message ? `${uid}-err` : hint ? `${uid}-hint` : undefined}
    onchange={(e) => commit(e.currentTarget.value)}
  />
  {#if message}
    <p class="error" id="{uid}-err">{message}</p>
  {:else if hint}
    <p class="hint" id="{uid}-hint">{hint}</p>
  {/if}
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }
  .error,
  .hint {
    margin: 0;
    font-size: 12px;
  }
  .error {
    color: var(--danger);
  }
  .hint {
    color: var(--fg-muted);
  }
</style>
