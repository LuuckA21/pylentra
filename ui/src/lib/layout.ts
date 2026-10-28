export type Dir = "row" | "column"; // row: a on the left, b on the right; column: a on top
export type Node =
  { pane: string } | { dir: Dir; ratio: number; a: Node; b: Node };
export type Side = "left" | "right" | "up" | "down";
export type Path = ("a" | "b")[];
export type Rect = { x: number; y: number; w: number; h: number }; // % of the tab
export type Divider = { path: Path; dir: Dir; area: Rect; ratio: number };

export const MAX_PANES = 4;

const EPS = 0.01;

export function split(
  root: Node,
  paneId: string,
  dir: Dir,
  newId: string,
): Node {
  if ("pane" in root)
    return root.pane === paneId
      ? { dir, ratio: 0.5, a: root, b: { pane: newId } }
      : root;
  return {
    ...root,
    a: split(root.a, paneId, dir, newId),
    b: split(root.b, paneId, dir, newId),
  };
}

export function remove(root: Node, paneId: string): Node | null {
  if ("pane" in root) return root.pane === paneId ? null : root;
  const a = remove(root.a, paneId);
  const b = remove(root.b, paneId);
  if (!a || !b) return a ?? b; // the sibling takes the parent's place
  return { ...root, a, b };
}

export function setRatio(root: Node, path: Path, ratio: number): Node {
  if ("pane" in root) return root;
  if (path.length === 0) return { ...root, ratio: clampRatio(ratio, 0, 0) };
  const [head, ...rest] = path;
  return { ...root, [head]: setRatio(root[head], rest, ratio) };
}

// totalPx = 0 when the size is unknown: only the global 0.1..0.9 bound applies.
export function clampRatio(
  ratio: number,
  totalPx: number,
  minPx: number,
): number {
  let r = Math.max(0.1, Math.min(0.9, ratio));
  if (totalPx > 0) {
    const m = minPx / totalPx;
    r = Math.max(m, Math.min(1 - m, r));
  }
  return r;
}

export function rects(root: Node): {
  panes: Record<string, Rect>;
  dividers: Divider[];
} {
  const panes: Record<string, Rect> = {};
  const dividers: Divider[] = [];
  const walk = (n: Node, r: Rect, path: Path) => {
    if ("pane" in n) {
      panes[n.pane] = r;
      return;
    }
    dividers.push({ path, dir: n.dir, area: r, ratio: n.ratio });
    const row = n.dir === "row";
    const first = row ? { ...r, w: r.w * n.ratio } : { ...r, h: r.h * n.ratio };
    const second = row
      ? { ...r, x: r.x + first.w, w: r.w - first.w }
      : { ...r, y: r.y + first.h, h: r.h - first.h };
    walk(n.a, first, [...path, "a"]);
    walk(n.b, second, [...path, "b"]);
  };
  walk(root, { x: 0, y: 0, w: 100, h: 100 }, []);
  return { panes, dividers };
}

export function neighbor(
  panes: Record<string, Rect>,
  paneId: string,
  side: Side,
): string | null {
  const p = panes[paneId];
  if (!p) return null;
  const horizontal = side === "left" || side === "right";
  let best: { id: string; q: Rect; overlap: number } | null = null;
  for (const [id, q] of Object.entries(panes)) {
    if (id === paneId) continue;
    const edge =
      side === "right"
        ? p.x + p.w
        : side === "left"
          ? p.x
          : side === "down"
            ? p.y + p.h
            : p.y;
    const opposite =
      side === "right"
        ? q.x
        : side === "left"
          ? q.x + q.w
          : side === "down"
            ? q.y
            : q.y + q.h;
    if (Math.abs(edge - opposite) > EPS) continue;
    const overlap = horizontal
      ? Math.min(p.y + p.h, q.y + q.h) - Math.max(p.y, q.y)
      : Math.min(p.x + p.w, q.x + q.w) - Math.max(p.x, q.x);
    if (overlap <= EPS) continue;
    const better =
      !best ||
      overlap > best.overlap + EPS ||
      (Math.abs(overlap - best.overlap) <= EPS &&
        (q.y < best.q.y - EPS ||
          (Math.abs(q.y - best.q.y) <= EPS && q.x < best.q.x)));
    if (better) best = { id, q, overlap };
  }
  return best?.id ?? null;
}

export function count(root: Node): number {
  return "pane" in root ? 1 : count(root.a) + count(root.b);
}

export function firstPane(root: Node): string {
  return "pane" in root ? root.pane : firstPane(root.a);
}
