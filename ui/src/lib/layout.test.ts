import { expect, test } from "vitest";
import {
  clampRatio,
  count,
  firstPane,
  neighbor,
  rects,
  remove,
  setRatio,
  split,
  type Node,
} from "./layout";

const A: Node = { pane: "A" };
const B: Node = { pane: "B" };
const C: Node = { pane: "C" };
// A | (B / C)
const tree: Node = {
  dir: "row",
  ratio: 0.5,
  a: A,
  b: { dir: "column", ratio: 0.5, a: B, b: C },
};

test("split replaces only the target leaf", () => {
  expect(split(A, "A", "row", "B")).toEqual({
    dir: "row",
    ratio: 0.5,
    a: A,
    b: B,
  });
  expect(split(tree, "C", "row", "D")).toEqual({
    ...tree,
    b: {
      dir: "column",
      ratio: 0.5,
      a: B,
      b: { dir: "row", ratio: 0.5, a: C, b: { pane: "D" } },
    },
  });
});

test("remove collapses the parent into the sibling", () => {
  expect(remove(tree, "A")).toEqual({ dir: "column", ratio: 0.5, a: B, b: C });
  expect(remove(tree, "C")).toEqual({ ...tree, b: B });
  expect(remove(A, "A")).toBeNull();
  expect(remove(tree, "Z")).toEqual(tree);
});

test("functions do not mutate the input", () => {
  const before = structuredClone(tree);
  split(tree, "B", "row", "D");
  remove(tree, "B");
  setRatio(tree, ["b"], 0.8);
  expect(tree).toEqual(before);
});

test("rects gives panes and dividers in percent", () => {
  const { panes, dividers } = rects(tree);
  expect(panes).toEqual({
    A: { x: 0, y: 0, w: 50, h: 100 },
    B: { x: 50, y: 0, w: 50, h: 50 },
    C: { x: 50, y: 50, w: 50, h: 50 },
  });
  expect(dividers).toEqual([
    { path: [], dir: "row", area: { x: 0, y: 0, w: 100, h: 100 }, ratio: 0.5 },
    {
      path: ["b"],
      dir: "column",
      area: { x: 50, y: 0, w: 50, h: 100 },
      ratio: 0.5,
    },
  ]);
});

test("neighbor finds the adjacent pane, topmost on ties", () => {
  const { panes } = rects(tree);
  expect(neighbor(panes, "C", "left")).toBe("A");
  expect(neighbor(panes, "A", "right")).toBe("B");
  expect(neighbor(panes, "B", "down")).toBe("C");
  expect(neighbor(panes, "C", "up")).toBe("B");
  expect(neighbor(panes, "A", "left")).toBeNull();
});

test("neighbor prefers the longer overlap", () => {
  const skewed: Node = {
    dir: "row",
    ratio: 0.5,
    a: A,
    b: { dir: "column", ratio: 0.3, a: B, b: C },
  };
  expect(neighbor(rects(skewed).panes, "A", "right")).toBe("C");
});

test("setRatio clamps to 0.1..0.9", () => {
  const ratioAt = (n: Node) => ("ratio" in n ? n.ratio : NaN);
  const b = (n: Node) => ("b" in n ? n.b : n);
  expect(ratioAt(b(setRatio(tree, ["b"], 0.95)))).toBe(0.9);
  expect(ratioAt(b(setRatio(tree, ["b"], 0.02)))).toBe(0.1);
  expect(ratioAt(setRatio(tree, [], 0.3))).toBe(0.3);
});

test("clampRatio honours the pixel minimum when the size is known", () => {
  expect(clampRatio(0.05, 1000, 160)).toBe(0.16);
  expect(clampRatio(0.5, 0, 160)).toBe(0.5);
  expect(clampRatio(0.95, 0, 160)).toBe(0.9);
  expect(clampRatio(0.95, 1000, 160)).toBeCloseTo(0.84);
});

test("clampRatio stays within 0.1..0.9 when the minimum does not fit", () => {
  for (const total of [100, 250, 320, 400]) {
    const r = clampRatio(0.5, total, 160);
    expect(r).toBeGreaterThanOrEqual(0.1);
    expect(r).toBeLessThanOrEqual(0.9);
  }
});

test("count and firstPane", () => {
  expect(count(tree)).toBe(3);
  expect(firstPane(tree)).toBe("A");
});
