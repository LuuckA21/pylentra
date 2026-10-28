const FOCUSABLE =
  'button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

const controls = (node: HTMLElement) => [
  ...node.querySelectorAll<HTMLElement>(FOCUSABLE),
];

// [data-autofocus], or the first control.
export function focusFirst(node: HTMLElement): void {
  (
    node.querySelector<HTMLElement>("[data-autofocus]") ?? controls(node)[0]
  )?.focus();
}

// Svelte action for dialogs: moves focus in, keeps Tab inside, closes on
// Escape and gives focus back on destroy. Without `onescape` the dialog handles
// Escape itself. Inside a pane without the focus ([data-unfocused]) it leaves
// the focus alone: the pane focuses it ([data-modal]) when it gets the focus.
export function modal(node: HTMLElement, onescape?: () => void) {
  node.dataset.modal = "";
  const waits = !!node.closest("[data-unfocused]");
  const previous = waits ? null : (document.activeElement as HTMLElement);
  if (!waits) focusFirst(node);

  const onkeydown = (e: KeyboardEvent) => {
    if (e.key === "Escape" && onescape) {
      e.stopPropagation();
      onescape();
    } else if (e.key === "Tab") {
      const all = controls(node);
      const [first, last] = [all[0], all[all.length - 1]];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last?.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first?.focus();
      }
    }
  };
  node.addEventListener("keydown", onkeydown);
  return {
    destroy() {
      node.removeEventListener("keydown", onkeydown);
      // Not when the focus has moved on meanwhile (to another pane, say).
      const now = document.activeElement;
      const free = !now || now === document.body || node.contains(now);
      if (free && previous?.isConnected) previous.focus();
    },
  };
}
