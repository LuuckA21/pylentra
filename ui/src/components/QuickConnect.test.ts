import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/svelte";
import type { Connection } from "../lib/bindings/Connection";
import { library } from "../lib/stores/library.svelte";
import { sessions } from "../lib/stores/sessions.svelte";
import QuickConnect from "./QuickConnect.svelte";

const conn = (id: string, name: string) =>
  ({
    id,
    name,
    group_id: null,
    host: `${name}.example.com`,
    port: 22,
    username: "luca",
  }) as Connection;

beforeEach(() => {
  sessions.tabs.splice(0);
  sessions.panes.splice(0);
  sessions.active = null;
  library.groups = [];
  library.connections = Array.from({ length: 8 }, (_, i) =>
    conn(`id${i}`, `web${i}`),
  );
});

const field = () => screen.getByRole("combobox");
const type = (text: string) =>
  fireEvent.input(field(), { target: { value: text } });

test("suggests saved connections that match, at most 6", async () => {
  render(QuickConnect, { onopen: vi.fn() });
  await type("web");
  // 6 matches + "Connect to"
  expect(screen.getAllByRole("option")).toHaveLength(7);
  expect(field().getAttribute("aria-expanded")).toBe("true");
  await type("web3");
  expect(screen.getAllByRole("option")).toHaveLength(2);
  expect(screen.getAllByRole("option")[1].textContent).toContain("web3");
});

test("Enter without a selection connects to the typed target", async () => {
  const onopen = vi.fn();
  render(QuickConnect, { onopen });
  await type("root@Host.example:2222");
  await fireEvent.keyDown(field(), { key: "Enter" });
  expect(onopen).not.toHaveBeenCalled();
  expect(sessions.panes[0].quick).toMatchObject({
    username: "root",
    host: "Host.example",
    port: 2222,
  });
});

test("ArrowDown + Enter opens the selected saved connection", async () => {
  const onopen = vi.fn();
  render(QuickConnect, { onopen });
  await type("web1");
  await fireEvent.keyDown(field(), { key: "ArrowDown" });
  expect(field().getAttribute("aria-activedescendant")).toBe(
    screen.getAllByRole("option")[0].id,
  );
  await fireEvent.keyDown(field(), { key: "Enter" });
  expect(onopen).toHaveBeenCalledWith("id1");
  expect(sessions.tabs).toHaveLength(0);
});

test("invalid text shows the error and does not connect", async () => {
  render(QuickConnect, { onopen: vi.fn() });
  await type("host");
  await fireEvent.keyDown(field(), { key: "Enter" });
  expect(screen.getByRole("alert").textContent).toBe("Add user@");
  expect(sessions.tabs).toHaveLength(0);
});

test("Escape closes the suggestions", async () => {
  render(QuickConnect, { onopen: vi.fn() });
  await type("web");
  await fireEvent.keyDown(field(), { key: "Escape" });
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(field().getAttribute("aria-expanded")).toBe("false");
});
