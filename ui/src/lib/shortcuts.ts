export type Action =
  "copy" | "paste" | "closeTab" | "search" | "nextTab" | "prevTab";

const WITH_SHIFT: Record<string, Action> = {
  C: "copy",
  V: "paste",
  W: "closeTab",
  F: "search",
};

// Ctrl+Shift+C/V/W/F and Ctrl+(Shift+)Tab; everything else (Ctrl+C included) belongs to the shell.
export function shortcutFor(e: KeyboardEvent): Action | null {
  if (!e.ctrlKey || e.altKey || e.metaKey) return null;
  if (e.key === "Tab") return e.shiftKey ? "prevTab" : "nextTab";
  return e.shiftKey ? (WITH_SHIFT[e.key.toUpperCase()] ?? null) : null;
}

// Finds `query` (case-insensitive) in the nearest row above `from`, wrapping
// around the buffer once.
// shortcut: matches within a single row and counts columns as UTF-16 units, so
// wrapped lines and wide characters are missed or misplaced; switch to
// @xterm/addon-search if that matters.
export function findUp(
  line: (row: number) => string | undefined,
  length: number,
  from: number,
  query: string,
): { row: number; col: number } | null {
  const q = query.toLowerCase();
  if (!q) return null;
  for (let i = 1; i <= length; i++) {
    const row = (((from - i) % length) + length) % length;
    const col = line(row)?.toLowerCase().indexOf(q) ?? -1;
    if (col >= 0) return { row, col };
  }
  return null;
}
