import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";

vi.mock("../lib/api", () => ({
  api: {
    sftpList: vi.fn(),
    sftpDownload: vi.fn(),
    sftpUploadPick: vi.fn(),
    sftpUploadDrop: vi.fn(),
    settingsSave: vi.fn(),
  },
}));

import { api } from "../lib/api";
import type { Entry } from "../lib/bindings/Entry";
import { initialPane, type PaneState } from "../lib/sessionReducer";
import { settings } from "../lib/stores/settings.svelte";
import { toasts } from "../lib/stores/toasts.svelte";
import FilePanel from "./FilePanel.svelte";

const pane: PaneState = {
  ...initialPane("t1", "c1"),
  sessionId: "s1",
  phase: "ready",
};

const file = (name: string, size = 10): Entry => ({
  name,
  kind: "File",
  size,
  modified: 1_700_000_000,
});
const dir = (name: string): Entry => ({
  name,
  kind: "Dir",
  size: null,
  modified: null,
});

const home = [file("b.txt", 2048), dir("zeta"), file("a.txt"), dir("alpha")];

beforeEach(() => {
  vi.resetAllMocks();
  settings.sftp_show_hidden = false;
  vi.mocked(api.settingsSave).mockResolvedValue();
  vi.mocked(api.sftpDownload).mockResolvedValue(1);
  vi.mocked(api.sftpUploadPick).mockResolvedValue(1);
  vi.mocked(api.sftpList).mockImplementation(async (_s, d) => ({
    dir: d === "" ? "/home/u" : d,
    entries: d === "" ? [...home, file(".bashrc")] : [file("inner")],
    truncated: false,
  }));
});

const names = () =>
  screen
    .getAllByRole("option")
    .map((o) => o.querySelector(".name")!.textContent);

async function shown() {
  render(FilePanel, { pane });
  await waitFor(() =>
    expect(screen.getAllByRole("option")).not.toHaveLength(0),
  );
}

test("lists folders first, then files, by name, from the home folder", async () => {
  await shown();
  expect(api.sftpList).toHaveBeenCalledWith("s1", "");
  expect(names()).toEqual(["alpha", "zeta", "a.txt", "b.txt"]);
  expect(screen.getByText("/home/u")).toBeTruthy();
  expect(screen.getByText("2.0 KiB")).toBeTruthy();
  expect(document.querySelector("[data-sftp-pane='t1']")).toBeTruthy();
});

test("hidden entries stay out until the switch is on, which saves the setting", async () => {
  await shown();
  expect(names()).not.toContain(".bashrc");
  await fireEvent.click(screen.getByRole("switch", { name: "Show hidden" }));
  await waitFor(() => expect(names()).toContain(".bashrc"));
  expect(api.settingsSave).toHaveBeenCalledWith(
    expect.objectContaining({ sftp_show_hidden: true }),
  );
});

test("a double click on a folder lists the joined path", async () => {
  await shown();
  await fireEvent.dblClick(screen.getByText("zeta"));
  expect(api.sftpList).toHaveBeenLastCalledWith("s1", "/home/u/zeta");
  await waitFor(() => expect(names()).toEqual(["inner"]));
});

test("Backspace goes to the parent folder", async () => {
  await shown();
  await fireEvent.keyDown(screen.getAllByRole("option")[0], {
    key: "Backspace",
  });
  expect(api.sftpList).toHaveBeenLastCalledWith("s1", "/home");
});

test("Shift+click selects the range and Download sends those paths", async () => {
  await shown();
  const download = screen.getByRole("button", { name: "Download" });
  expect((download as HTMLButtonElement).disabled).toBe(true);
  const rows = screen.getAllByRole("option");
  await fireEvent.click(rows[1]);
  await fireEvent.click(rows[3], { shiftKey: true });
  expect(rows.map((r) => r.getAttribute("aria-selected"))).toEqual([
    "false",
    "true",
    "true",
    "true",
  ]);
  await fireEvent.click(download);
  expect(api.sftpDownload).toHaveBeenCalledWith("s1", [
    "/home/u/zeta",
    "/home/u/a.txt",
    "/home/u/b.txt",
  ]);
});

