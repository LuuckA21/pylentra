const FOCUSABLE =
  'button:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

// Svelte action for dialogs: moves focus in ([data-autofocus] or the first
// control), keeps Tab inside, closes on Escape and gives focus back on destroy.
export function modal(node: HTMLElement, onescape: () => void) {
  const previous = document.activeElement as HTMLElement | null;
  const controls = () => [...node.querySelectorAll<HTMLElement>(FOCUSABLE)];
  (
    node.querySelector<HTMLElement>("[data-autofocus]") ?? controls()[0]
  )?.focus();

  const onkeydown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      onescape();
    } else if (e.key === "Tab") {
      const all = controls();
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
      if (previous?.isConnected) previous.focus();
    },
  };
}
