import { expect, test, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import type { Snippet } from "../lib/bindings/Snippet";
import { rememberVars } from "../lib/snippets";
import SnippetVarsDialog from "./SnippetVarsDialog.svelte";

const snippet: Snippet = { id: "d1", name: "Tail", text: "", run: false };

function setup(names: string[]) {
  const p = { snippet, names, onsubmit: vi.fn(), oncancel: vi.fn() };
  render(SnippetVarsDialog, p);
  return p;
}

test("a labelled field per name, the first focused, in a modal dialog", () => {
  setup(["host", "port"]);
  const dialog = screen.getByRole("dialog", { name: "Values for Tail" });
  expect(dialog.getAttribute("aria-modal")).toBe("true");
  expect(document.activeElement).toBe(screen.getByLabelText("host"));
  expect(screen.getByLabelText("port")).toBeTruthy();
});

test("fields start from the remembered values", () => {
  rememberVars("d1", { host: "web" });
  setup(["host", "port"]);
  expect((screen.getByLabelText("host") as HTMLInputElement).value).toBe("web");
  expect((screen.getByLabelText("port") as HTMLInputElement).value).toBe("");
});

test("Enter submits the values, empty ones included", async () => {
  const { onsubmit } = setup(["a", "b"]);
  await fireEvent.input(screen.getByLabelText("a"), { target: { value: "1" } });
  await fireEvent.submit(screen.getByLabelText("a").closest("form")!);
  expect(onsubmit).toHaveBeenCalledWith({ a: "1", b: "" });
});

test("a name like __proto__ becomes an own property", async () => {
  const { onsubmit } = setup(["__proto__"]);
  await fireEvent.input(screen.getByLabelText("__proto__"), {
    target: { value: "v" },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Send" }));
  const values = onsubmit.mock.calls[0][0];
  expect(Object.hasOwn(values, "__proto__")).toBe(true);
  expect(values["__proto__"]).toBe("v");
});

test("Esc and Cancel cancel", async () => {
  const { oncancel, onsubmit } = setup(["a"]);
  await fireEvent.keyDown(screen.getByLabelText("a"), { key: "Escape" });
  await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(oncancel).toHaveBeenCalledTimes(2);
  expect(onsubmit).not.toHaveBeenCalled();
});
