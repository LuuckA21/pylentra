import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import { flushSync } from "svelte";

// xterm needs a real browser: a fake that records its options and listeners.
const made = vi.hoisted(() => ({
  terms: [] as {
    options: Record<string, unknown>;
    selection: string;
    onSelect?: () => void;
    paste: ReturnType<typeof vi.fn>;
    input: ReturnType<typeof vi.fn>;
    focus: ReturnType<typeof vi.fn>;
    modes: { bracketedPasteMode: boolean };
  }[],
}));
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    options: Record<string, unknown>;
    cols = 80;
    rows = 24;
    selection = "";
    onSelect?: () => void;
    paste = vi.fn();
    input = vi.fn();
    focus = vi.fn();
    modes = { bracketedPasteMode: false };
    constructor(options: Record<string, unknown>) {
      this.options = options;
      made.terms.push(this);
    }
    loadAddon() {}
    open() {}
    attachCustomKeyEventHandler() {}
    onData() {}
    onBinary() {}
    onResize() {}
    onSelectionChange(cb: () => void) {
      this.onSelect = cb;
    }
    getSelection() {
      return this.selection;
    }
    dispose() {}
  },
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit() {}
  },
}));
vi.mock("../lib/stores/sessions.svelte", () => ({
  answer: vi.fn(),
  connect: vi.fn(),
  cycleTab: vi.fn(),
  resize: vi.fn(),
  write: vi.fn(),
}));

import type { SessionEvent } from "../lib/bindings/SessionEvent";
import { RECONNECT_DELAYS } from "../lib/reconnect";
import { initialTab, type TabState } from "../lib/sessionReducer";
import { connect } from "../lib/stores/sessions.svelte";
import { settings } from "../lib/stores/settings.svelte";
import { snippets } from "../lib/stores/snippets.svelte";
import TerminalTab from "./TerminalTab.svelte";

