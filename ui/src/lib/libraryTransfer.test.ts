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

test("export passes the local time with its offset and stays silent when cancelled", async () => {
  const tz = process.env.TZ;
  process.env.TZ = "Europe/Rome";
  vi.useFakeTimers({ toFake: ["Date"] });
  // 00:30 in Italy is still the day before in UTC; the file name takes this date.
  vi.setSystemTime(new Date("2026-10-29T23:30:00Z"));
  try {
    vi.mocked(api.libraryExport).mockResolvedValue(null);
    await exportLibrary();
    const [at] = vi.mocked(api.libraryExport).mock.calls[0];
    expect(at).toBe("2026-10-30T00:30:00+01:00");
    expect(toasts).toHaveLength(0);
  } finally {
    vi.useRealTimers();
    process.env.TZ = tz;
  }
});

test("a second export or import while one is open does nothing", async () => {
  let finish!: (v: null) => void;
  vi.mocked(api.libraryExport).mockReturnValue(
    new Promise((r) => (finish = r)),
  );
  const first = exportLibrary();
  const second = exportLibrary();
  expect(await pickImport()).toBeNull();
  expect(api.libraryExport).toHaveBeenCalledTimes(1);
  expect(api.libraryImportPreview).not.toHaveBeenCalled();
  finish(null);
  await Promise.all([first, second]);
  vi.mocked(api.libraryImportPreview).mockResolvedValue(null);
  await pickImport();
  expect(api.libraryImportPreview).toHaveBeenCalledTimes(1);
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

test("export never replaces an existing file: the reason is a toast", async () => {
  vi.mocked(api.libraryExport).mockRejectedValue({
    code: "export.file_exists",
    params: { name: "backup.toml" },
  });
  await exportLibrary();
  expect(messages()).toEqual([
    ["error", "A file named backup.toml already exists: choose another name."],
  ]);
  setLang("it");
  await exportLibrary();
  expect(toasts[1].message).toBe(
    "Esiste già un file backup.toml: scegli un altro nome.",
  );
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
