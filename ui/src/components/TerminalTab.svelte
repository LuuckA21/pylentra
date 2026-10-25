<script lang="ts">
  import { FitAddon } from "@xterm/addon-fit";
  import { Terminal } from "@xterm/xterm";
  import "@xterm/xterm/css/xterm.css";
  import { onMount, untrack } from "svelte";
  import type { PromptAnswer } from "../lib/bindings/PromptAnswer";
  import type { Snippet } from "../lib/bindings/Snippet";
  import { t } from "../lib/i18n";
  import { RECONNECT_DELAYS, shouldReconnect } from "../lib/reconnect";
  import type { TabState } from "../lib/sessionReducer";
  import { appTerminalTheme, terminalTheme } from "../lib/terminalThemes";
  import { body, needsConfirm } from "../lib/snippets";
  import { fromOtherModal, shortcutFor, type Action } from "../lib/shortcuts";
  import {
    answer,
    connect,
    cycleTab,
    resize,
    write,
  } from "../lib/stores/sessions.svelte";
  import { library } from "../lib/stores/library.svelte";
  import { settings } from "../lib/stores/settings.svelte";
  import { toastError } from "../lib/stores/toasts.svelte";
  import ForwardPanel from "./ForwardPanel.svelte";
  import ErrorOverlay from "./ErrorOverlay.svelte";
  import HostKeyDialog from "./HostKeyDialog.svelte";
  import ReconnectOverlay from "./ReconnectOverlay.svelte";
  import SecretPrompt from "./SecretPrompt.svelte";
  import SnippetForm from "./SnippetForm.svelte";
  import SnippetPalette from "./SnippetPalette.svelte";
  import StatusBar from "./StatusBar.svelte";
  import ConfirmDialog from "./ui/ConfirmDialog.svelte";

  let {
    tab,
    active,
    onedit,
    onsave,
    onclose,
  }: {
    tab: TabState;
    active: boolean;
    onedit: (connectionId: string) => void;
    onsave: (tabId: string) => void;
    // The parent decides whether closing needs confirmation.
    onclose: (tabId: string) => void;
  } = $props();

  let el: HTMLDivElement;
  let root: HTMLDivElement;
  let forwardsOpen = $state(false);
  let snippetsOpen = $state(false);
  // The terminal selection when the palette opened.
  let selection = $state("");
  // A multi-line snippet waiting for the user's go-ahead, and the new-snippet form.
  let confirming = $state<Snippet>();
  let newSnippet = $state<{ text: string }>();
  let term: Terminal | undefined;
  let fit: FitAddon | undefined;

  const theme = () => terminalTheme(settings.terminal_theme, appTerminalTheme);

  // Automatic reconnection: retries launched in the current chain (0 = none),
  // and whether the next one is being waited for (the overlay shows then).
  let retries = $state(0);
  let waiting = $state(false);
  let retryTimer: ReturnType<typeof setTimeout> | undefined;

  function launch() {
    const tm = term;
    if (!tm) return;
    // Per session: tab.readyAt is not read here because connect resets it.
    let wasReady = false;
    connect(
      tab.tabId,
      tm.cols,
      tm.rows,
      (bytes) => tm.write(bytes),
      (e) => {
        if (e.type === "Status" && e.stage === "Ready") {
          wasReady = true;
          retries = 0;
        } else if (e.type === "Closed") {
          if (e.reason.type !== "Error")
            tm.write(`\r\n${t("session.ended")}\r\n`);
          // A retry that fails is still part of a chain that started from a ready session.
          else if (
            retries < RECONNECT_DELAYS.length &&
            shouldReconnect(
              e.reason,
              wasReady || retries > 0,
              settings.auto_reconnect,
            )
          ) {
            waiting = true;
            clearTimeout(retryTimer);
            retryTimer = setTimeout(() => {
              // The setting may have been turned off during the wait.
              if (!settings.auto_reconnect) return cancelRetry();
              waiting = false;
              retries++;
              launch();
            }, RECONNECT_DELAYS[retries]);
          }
        }
      },
    );
  }

  function cancelRetry() {
    clearTimeout(retryTimer);
    waiting = false;
    retries = 0;
  }

  // User-initiated (re)connection starts a new chain.
  function start() {
    cancelRetry();
    launch();
  }

  const onanswer = (a: PromptAnswer) => answer(tab.tabId, a);

  onMount(() => {
    const tm = new Terminal({
      fontFamily: settings.font_family,
      fontSize: settings.font_size,
      theme: theme(),
      cursorStyle: settings.cursor_style,
      cursorBlink: settings.cursor_blink,
      scrollback: settings.scrollback,
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
    tm.onSelectionChange(() => {
      const text = settings.copy_on_select ? tm.getSelection() : "";
      // A clipboard error here would only be noise.
      if (text) navigator.clipboard?.writeText(text).catch(() => {});
    });

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
      clearTimeout(retryTimer);
      ro.disconnect();
      mo.disconnect();
      dark.removeEventListener("change", retheme);
      term = undefined;
      fit = undefined;
      tm.dispose();
    };
  });

  // Open terminals follow the terminal settings.
  $effect(() => {
    const o = {
      fontFamily: settings.font_family,
      fontSize: settings.font_size,
      theme: theme(),
      cursorStyle: settings.cursor_style,
      cursorBlink: settings.cursor_blink,
      scrollback: settings.scrollback,
    };
    if (!term) return;
    Object.assign(term.options, o);
    fit?.fit();
  });

  // Focus the terminal when the tab is shown, unless an overlay holds the focus.
  $effect(() => {
    if (active && !untrack(() => tab.prompt || tab.closed?.type === "Error"))
      term?.focus();
  });

  // A session that dropped under an open palette must not receive anything:
  // the Enter would also restart a closed tab.
  $effect(() => {
    if (tab.phase !== "ready") {
      snippetsOpen = false;
      confirming = undefined;
    }
  });

  // A palette left open on a hidden tab would catch the keys of the next visit.
  $effect(() => {
    if (!active) snippetsOpen = false;
  });

  // The two panels share a spot; the forwards one can also open by itself.
  $effect(() => {
    if (forwardsOpen) snippetsOpen = false;
  });

  function toggleSnippets() {
    if (snippetsOpen) return closeSnippets();
    if (tab.phase !== "ready" || confirming || newSnippet) return;
    selection = term?.getSelection() ?? "";
    forwardsOpen = false;
    snippetsOpen = true;
  }

  function closeSnippets() {
    snippetsOpen = false;
    term?.focus();
  }

  function paste(s: Snippet) {
    confirming = undefined;
    if (tab.phase === "ready") {
      term?.paste(body(s.text));
      // Through onData, like typing.
      if (s.run) term?.input("\r");
    }
    term?.focus();
  }

  function pick(s: Snippet) {
    snippetsOpen = false;
    if (term && needsConfirm(s.text, s.run, term.modes.bracketedPasteMode))
      confirming = s;
    else paste(s);
  }

  function run(a: Action, e: KeyboardEvent) {
    // Copy and paste act on the terminal only when it has focus; elsewhere the
    // native behavior stays.
    // Search and quick connect focus their own fields, which handle them.
    const inTerminal = el.contains(e.target as Node);
    if (
      a === "search" ||
      a === "quickConnect" ||
      ((a === "copy" || a === "paste") && !inTerminal)
    )
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
    } else if (a === "closeTab") onclose(tab.tabId);
    else if (a === "nextTab") cycleTab(1);
    else if (a === "prevTab") cycleTab(-1);
    else if (a === "snippets") toggleSnippets();
  }

  function onwindowkey(e: KeyboardEvent) {
    // Our own modals sit inside root, so fromOtherModal lets them through.
    if (confirming || newSnippet || fromOtherModal(e, root)) return;
    const a = active ? shortcutFor(e) : null;
    if (a) run(a, e);
  }
