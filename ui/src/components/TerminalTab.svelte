<script lang="ts">
  import { FitAddon } from "@xterm/addon-fit";
  import { Terminal, type ITheme } from "@xterm/xterm";
  import "@xterm/xterm/css/xterm.css";
  import { onMount, untrack } from "svelte";
  import type { PromptAnswer } from "../lib/bindings/PromptAnswer";
  import { t } from "../lib/i18n";
  import type { TabState } from "../lib/sessionReducer";
  import { shortcutFor, type Action } from "../lib/shortcuts";
  import {
    answer,
    closeTab,
    connect,
    cycleTab,
    resize,
    write,
  } from "../lib/stores/sessions.svelte";
  import { settings } from "../lib/stores/settings.svelte";
  import { toastError } from "../lib/stores/toasts.svelte";
  import ForwardPanel from "./ForwardPanel.svelte";
  import ErrorOverlay from "./ErrorOverlay.svelte";
  import HostKeyDialog from "./HostKeyDialog.svelte";
  import SecretPrompt from "./SecretPrompt.svelte";

  let {
    tab,
    active,
    onedit,
  }: {
    tab: TabState;
    active: boolean;
    onedit: (connectionId: string) => void;
  } = $props();

  let el: HTMLDivElement;
  let term: Terminal | undefined;
  let fit: FitAddon | undefined;

  // Colors come from the app's CSS tokens, so the terminal follows data-theme.
  function theme(): ITheme {
    const css = getComputedStyle(document.documentElement);
    const v = (name: string) => css.getPropertyValue(name).trim();
    return {
      background: v("--bg"),
      foreground: v("--fg"),
      cursor: v("--fg"),
      selectionBackground: v("--bg-selected"),
    };
  }

  function start() {
    const tm = term;
    if (!tm) return;
    connect(
      tab.tabId,
      tm.cols,
      tm.rows,
      (bytes) => tm.write(bytes),
      (e) => {
        if (e.type === "Closed" && e.reason.type !== "Error")
          tm.write(`\r\n${t("session.ended")}\r\n`);
      },
    );
  }

  const onanswer = (a: PromptAnswer) => answer(tab.tabId, a);

  onMount(() => {
    const tm = new Terminal({
      fontFamily: settings.font_family,
      fontSize: settings.font_size,
      theme: theme(),
    });
    const fa = new FitAddon();
    tm.loadAddon(fa);
    tm.open(el);
    fa.fit();
    term = tm;
    fit = fa;

    // Our shortcuts never reach the shell; the window handler below runs them.
    tm.attachCustomKeyEventHandler((e) => !shortcutFor(e));
    tm.onData((d) => {
      if (tab.phase === "ready") write(tab, d);
      else if (tab.phase === "closed" && tab.closed?.type !== "Error") {
        if (d === "\r") start();
      }
    });
    tm.onBinary((d) =>
      write(
        tab,
        Uint8Array.from(d, (c) => c.charCodeAt(0)),
      ),
    );
    tm.onResize(({ cols, rows }) => resize(tab, cols, rows));

    let timer: ReturnType<typeof setTimeout> | undefined;
    const ro = new ResizeObserver(() => {
      clearTimeout(timer);
      timer = setTimeout(() => fa.fit(), 50);
    });
    ro.observe(el);

    const retheme = () => (tm.options.theme = theme());
    const mo = new MutationObserver(retheme);
    mo.observe(document.documentElement, { attributeFilter: ["data-theme"] });
    const dark = matchMedia("(prefers-color-scheme: dark)");
    dark.addEventListener("change", retheme);

    start();
    return () => {
      clearTimeout(timer);
      ro.disconnect();
      mo.disconnect();
      dark.removeEventListener("change", retheme);
      term = undefined;
      fit = undefined;
      tm.dispose();
    };
  });

  // Open terminals follow font changes from the settings dialog.
  $effect(() => {
    const { font_family, font_size } = settings;
    if (!term) return;
    term.options.fontFamily = font_family;
    term.options.fontSize = font_size;
    fit?.fit();
  });

  // Focus the terminal when the tab is shown, unless an overlay holds the focus.
  $effect(() => {
    if (active && !untrack(() => tab.prompt || tab.closed?.type === "Error"))
      term?.focus();
  });

  function run(a: Action, e: KeyboardEvent) {
    // Copy and paste act on the terminal only when it has focus; elsewhere the
    // native behavior stays.
    // Search focuses the sidebar's search box, which handles it.
    const inTerminal = el.contains(e.target as Node);
    if (a === "search" || ((a === "copy" || a === "paste") && !inTerminal))
      return;
    e.preventDefault();
    if (a === "copy") {
      const text = term?.getSelection();
      if (text) navigator.clipboard.writeText(text).catch(toastError);
    } else if (a === "paste") {
      navigator.clipboard
        .readText()
        .then((text) => term?.paste(text))
        .catch(toastError);
    } else if (a === "closeTab") closeTab(tab.tabId);
    else if (a === "nextTab") cycleTab(1);
    else if (a === "prevTab") cycleTab(-1);
  }

  function onwindowkey(e: KeyboardEvent) {
    const a = active ? shortcutFor(e) : null;
    if (a) run(a, e);
  }
</script>

<svelte:window onkeydown={onwindowkey} />

<div class="tab" class:active>
  <div class="terminal" bind:this={el}></div>
  {#if tab.phase === "ready"}<ForwardPanel {tab} />{/if}

  {#if tab.phase === "connecting" || tab.phase === "authenticating"}
    <p class="status" role="status">
      {t(
        tab.phase === "connecting"
          ? "session.connecting"
          : "session.authenticating",
      )}
    </p>
  {/if}

  {#key tab.prompt}
    {#if tab.prompt?.type === "HostKeyPrompt"}
      <HostKeyDialog prompt={tab.prompt} {onanswer} />
    {:else if tab.prompt?.type === "SecretPrompt"}
      <SecretPrompt
        prompt={tab.prompt}
        connectionId={tab.connectionId}
        {onanswer}
      />
    {/if}
  {/key}

  {#if tab.closed?.type === "Error"}
    <ErrorOverlay
      error={tab.closed.error}
      mismatch={tab.mismatch}
      onretry={start}
      onedit={() => onedit(tab.connectionId)}
    />
  {/if}
</div>

<style>
  .tab {
    position: absolute;
    inset: 0;
    display: none;
    flex-direction: column;
  }
  .tab.active {
    display: flex;
  }
  .terminal {
    flex: 1;
    min-height: 0;
    background: var(--bg);
  }
  .status {
    position: absolute;
    top: 8px;
    right: 12px;
    margin: 0;
    padding: 2px 8px;
    border-radius: 4px;
    background: var(--bg-alt);
    color: var(--fg-muted);
  }
</style>
