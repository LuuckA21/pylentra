import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import { flushSync } from "svelte";

// xterm needs a real browser: a fake that records its options and listeners.
const made = vi.hoisted(() => ({
  terms: [] as {
    options: Record<string, unknown>;
    selection: string;
    onSelect?: () => void;
    keyHandler?: (e: KeyboardEvent) => boolean;
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
    keyHandler?: (e: KeyboardEvent) => boolean;
    attachCustomKeyEventHandler(cb: (e: KeyboardEvent) => boolean) {
      this.keyHandler = cb;
    }
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
  focusPane: vi.fn(),
  moveFocus: vi.fn(),
  resize: vi.fn(),
  write: vi.fn(),
}));

import type { SessionEvent } from "../lib/bindings/SessionEvent";
import { RECONNECT_DELAYS } from "../lib/reconnect";
import { initialPane, type PaneState } from "../lib/sessionReducer";
import {
  connect,
  cycleTab,
  focusPane,
  moveFocus,
} from "../lib/stores/sessions.svelte";
import { settings } from "../lib/stores/settings.svelte";
import { snippets } from "../lib/stores/snippets.svelte";
import TerminalPane from "./TerminalPane.svelte";

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
  render(TerminalPane, {
    pane: initialPane("t1", "c1"),
    visible: true,
    focused: true,
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
  const { rerender } = render(TerminalPane, {
    pane: initialPane("t1", "c1"),
    visible: true,
    focused: true,
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
    pane: {
      ...initialPane("t1", "c1"),
      phase: "closed",
      closed: { type: "Error", error: { code: "network.timeout", params: {} } },
    } satisfies PaneState,
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
  const { unmount } = render(TerminalPane, {
    pane: initialPane("t1", "c1"),
    visible: true,
    focused: true,
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
  const { rerender } = render(TerminalPane, {
    pane: initialPane("t1", "c1"),
    visible: true,
    focused: true,
    onedit: vi.fn(),
    onsave: vi.fn(),
    onclose: vi.fn(),
  });
  ready();
  drop();
  await rerender({
    pane: {
      ...initialPane("t1", "c1"),
      phase: "closed",
      closed: { type: "Error", error: { code: "network.timeout", params: {} } },
    } satisfies PaneState,
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

test("Ctrl+Shift+W asks the parent to close the pane", async () => {
  const onclose = vi.fn();
  render(TerminalPane, {
    pane: initialPane("t1", "c1"),
    visible: true,
    focused: true,
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
const readyPane = { ...initialPane("t1", "c1"), phase: "ready" } as PaneState;
const props = (pane: PaneState) => ({
  pane,
  visible: true,
  focused: true,
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
  const { rerender } = render(TerminalPane, props(initialPane("t1", "c1")));
  await shortcut();
  expect(screen.queryByRole("dialog", { name: "Snippets" })).toBeNull();
  await rerender(props(readyPane));
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
  await rerender({ pane: { ...readyPane, phase: "closed" } });
  expect(screen.queryByRole("dialog", { name: "Snippets" })).toBeNull();
  expect(term.paste).not.toHaveBeenCalled();
  expect(term.input).not.toHaveBeenCalled();
});

test("a pending multi-line confirm goes away when the session drops", async () => {
  const term = setup2();
  await pick("a\nb", false);
  expect(screen.getByRole("alertdialog")).toBeTruthy();
  await view.rerender({ pane: { ...readyPane, phase: "closed" } });
  expect(screen.queryByRole("alertdialog")).toBeNull();
  expect(term.paste).not.toHaveBeenCalled();
});

const dlg = () => screen.getByRole("dialog", { name: "Values for S" });
const typeVar = (label: string, value: string) =>
  fireEvent.input(screen.getByLabelText(label), { target: { value } });

test("a snippet with a variable asks, then pastes the filled text", async () => {
  const term = setup2();
  await pick("tail {{s}}", false);
  expect(dlg()).toBeTruthy();
  expect(term.paste).not.toHaveBeenCalled();
  await typeVar("s", "app");
  await fireEvent.submit(dlg().querySelector("form")!);
  expect(term.paste).toHaveBeenCalledWith("tail app");
  expect(screen.queryByRole("dialog", { name: "Values for S" })).toBeNull();
});

test("a variable snippet that runs sends Enter after the filled paste", async () => {
  const term = setup2();
  await pick("tail {{s}}", true);
  await typeVar("s", "app");
  await fireEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(term.paste).toHaveBeenCalledWith("tail app");
  expect(term.input).toHaveBeenCalledWith("\r");
});

test("cancelling the variables dialog sends nothing and focuses the terminal", async () => {
  const term = setup2();
  await pick("tail {{s}}", false);
  term.focus.mockClear();
  await fireEvent.keyDown(screen.getByLabelText("s"), { key: "Escape" });
  expect(screen.queryByRole("dialog", { name: "Values for S" })).toBeNull();
  expect(term.paste).not.toHaveBeenCalled();
  expect(term.focus).toHaveBeenCalled();
});

test("the dialog proposes the last values", async () => {
  setup2();
  await pick("tail {{r}}", false);
  await typeVar("r", "app");
  await fireEvent.click(screen.getByRole("button", { name: "Send" }));
  await shortcut();
  await fireEvent.click(screen.getByRole("option"));
  expect((screen.getByLabelText("r") as HTMLInputElement).value).toBe("app");
});

test("a snippet without variables skips the dialog", async () => {
  const term = setup2();
  await pick("tail x", false);
  expect(screen.queryByRole("dialog", { name: "Values for S" })).toBeNull();
  expect(term.paste).toHaveBeenCalledWith("tail x");
});

test("the {{{{ escape applies to a snippet without variables", async () => {
  const term = setup2();
  await pick("echo {{{{x}}", false);
  expect(screen.queryByRole("dialog", { name: "Values for S" })).toBeNull();
  expect(term.paste).toHaveBeenCalledWith("echo {{x}}");
});

// An <input> strips newlines from values, so the lines come from the snippet.
test("a filled multi-line snippet still asks for confirmation", async () => {
  const term = setup2();
  await pick("echo {{m}}\nls", false);
  await typeVar("m", "x");
  await fireEvent.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByRole("alertdialog")).toBeTruthy();
  expect(term.paste).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole("button", { name: "Paste" }));
  expect(term.paste).toHaveBeenCalledWith("echo x\nls");
});

test("the variables dialog goes away when the session drops", async () => {
  const term = setup2();
  await pick("tail {{s}}", false);
  await view.rerender({ pane: { ...readyPane, phase: "closed" } });
  expect(screen.queryByRole("dialog", { name: "Values for S" })).toBeNull();
  expect(term.paste).not.toHaveBeenCalled();
});

test("tab shortcuts do not act behind the variables dialog", async () => {
  const p = props(readyPane);
  render(TerminalPane, p);
  await pick("tail {{s}}", false);
  await fireEvent.keyDown(document.body, {
    key: "W",
    ctrlKey: true,
    shiftKey: true,
  });
  expect(dlg()).toBeTruthy();
  expect(p.onclose).not.toHaveBeenCalled();
});

test("switching to another tab closes the palette", async () => {
  setup2();
  await shortcut();
  expect(screen.getByRole("dialog", { name: "Snippets" })).toBeTruthy();
  await view.rerender({ visible: false });
  expect(screen.queryByRole("dialog", { name: "Snippets" })).toBeNull();
});

test("Esc in the palette gives the focus back to the terminal", async () => {
  const term = setup2();
  await shortcut();
  term.focus.mockClear();
  await fireEvent.keyDown(screen.getByRole("combobox"), { key: "Escape" });
  expect(screen.queryByRole("dialog", { name: "Snippets" })).toBeNull();
  expect(term.focus).toHaveBeenCalled();
});

test("tab shortcuts do not act behind the new-snippet form", async () => {
  const p = props(readyPane);
  render(TerminalPane, p);
  await shortcut();
  await fireEvent.click(screen.getByText("New snippet"));
  expect(screen.getByRole("dialog", { name: "New snippet" })).toBeTruthy();
  await fireEvent.keyDown(document.body, { key: "Tab", ctrlKey: true });
  await fireEvent.keyDown(document.body, {
    key: "W",
    ctrlKey: true,
    shiftKey: true,
  });
  expect(cycleTab).not.toHaveBeenCalled();
  expect(p.onclose).not.toHaveBeenCalled();
});

test("a visible pane without the focus ignores pane shortcuts", async () => {
  const p = { ...props(readyPane), focused: false };
  render(TerminalPane, p);
  await fireEvent.keyDown(document.body, {
    key: "W",
    ctrlKey: true,
    shiftKey: true,
  });
  await shortcut();
  expect(p.onclose).not.toHaveBeenCalled();
  expect(screen.queryByRole("dialog", { name: "Snippets" })).toBeNull();
  expect(made.terms[0].focus).not.toHaveBeenCalled();
});

test("opening the forwards panel closes the palette", async () => {
  setup2();
  await shortcut();
  expect(screen.getByRole("dialog", { name: "Snippets" })).toBeTruthy();
  await fireEvent.click(screen.getByRole("button", { name: "Forward" }));
  expect(screen.queryByRole("dialog", { name: "Snippets" })).toBeNull();
  expect(screen.getByRole("dialog", { name: "Port forwarding" })).toBeTruthy();
});

let view: ReturnType<typeof render>;
function setup2() {
  view = render(TerminalPane, props(readyPane));
  return made.terms[0];
}

test("xterm leaves F11 to the window", () => {
  const term = setup();
  expect(term.keyHandler?.(new KeyboardEvent("keydown", { key: "F11" }))).toBe(
    false,
  );
  expect(term.keyHandler?.(new KeyboardEvent("keydown", { key: "a" }))).toBe(
    true,
  );
});

const splitKey = () =>
  fireEvent.keyDown(document.body, { key: "D", ctrlKey: true, shiftKey: true });

test("Ctrl+Shift+D asks to split the focused pane to the right", async () => {
  const p = { ...props(readyPane), canSplit: true, onsplit: vi.fn() };
  render(TerminalPane, p);
  await splitKey();
  await fireEvent.keyDown(document.body, {
    key: "E",
    ctrlKey: true,
    shiftKey: true,
  });
  expect(p.onsplit.mock.calls).toEqual([
    ["t1", "row"],
    ["t1", "column"],
  ]);
});

test.each([
  ["not focused", { focused: false }],
  ["hidden", { visible: false }],
  ["full", { canSplit: false }],
])("a pane that is %s does not split", async (_, extra) => {
  const p = { ...props(readyPane), canSplit: true, onsplit: vi.fn(), ...extra };
  render(TerminalPane, p);
  await splitKey();
  expect(p.onsplit).not.toHaveBeenCalled();
});

test("Alt+arrows in the focused pane move the focus", async () => {
  vi.mocked(moveFocus).mockClear();
  render(TerminalPane, props(readyPane));
  await fireEvent.keyDown(document.body, { key: "ArrowRight", altKey: true });
  await fireEvent.keyDown(document.body, { key: "ArrowUp", altKey: true });
  expect(vi.mocked(moveFocus).mock.calls).toEqual([["right"], ["up"]]);
});

test("Alt+arrows in a pane without the focus do nothing", async () => {
  vi.mocked(moveFocus).mockClear();
  render(TerminalPane, { ...props(readyPane), focused: false });
  await fireEvent.keyDown(document.body, { key: "ArrowRight", altKey: true });
  expect(moveFocus).not.toHaveBeenCalled();
});

test("focusing or pressing inside the pane focuses it", async () => {
  vi.mocked(focusPane).mockClear();
  const { container } = render(TerminalPane, {
    ...props(readyPane),
    focused: false,
  });
  const terminal = container.querySelector(".terminal")!;
  await fireEvent.focusIn(terminal);
  await fireEvent.pointerDown(terminal);
  expect(vi.mocked(focusPane).mock.calls).toEqual([["t1"], ["t1"]]);
});

test("the terminal takes the keyboard focus when its pane gets the focus", async () => {
  const p = { ...props(readyPane), focused: false };
  const { rerender } = render(TerminalPane, p);
  expect(made.terms[0].focus).not.toHaveBeenCalled();
  await rerender({ ...p, focused: true });
  expect(made.terms[0].focus).toHaveBeenCalled();
});
