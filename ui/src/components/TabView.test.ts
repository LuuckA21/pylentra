import { beforeEach, expect, test, vi } from "vitest";
import { render } from "@testing-library/svelte";

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
vi.mock("../lib/api", () => ({
  api: {
    sessionOpen: vi.fn(() => new Promise(() => {})),
    sessionClose: vi.fn(),
  },
}));

import { openTab, sessions } from "../lib/stores/sessions.svelte";
import TabView from "./TabView.svelte";

beforeEach(() => {
  sessions.tabs.splice(0);
  sessions.panes.splice(0);
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

const show = (visible: boolean) => {
  openTab("c1");
  return render(TabView, {
    tab: sessions.tabs[0],
    visible,
    onedit: vi.fn(),
    onsave: vi.fn(),
    onclose: vi.fn(),
  });
};

test("a single pane fills the tab", () => {
  const { container } = show(true);
  const slots = container.querySelectorAll<HTMLElement>(".slot");
  expect(slots).toHaveLength(1);
  const { left, top, width, height } = slots[0].style;
  expect({ left, top, width, height }).toEqual({
    left: "0%",
    top: "0%",
    width: "100%",
    height: "100%",
  });
  expect(made.terms).toHaveLength(1);
});

test("a tab that is not visible is hidden", () => {
  const { container } = show(false);
  expect((container.firstElementChild as HTMLElement).hidden).toBe(true);
});
