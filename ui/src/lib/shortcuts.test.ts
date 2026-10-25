import { expect, test } from "vitest";
import { fromOtherModal, shortcutFor } from "./shortcuts";

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
  expect(shortcutFor(key("K", { ctrl: true, shift: true }))).toBe(
    "quickConnect",
  );
  expect(shortcutFor(key("P", { ctrl: true, shift: true }))).toBe("snippets");
  expect(shortcutFor(key("p", { ctrl: true }))).toBeNull();
  expect(shortcutFor(key("k", { ctrl: true }))).toBeNull(); // Ctrl+K stays with the shell
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

test("shortcuts from a dialog outside their owner are ignored", () => {
  document.body.innerHTML = `
    <div id="tab"><div aria-modal="true"><input id="prompt" /></div><div id="term"></div></div>
    <div aria-modal="true"><input id="form" /></div>`;
  const from = (id: string) => {
    const e = key("W", { ctrl: true, shift: true });
    Object.defineProperty(e, "target", { value: document.getElementById(id) });
    return e;
  };
  const tab = document.getElementById("tab");
  expect(fromOtherModal(from("form"), tab)).toBe(true);
  expect(fromOtherModal(from("prompt"), tab)).toBe(false);
  expect(fromOtherModal(from("term"), tab)).toBe(false);
  expect(fromOtherModal(from("prompt"))).toBe(true);
  expect(fromOtherModal(key("W"))).toBe(false); // target-less (window) events
});
