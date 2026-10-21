<script lang="ts">
  import type { AuthMethod } from "../lib/bindings/AuthMethod";
  import type { Connection } from "../lib/bindings/Connection";
  import type { ForwardRule } from "../lib/bindings/ForwardRule";
  import { errorMessage, t, type MessageKey } from "../lib/i18n";
  import { modal } from "../lib/modal";
  import { library, saveConnection } from "../lib/stores/library.svelte";
  import { isApiError, toast, toastError } from "../lib/stores/toasts.svelte";
  import { validate } from "../lib/validate";

  let {
    connection,
    onsaved,
    oncancel,
  }: {
    connection?: Connection;
    onsaved: () => void;
    oncancel: () => void;
  } = $props();

  // The form edits a copy; `connection` is only read when the form opens.
  // svelte-ignore state_referenced_locally
  const initial = $state.snapshot(connection);
  const id = initial?.id ?? crypto.randomUUID();
  const auth = initial?.auth;

  type Row = {
    type: "Local" | "Remote";
    bind_host: string;
    bind_port: number | null;
    dest_host: string;
    dest_port: number | null;
    auto_start: boolean;
  };

  let name = $state(initial?.name ?? "");
  let groupId = $state(initial?.group_id ?? "");
  let host = $state(initial?.host ?? "");
  let port = $state<number | null>(initial?.port ?? 22);
  let username = $state(initial?.username ?? "");
  let authType = $state<AuthMethod["type"]>(auth?.type ?? "Password");
  let savePassword = $state(auth?.type === "Password" && auth.save);
  let keyPath = $state(auth?.type === "Key" ? auth.path : "");
  let savePassphrase = $state(auth?.type === "Key" && auth.save_passphrase);
  let password = $state("");
  let passphrase = $state("");
  let jumpVia = $state(initial?.jump_via ?? "");
  let forwards = $state<Row[]>(
    (initial?.forwards ?? []).map((r) => ({
      bind_host: "",
      ...r,
    })),
  );
  let errors = $state<Record<string, string>>({});
  let saving = $state(false);
  let formEl: HTMLFormElement | undefined = $state();

  const groups = $derived(
    [...library.groups].sort((a, b) => a.name.localeCompare(b.name)),
  );
  const jumpHosts = $derived(
    library.connections
      .filter((c) => c.id !== id)
      .sort((a, b) => a.name.localeCompare(b.name)),
  );

  const eid = (field: string) => `err-${field.replace(/\W+/g, "_")}`;
  const num = (v: string) => (v === "" ? null : Number(v));

  function addForward(type: Row["type"]) {
    forwards.push({
      type,
      bind_host: type === "Local" ? "127.0.0.1" : "",
      bind_port: null,
      dest_host: "localhost",
      dest_port: null,
      auto_start: false,
    });
  }

  const fieldEl = (field: string) =>
    Array.from(
      formEl?.querySelectorAll<HTMLElement>("[data-field]") ?? [],
    ).find((el) => el.dataset.field === field);

  function build(): Connection {
    let method: AuthMethod = { type: "Agent" };
    if (authType === "Password")
      method = { type: "Password", save: savePassword };
    if (authType === "Key")
      method = {
        type: "Key",
        path: keyPath.trim(),
        save_passphrase: savePassphrase,
      };
    return {
      id,
      name: name.trim(),
      group_id: groupId || null,
      host: host.trim(),
      port: port!,
      username: username.trim(),
      auth: method,
      jump_via: jumpVia || null,
      forwards: forwards.map((r): ForwardRule => {
        const common = {
          bind_port: r.bind_port!,
          dest_host: r.dest_host.trim(),
          dest_port: r.dest_port!,
          auto_start: r.auto_start,
        };
        return r.type === "Local"
          ? { type: "Local", bind_host: r.bind_host.trim(), ...common }
          : { type: "Remote", ...common };
      }),
    };
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    const found = validate({ name, host, port, username, forwards });
    errors = Object.fromEntries(
      Object.entries(found).map(([field, key]) => [
        field,
        t(key as MessageKey),
      ]),
    );
    const [first] = Object.keys(found);
    if (first) return fieldEl(first)?.focus();

    // Never send an empty string: the backend would store it as the secret.
    const pw =
      authType === "Password" && savePassword && password ? password : null;
    const pp =
      authType === "Key" && savePassphrase && passphrase ? passphrase : null;
    saving = true;
    try {
      const warnings = await saveConnection(
        build(),
        pw ?? undefined,
        pp ?? undefined,
      );
      for (const w of warnings)
        toast(t("warning.keyFileMissing", { path: w.path }), "warning");
      onsaved();
    } catch (err) {
      const field =
        isApiError(err) && err.code === "config.invalid"
          ? err.params.field
          : "";
      const el = field ? fieldEl(field) : undefined;
      if (el) {
        errors = { [field]: errorMessage(err as never) };
        el.focus();
      } else toastError(err);
    } finally {
      saving = false;
    }
  }
