<script lang="ts">
  import type { PromptAnswer } from "../lib/bindings/PromptAnswer";
  import type { SessionEvent } from "../lib/bindings/SessionEvent";
  import { t } from "../lib/i18n";
  import { modal } from "../lib/modal";

  let {
    prompt,
    onanswer,
  }: {
    prompt: Extract<SessionEvent, { type: "HostKeyPrompt" }>;
    onanswer: (a: PromptAnswer) => void;
  } = $props();

  const uid = $props.id();
  const cancel = () => onanswer({ type: "Cancel" });
</script>

<div class="tab-overlay">
  <div
    class="tab-panel"
    role="dialog"
    aria-modal="true"
    aria-labelledby="{uid}-title"
    aria-describedby="{uid}-body"
    use:modal={cancel}
  >
    <h2 id="{uid}-title">{t("hostkey.title")}</h2>
    <p id="{uid}-body">
      {t("hostkey.body", { host: prompt.host, port: prompt.port })}
    </p>
    <dl>
      <dt>{t("hostkey.algo")}</dt>
      <dd><code>{prompt.algo}</code></dd>
      <dt>{t("hostkey.fingerprint")}</dt>
      <dd><code>{prompt.fingerprint}</code></dd>
    </dl>
    <div class="actions">
      <button type="button" onclick={cancel}>{t("common.cancel")}</button>
      <button
        type="button"
        class="primary"
        onclick={() => onanswer({ type: "HostKey", accept: true })}
        >{t("hostkey.accept")}</button
      >
    </div>
  </div>
</div>

<style>
  dd {
    margin: 0 0 8px;
  }
  dt {
    color: var(--fg-muted);
  }
</style>
