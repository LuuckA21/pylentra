import { expect, test } from "vitest";
import type { Snippet } from "./bindings/Snippet";
import { body, firstLine, groupSnippets, needsConfirm } from "./snippets";

const s = (name: string, text: string, connection_id?: string): Snippet => ({
  id: name,
  name,
  text,
  run: false,
  connection_id,
});

test("firstLine is the first non-blank line, trimmed", () => {
  expect(firstLine("\n  cd /srv\nls")).toBe("cd /srv");
});

test("body drops trailing newlines", () => {
  expect(body("ls\r\n\n")).toBe("ls");
});

test("needsConfirm only for multi-line text pasted without run or bracketed paste", () => {
  expect(needsConfirm("ls\n", false, false)).toBe(false);
  expect(needsConfirm("a\r\nb", false, false)).toBe(true);
  expect(needsConfirm("a\nb", true, false)).toBe(false);
  expect(needsConfirm("a\nb", false, true)).toBe(false);
});

test("groupSnippets filters accent-insensitively, splits own/global, sorts", () => {
  const list = [
    s("b", "x", "B"),
    s("Riavvia", "systemctl restart", "A"),
    s("Ångström", "echo x"),
    s("Zeta", "y"),
  ];
  expect(groupSnippets(list, "angst", "A").global.map((x) => x.name)).toEqual([
    "Ångström",
  ]);
  expect(groupSnippets(list, "echo", "A").global.map((x) => x.name)).toEqual([
    "Ångström",
  ]);
  const g = groupSnippets(list, "", "A");
  expect(g.own.map((x) => x.name)).toEqual(["Riavvia"]);
  expect(g.global.map((x) => x.name)).toEqual(["Ångström", "Zeta"]);
  expect(groupSnippets(list, "", null).own).toEqual([]);
});
