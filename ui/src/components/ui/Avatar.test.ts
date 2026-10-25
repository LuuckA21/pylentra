import { expect, test } from "vitest";
import { render } from "@testing-library/svelte";
import Avatar from "./Avatar.svelte";

test("with an icon it shows only a decorative svg", () => {
  const { container } = render(Avatar, {
    name: "prod-db",
    icon: "database",
    color: "red",
  });
  expect(container.querySelector("svg[aria-hidden='true']")).not.toBeNull();
  expect(container.textContent?.trim()).toBe("");
});

test("without an icon it shows the initials and the color class", () => {
  const { container, getByText } = render(Avatar, {
    name: "prod-db",
    color: "red",
  });
  expect(getByText("PD")).toBeTruthy();
  expect(container.querySelector(".tag-red")).not.toBeNull();
});

test("a null color is the neutral tag", () => {
  const { container } = render(Avatar, { name: "prod-db", color: null });
  expect(container.querySelector(".tag-none")).not.toBeNull();
});
