import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";

vi.mock("../lib/api", () => ({
  api: {
    libraryImport: vi.fn(),
    libraryGet: vi.fn(),
    startupWarnings: vi.fn(),
    snippetsGet: vi.fn(),
  },
}));

import { api } from "../lib/api";
import type { ImportPick } from "../lib/bindings/ImportPick";
import { setLang } from "../lib/i18n";
import { library } from "../lib/stores/library.svelte";
import { toasts } from "../lib/stores/toasts.svelte";
import LibraryImportDialog from "./LibraryImportDialog.svelte";

const pick: ImportPick = {
  path: "/home/u/export.toml",
  preview: {
    rows: [
      {
        id: "b",
        name: "bastion",
        target: "root@bastion:22",
        group: "Prod",
        duplicate: false,
        missing_key: null,
        jump: null,
      },
      {
        id: "a",
        name: "app",
        target: "luca@app:2222",
        group: null,
        duplicate: false,
        missing_key: "/home/u/.ssh/id_app",
        jump: { name: "bastion", in_file: "b", existing: false },
      },
      {
        id: "d",
        name: "dup",
        target: "luca@dup:22",
        group: null,
        duplicate: true,
        missing_key: null,
        jump: null,
      },
    ],
    groups_new: 2,
    groups_reused: 1,
    snippets_global: 3,
    snippets_per_connection: 4,
  },
};

const open = async () => {
  const onclose = vi.fn();
  render(LibraryImportDialog, { pick, onclose });
  return onclose;
};
const box = (name: string) =>
  screen.getByRole("checkbox", { name: new RegExp(`\\b${name}\\b`) });
const importBtn = (n: number) =>
  screen.getByRole("button", { name: `Import (${n})` });

beforeEach(() => {
  vi.resetAllMocks();
  setLang("en");
  toasts.splice(0);
  library.warnings = [];
  vi.mocked(api.libraryGet).mockResolvedValue({
    version: 1,
    groups: [],
    connections: [],
  });
  vi.mocked(api.startupWarnings).mockResolvedValue([]);
  vi.mocked(api.snippetsGet).mockResolvedValue([]);
  vi.mocked(api.libraryImport).mockResolvedValue({
    connections: 2,
    groups: 1,
    snippets: 5,
  });
});

test("is a labelled modal dialog showing the file path", async () => {
  await open();
  const dialog = screen.getByRole("dialog", { name: "Import library" });
  expect(dialog.getAttribute("aria-modal")).toBe("true");
  expect(dialog.textContent).toContain("/home/u/export.toml");
});

test("one row per connection; duplicates start unchecked and are marked", async () => {
  await open();
  expect(screen.getAllByRole("checkbox")).toHaveLength(3);
  expect((box("bastion") as HTMLInputElement).checked).toBe(true);
  expect((box("app") as HTMLInputElement).checked).toBe(true);
  expect((box("dup") as HTMLInputElement).checked).toBe(false);
  expect(screen.getByText("already present")).toBeTruthy();
  expect(screen.getByText(/luca@app:2222/)).toBeTruthy();
  expect(screen.getByText(/Prod/)).toBeTruthy();
});

test("flags a missing key and shows the summary", async () => {
  await open();
  expect(screen.getByText("key not found: /home/u/.ssh/id_app")).toBeTruthy();
  expect(
    screen.getByText(
      "Groups in the file: 2 new, 1 already present. Snippets: 3 new global, 4 tied to connections.",
    ),
  ).toBeTruthy();
});

test("the jump host note follows the selection", async () => {
  await open();
  expect(screen.getByText("via bastion")).toBeTruthy();
  await fireEvent.click(box("bastion"));
  expect(
    screen.getByText("will have no jump host (bastion not imported)"),
  ).toBeTruthy();
});

test("the button counts the selection and is off at zero", async () => {
  await open();
  expect((importBtn(2) as HTMLButtonElement).disabled).toBe(false);
  await fireEvent.click(box("bastion"));
  await fireEvent.click(box("app"));
  expect((importBtn(0) as HTMLButtonElement).disabled).toBe(true);
});

