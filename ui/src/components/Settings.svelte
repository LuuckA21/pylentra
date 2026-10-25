<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { openPath } from "@tauri-apps/plugin-opener";
  import { onMount } from "svelte";
  import { api } from "../lib/api";
  import type { Settings } from "../lib/bindings/Settings";
  import { errorMessage, t, type MessageKey } from "../lib/i18n";
  import { modal } from "../lib/modal";
  import { getPref, setPref } from "../lib/prefs";
  import { saveSettings } from "../lib/stores/settings.svelte";
  import { isApiError, toastError } from "../lib/stores/toasts.svelte";
  import AppearanceSection from "./settings/AppearanceSection.svelte";
  import ConnectionsSection from "./settings/ConnectionsSection.svelte";
  import SecuritySection from "./settings/SecuritySection.svelte";
  import TerminalSection from "./settings/TerminalSection.svelte";
  import UpdatesSection from "./settings/UpdatesSection.svelte";
  import Icon, { type UiIcon } from "./ui/Icon.svelte";

  let { onclose }: { onclose: () => void } = $props();

  const SECTIONS = [
    { id: "appearance", icon: "palette" },
    { id: "terminal", icon: "terminal-ui" },
    { id: "connections", icon: "bolt" },
    { id: "security", icon: "shield" },
    { id: "updates", icon: "update" },
  ] as const satisfies readonly { id: string; icon: UiIcon }[];
  type SectionId = (typeof SECTIONS)[number]["id"];

  // Where each field the core validates lives, to show its error in place.
  const FIELD_SECTION: Record<string, SectionId> = {
    font_size: "terminal",
    scrollback: "terminal",
    default_user: "connections",
    default_port: "connections",
    connect_timeout_secs: "connections",
    keepalive_secs: "connections",
  };

  const uid = $props.id();
  const stored = getPref<string>("pylentra.settings.section", "appearance");
  let section = $state<SectionId>(
    SECTIONS.find((s) => s.id === stored)?.id ?? "appearance",
  );
  let version = $state("");
  let errors = $state<Record<string, string>>({});

  onMount(() => {
    getVersion()
      .then((v) => (version = v))
      .catch(() => {});
  });

  const show = (id: SectionId) => {
    section = id;
    setPref("pylentra.settings.section", id);
  };

  // Every control saves on change; the theme and language apply right away.
  // The core refuses the whole object for one bad field, so that field's error
  // is shown where it is, even when it is not the one just edited.
  async function save(patch: Partial<Settings>) {
    try {
      await saveSettings(patch);
      errors = {};
    } catch (e) {
      const field = isApiError(e) ? String(e.params.field) : "";
      if (
        isApiError(e) &&
        e.code === "config.invalid" &&
        field in FIELD_SECTION
      ) {
        errors = { [field]: errorMessage(e) };
        show(FIELD_SECTION[field]);
      } else toastError(e);
    }
  }

  const openLogs = async () => {
    try {
      await openPath(await api.logDir());
    } catch (e) {
      toastError(e);
    }
  };
</script>

<div class="backdrop">
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="{uid}-title"
    use:modal={onclose}
  >
    <aside>
      <h2 id="{uid}-title">{t("settings.title")}</h2>
      <nav aria-label={t("settings.title")}>
        {#each SECTIONS as s (s.id)}
          <button
            type="button"
            aria-current={section === s.id ? "page" : undefined}
            onclick={() => show(s.id)}
          >
            <Icon name={s.icon} size={18} />
            {t(`settings.section.${s.id}` as MessageKey)}
          </button>
        {/each}
      </nav>
      <footer>
        {#if version}<span>{t("settings.version", { version })}</span>{/if}
        <button type="button" class="link" onclick={openLogs}
          >{t("settings.openLogs")}</button
        >
      </footer>
    </aside>

    <section>
      <header>
        <h3>{t(`settings.section.${section}` as MessageKey)}</h3>
        <button
          type="button"
          class="close"
          aria-label={t("common.close")}
          onclick={onclose}><Icon name="close" size={18} /></button
        >
      </header>
      <div class="content">
        {#if section === "appearance"}
          <AppearanceSection {save} {errors} />
        {:else if section === "terminal"}
          <TerminalSection {save} {errors} />
        {:else if section === "connections"}
          <ConnectionsSection {save} {errors} />
        {:else if section === "security"}
          <SecuritySection {save} {errors} />
        {:else}
          <UpdatesSection {save} {errors} {version} />
        {/if}
      </div>
    </section>
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
    display: grid;
    grid-template-columns: 220px 1fr;
    width: min(920px, calc(100vw - 32px));
    height: min(640px, calc(100vh - 32px));
    overflow: hidden;
    background: var(--surface-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-xl);
    box-shadow: var(--shadow);
  }
  aside {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
    padding: var(--space-5) var(--space-4);
    background: var(--surface-0);
    border-right: 1px solid var(--border);
  }
  h2 {
    margin: 0 var(--space-2);
    font-size: 18px;
    font-weight: 700;
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    flex: 1;
  }
  nav button {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    height: 40px;
    border-color: transparent;
    border-radius: var(--radius-md);
    background: transparent;
    text-align: left;
  }
  nav button:hover {
    background: var(--surface-2);
  }
  nav button[aria-current="page"] {
    background: var(--surface-3);
    font-weight: 700;
  }
  nav button[aria-current="page"] :global(svg) {
    color: var(--accent);
  }
  footer {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    padding: 0 var(--space-2);
    color: var(--fg-muted);
    font-size: 12px;
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    text-align: left;
    text-decoration: underline;
  }
  section {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--space-5) var(--space-6) var(--space-3);
  }
  h3 {
    margin: 0;
    font-size: 20px;
    font-weight: 700;
  }
  .close {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    padding: 0;
    border-color: transparent;
    background: transparent;
  }
  .content {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
    flex: 1;
    overflow-y: auto;
    padding: var(--space-2) var(--space-6) var(--space-6);
  }

  @media (max-width: 760px) {
    .dialog {
      grid-template-columns: 1fr;
      grid-template-rows: auto 1fr;
    }
    aside {
      flex-direction: row;
      flex-wrap: wrap;
      align-items: center;
      padding: var(--space-3) var(--space-4);
      border-right: 0;
      border-bottom: 1px solid var(--border);
    }
    nav {
      flex-direction: row;
      flex-wrap: wrap;
      flex: 1 1 100%;
      order: 3;
    }
    /* The categories become a row of pills. */
    nav button {
      height: auto;
      padding: 2px var(--space-3);
      border-radius: 999px;
    }
    footer {
      flex-direction: row;
      align-items: center;
      margin-left: auto;
      gap: var(--space-3);
    }
  }
</style>
