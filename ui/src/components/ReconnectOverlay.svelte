<script lang="ts">
  import { t } from "../lib/i18n";
  import { modal } from "../lib/modal";

  let {
    attempt,
    max,
    oncancel,
  }: { attempt: number; max: number; oncancel: () => void } = $props();
</script>

<div class="tab-overlay">
  <div class="tab-panel" use:modal={oncancel}>
    <p role="status">{t("reconnect.attempt", { n: attempt, max })}</p>
    <div class="actions">
      <button type="button" class="primary" data-autofocus onclick={oncancel}
        >{t("reconnect.cancel")}</button
      >
    </div>
  </div>
</div>

<style>
  p {
    margin: 0;
  }
</style>
