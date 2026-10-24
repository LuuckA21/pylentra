import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";

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
const win = vi.hoisted(() => ({
  closeRequested: undefined as
    ((e: { preventDefault: () => void }) => void) | undefined,
  unlisten: vi.fn(),
  destroy: vi.fn(),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    onCloseRequested: (cb: typeof win.closeRequested) => {
      win.closeRequested = cb;
      return Promise.resolve(win.unlisten);
    },
    destroy: win.destroy,
  }),
}));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
// xterm needs a real browser; the tab strip is what is under test.
vi.mock("./components/TerminalTab.svelte", () => ({ default: () => {} }));

import { check } from "@tauri-apps/plugin-updater";
import { warn } from "@tauri-apps/plugin-log";
import App from "./App.svelte";
import { api } from "./lib/api";
import { setLang } from "./lib/i18n";
import { openTab, sessions } from "./lib/stores/sessions.svelte";
import { settings } from "./lib/stores/settings.svelte";
import { toasts } from "./lib/stores/toasts.svelte";
import { updates } from "./lib/stores/updates.svelte";

beforeEach(() => {
  vi.mocked(check).mockClear();
  vi.mocked(warn).mockClear();
  win.destroy.mockClear();
  win.unlisten.mockClear();
  sessions.tabs.splice(0);
  sessions.active = null;
  toasts.splice(0);
  updates.available = null;
  Object.assign(settings, { confirm_close_active: true, check_updates: true });
  setLang("en");
});

const openReady = () => {
  openTab("c1");
  sessions.tabs[0].phase = "ready";
};
const closeX = () =>
  fireEvent.click(screen.getByRole("button", { name: "Close tab" }));

test("the close button of an inactive tab stays focusable", () => {
  openTab("c1");
  openTab("c2");
  render(App);
  const [first] = screen.getAllByRole("button", { name: "Close tab" });
  first.focus();
  expect(document.activeElement).toBe(first);
});

test("closing a ready tab asks first; Cancel keeps it, Close removes it", async () => {
  openReady();
  render(App);
  await closeX();
  const dialog = screen.getByRole("alertdialog");
  expect(dialog.textContent).toContain("Close this session?");
  await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("alertdialog")).toBeNull();
  expect(sessions.tabs).toHaveLength(1);

  await closeX();
  await fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(sessions.tabs).toHaveLength(0);
});

test("closing a closed tab, or with the setting off, does not ask", async () => {
  openTab("c1");
  sessions.tabs[0].phase = "closed";
  render(App);
  await closeX();
  expect(screen.queryByRole("alertdialog")).toBeNull();
  expect(sessions.tabs).toHaveLength(0);

  openReady();
  settings.confirm_close_active = false;
  await screen.findByRole("button", { name: "Close tab" });
  await closeX();
  expect(screen.queryByRole("alertdialog")).toBeNull();
  expect(sessions.tabs).toHaveLength(0);
});

test("the window close is held back while a session is ready", async () => {
  openReady();
  render(App);
  await waitFor(() => expect(win.closeRequested).toBeDefined());
  const preventDefault = vi.fn();
  win.closeRequested?.({ preventDefault });
  expect(preventDefault).toHaveBeenCalled();
  expect(await screen.findByText("1 session is still connected.")).toBeTruthy();
  expect(win.destroy).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(win.destroy).toHaveBeenCalled();
});

test("the window closes freely without ready sessions or with the setting off", async () => {
  render(App);
  await waitFor(() => expect(win.closeRequested).toBeDefined());
  const preventDefault = vi.fn();
  win.closeRequested?.({ preventDefault });
  openReady();
  settings.confirm_close_active = false;
  win.closeRequested?.({ preventDefault });
  expect(preventDefault).not.toHaveBeenCalled();
});

test("unmounting stops listening for window close", async () => {
  const { unmount } = render(App);
  await waitFor(() => expect(win.closeRequested).toBeDefined());
  unmount();
  await waitFor(() => expect(win.unlisten).toHaveBeenCalled());
});

test("startup check stores the update; failures only warn", async () => {
  vi.mocked(check).mockResolvedValueOnce({ version: "9.9.9" } as never);
  const { unmount } = render(App);
  await waitFor(() => expect(updates.available?.version).toBe("9.9.9"));
  expect(screen.getByText("Version 9.9.9 available")).toBeTruthy();
  unmount();

  updates.available = null;
  vi.mocked(check).mockRejectedValueOnce(new Error("offline"));
  render(App);
  await waitFor(() =>
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("offline")),
  );
  expect(toasts).toHaveLength(0);
});

test("startup check is skipped when check_updates is off", async () => {
  vi.mocked(api.settingsGet).mockResolvedValueOnce({
    check_updates: false,
  } as never);
  render(App);
  await waitFor(() => expect(settings.check_updates).toBe(false));
  await new Promise((r) => setTimeout(r, 0));
  expect(check).not.toHaveBeenCalled();
});
