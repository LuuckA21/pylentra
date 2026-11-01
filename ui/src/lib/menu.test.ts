import { expect, test, vi } from "vitest";
import { menuKeydown } from "./menu";

const setup = () => {
  document.body.innerHTML =
    '<div role="menu"><button>a</button><button disabled>b</button><button>c</button></div>';
  const menu = document.querySelector<HTMLElement>("[role=menu]")!;
  const [a, , c] = menu.querySelectorAll("button");
  const close = vi.fn();
  const key = (k: string) => {
    const e = new KeyboardEvent("keydown", { key: k, cancelable: true });
    menuKeydown(e, menu, close);
    return e;
  };
  return { a, c, close, key };
};

test("arrows move over the enabled items and wrap around", () => {
  const { a, c, key } = setup();
  a.focus();
  expect(key("ArrowDown").defaultPrevented).toBe(true);
  expect(document.activeElement).toBe(c);
  key("ArrowDown");
  expect(document.activeElement).toBe(a);
  key("ArrowUp");
  expect(document.activeElement).toBe(c);
});

test("Escape closes and refocuses, Tab closes and leaves the focus alone", () => {
  const { close, key } = setup();
  expect(key("Escape").defaultPrevented).toBe(true);
  expect(close).toHaveBeenLastCalledWith(true);
  expect(key("Tab").defaultPrevented).toBe(false);
  expect(close).toHaveBeenLastCalledWith(false);
});
