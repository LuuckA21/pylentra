import { expect, test } from "vitest";
import { findUp, shortcutFor } from "./shortcuts";

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

test("findUp searches upward from a row and wraps around", () => {
  const lines = ["foo bar", "nothing", "a Bar here", "end"];
  const get = (row: number) => lines[row];
  expect(findUp(get, lines.length, lines.length, "bar")).toEqual({
    row: 2,
    col: 2,
  });
  expect(findUp(get, lines.length, 2, "bar")).toEqual({ row: 0, col: 4 });
  expect(findUp(get, lines.length, 0, "bar")).toEqual({ row: 2, col: 2 });
  expect(findUp(get, lines.length, 4, "zzz")).toBeNull();
  expect(findUp(get, lines.length, 4, "")).toBeNull();
});
