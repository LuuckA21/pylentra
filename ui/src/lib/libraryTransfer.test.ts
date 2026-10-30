import { beforeEach, expect, test, vi } from "vitest";

vi.mock("./api", () => ({
  api: { libraryExport: vi.fn(), libraryImportPreview: vi.fn() },
}));

import { api } from "./api";
import { setLang } from "./i18n";
import { exportLibrary, jumpText, pickImport } from "./libraryTransfer";
import type { ImportRow } from "./bindings/ImportRow";
import { toasts } from "./stores/toasts.svelte";

beforeEach(() => {
  vi.resetAllMocks();
  toasts.splice(0);
  setLang("en");
});

const messages = () => toasts.map((x) => [x.kind, x.message]);

test("export passes an ISO date and stays silent when cancelled", async () => {
  vi.mocked(api.libraryExport).mockResolvedValue(null);
  await exportLibrary();
  const [at] = vi.mocked(api.libraryExport).mock.calls[0];
  expect(new Date(at).toISOString()).toBe(at);
  expect(toasts).toHaveLength(0);
});

test("export reports the path", async () => {
  vi.mocked(api.libraryExport).mockResolvedValue({
    path: "/x/a.toml",
    snippets: true,
  });
  await exportLibrary();
  expect(messages()).toEqual([["success", "Library exported to /x/a.toml"]]);
});

test("export warns when snippets were left out", async () => {
  vi.mocked(api.libraryExport).mockResolvedValue({
    path: "/x/a.toml",
    snippets: false,
  });
  await exportLibrary();
  expect(messages()).toEqual([
    ["success", "Library exported to /x/a.toml"],
    ["warning", "Snippets are unavailable and were not exported."],
  ]);
});

test("export failure is a toast", async () => {
  vi.mocked(api.libraryExport).mockRejectedValue({
    code: "io.failed",
    params: {},
  });
  await exportLibrary();
  expect(toasts).toHaveLength(1);
  expect(toasts[0].kind).toBe("error");
});

test("pickImport returns the pick, or null when cancelled", async () => {
  const pick = { path: "/x/a.toml", preview: {} };
  vi.mocked(api.libraryImportPreview).mockResolvedValueOnce(pick as never);
  expect(await pickImport()).toBe(pick);
  vi.mocked(api.libraryImportPreview).mockResolvedValueOnce(null);
  expect(await pickImport()).toBeNull();
  expect(toasts).toHaveLength(0);
});

test.each([
  [
    { code: "config.unsupported_version", params: {} },
    "The file was created by a newer version of Pylentra.",
  ],
  [
    { code: "config.invalid", params: { field: "format", reason: "x" } },
    "The file is not a Pylentra export.",
  ],
])("pickImport maps %j to a dedicated toast", async (err, message) => {
  vi.mocked(api.libraryImportPreview).mockRejectedValue(err);
  expect(await pickImport()).toBeNull();
  expect(messages()).toEqual([["error", message]]);
});

test("pickImport shows other errors as they are", async () => {
  vi.mocked(api.libraryImportPreview).mockRejectedValue({
    code: "config.invalid",
    params: { field: "groups", reason: "dup" },
  });
  expect(await pickImport()).toBeNull();
  expect(toasts[0].message).toBe("Invalid value for groups: dup");
});

const row = (jump: ImportRow["jump"]): ImportRow => ({
  id: "r",
  name: "app",
  target: "u@app:22",
  group: null,
  duplicate: false,
  missing_key: null,
  jump,
});

test("jumpText words the jump host note from the selection", () => {
  const in_file = { name: "bastion", in_file: "b", existing: true };
  expect(jumpText(row(null), new Set())).toBeNull();
  expect(jumpText(row(in_file), new Set(["b"]))).toBe("via bastion");
  expect(jumpText(row(in_file), new Set())).toBe(
    "via bastion (already present)",
  );
  expect(
    jumpText(
      row({ name: "bastion", in_file: "b", existing: false }),
      new Set(),
    ),
  ).toBe("will have no jump host (bastion not imported)");
  expect(
    jumpText(row({ name: "old", in_file: null, existing: true }), new Set()),
  ).toBe("via old (already present)");
});
