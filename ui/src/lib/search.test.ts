import { expect, test } from "vitest";
import type { Connection } from "./bindings/Connection";
import { matches } from "./search";

const conn = (o: Partial<Connection>): Connection => ({
  id: "1",
  name: "n",
  group_id: null,
  host: "h",
  port: 22,
  username: "u",
  auth: { type: "Agent" },
  jump_via: null,
  forwards: [],
  ...o,
});

test("search ignores case and accents", () => {
  expect(matches(conn({ name: "Prod-DB Città" }), "citta")).toBe(true);
  expect(matches(conn({ name: "Prod-DB Città" }), "PROD")).toBe(true);
  expect(matches(conn({ name: "Prod-DB" }), "zzz")).toBe(false);
});

test("search matches host and username", () => {
  const c = conn({ host: "10.0.0.5", username: "root" });
  expect(matches(c, "10.0.")).toBe(true);
  expect(matches(c, "root")).toBe(true);
});

test("empty query matches everything", () =>
  expect(matches(conn({}), "  ")).toBe(true));
