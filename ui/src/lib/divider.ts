// Shared logic of the draggable dividers (`.divider` in app.css): the panes'
// split and the file panel's edge.

const STEP = 0.05;

// Called on the divider's pointerdown: until the pointer is released, each move
// reports the pointer's position as a fraction of `el` along `dir`.
export function startDrag(
  e: PointerEvent,
  el: HTMLElement,
  onRatio: (r: number) => void,
  dir: "row" | "column",
): void {
  const handle = e.currentTarget as HTMLElement;
  handle.setPointerCapture(e.pointerId);
  const move = (m: PointerEvent) => {
    const box = el.getBoundingClientRect();
    const size = dir === "row" ? box.width : box.height;
    if (size <= 0) return;
    onRatio(
      (dir === "row" ? m.clientX - box.left : m.clientY - box.top) / size,
    );
  };
  const up = (u: PointerEvent) => {
    handle.releasePointerCapture(u.pointerId);
    end();
  };
  const end = () => {
    handle.removeEventListener("pointermove", move);
    handle.removeEventListener("pointerup", up);
    handle.removeEventListener("lostpointercapture", end);
  };
  handle.addEventListener("pointermove", move);
  handle.addEventListener("pointerup", up);
  handle.addEventListener("lostpointercapture", end);
}

// The divider's new ratio for a key, or null when the key is not one of its own.
// The arrows follow the divider's aria-orientation: a vertical line moves
// left and right.
export function keyStep(e: KeyboardEvent, ratio: number): number | null {
  // Alt+arrow is the window's pane-focus shortcut, not a resize.
  if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return null;
  const vertical =
    (e.currentTarget as HTMLElement | null)?.getAttribute(
      "aria-orientation",
    ) === "vertical";
  const back = vertical ? "ArrowLeft" : "ArrowUp";
  const fwd = vertical ? "ArrowRight" : "ArrowDown";
  const next =
    e.key === back
      ? ratio - STEP
      : e.key === fwd
        ? ratio + STEP
        : e.key === "Enter"
          ? 0.5
          : null;
  if (next !== null) e.preventDefault();
  return next;
}
