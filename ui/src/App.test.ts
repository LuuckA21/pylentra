import { beforeEach, expect, test, vi } from "vitest";
import { render, screen } from "@testing-library/svelte";

vi.mock("./lib/api", () => ({
  api: {
    libraryGet: vi.fn().mockResolvedValue({ groups: [], connections: [] }),
    startupWarnings: vi.fn().mockResolvedValue([]),
    settingsGet: vi.fn().mockResolvedValue({}),
  },
}));
vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn().mockResolvedValue(null),
}));
vi.mock("@tauri-apps/plugin-log", () => ({ warn: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
// xterm needs a real browser; the tab strip is what is under test.
vi.mock("./components/TerminalTab.svelte", () => ({ default: () => {} }));

import App from "./App.svelte";
import { openTab, sessions } from "./lib/stores/sessions.svelte";

beforeEach(() => {
  sessions.tabs.splice(0);
  sessions.active = null;
});

test("the close button of an inactive tab stays focusable", () => {
  openTab("c1");
  openTab("c2");
  render(App);
  const [first] = screen.getAllByRole("button", { name: "Close tab" });
  first.focus();
  expect(document.activeElement).toBe(first);
});