test("import sends the selection in file order, reports, reloads and closes", async () => {
  const onclose = await open();
  await fireEvent.click(box("dup"));
  await fireEvent.click(importBtn(3));
  await waitFor(() => expect(onclose).toHaveBeenCalledOnce());
  expect(api.libraryImport).toHaveBeenCalledWith(["b", "a", "d"]);
  expect(toasts.map((x) => [x.kind, x.message])).toEqual([
    ["success", "Imported — connections: 2, groups: 1, snippets: 5"],
  ]);
  expect(api.libraryGet).toHaveBeenCalled();
  expect(api.snippetsGet).toHaveBeenCalled();
});

test("a failed import shows the reason and stays open", async () => {
  vi.mocked(api.libraryImport).mockRejectedValue({
    code: "config.read_only",
    params: {},
  });
  const onclose = await open();
  await fireEvent.click(importBtn(2));
  await screen.findByText(/read-only/i);
  expect(onclose).not.toHaveBeenCalled();
  expect(toasts).toHaveLength(0);
});

test("a snippet failure after the connections went in reports them and closes", async () => {
  vi.mocked(api.libraryImport).mockRejectedValue({
    code: "config.read_only",
    params: { connections_imported: 2 },
  });
  const onclose = await open();
  await fireEvent.click(importBtn(2));
  await waitFor(() => expect(onclose).toHaveBeenCalledOnce());
  expect(toasts).toHaveLength(1);
  expect(toasts[0].message).toMatch(
    /^Imported 2 connections; snippets were not: /,
  );
  expect(api.libraryGet).toHaveBeenCalled();
  expect(api.snippetsGet).toHaveBeenCalled();
});

test("Escape closes without importing", async () => {
  const onclose = await open();
  await fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(onclose).toHaveBeenCalledOnce();
  expect(api.libraryImport).not.toHaveBeenCalled();
});

test("while importing, Cancel is off and Escape does nothing", async () => {
  let fail!: (e: unknown) => void;
  vi.mocked(api.libraryImport).mockReturnValue(
    new Promise((_, reject) => (fail = reject)),
  );
  const onclose = await open();
  const cancel = screen.getByRole("button", {
    name: "Cancel",
  }) as HTMLButtonElement;
  await fireEvent.click(importBtn(2));
  expect(cancel.disabled).toBe(true);
  await fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(onclose).not.toHaveBeenCalled();
  fail({ code: "config.read_only", params: {} });
  await waitFor(() => expect(cancel.disabled).toBe(false));
  await fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(onclose).toHaveBeenCalledOnce();
});

test("a failed reload after the import is shown too", async () => {
  vi.mocked(api.snippetsGet).mockRejectedValue({
    code: "io",
    params: { reason: "gone" },
  });
  const onclose = await open();
  await fireEvent.click(importBtn(2));
  await waitFor(() => expect(onclose).toHaveBeenCalledOnce());
  expect(toasts.map((x) => [x.kind, x.message])).toEqual([
    ["success", "Imported — connections: 2, groups: 1, snippets: 5"],
    ["error", "I/O error: gone"],
  ]);
});

test("when both reloads fail, both errors are shown", async () => {
  vi.mocked(api.libraryGet).mockRejectedValue({
    code: "io",
    params: { reason: "library gone" },
  });
  vi.mocked(api.snippetsGet).mockRejectedValue({
    code: "io",
    params: { reason: "snippets gone" },
  });
  const onclose = await open();
  await fireEvent.click(importBtn(2));
  await waitFor(() => expect(onclose).toHaveBeenCalledOnce());
  expect(toasts.map((x) => x.message)).toEqual([
    "Imported — connections: 2, groups: 1, snippets: 5",
    "I/O error: library gone",
    "I/O error: snippets gone",
  ]);
});
