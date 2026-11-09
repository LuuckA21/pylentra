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
  .dialog {
    max-width: min(440px, calc(100vw - 32px));
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
