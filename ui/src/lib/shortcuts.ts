export type Action =
  | "copy"
  | "paste"
  | "closeTab"
  | "search"
  | "nextTab"
  | "prevTab"
  | "quickConnect";

const WITH_SHIFT: Record<string, Action> = {
  C: "copy",
  V: "paste",
  W: "closeTab",
  F: "search",
  K: "quickConnect",
};

// Ctrl+Shift+C/V/W/F/K and Ctrl+(Shift+)Tab; everything else (Ctrl+C included) belongs to the shell.
export function shortcutFor(e: KeyboardEvent): Action | null {
  if (!e.ctrlKey || e.altKey || e.metaKey) return null;
  if (e.key === "Tab") return e.shiftKey ? "prevTab" : "nextTab";
  return e.shiftKey ? (WITH_SHIFT[e.key.toUpperCase()] ?? null) : null;
}

// A window shortcut must not act behind a dialog: true when `e` comes from a
// modal that is not inside `own` (whose own prompts keep their shortcuts).
export function fromOtherModal(e: Event, own?: Node | null): boolean {
  const target = e.target instanceof Element ? e.target : null;
  const dialog = target?.closest('[aria-modal="true"]');
  return !!dialog && !own?.contains(dialog);
}
