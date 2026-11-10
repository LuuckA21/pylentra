import { expect, test, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import ConflictPrompt from "./ConflictPrompt.svelte";

test("Skip with apply to all answers once for every conflict", async () => {
  const onanswer = vi.fn();
  render(ConflictPrompt, { name: "site/index.html", onanswer });
  expect(screen.getByRole("dialog").textContent).toContain("site/index.html");
  // The safe choice has the focus: Enter never overwrites.
  expect(document.activeElement).toBe(
    screen.getByRole("button", { name: "Skip" }),
  );
  await fireEvent.click(screen.getByLabelText("Apply to all"));
  await fireEvent.click(screen.getByRole("button", { name: "Skip" }));
  expect(onanswer).toHaveBeenCalledWith({
    type: "Conflict",
    choice: "Skip",
    all: true,
  });
});

test("Escape cancels the transfer", async () => {
  const onanswer = vi.fn();
  render(ConflictPrompt, { name: "a", onanswer });
  await fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
  expect(onanswer).toHaveBeenCalledWith({
    type: "Conflict",
    choice: "Cancel",
    all: false,
  });
});
