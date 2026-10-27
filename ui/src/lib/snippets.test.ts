import { expect, test } from "vitest";
import type { Snippet } from "./bindings/Snippet";
import {
  body,
  firstLine,
  fillVars,
  groupSnippets,
  needsConfirm,
  recallVar,
  rememberVars,
  snippetVars,
} from "./snippets";

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
  const known = new Set(["A", "B"]);
  expect(
    groupSnippets(list, "angst", "A", known).global.map((x) => x.name),
  ).toEqual(["Ångström"]);
  expect(
    groupSnippets(list, "echo", "A", known).global.map((x) => x.name),
  ).toEqual(["Ångström"]);
  const g = groupSnippets(list, "", "A", known);
  expect(g.own.map((x) => x.name)).toEqual(["Riavvia"]);
  expect(g.global.map((x) => x.name)).toEqual(["Ångström", "Zeta"]);
  expect(groupSnippets(list, "", null, known).own).toEqual([]);
});

test("groupSnippets shows a snippet of a deleted connection as global", () => {
  const g = groupSnippets([s("Ghost", "x", "gone")], "", "A", new Set(["A"]));
  expect(g.own).toEqual([]);
  expect(g.global.map((x) => x.name)).toEqual(["Ghost"]);
});

test("snippetVars lists distinct names in order of appearance", () => {
  expect(snippetVars("tail {{ servizio }} {{n}} {{servizio}}")).toEqual([
    "servizio",
    "n",
  ]);
});

test("snippetVars ignores escapes and invalid names", () => {
  expect(snippetVars("{{{{x}}")).toEqual([]);
  expect(snippetVars("{{ }} {{a b}} {{ .Values.x }} {{x!}}")).toEqual([]);
  expect(snippetVars(`{{${"a".repeat(41)}}}`)).toEqual([]);
  expect(snippetVars(`{{${"a".repeat(40)}}}`)).toEqual(["a".repeat(40)]);
});

test("fillVars replaces in one pass and inserts values literally", () => {
  expect(fillVars("a {{x}} b {{ x }}", { x: "1" })).toBe("a 1 b 1");
  expect(fillVars("{{{{x}}", {})).toBe("{{x}}");
  expect(fillVars("{{x}}", { x: "$& `$'\n" })).toBe("$& `$'\n");
  expect(fillVars("{{a}}{{b}}", { a: "{{b}}", b: "x" })).toBe("{{b}}x");
});

test("recallVar returns what rememberVars stored, per snippet", () => {
  rememberVars("s1", { x: "v" });
  expect(recallVar("s1", "x")).toBe("v");
  expect(recallVar("s2", "x")).toBe("");
});
