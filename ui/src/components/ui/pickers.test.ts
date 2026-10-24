import { expect, test } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import ColorPicker from "./ColorPicker.svelte";
import IconPicker from "./IconPicker.svelte";

const colors = () =>
  render(ColorPicker, { value: null, noneLabel: "From group", label: "Color" });

test("color picker: radiogroup with a labelled none and one swatch per color", () => {
  colors();
  const group = screen.getByRole("radiogroup", { name: "Color" });
  expect(group.querySelectorAll("[role=radio]")).toHaveLength(9);
  expect(screen.getByRole("radio", { name: "From group" })).toBeTruthy();
  expect(screen.getByRole("radio", { name: "Violet" })).toBeTruthy();
});

test("color picker: one tab stop on the checked item, arrows wrap and select", async () => {
  colors();
  const none = screen.getByRole("radio", { name: "From group" });
  expect(none.getAttribute("aria-checked")).toBe("true");
  expect(screen.getAllByRole("radio").filter((r) => r.tabIndex === 0)).toEqual([
    none,
  ]);
  await fireEvent.keyDown(none, { key: "ArrowLeft" });
  const gray = screen.getByRole("radio", { name: "Gray" });
  expect(gray.getAttribute("aria-checked")).toBe("true");
  expect(document.activeElement).toBe(gray);
  await fireEvent.keyDown(gray, { key: "ArrowRight" });
  expect(none.getAttribute("aria-checked")).toBe("true");
  await fireEvent.keyDown(none, { key: "ArrowDown" });
  expect(
    screen.getByRole("radio", { name: "Red" }).getAttribute("aria-checked"),
  ).toBe("true");
});

test("icon picker: Initials plus eight icons, click selects", async () => {
  render(IconPicker, { value: null, label: "Icon" });
  expect(screen.getAllByRole("radio")).toHaveLength(9);
  const initials = screen.getByRole("radio", { name: "Initials" });
  expect(initials.getAttribute("aria-checked")).toBe("true");
  await fireEvent.click(screen.getByRole("radio", { name: "Raspberry Pi" }));
  expect(initials.getAttribute("aria-checked")).toBe("false");
  expect(
    screen
      .getByRole("radio", { name: "Raspberry Pi" })
      .getAttribute("aria-checked"),
  ).toBe("true");
});