</script>

<svelte:window onkeydown={onwindowkey} />

<div class="tab" class:active bind:this={root}>
  <div class="inset"><div class="terminal" bind:this={el}></div></div>
  <div class="anchor">
    {#if tab.phase === "ready"}
      <ForwardPanel
        {tab}
        bind:open={forwardsOpen}
        onclose={() => (forwardsOpen = false)}
      />
      {#if snippetsOpen}
        <SnippetPalette
          connectionId={tab.quick ? null : tab.connectionId}
          connectionName={library.connections.find(
            (c) => c.id === tab.connectionId,
          )?.name}
          {selection}
          onpick={pick}
          onnew={(text) => {
            snippetsOpen = false;
            newSnippet = { text };
          }}
          onclose={closeSnippets}
        />
      {/if}
    {/if}
    <StatusBar
      {tab}
      connection={library.connections.find((c) => c.id === tab.connectionId)}
      groups={library.groups}
      onforwards={() => (forwardsOpen = !forwardsOpen)}
      {forwardsOpen}
      onsnippets={toggleSnippets}
      {snippetsOpen}
      onsave={tab.quick ? () => onsave(tab.tabId) : undefined}
    />
  </div>

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
        quick={tab.quick}
        {onanswer}
      />
    {/if}
  {/key}

  {#if confirming}
    {@const s = confirming}
    <ConfirmDialog
      title={t("snippet.confirmTitle")}
      message={t("snippet.confirmBody")}
      confirmLabel={t("snippet.confirmOk")}
      onconfirm={() => paste(s)}
      oncancel={() => {
        confirming = undefined;
        term?.focus();
      }}
    />
  {/if}

  {#if newSnippet}
    <SnippetForm
      preset={{
        text: newSnippet.text,
        connectionId: tab.quick ? null : tab.connectionId,
      }}
      onclose={() => {
        newSnippet = undefined;
        term?.focus();
      }}
    />
  {/if}

  {#if waiting}
    <ReconnectOverlay
      attempt={retries + 1}
      max={RECONNECT_DELAYS.length}
      oncancel={cancelRetry}
    />
  {:else if tab.closed?.type === "Error"}
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
    overflow: hidden;
    background: var(--surface-1);
    border-radius: var(--radius-lg);
  }
  .tab.active {
    display: flex;
  }
  /* The fit addon measures its parent, so the padding sits one level above. */
  .inset {
    flex: 1;
    min-height: 0;
    padding: var(--space-4) var(--space-5);
  }
  .terminal {
    height: 100%;
  }
  .anchor {
    position: relative;
  }
  .status {
    position: absolute;
    top: var(--space-3);
    right: var(--space-4);
    margin: 0;
    padding: 2px var(--space-3);
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--fg-muted);
  }
</style>
