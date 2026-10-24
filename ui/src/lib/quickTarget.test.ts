import { expect, test } from "vitest";
import { parseTarget, type QuickTarget } from "./quickTarget";

const d = { user: "luca", port: 22 };
const ok = (text: string): QuickTarget => {
  const r = parseTarget(text, d);
  if (!r.ok) throw new Error(`expected ok for ${text}, got ${r.error}`);
  return r.target;
};

test("parses user, host and port", () => {
  expect(parseTarget("root@10.0.0.1:2222", d)).toEqual({
    ok: true,
    target: { user: "root", host: "10.0.0.1", port: 2222 },
  });
  expect(parseTarget("  host  ", d)).toEqual({
    ok: true,
    target: { user: "luca", host: "host", port: 22 },
  });
});

test("IPv6 forms", () => {
  expect(ok("[::1]:2200")).toEqual({ user: "luca", host: "::1", port: 2200 });
  expect(ok("[::1]").port).toBe(22);
  expect(ok("::1")).toEqual({ user: "luca", host: "::1", port: 22 });
  expect(ok("luca@fe80::1").host).toBe("fe80::1");
});

test("missing user without default", () => {
  expect(parseTarget("host", { user: null, port: 22 })).toEqual({
    ok: false,
    error: "quick.error.user",
  });
});

test("port errors", () => {
  for (const bad of [
    "h:0",
    "h:65536",
    "h:abc",
    "h:",
    "h:+22",
    "h:2.2",
    "[::1]:",
  ])
    expect(parseTarget(bad, d), bad).toEqual({
      ok: false,
      error: "quick.error.port",
    });
});

test("format errors", () => {
  for (const bad of [
    "",
    "a b",
    "a@@b",
    "@h",
    "u@",
    "[::1",
    "[]",
    ":22",
    "[::1]x",
    "u:p@h",
    "ssh://u@h",
    "-oProxyCommand=x@h",
    "u[@h",
    "h: 22",
  ])
    expect(parseTarget(bad, d), bad).toEqual({
      ok: false,
      error: "quick.error.format",
    });
});
