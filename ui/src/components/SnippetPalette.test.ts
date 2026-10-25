import { tick } from "svelte";
import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/svelte";
import type { Connection } from "../lib/bindings/Connection";
import type { Snippet } from "../lib/bindings/Snippet";
import { library } from "../lib/stores/library.svelte";
import { snippets } from "../lib/stores/snippets.svelte";
import SnippetPalette from "./SnippetPalette.svelte";

const snip = (
  id: string,
  name: string,
  text: string,
  o: Partial<Snippet> = {},
) => ({ id, name, text, run: false, ...o }) as Snippet;

beforeEach(() => {
  library.warnings = [];
  library.connections = ["A", "B"].map((id) => ({ id }) as Connection);
  snippets.list = [
    snip("1", "Zeta", "echo z", { connection_id: "A", run: true }),
    snip("2", "Alpha", "ls -l", { connection_id: "A" }),
    snip("3", "Global one", "echo g"),
    snip("4", "Other host", "pwd", { connection_id: "B" }),
  ];
});

const setup = (props: Partial<Parameters<typeof render>[1]> = {}) => {
  const handlers = { onpick: vi.fn(), onnew: vi.fn(), onclose: vi.fn() };
  render(SnippetPalette, {
    connectionId: "A",
    connectionName: "web",
    selection: "",
    ...handlers,
    ...props,
  });
  return handlers;
};
const names = () =>
  screen
    .getAllByRole("option")
    .map((o) => o.querySelector(".name")?.textContent);
const input = () => screen.getByRole("combobox");

test("the connection group comes before Global, each sorted", () => {
  setup();
  const groups = screen.getAllByRole("group");
  expect(groups.map((g) => g.getAttribute("aria-label"))).toEqual([
    "web",
    "Global",
  ]);
  expect(names()).toEqual(["Alpha", "Zeta", "Global one"]);
  expect(within(groups[0]).getAllByRole("option")).toHaveLength(2);
});

test("the highlighted item stays valid when the list shrinks", async () => {
  const { onpick } = setup();
  await fireEvent.keyDown(input(), { key: "ArrowDown" });
  await fireEvent.keyDown(input(), { key: "ArrowDown" });
  snippets.list = [snippets.list[1]];
  await tick();
  const id = input().getAttribute("aria-activedescendant");
  expect(id && document.getElementById(id)).toBeTruthy();
  await fireEvent.keyDown(input(), { key: "Enter" });
  expect(onpick).toHaveBeenCalledWith(expect.objectContaining({ id: "2" }));
});

test("a quick tab only sees global snippets", () => {
  setup({ connectionId: null, connectionName: undefined });
  expect(screen.getAllByRole("group")).toHaveLength(1);
  expect(names()).toEqual(["Global one"]);
});

test("typing filters", async () => {
  setup();
  await fireEvent.input(input(), { target: { value: "echo" } });
  expect(names()).toEqual(["Zeta", "Global one"]);
  await fireEvent.input(input(), { target: { value: "nothing" } });
  expect(screen.getByText("No matches.")).toBeTruthy();
});

test("arrow keys move the active option, Enter picks it, Esc closes", async () => {
  const { onpick, onclose } = setup();
  const opts = screen.getAllByRole("option");
  expect(input().getAttribute("aria-activedescendant")).toBe(opts[0].id);
  await fireEvent.keyDown(input(), { key: "ArrowDown" });
  expect(input().getAttribute("aria-activedescendant")).toBe(opts[1].id);
  expect(opts[1].getAttribute("aria-selected")).toBe("true");
  await fireEvent.keyDown(input(), { key: "Enter" });
  expect(onpick).toHaveBeenCalledWith(snippets.list[0]); // Zeta
  await fireEvent.keyDown(input(), { key: "Escape" });
  expect(onclose).toHaveBeenCalledTimes(1);
});

test("clicking an option picks it", async () => {
  const { onpick } = setup();
  await fireEvent.click(screen.getByText("Global one"));
  expect(onpick).toHaveBeenCalledWith(snippets.list[2]);
});

test("only snippets that run get the pill", () => {
  setup();
  const pills = screen.getAllByText("runs");
  expect(pills).toHaveLength(1);
  expect(pills[0].closest("[role=option]")?.textContent).toContain("Zeta");
});

test("save selection shows only with a selection", async () => {
  const { unmount } = render(SnippetPalette, {
    connectionId: "A",
    selection: "  ",
    onpick: vi.fn(),
    onnew: vi.fn(),
    onclose: vi.fn(),
  });
  expect(screen.queryByText("Save selection as snippet")).toBeNull();
  unmount();
  const { onnew } = setup({ selection: "ls" });
  await fireEvent.click(screen.getByText("Save selection as snippet"));
  expect(onnew).toHaveBeenCalledWith("ls");
  await fireEvent.click(screen.getByText("New snippet"));
  expect(onnew).toHaveBeenLastCalledWith("");
});

test("a snippet of a deleted connection shows under Global", () => {
  snippets.list = [snip("5", "Ghost", "echo", { connection_id: "gone" })];
  setup();
  expect(names()).toEqual(["Ghost"]);
  expect(screen.getAllByRole("group")).toHaveLength(1);
});

test("a read-only snippets file disables both new buttons", () => {
  library.warnings = [
    { code: "config.read_only", params: { file: "snippets.toml" } },
  ];
  setup({ selection: "ls" });
  for (const name of ["New snippet", "Save selection as snippet"])
    expect(
      (screen.getByRole("button", { name }) as HTMLButtonElement).disabled,
    ).toBe(true);
});
