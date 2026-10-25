import { afterEach, expect, test } from "vitest";
import { render, screen } from "@testing-library/svelte";
import { setLang } from "../lib/i18n";
import { library } from "../lib/stores/library.svelte";
import Banner from "./Banner.svelte";

afterEach(() => {
  library.warnings = [];
});

test("a warning about a named file starts with the file name", () => {
  setLang("en");
  library.warnings = [
    {
      code: "config.unreadable",
      params: { reason: "Is a directory", file: "snippets.toml" },
    },
    { code: "config.read_only", params: { found: "3" } },
  ];
  render(Banner);
  const lines = screen.getAllByRole("paragraph").map((p) => p.textContent);
  expect(lines[0]).toBe(
    "snippets.toml: The file could not be read (Is a directory). It stays untouched and read-only until the next start.",
  );
  expect(lines[1]).not.toContain(":");
});
