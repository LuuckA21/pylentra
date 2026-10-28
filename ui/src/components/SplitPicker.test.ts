import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import type { Connection } from "../lib/bindings/Connection";
import { library } from "../lib/stores/library.svelte";
import { sessions } from "../lib/stores/sessions.svelte";
import SplitPicker from "./SplitPicker.svelte";

beforeEach(() => {
  sessions.tabs.splice(0);
  sessions.panes.splice(0);
  library.groups = [];
  library.connections = [
    {
      id: "c1",
      name: "web",
      group_id: null,
      host: "web.example.com",
      port: 22,
      username: "luca",
    } as Connection,
  ];
});

const setup = () => {
  const props = { onpick: vi.fn(), oncancel: vi.fn() };
  render(SplitPicker, props);
  return props;
};
const field = () => screen.getByRole("combobox");
const type = (text: string) =>
  fireEvent.input(field(), { target: { value: text } });

test("is a modal dialog titled for the new pane, with the field focused", () => {
  setup();
  const dialog = screen.getByRole("dialog", { name: "Open in the new pane" });
  expect(dialog.getAttribute("aria-modal")).toBe("true");
  expect(document.activeElement).toBe(field());
});

test("choosing a saved connection picks its id", async () => {
  const p = setup();
  await type("web");
  await fireEvent.click(screen.getAllByRole("option")[0]);
  expect(p.onpick).toHaveBeenCalledWith("c1");
  expect(sessions.tabs).toHaveLength(0);
});

test("a typed target and Enter picks it, without opening a tab", async () => {
  const p = setup();
  await type("u@h");
  await fireEvent.keyDown(field(), { key: "Enter" });
  expect(p.onpick).toHaveBeenCalledWith({ user: "u", host: "h", port: 22 });
  expect(sessions.tabs).toHaveLength(0);
});

test("Escape on open suggestions only closes them", async () => {
  const p = setup();
  await type("web");
  await fireEvent.keyDown(field(), { key: "Escape" });
  expect(p.oncancel).not.toHaveBeenCalled();
  await fireEvent.keyDown(field(), { key: "Escape" });
  expect(p.oncancel).toHaveBeenCalledTimes(1);
  expect(p.onpick).not.toHaveBeenCalled();
});

test("a click outside the dialog cancels", async () => {
  const p = setup();
  await fireEvent.mouseDown(screen.getByRole("dialog"));
  expect(p.oncancel).not.toHaveBeenCalled();
  await fireEvent.mouseDown(screen.getByRole("dialog").parentElement!);
  expect(p.oncancel).toHaveBeenCalledTimes(1);
  expect(p.onpick).not.toHaveBeenCalled();
});
