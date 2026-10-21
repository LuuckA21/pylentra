import { expect, test } from "vitest";
import { shortcutFor } from "./shortcuts";

const key = (
  k: string,
  m: { ctrl?: boolean; shift?: boolean; alt?: boolean } = {},
) =>
  new KeyboardEvent("keydown", {
    key: k,
    ctrlKey: m.ctrl,
    shiftKey: m.shift,
    altKey: m.alt,
  });

test("shortcuts", () => {
  expect(shortcutFor(key("C", { ctrl: true, shift: true }))).toBe("copy");
  expect(shortcutFor(key("V", { ctrl: true, shift: true }))).toBe("paste");
  expect(shortcutFor(key("W", { ctrl: true, shift: true }))).toBe("closeTab");
  expect(shortcutFor(key("F", { ctrl: true, shift: true }))).toBe("search");
  expect(shortcutFor(key("c", { ctrl: true }))).toBeNull(); // SIGINT stays with the shell
  expect(shortcutFor(key("Tab", { ctrl: true }))).toBe("nextTab");
  expect(shortcutFor(key("Tab", { ctrl: true, shift: true }))).toBe("prevTab");
});

test("other combos reach the shell", () => {
  expect(shortcutFor(key("v", { ctrl: true }))).toBeNull();
  expect(shortcutFor(key("Tab"))).toBeNull();
  expect(shortcutFor(key("C", { shift: true }))).toBeNull();
  expect(
    shortcutFor(key("C", { ctrl: true, shift: true, alt: true })),
  ).toBeNull();
});