</script>

{#snippet error(field: string)}
  {#if errors[field]}
    <p class="error" id={eid(field)}>{errors[field]}</p>
  {/if}
{/snippet}

<!-- Text/number input with a label, aria-invalid and the error linked via aria-describedby. -->
{#snippet input(
  label: string,
  field: string,
  value: string | number | null,
  set: (v: string) => void,
  opts: { type?: string; placeholder?: string; hint?: string } = {},
)}
  {@const uid = `in-${eid(field)}`}
  <div class="field">
    <label for={uid}>{label}</label>
    <input
      id={uid}
      data-field={field}
      type={opts.type ?? "text"}
      placeholder={opts.placeholder}
      value={value ?? ""}
      aria-invalid={errors[field] ? "true" : undefined}
      aria-describedby={errors[field] ? eid(field) : opts.hint}
      oninput={(e) => set(e.currentTarget.value)}
    />
    {@render error(field)}
  </div>
{/snippet}

{#snippet check(label: string, checked: boolean, set: (v: boolean) => void)}
  <label class="check">
    <input
      type="checkbox"
      {checked}
      onchange={(e) => set(e.currentTarget.checked)}
    />
    {label}
  </label>
{/snippet}

<div class="backdrop">
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="form-title"
    use:modal={oncancel}
  >
    <h2 id="form-title">
      {connection ? t("form.titleEdit") : t("form.titleNew")}
    </h2>
    <form bind:this={formEl} onsubmit={submit} novalidate>
      <fieldset>
        <legend>{t("form.general")}</legend>
        {@render input(t("form.name"), "name", name, (v) => (name = v))}
        <div class="field">
          <label for="f-group">{t("form.group")}</label>
          <select id="f-group" bind:value={groupId}>
            <option value="">{t("sidebar.noGroup")}</option>
            {#each groups as g (g.id)}
              <option value={g.id}>{g.name}</option>
            {/each}
          </select>
        </div>
        {@render input(t("form.host"), "host", host, (v) => (host = v))}
        {@render input(t("form.port"), "port", port, (v) => (port = num(v)), {
          type: "number",
        })}
        {@render input(
          t("form.username"),
          "username",
          username,
          (v) => (username = v),
        )}
      </fieldset>

      <fieldset>
        <legend>{t("form.auth")}</legend>
        <div class="field">
          <label for="f-auth">{t("form.authMethod")}</label>
          <select id="f-auth" bind:value={authType}>
            {#each ["Password", "Key", "Agent"] as const as m (m)}
              <option value={m}>{t(`form.auth.${m}`)}</option>
            {/each}
          </select>
        </div>
        {#if authType === "Password"}
          {@render check(
            t("form.savePassword"),
            savePassword,
            (v) => (savePassword = v),
          )}
          {#if savePassword}
            {@render input(
              t("form.password"),
              "password",
              password,
              (v) => (password = v),
              {
                type: "password",
                hint: "hint-password",
              },
            )}
            <p class="hint" id="hint-password">{t("form.secretHint")}</p>
          {/if}
        {:else if authType === "Key"}
          {@render input(
            t("form.keyPath"),
            "auth.path",
            keyPath,
            (v) => (keyPath = v),
            {
              placeholder: "~/.ssh/id_ed25519",
            },
          )}
          {@render check(
            t("form.savePassphrase"),
            savePassphrase,
            (v) => (savePassphrase = v),
          )}
          {#if savePassphrase}
            {@render input(
              t("form.passphrase"),
              "passphrase",
              passphrase,
              (v) => (passphrase = v),
              {
                type: "password",
                hint: "hint-passphrase",
              },
            )}
            <p class="hint" id="hint-passphrase">{t("form.secretHint")}</p>
          {/if}
        {/if}
      </fieldset>

      <fieldset>
        <legend>{t("form.jump")}</legend>
        <div class="field">
          <label for="f-jump">{t("form.jumpVia")}</label>
          <select
            id="f-jump"
            data-field="jump_via"
            bind:value={jumpVia}
            aria-invalid={errors.jump_via ? "true" : undefined}
            aria-describedby={errors.jump_via ? eid("jump_via") : undefined}
          >
            <option value="">{t("common.none")}</option>
            {#each jumpHosts as c (c.id)}
              <option value={c.id}>{c.name}</option>
            {/each}
          </select>
          {@render error("jump_via")}
        </div>
      </fieldset>

      <fieldset>
        <legend>{t("form.forwards")}</legend>
        {#if !forwards.length}
          <p class="hint">{t("form.noForwards")}</p>
        {/if}
        {#each forwards as r, i (i)}
          {@const p = `forwards[${i}]`}
          <fieldset class="forward">
            <legend
              >{t(r.type === "Local" ? "form.local" : "form.remote", {
                n: i + 1,
              })}</legend
            >
            {#if r.type === "Local"}
              {@render input(
                t("form.bindHost"),
                `${p}.bind_host`,
                r.bind_host,
                (v) => (r.bind_host = v),
              )}
            {/if}
            {@render input(
              t("form.bindPort"),
              `${p}.bind_port`,
              r.bind_port,
              (v) => (r.bind_port = num(v)),
              { type: "number" },
            )}
            {@render input(
              t("form.destHost"),
              `${p}.dest_host`,
              r.dest_host,
              (v) => (r.dest_host = v),
            )}
            {@render input(
              t("form.destPort"),
              `${p}.dest_port`,
              r.dest_port,
              (v) => (r.dest_port = num(v)),
              { type: "number" },
            )}
            {@render check(
              t("form.autoStart"),
              r.auto_start,
              (v) => (r.auto_start = v),
            )}
            <button
              type="button"
              class="danger"
              onclick={() => forwards.splice(i, 1)}
            >
              {t("common.remove")}
            </button>
          </fieldset>
        {/each}
        <div class="actions">
          <button type="button" onclick={() => addForward("Local")}
            >{t("form.addLocal")}</button
          >
          <button type="button" onclick={() => addForward("Remote")}
            >{t("form.addRemote")}</button
          >
        </div>
      </fieldset>

      <div class="actions end">
        <button type="button" onclick={oncancel}>{t("common.cancel")}</button>
        <button type="submit" class="primary" disabled={saving}
          >{t("common.save")}</button
        >
      </div>
    </form>
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
    width: min(560px, calc(100vw - 32px));
    max-height: calc(100vh - 32px);
    overflow-y: auto;
    padding: 16px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
  }
  h2 {
    margin: 0 0 8px;
    font-size: 16px;
  }
  fieldset {
    margin: 0 0 12px;
    padding: 8px 12px 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 8px;
  }
  .check {
    display: flex;
    gap: 6px;
    align-items: center;
    margin-bottom: 8px;
  }
  .error {
    margin: 0;
    color: var(--danger);
    font-size: 12px;
  }
  .hint {
    margin: 0 0 8px;
    color: var(--fg-muted);
    font-size: 12px;
  }
  .actions {
    display: flex;
    gap: 8px;
  }
  .actions.end {
    justify-content: flex-end;
  }
</style>
