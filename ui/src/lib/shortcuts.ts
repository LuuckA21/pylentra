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
