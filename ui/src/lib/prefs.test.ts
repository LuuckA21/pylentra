import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { getPref, setPref } from "./prefs";

beforeEach(() => localStorage.clear());
afterEach(() => vi.restoreAllMocks());

test("a stored value round-trips", () => {
  setPref("k", { a: [1, 2] });
  expect(getPref("k", null)).toEqual({ a: [1, 2] });
});

test("missing key gives the fallback", () => {
  expect(getPref("nope", 7)).toBe(7);
});

test("corrupt JSON gives the fallback", () => {
  localStorage.setItem("k", "{oops");
  expect(getPref("k", "fb")).toBe("fb");
});

test("throwing storage gives the fallback and setPref does not throw", () => {
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("blocked");
  });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("quota");
  });
  expect(getPref("k", "fb")).toBe("fb");
  expect(() => setPref("k", 1)).not.toThrow();
});
