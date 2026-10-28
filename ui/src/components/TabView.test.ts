import { beforeEach, expect, test, vi } from "vitest";
import { render, screen, within } from "@testing-library/svelte";
import { tick } from "svelte";

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
    textarea: HTMLTextAreaElement;
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
    textarea = document.createElement("textarea");
    focus = vi.fn(() => this.textarea.focus());
    modes = { bracketedPasteMode: false };
    constructor(options: Record<string, unknown>) {
      this.options = options;
      made.terms.push(this);
    }
    loadAddon() {}
    open(parent: HTMLElement) {
      parent.append(this.textarea);
    }
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

import type { PaneState } from "../lib/sessionReducer";
import {
  focusPane,
  moveFocus,
  openTab,
  sessions,
  splitPane,
} from "../lib/stores/sessions.svelte";
import TabView from "./TabView.svelte";

beforeEach(() => {
  sessions.tabs.splice(0);
  sessions.panes.splice(0);
  sessions.active = null;
  made.terms.splice(0);
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
    onsplit: vi.fn(),
  });
};

const box = (el: HTMLElement) => {
  const { left, top, width, height } = el.style;
  return { left, top, width, height };
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

test("a split shows both panes side by side without recreating the first", async () => {
  const { container } = show(true);
  splitPane(sessions.panes[0].paneId, "row", "c2");
  await tick();
  const slots = container.querySelectorAll<HTMLElement>(".slot");
  expect([...slots].map(box)).toEqual([
    { left: "0%", top: "0%", width: "50%", height: "100%" },
    { left: "50%", top: "0%", width: "50%", height: "100%" },
  ]);
  expect(made.terms).toHaveLength(2);
});

test("a prompt stays in the pane it belongs to after a split", async () => {
  const { container } = show(true);
  sessions.panes[0].prompt = {
    type: "SecretPrompt",
    host: "h",
    port: 22,
    kind: { type: "Password" },
    can_save: false,
  };
  await tick();
  const prompt = screen.getByRole("dialog");
  splitPane(sessions.panes[0].paneId, "row", "c2");
  await tick();
  const slots = container.querySelectorAll<HTMLElement>(".slot");
  expect(within(slots[0]).getByRole("dialog")).toBe(prompt);
  expect(within(slots[1]).queryByRole("dialog")).toBeNull();
});

// Two panes side by side, A focused (its terminal has the keyboard).
const twoPanes = async () => {
  const { container } = show(true);
  const a = sessions.panes[0].paneId;
  const b = splitPane(a, "row", "c2")!;
  focusPane(a);
  await tick();
  const [slotA, slotB] = container.querySelectorAll<HTMLElement>(".slot");
  expect(slotA.contains(document.activeElement)).toBe(true);
  return { a, b, slotA, slotB };
};

const prompts: [string, NonNullable<PaneState["prompt"]>][] = [
  [
    "SecretPrompt",
    {
      type: "SecretPrompt",
      host: "h",
      port: 22,
      kind: { type: "Password" },
      can_save: false,
    },
  ],
  [
    "HostKeyPrompt",
    {
      type: "HostKeyPrompt",
      host: "h",
      port: 22,
      algo: "ssh-ed25519",
      fingerprint: "SHA256:x",
    },
  ],
];

test.each(prompts)(
  "a %s in a pane without the focus waits for it",
  async (_, prompt) => {
    const { a, b, slotA, slotB } = await twoPanes();
    sessions.panes[1].prompt = prompt;
    await tick();
    const dialog = within(slotB).getByRole("dialog");
    expect(slotA.contains(document.activeElement)).toBe(true);
    expect(sessions.tabs[0].focus).toBe(a);

    focusPane(b);
    await tick();
    expect(dialog.contains(document.activeElement)).toBe(true);

    // Once answered, the keyboard goes to the pane's terminal.
    sessions.panes[1].prompt = undefined;
    await tick();
    expect(document.activeElement).toBe(made.terms[1].textarea);
  },
);

test("a prompt in the focused pane takes the focus at once", async () => {
  const { slotA } = await twoPanes();
  sessions.panes[0].prompt = prompts[0][1];
  await tick();
  const dialog = within(slotA).getByRole("dialog");
  expect(dialog.contains(document.activeElement)).toBe(true);
});

test("a prompt closing in a pane the focus has left does not take it back", async () => {
  const { b, slotB } = await twoPanes();
  sessions.panes[0].prompt = prompts[0][1];
  await tick();
  moveFocus("right");
  await tick();
  expect(slotB.contains(document.activeElement)).toBe(true);
  sessions.panes[0].prompt = undefined;
  await tick();
  expect(slotB.contains(document.activeElement)).toBe(true);
  expect(sessions.tabs[0].focus).toBe(b);
});
