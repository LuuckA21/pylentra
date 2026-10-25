import { beforeEach, expect, test } from "vitest";
import { recentIds, recordRecent } from "./recent";

beforeEach(() => localStorage.clear());

const all = (...ids: string[]) => new Set(ids);

test("most recent first, no duplicates, at most 5", () => {
  for (const id of ["a", "b", "c", "a", "d", "e", "f"]) recordRecent(id);
  expect(recentIds(all("a", "b", "c", "d", "e", "f"))).toEqual([
    "f",
    "e",
    "d",
    "a",
    "c",
  ]);
});

test("ids not in the library are dropped", () => {
  recordRecent("a");
  recordRecent("gone");
  expect(recentIds(all("a"))).toEqual(["a"]);
});

test("a corrupt stored value reads as empty", () => {
  localStorage.setItem("pylentra.recent", '{"x":1}');
  expect(recentIds(all("a"))).toEqual([]);
  recordRecent("a");
  expect(recentIds(all("a"))).toEqual(["a"]);
});
