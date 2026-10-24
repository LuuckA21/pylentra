import { beforeEach, expect, test, vi } from "vitest";
import { render } from "@testing-library/svelte";
import { flushSync } from "svelte";

// xterm needs a real browser: a fake that records its options and listeners.
const made = vi.hoisted(() => ({
  terms: [] as {
    options: Record<string, unknown>;
    selection: string;
    onSelect?: () => void;
  }[],
}));
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    options: Record<string, unknown>;
    cols = 80;
    rows = 24;
    selection = "";
    onSelect?: () => void;
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
    focus() {}
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
  closeTab: vi.fn(),
  connect: vi.fn(),
  cycleTab: vi.fn(),
  resize: vi.fn(),
  write: vi.fn(),
}));

import { initialTab } from "../lib/sessionReducer";
import { settings } from "../lib/stores/settings.svelte";
import TerminalTab from "./TerminalTab.svelte";

beforeEach(() => {
  made.terms.length = 0;
  Object.assign(settings, {
    terminal_theme: "app",
    cursor_style: "block",
    cursor_blink: true,
    scrollback: 10000,
    copy_on_select: false,
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