beforeEach(() => {
  made.terms.length = 0;
  Object.assign(settings, {
    terminal_theme: "app",
    cursor_style: "block",
    cursor_blink: true,
    scrollback: 10000,
    copy_on_select: false,
    auto_reconnect: true,
  });
  vi.stubGlobal("matchMedia", () => ({
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
});

const setup = () => {
  render(TerminalTab, {
    tab: initialTab("t1", "c1"),
    active: true,
    onedit: vi.fn(),
    onsave: vi.fn(),
    onclose: vi.fn(),
  });
  return made.terms[0];
};

test("changing cursor style updates open terminals", () => {
  const term = setup();
  expect(term.options.cursorStyle).toBe("block");
  settings.cursor_style = "bar";
  settings.cursor_blink = false;
  settings.scrollback = 2000;
  settings.terminal_theme = "nord";
  flushSync();
  expect(made.terms).toHaveLength(1);
  expect(term.options).toMatchObject({
    cursorStyle: "bar",
    cursorBlink: false,
    scrollback: 2000,
    theme: { background: "#2e3440" },
  });
});

test("copy on select copies a non-empty selection", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  const term = setup();
  term.selection = "ls -l";
  term.onSelect?.();
  expect(writeText).not.toHaveBeenCalled();
  settings.copy_on_select = true;
  term.onSelect?.();
  expect(writeText).toHaveBeenCalledWith("ls -l");
  term.selection = "";
  term.onSelect?.();
  expect(writeText).toHaveBeenCalledTimes(1);
});

test("copy on select ignores a rejected clipboard write", async () => {
  const writeText = vi.fn().mockRejectedValue(new Error("denied"));
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  const unhandled = vi.fn();
  process.on("unhandledRejection", unhandled);
  settings.copy_on_select = true;
  const term = setup();
  term.selection = "ls";
  term.onSelect?.();
  await new Promise((r) => setTimeout(r, 0));
  process.off("unhandledRejection", unhandled);
  expect(writeText).toHaveBeenCalledWith("ls");
  expect(unhandled).not.toHaveBeenCalled();
});

// Reconnection: the mocked connect hands us each session's event callback.
const emit = (e: SessionEvent) => {
  vi.mocked(connect).mock.lastCall![4](e);
  flushSync();
};
const ready = () => emit({ type: "Status", stage: "Ready" });
const drop = (code = "network.timeout") =>
  emit({
    type: "Closed",
    reason: { type: "Error", error: { code, params: {} } },
  });
const attempts = () => screen.queryByText(/Reconnecting/)?.textContent;

test("retries after 1, 2, 5, 10, 30 s and stops after 5", async () => {
  vi.useFakeTimers();
  vi.mocked(connect).mockClear();
  setup();
  ready();
  for (const [i, ms] of RECONNECT_DELAYS.entries()) {
    drop();
    await vi.advanceTimersByTimeAsync(ms - 1);
    expect(connect).toHaveBeenCalledTimes(i + 1);
    await vi.advanceTimersByTimeAsync(1);
    expect(connect).toHaveBeenCalledTimes(i + 2);
  }
  drop();
  expect(attempts()).toBeUndefined();
  await vi.advanceTimersByTimeAsync(60000);
  expect(connect).toHaveBeenCalledTimes(RECONNECT_DELAYS.length + 1);
  vi.useRealTimers();
});

test("shows the attempt and Cancel stops the retries", async () => {
  vi.useFakeTimers();
  vi.mocked(connect).mockClear();
  const { rerender } = render(TerminalTab, {
    tab: initialTab("t1", "c1"),
    active: true,
    onedit: vi.fn(),
    onsave: vi.fn(),
    onclose: vi.fn(),
  });
  ready();
  drop();
  expect(attempts()).toBe("Reconnecting\u2026 attempt 1 of 5");
  const cancel = screen.getByRole("button", { name: "Cancel" });
  expect(document.activeElement).toBe(cancel);
  // The reducer would have stored the reason on the tab.
  await rerender({
    tab: {
      ...initialTab("t1", "c1"),
      phase: "closed",
      closed: { type: "Error", error: { code: "network.timeout", params: {} } },
    } satisfies TabState,
  });
  await fireEvent.click(cancel);
  await vi.advanceTimersByTimeAsync(60000);
  expect(connect).toHaveBeenCalledTimes(1);
  expect(attempts()).toBeUndefined();
  expect(screen.getByRole("alertdialog")).toBeTruthy();
  vi.useRealTimers();
});

test("a successful retry clears the overlay and restarts the count", async () => {
  vi.useFakeTimers();
  vi.mocked(connect).mockClear();
  setup();
  ready();
  drop();
  await vi.advanceTimersByTimeAsync(1000);
  expect(attempts()).toBeUndefined();
  ready();
  drop();
  expect(attempts()).toBe("Reconnecting\u2026 attempt 1 of 5");
  vi.useRealTimers();
});

test("a failed retry counts as the next attempt", async () => {
  vi.useFakeTimers();
  vi.mocked(connect).mockClear();
  setup();
  ready();
  drop();
  await vi.advanceTimersByTimeAsync(1000);
  drop("network.refused");
  expect(attempts()).toBe("Reconnecting\u2026 attempt 2 of 5");
  vi.useRealTimers();
});

test.each([
  ["a session that never got ready", "network.timeout", true, false],
  ["an authentication error", "auth.all_failed", true, true],
  ["auto reconnect off", "network.timeout", false, true],
])("no retry for %s", async (_, code, enabled, wasReady) => {
  vi.useFakeTimers();
  vi.mocked(connect).mockClear();
  settings.auto_reconnect = enabled;
  setup();
  if (wasReady) ready();
  drop(code);
  await vi.advanceTimersByTimeAsync(60000);
  expect(connect).toHaveBeenCalledTimes(1);
  expect(attempts()).toBeUndefined();
  vi.useRealTimers();
});

test("unmounting cancels the pending retry", async () => {
  vi.useFakeTimers();
  vi.mocked(connect).mockClear();
  const { unmount } = render(TerminalTab, {
    tab: initialTab("t1", "c1"),
    active: true,
    onedit: vi.fn(),
    onsave: vi.fn(),
    onclose: vi.fn(),
  });
  ready();
  drop();
  unmount();
  await vi.advanceTimersByTimeAsync(60000);
  expect(connect).toHaveBeenCalledTimes(1);
  vi.useRealTimers();
});

test("turning auto reconnect off during the wait stops the chain", async () => {
  vi.useFakeTimers();
  vi.mocked(connect).mockClear();
  const { rerender } = render(TerminalTab, {
    tab: initialTab("t1", "c1"),
    active: true,
    onedit: vi.fn(),
    onsave: vi.fn(),
    onclose: vi.fn(),
  });
  ready();
  drop();
  await rerender({
    tab: {
      ...initialTab("t1", "c1"),
      phase: "closed",
      closed: { type: "Error", error: { code: "network.timeout", params: {} } },
    } satisfies TabState,
  });
  settings.auto_reconnect = false;
  await vi.advanceTimersByTimeAsync(1000);
  expect(connect).toHaveBeenCalledTimes(1);
  expect(attempts()).toBeUndefined();
  expect(screen.getByRole("alertdialog")).toBeTruthy();
  vi.useRealTimers();
});

test("a second Closed during the wait does not add a second retry", async () => {
  vi.useFakeTimers();
  vi.mocked(connect).mockClear();
  setup();
  ready();
  drop();
  drop();
  await vi.advanceTimersByTimeAsync(60000);
  expect(connect).toHaveBeenCalledTimes(2);
  vi.useRealTimers();
});

test("Ctrl+Shift+W asks the parent to close the tab", async () => {
  const onclose = vi.fn();
  render(TerminalTab, {
    tab: initialTab("t1", "c1"),
    active: true,
    onedit: vi.fn(),
    onsave: vi.fn(),
    onclose,
  });
  await fireEvent.keyDown(document.body, {
    key: "W",
    ctrlKey: true,
    shiftKey: true,
  });
  expect(onclose).toHaveBeenCalledWith("t1");
});

// Snippet palette.
const readyTab = { ...initialTab("t1", "c1"), phase: "ready" } as TabState;
const props = (tab: TabState) => ({
  tab,
  active: true,
  onedit: vi.fn(),
  onsave: vi.fn(),
  onclose: vi.fn(),
});
const shortcut = () =>
  fireEvent.keyDown(document.body, { key: "P", ctrlKey: true, shiftKey: true });
const pick = async (text: string, run: boolean) => {
  snippets.list = [{ id: "s1", name: "S", text, run }];
  await shortcut();
  await fireEvent.click(screen.getByRole("option"));
};

test("Ctrl+Shift+P opens the palette on a ready tab only", async () => {
  const { rerender } = render(TerminalTab, props(initialTab("t1", "c1")));
  await shortcut();
  expect(screen.queryByRole("dialog", { name: "Snippets" })).toBeNull();
  await rerender(props(readyTab));
  await shortcut();
  expect(screen.getByRole("dialog", { name: "Snippets" })).toBeTruthy();
});

test("a snippet is pasted without its trailing newline and not run", async () => {
  const term = setup2();
  await pick("ls\n", false);
  expect(term.paste).toHaveBeenCalledWith("ls");
  expect(term.input).not.toHaveBeenCalled();
  expect(screen.queryByRole("dialog", { name: "Snippets" })).toBeNull();
  expect(term.focus).toHaveBeenCalled();
});

test("a snippet that runs sends Enter after the paste", async () => {
  const term = setup2();
  await pick("ls", true);
  expect(term.paste).toHaveBeenCalledWith("ls");
  expect(term.input).toHaveBeenCalledWith("\r");
});

test("several lines without bracketed paste ask first", async () => {
  const term = setup2();
  await pick("a\nb", false);
  expect(screen.getByRole("alertdialog")).toBeTruthy();
  await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(term.paste).not.toHaveBeenCalled();
  expect(screen.queryByRole("alertdialog")).toBeNull();
  await pick("a\nb", false);
  await fireEvent.click(screen.getByRole("button", { name: "Paste" }));
  expect(term.paste).toHaveBeenCalledWith("a\nb");
});

test("several lines with bracketed paste go straight through", async () => {
  const term = setup2();
  term.modes.bracketedPasteMode = true;
  await pick("a\nb", false);
  expect(screen.queryByRole("alertdialog")).toBeNull();
  expect(term.paste).toHaveBeenCalledWith("a\nb");
});

test("the palette closes when the session drops, and nothing is sent", async () => {
  const term = setup2();
  const { rerender } = view;
  snippets.list = [{ id: "s1", name: "S", text: "ls", run: true }];
  await shortcut();
  await rerender({ tab: { ...readyTab, phase: "closed" } });
  expect(screen.queryByRole("dialog", { name: "Snippets" })).toBeNull();
  expect(term.paste).not.toHaveBeenCalled();
  expect(term.input).not.toHaveBeenCalled();
});

test("Esc in the palette gives the focus back to the terminal", async () => {
  const term = setup2();
  await shortcut();
  term.focus.mockClear();
  await fireEvent.keyDown(screen.getByRole("combobox"), { key: "Escape" });
  expect(screen.queryByRole("dialog", { name: "Snippets" })).toBeNull();
  expect(term.focus).toHaveBeenCalled();
});

let view: ReturnType<typeof render>;
function setup2() {
  view = render(TerminalTab, props(readyTab));
  return made.terms[0];
}
