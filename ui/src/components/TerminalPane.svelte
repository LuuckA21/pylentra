<script lang="ts">
  import { FitAddon } from "@xterm/addon-fit";
  import { Terminal } from "@xterm/xterm";
  import "@xterm/xterm/css/xterm.css";
  import { onMount, untrack } from "svelte";
  import type { PromptAnswer } from "../lib/bindings/PromptAnswer";
  import type { Snippet } from "../lib/bindings/Snippet";
  import { t } from "../lib/i18n";
  import { focusFirst } from "../lib/modal";
  import type { Dir, Side } from "../lib/layout";
  import { RECONNECT_DELAYS, shouldReconnect } from "../lib/reconnect";
  import type { PaneState } from "../lib/sessionReducer";
  import { appTerminalTheme, terminalTheme } from "../lib/terminalThemes";
  import {
    body,
    fillVars,
    needsConfirm,
    rememberVars,
    snippetVars,
  } from "../lib/snippets";
  import { fromOtherModal, shortcutFor, type Action } from "../lib/shortcuts";
  import {
    answer,
    connect,
    cycleTab,
    focusPane,
    moveFocus,
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
  import SnippetVarsDialog from "./SnippetVarsDialog.svelte";
  import StatusBar from "./StatusBar.svelte";
  import ConfirmDialog from "./ui/ConfirmDialog.svelte";

  let {
    pane,
    visible,
    focused,
    onedit,
    onsave,
    onclose,
    canSplit = false,
    onsplit,
    multi = false,
  }: {
    pane: PaneState;
    // Its tab is shown, and it is the tab's focused pane.
    visible: boolean;
    focused: boolean;
    // A saved connection only: a quick pane's error offers onsave instead.
    onedit: (connectionId: string) => void;
    onsave: (paneId: string) => void;
    // The parent decides whether closing needs confirmation.
    onclose: (paneId: string) => void;
    // The tab has room for another pane; onsplit opens the picker for it.
    canSplit?: boolean;
    onsplit?: (paneId: string, dir: Dir) => void;
    // The tab holds other panes: the focused one shows an accent border.
    multi?: boolean;
  } = $props();

  // The pane's own shortcuts and keyboard focus.
  const active = $derived(visible && focused);
  let el: HTMLDivElement;
  let root: HTMLDivElement;
  let forwardsOpen = $state(false);
  let snippetsOpen = $state(false);
  // The terminal selection when the palette opened.
  let selection = $state("");
  // A multi-line snippet waiting for the user's go-ahead, a snippet waiting for
  // its variables, and the new-snippet form.
  let confirming = $state<Snippet>();
  let asking = $state<Snippet>();
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
    // Per session: pane.readyAt is not read here because connect resets it.
    let wasReady = false;
    connect(
      pane.paneId,
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

  const onanswer = (a: PromptAnswer) => answer(pane.paneId, a);

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
      if (pane.phase === "ready") write(pane, d);
      else if (pane.phase === "closed" && pane.closed?.type !== "Error") {
        if (d === "\r") start();
      }
    });
    tm.onBinary((d) =>
      write(
        pane,
        Uint8Array.from(d, (c) => c.charCodeAt(0)),
      ),
    );
    tm.onResize(({ cols, rows }) => resize(pane, cols, rows));
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

  // An overlay that opened while the pane waited takes the focus, as if just
  // opened; else the terminal.
  function focusInside() {
    const overlay = root.querySelector<HTMLElement>("[data-modal]");
    if (overlay) focusFirst(overlay);
    else term?.focus();
  }

  // When the pane gets the focus, unless something in it (reached with Tab,
  // say) already holds it.
  $effect(() => {
    if (active && !root.contains(document.activeElement)) focusInside();
  });

  // A closed overlay, or a click on plain text, drops the focus to the body.
  const free = () =>
    !document.activeElement || document.activeElement === document.body;

  // When an overlay opens or closes, only if the focus is free: a window
  // dialog or field keeps it.
  $effect(() => {
    void [pane.prompt, pane.closed, waiting];
    if (untrack(() => active) && free()) focusInside();
  });

  // A session that dropped under an open palette must not receive anything:
  // the Enter would also restart a closed pane.
  $effect(() => {
    if (pane.phase !== "ready") {
      snippetsOpen = false;
      confirming = undefined;
      asking = undefined;
    }
  });

  // A palette left open on a hidden or unfocused pane would catch the keys of
  // the next visit.
  $effect(() => {
    if (!active) snippetsOpen = false;
  });

  // The two panels share a spot; the forwards one can also open by itself.
  $effect(() => {
    if (forwardsOpen) snippetsOpen = false;
  });

  function toggleSnippets() {
    if (snippetsOpen) return closeSnippets();
    if (pane.phase !== "ready" || confirming || asking || newSnippet) return;
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
    if (pane.phase === "ready") {
      term?.paste(body(s.text));
      // Through onData, like typing.
      if (s.run) term?.input("\r");
    }
    term?.focus();
  }

  function pick(s: Snippet) {
    snippetsOpen = false;
    if (snippetVars(s.text).length) asking = s;
    // Escapes like {{{{ apply with or without variables.
    else send({ ...s, text: fillVars(s.text, {}) });
  }

  function send(s: Snippet) {
    if (term && needsConfirm(s.text, s.run, term.modes.bracketedPasteMode))
      confirming = s;
    else paste(s);
  }

  function run(a: Action, e: KeyboardEvent) {
    // Copy and paste act on the terminal only when it has focus; elsewhere the
    // native behavior stays.
    // Search and quick connect focus their own fields, which handle them;
    // fullscreen belongs to App.
    const inTerminal = el.contains(e.target as Node);
    if (
      a === "search" ||
      a === "quickConnect" ||
      a === "fullscreen" ||
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
    } else if (a === "closePane") onclose(pane.paneId);
    else if (a === "nextTab") cycleTab(1);
    else if (a === "prevTab") cycleTab(-1);
    else if (a === "snippets") toggleSnippets();
    else if ((a === "splitRight" || a === "splitDown") && canSplit)
      onsplit?.(pane.paneId, a === "splitRight" ? "row" : "column");
    else if (SIDES[a]) moveFocus(SIDES[a]);
  }

  const SIDES: Partial<Record<Action, Side>> = {
    focusLeft: "left",
    focusRight: "right",
    focusUp: "up",
    focusDown: "down",
  };

  function onwindowkey(e: KeyboardEvent) {
    // Our own modals sit inside root, so fromOtherModal lets them through.
    if (confirming || asking || newSnippet || fromOtherModal(e, root)) return;
    const a = active ? shortcutFor(e) : null;
    if (a) run(a, e);
  }
