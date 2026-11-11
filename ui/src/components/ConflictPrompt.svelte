<script lang="ts">
  import type { ConflictChoice } from "../lib/bindings/ConflictChoice";
  import type { PromptAnswer } from "../lib/bindings/PromptAnswer";
  import { t } from "../lib/i18n";
  import { modal } from "../lib/modal";

  let {
    name,
    onanswer,
  }: {
    /** The file's path, relative to the transferred folder. */
    name: string;
    onanswer: (a: PromptAnswer) => void;
  } = $props();

  const uid = $props.id();
  let all = $state(false);

  const send = (choice: ConflictChoice) =>
    onanswer({ type: "Conflict", choice, all });
</script>

<div class="tab-overlay">
  <div
    class="tab-panel"
    role="dialog"
    aria-modal="true"
    aria-labelledby="{uid}-title"
    use:modal={() => send("Cancel")}
  >
    <h2 id="{uid}-title">{t("sftp.conflict.title")}</h2>
    <p class="name">{t("sftp.conflict.body", { name })}</p>
    <label class="check">
      <input type="checkbox" bind:checked={all} />
      {t("sftp.conflict.all")}
    </label>
    <div class="actions">
      <button type="button" onclick={() => send("Cancel")}
        >{t("common.cancel")}</button
      >
      <!-- The safe choice takes the focus: Enter never overwrites. -->
      <button type="button" data-autofocus onclick={() => send("Skip")}
        >{t("sftp.conflict.skip")}</button
      >
      <button type="button" class="primary" onclick={() => send("Overwrite")}
        >{t("sftp.conflict.overwrite")}</button
      >
    </div>
  </div>
</div>

<style>
  .name {
    overflow-wrap: anywhere;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
  }
</style>