test("Ctrl+click and Space toggle single rows; Shift+arrows extend", async () => {
  await shown();
  const rows = screen.getAllByRole("option");
  await fireEvent.click(rows[0]);
  await fireEvent.click(rows[2], { ctrlKey: true });
  const sel = () => rows.map((r) => r.getAttribute("aria-selected") === "true");
  expect(sel()).toEqual([true, false, true, false]);
  await fireEvent.keyDown(rows[2], { key: " " });
  expect(sel()).toEqual([true, false, false, false]);
  await fireEvent.keyDown(rows[2], { key: "ArrowDown", shiftKey: true });
  expect(sel()).toEqual([false, false, true, true]);
});

test("Upload folder asks for a folder to send to the shown one", async () => {
  await shown();
  await fireEvent.click(screen.getByRole("button", { name: "Upload folder" }));
  expect(api.sftpUploadPick).toHaveBeenCalledWith("s1", "/home/u", true);
});

test("a truncated listing says so", async () => {
  vi.mocked(api.sftpList).mockResolvedValue({
    dir: "/big",
    entries: [file("x")],
    truncated: true,
  });
  await shown();
  expect(screen.getByRole("status").textContent).toMatch(/first 100 000/);
});

test("a range over 5 000 rows stays quick and downloads them all", async () => {
  const many = Array.from({ length: 5_000 }, (_, i) =>
    file(`f${String(i).padStart(5, "0")}`),
  );
  vi.mocked(api.sftpList).mockResolvedValue({
    dir: "/big",
    entries: many,
    truncated: false,
  });
  await shown();
  const rows = screen.getAllByRole("option");
  await fireEvent.click(rows[0]);
  const start = performance.now();
  await fireEvent.keyDown(rows[0], { key: "End", shiftKey: true });
  await fireEvent.click(screen.getByRole("button", { name: "Download" }));
  // A scan of the selection per row takes far longer; the bound leaves room for a loaded CI.
  expect(performance.now() - start).toBeLessThan(5000);
  const items = vi.mocked(api.sftpDownload).mock.calls[0][1];
  expect(items).toHaveLength(5_000);
  expect(items.at(-1)).toBe("/big/f04999");
}, 15_000);

test("an upload into the shown folder refreshes it when it ends", async () => {
  const { rerender } = render(FilePanel, { pane });
  await waitFor(() => expect(screen.getByText("/home/u")).toBeTruthy());
  await fireEvent.click(screen.getByRole("button", { name: "Upload files" }));
  const listed = () => vi.mocked(api.sftpList).mock.calls.length;
  const before = listed();
  const up = (state: PaneState["transfers"][number]["state"]) =>
    rerender({ pane: { ...pane, transfers: { 1: { name: "a", state } } } });
  await up({
    type: "Running",
    done_bytes: 1,
    total_bytes: 2,
    files_done: 0,
    current: "a",
  });
  expect(listed()).toBe(before);
  await up({ type: "Finished", files: 1, skipped: [] });
  await waitFor(() => expect(listed()).toBe(before + 1));
  expect(api.sftpList).toHaveBeenLastCalledWith("s1", "/home/u");
  // Once only.
  await up({ type: "Finished", files: 1, skipped: [] });
  expect(listed()).toBe(before + 1);
  expect(screen.getByText("1 file")).toBeTruthy();
});

test("a drop that is gone asks to drop the files again", async () => {
  vi.mocked(api.sftpUploadDrop).mockRejectedValue({
    code: "not_found",
    params: { what: "drop" },
  });
  toasts.splice(0);
  await shown();
  document
    .querySelector("[data-sftp-pane]")!
    .dispatchEvent(new CustomEvent("sftp-drop", { detail: { dropId: 3 } }));
  expect(api.sftpUploadDrop).toHaveBeenCalledWith("s1", 3, "/home/u");
  await waitFor(() => expect(toasts[0]?.message).toMatch(/drop them again/));
});