</script>

<svelte:window onkeydown={onwindowkey} />

<!-- A press or a focus anywhere in the pane makes it the tab's focused one;
     the keyboard gets there with Alt+arrows and Tab. -->
<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
<div
  class="pane"
  class:focused={focused && multi}
  data-unfocused={!active || undefined}
  bind:this={root}
  onfocusin={() => focusPane(pane.paneId)}
  onpointerdown={() => focusPane(pane.paneId)}
  onclick={() => active && free() && focusInside()}
>
  <div class="inset"><div class="terminal" bind:this={el}></div></div>
  <div class="anchor">
    {#if pane.phase === "ready"}
      <ForwardPanel
        {pane}
        bind:open={forwardsOpen}
        onclose={() => (forwardsOpen = false)}
      />
      {#if snippetsOpen}
        <SnippetPalette
          connectionId={pane.quick ? null : pane.connectionId}
          connectionName={library.connections.find(
            (c) => c.id === pane.connectionId,
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
      {pane}
      connection={library.connections.find((c) => c.id === pane.connectionId)}
      groups={library.groups}
      onforwards={() => (forwardsOpen = !forwardsOpen)}
      {forwardsOpen}
      onsnippets={toggleSnippets}
      {snippetsOpen}
      onsave={pane.quick ? () => onsave(pane.paneId) : undefined}
      onsplit={onsplit && ((dir) => onsplit(pane.paneId, dir))}
      {canSplit}
    />
  </div>

  {#if pane.phase === "connecting" || pane.phase === "authenticating"}
    <p class="status" role="status">
      {t(
        pane.phase === "connecting"
          ? "session.connecting"
          : "session.authenticating",
      )}
    </p>
  {/if}

  {#key pane.prompt}
    {#if pane.prompt?.type === "HostKeyPrompt"}
      <HostKeyDialog prompt={pane.prompt} {onanswer} />
    {:else if pane.prompt?.type === "SecretPrompt"}
      <SecretPrompt
        prompt={pane.prompt}
        connectionId={pane.connectionId}
        quick={pane.quick}
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

  {#if asking}
    {@const s = asking}
    <SnippetVarsDialog
      snippet={s}
      names={snippetVars(s.text)}
      onsubmit={(values) => {
        // s follows `asking`, so read it before clearing.
        const filled = { ...s, text: fillVars(s.text, values) };
        rememberVars(s.id, values);
        asking = undefined;
        send(filled);
      }}
      oncancel={() => {
        asking = undefined;
        term?.focus();
      }}
    />
  {/if}

  {#if newSnippet}
    <SnippetForm
      preset={{
        text: newSnippet.text,
        connectionId: pane.quick ? null : pane.connectionId,
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
  {:else if pane.closed?.type === "Error"}
    <ErrorOverlay
      error={pane.closed.error}
      mismatch={pane.mismatch}
      onretry={start}
      onedit={() =>
        pane.quick ? onsave(pane.paneId) : onedit(pane.connectionId)}
    />
  {/if}
</div>

<style>
  .pane {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--surface-1);
    border-radius: var(--radius-lg);
  }
  /* Inside the pane, so the terminal keeps its size; an overlay, so the status
     bar does not paint over it. */
  .pane.focused::after {
    content: "";
    position: absolute;
    inset: 0;
    border: 2px solid var(--accent);
    border-radius: inherit;
    pointer-events: none;
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
