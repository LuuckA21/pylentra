import { describe, expect, test } from "vitest";
import { readFileSync } from "node:fs";
import { contrastRatio, tokensOf } from "./contrast";

// Not `?raw`: Vitest stubs CSS imports to "" unless css processing is enabled.
const css = readFileSync("src/app.css", "utf8");

const COLORS = [
  "red",
  "orange",
  "amber",
  "green",
  "teal",
  "blue",
  "violet",
  "gray",
  "none",
];

test("contrastRatio", () => {
  expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 1);
  expect(contrastRatio("#777777", "#ffffff")).toBeCloseTo(4.48, 2);
});

test("tokensOf reads only the block with that exact selector", () => {
  const src =
    ':root { --a: #111111; --b: 4px; }\n:root[data-theme="dark"] { --a: #222222; }';
  expect(tokensOf(src, ":root")).toEqual({ "--a": "#111111" });
  expect(tokensOf(src, ':root[data-theme="dark"]')).toEqual({
    "--a": "#222222",
  });
});

const themes = { light: ":root", dark: ':root[data-theme="dark"]' };

describe.each(Object.entries(themes))("%s", (_name, selector) => {
  const t = tokensOf(css, selector);
  const pairs: [string, string][] = [
    ["--fg", "--surface-0"],
    ["--fg", "--surface-1"],
    ["--fg-muted", "--surface-1"],
    ["--accent-fg", "--accent"],
    ...COLORS.map((c): [string, string] => [`--tag-${c}-fg`, `--tag-${c}-bg`]),
  ];

  test.each(pairs)("%s on %s reaches 4.5:1", (fg, bg) => {
    expect(t[fg], fg).toBeDefined();
    expect(t[bg], bg).toBeDefined();
    expect(contrastRatio(t[fg], t[bg])).toBeGreaterThanOrEqual(4.5);
  });

  test("defines a solid for every tag color", () => {
    for (const c of COLORS) expect(t[`--tag-${c}-solid`], c).toBeDefined();
  });
});

test("the system dark block repeats the dark tokens", () => {
  expect(tokensOf(css, ':root[data-theme="system"]')).toEqual(
    tokensOf(css, ':root[data-theme="dark"]'),
  );
});
