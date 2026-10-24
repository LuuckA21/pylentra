import { expect, test } from "vitest";
import { PALETTES, terminalTheme } from "./terminalThemes";

test("app follows the tokens", () => {
  expect(
    terminalTheme("app", () => ({ background: "#123456" })).background,
  ).toBe("#123456");
});

test("every palette defines the 16 ANSI colors", () => {
  const ansi = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
  ];
  const keys = [
    ...ansi,
    ...ansi.map((c) => `bright${c[0].toUpperCase()}${c.slice(1)}`),
  ];
  for (const [name, th] of Object.entries(PALETTES)) {
    for (const k of [
      ...keys,
      "background",
      "foreground",
      "cursor",
      "selectionBackground",
    ])
      expect(th[k as keyof typeof th], `${name}.${k}`).toMatch(
        /^#[0-9a-f]{6}$/,
      );
  }
});
