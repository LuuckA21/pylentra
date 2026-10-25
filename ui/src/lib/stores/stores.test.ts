import { beforeEach, expect, test, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => invoke(...a),
  Channel: class {},
}));
vi.mock("@tauri-apps/plugin-log", () => ({ warn: vi.fn() }));

import {
  deleteConnection,
  isReadOnly,
  isSnippetsReadOnly,
  library,
  refresh,
  saveConnection,
} from "./library.svelte";
import { settings, loadSettings, saveSettings } from "./settings.svelte";
import { t } from "../i18n";

const lib = { version: 1, groups: [{ id: "g", name: "G" }], connections: [] };

beforeEach(() => invoke.mockReset());

test("refresh loads library and startup warnings", async () => {
  const w = { code: "config.read_only", params: {} };
  invoke.mockImplementation(async (cmd: string) =>
    cmd === "library_get" ? lib : [w],
  );
  await refresh();
  expect(library.groups).toEqual(lib.groups);
  expect(library.warnings).toEqual([w]);
});

test("saveConnection saves then refreshes and returns warnings", async () => {
  const calls: string[] = [];
  invoke.mockImplementation(async (cmd: string) => {
    calls.push(cmd);
    return cmd === "library_get" ? lib : [];
  });
  const warnings = await saveConnection({} as never, "pw");
  expect(warnings).toEqual([]);
  expect(calls.slice(0, 2)).toEqual(["connection_save", "library_get"]);
});

test("snippets.toml warnings only affect isSnippetsReadOnly", () => {
  library.warnings = [
    { code: "config.read_only", params: { found: "2", file: "snippets.toml" } },
  ];
  expect(isReadOnly()).toBe(false);
  expect(isSnippetsReadOnly()).toBe(true);
  library.warnings = [];
});

test("an unreadable snippets.toml makes the snippets read-only", () => {
  library.warnings = [
    {
      code: "config.unreadable",
      params: { reason: "Is a directory", file: "snippets.toml" },
    },
  ];
  expect(isReadOnly()).toBe(false);
  expect(isSnippetsReadOnly()).toBe(true);
  library.warnings = [];
});

test("deleteConnection reloads the snippets too", async () => {
  const calls: string[] = [];
  invoke.mockImplementation(async (cmd: string) => {
    calls.push(cmd);
    return cmd === "library_get" ? lib : [];
  });
  await deleteConnection("c");
  expect(calls[0]).toBe("connection_delete");
  expect(calls).toContain("snippets_get");
});

test("a failed snippet reload does not fail a delete that went through", async () => {
  invoke.mockImplementation(async (cmd: string) => {
    if (cmd === "snippets_get") throw { code: "io", params: {} };
    return cmd === "library_get" ? lib : [];
  });
  await expect(deleteConnection("c")).resolves.toEqual([]);
});

test("settings apply data-theme on <html> and the language", async () => {
  invoke.mockImplementation(async (cmd: string, a?: { settings?: object }) =>
    cmd === "settings_get"
      ? { theme: "System", font_family: "m", font_size: 13, language: "it" }
      : a?.settings,
  );
  await loadSettings();
  expect(document.documentElement.dataset.theme).toBe("system");
  expect(t("error.cancelled")).toBe("Annullato.");
  await saveSettings({ theme: "Dark", language: "en" });
  expect(settings.theme).toBe("Dark");
  expect(document.documentElement.dataset.theme).toBe("dark");
  expect(t("error.cancelled")).toBe("Cancelled.");
});
