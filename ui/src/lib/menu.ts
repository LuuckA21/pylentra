/** Enabled items of a menu, in order: the only ones the keyboard reaches. */
export const menuItems = (menu: HTMLElement) => [
  ...menu.querySelectorAll<HTMLElement>("button:not(:disabled)"),
];

/**
 * Keyboard of a context menu: arrows move between the enabled items, wrapping;
 * Escape closes and gives the focus back (`close(true)`); Tab closes and lets
 * the focus go on (`close(false)`).
 */
export function menuKeydown(
  e: KeyboardEvent,
  menu: HTMLElement,
  close: (refocus: boolean) => void,
): void {
  if (e.key === "Escape") {
    e.preventDefault();
    close(true);
  } else if (e.key === "Tab") {
    close(false);
  } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    const items = menuItems(menu);
    if (!items.length) return;
    e.preventDefault();
    const i = items.indexOf(document.activeElement as HTMLElement);
    const step = e.key === "ArrowDown" ? 1 : -1;
    items[(i + step + items.length) % items.length].focus();
  }
}
