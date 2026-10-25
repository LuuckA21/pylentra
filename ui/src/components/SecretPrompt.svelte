<script lang="ts">
  import type { Connection } from "../lib/bindings/Connection";
  import type { PromptAnswer } from "../lib/bindings/PromptAnswer";
  import type { SessionEvent } from "../lib/bindings/SessionEvent";
  import { t } from "../lib/i18n";
  import { modal } from "../lib/modal";
  import { library } from "../lib/stores/library.svelte";

  let {
    prompt,
    connectionId,
    quick,
    onanswer,
  }: {
    prompt: Extract<SessionEvent, { type: "SecretPrompt" }>;
    connectionId: string;
    /** A quick tab's connection, which is not in the library. */
    quick?: Connection;
    onanswer: (a: PromptAnswer) => void;
  } = $props();

  const uid = $props.id();

  // Each question arrives as a new prompt and remounts this component, so the
  // props are read once.
  // svelte-ignore state_referenced_locally
  const { host, port, kind, can_save } = prompt;

  // The prompt belongs to one hop of the jump chain: find its connection by host
  // and port. The core lowercases the prompt's host.
  function hop(): Connection | undefined {
    let c = quick ?? library.connections.find((x) => x.id === connectionId);
    for (let depth = 0; c && depth <= 5; depth++) {
      if (c.host.toLowerCase() === host && c.port === port) return c;
      const via = c.jump_via;
      c = library.connections.find((x) => x.id === via);
    }
  }

  const conn = hop();
  const target = `${conn ? `${conn.username}@` : ""}${host}:${port}`;
  const title =
    kind.type === "Password"
      ? t("secret.password", { target })
      : kind.type === "Passphrase"
        ? t("secret.passphrase", { target })
        : t("secret.kbd", { target });

  let save = $state(
    can_save &&
      (kind.type === "Password"
        ? conn?.auth.type === "Password" && conn.auth.save
        : kind.type === "Passphrase"
          ? conn?.auth.type === "Key" && conn.auth.save_passphrase
          : false),
  );
  let values = $state(
    kind.type === "KeyboardInteractive" ? kind.prompts.map(() => "") : [""],
  );

  const cancel = () => onanswer({ type: "Cancel" });

  function submit(e: SubmitEvent) {
    e.preventDefault();
    onanswer(
      kind.type === "KeyboardInteractive"
        ? { type: "KeyboardInteractive", responses: values, save }
        : { type: "Secret", value: values[0], save },
    );
  }
</script>

<div class="tab-overlay">
  <div
    class="tab-panel"
    role="dialog"
    aria-modal="true"
    aria-labelledby="{uid}-title"
    use:modal={cancel}
  >
    <form onsubmit={submit}>
      <h2 id="{uid}-title">{title}</h2>
      {#if kind.type === "Passphrase"}
        <p class="muted">{t("secret.attempt", { n: kind.attempt })}</p>
      {/if}
      {#if kind.type === "KeyboardInteractive"}
        {#if kind.name}<p>{kind.name}</p>{/if}
        {#if kind.instructions}<p class="muted">{kind.instructions}</p>{/if}
        {#each kind.prompts as p, i (i)}
          <label class="field">
            {p.text}
            <input
              type={p.echo ? "text" : "password"}
              autocomplete="off"
              bind:value={values[i]}
            />
          </label>
        {/each}
      {:else}
        <label class="field">
          {kind.type === "Password" ? t("form.password") : t("form.passphrase")}
          <input type="password" autocomplete="off" bind:value={values[0]} />
        </label>
      {/if}
      <!-- The core never saves keyboard-interactive answers. -->
      {#if can_save && kind.type !== "KeyboardInteractive"}
        <label class="check">
          <input type="checkbox" bind:checked={save} />
          {t("secret.save")}
        </label>
      {/if}
      <div class="actions">
        <button type="button" onclick={cancel}>{t("common.cancel")}</button>
        <button type="submit" class="primary">{t("secret.submit")}</button>
      </div>
    </form>
  </div>
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 8px;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .muted {
    color: var(--fg-muted);
  }
</style>
