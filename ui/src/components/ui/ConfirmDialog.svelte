<script lang="ts">
  import { t } from "../../lib/i18n";
  import { modal } from "../../lib/modal";

  let {
    title,
    message,
    confirmLabel,
    danger = false,
    onconfirm,
    oncancel,
  }: {
    title: string;
    message: string;
    confirmLabel: string;
    danger?: boolean;
    onconfirm: () => void;
    oncancel: () => void;
  } = $props();
</script>

<div class="backdrop">
  <div
    class="dialog"
    role="alertdialog"
    aria-modal="true"
    aria-labelledby="confirm-title"
    aria-describedby="confirm-body"
    use:modal={oncancel}
  >
    <h2 id="confirm-title">{title}</h2>
    <p id="confirm-body">{message}</p>
    <div class="actions">
      <button type="button" data-autofocus onclick={oncancel}
        >{t("common.cancel")}</button
      >
      <button type="button" class:danger onclick={onconfirm}
        >{confirmLabel}</button
      >
    </div>
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
    max-width: min(440px, calc(100vw - 32px));
    padding: var(--space-6);
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-xl);
    box-shadow: var(--shadow);
  }
  h2 {
    margin: 0 0 8px;
    font-size: 16px;
  }
  p {
    margin: 0;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 20px;
  }
</style>
