export type Action =
  | "copy"
  | "paste"
  | "closePane"
  | "search"
  | "nextTab"
  | "prevTab"
  | "quickConnect"
  | "snippets"
  | "fullscreen"
  | "splitRight"
  | "splitDown"
  | "focusLeft"
  | "focusRight"
  | "focusUp"
  | "focusDown";

const WITH_SHIFT: Record<string, Action> = {
  C: "copy",
  V: "paste",
  W: "closePane",
  F: "search",
  K: "quickConnect",
  P: "snippets",
  D: "splitRight",
  E: "splitDown",
};

const WITH_ALT: Record<string, Action> = {
  ArrowLeft: "focusLeft",
  ArrowRight: "focusRight",
  ArrowUp: "focusUp",
  ArrowDown: "focusDown",
};

// F11, Alt+arrows, Ctrl+Shift+C/V/W/F/K/P/D/E and Ctrl+(Shift+)Tab; everything
// else (Ctrl+C, Alt+letter, Ctrl+Alt+arrow included) belongs to the shell.
export function shortcutFor(e: KeyboardEvent): Action | null {
  if (e.altKey && !e.ctrlKey && !e.shiftKey && !e.metaKey)
    return WITH_ALT[e.key] ?? null;
  if (e.key === "F11" && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey)
    return "fullscreen";
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
