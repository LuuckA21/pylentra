import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";

vi.mock("../lib/api", () => ({
  api: {
    libraryGet: vi.fn(),
    startupWarnings: vi.fn(),
    connectionSave: vi.fn(),
    connectionDependents: vi.fn(),
    connectionDelete: vi.fn(),
    groupSave: vi.fn(),
    groupDelete: vi.fn(),
  },
}));

import { api } from "../lib/api";
import { library } from "../lib/stores/library.svelte";
import type { Connection } from "../lib/bindings/Connection";
import type { Group } from "../lib/bindings/Group";
import Sidebar from "./Sidebar.svelte";

const conn = (id: string, name: string, host: string): Connection => ({
  id,
  name,
  group_id: null,
  host,
  port: 22,
  username: "luca",
  auth: { type: "Agent" },
  jump_via: null,
  forwards: [],
});
const web = conn("a", "web", "web.example.com");
const db = conn("b", "db", "db.example.com");
const bastion = conn("c", "bastion", "bastion.example.com");

function setup() {
  const props = {
    openConnectionIds: new Set<string>(),
    onopen: vi.fn(),
    onedit: vi.fn(),
  };
  render(Sidebar, props);
  return props;
}

// What the mocked backend returns; group tests change it like the real store would.
let groups: Group[] = [];

beforeEach(() => {
  vi.resetAllMocks();
  groups = [];
  library.groups = [];
  library.connections = [web, db, bastion];
  vi.mocked(api.libraryGet).mockImplementation(async () => ({
    version: 1,
    groups,
    connections: [web, db, bastion],
  }));
  vi.mocked(api.groupSave).mockImplementation(async (g) => {
    groups = [...groups.filter((x) => x.id !== g.id), g];
  });
  vi.mocked(api.groupDelete).mockResolvedValue();
  vi.mocked(api.startupWarnings).mockResolvedValue([]);
  vi.mocked(api.connectionSave).mockResolvedValue([]);
  vi.mocked(api.connectionDependents).mockResolvedValue([]);
  vi.mocked(api.connectionDelete).mockResolvedValue();
});

test("search filters list", async () => {
  setup();
  expect(screen.getAllByRole("option")).toHaveLength(3);
  await fireEvent.input(screen.getByRole("searchbox"), {
    target: { value: "bast" },
  });
  const rows = screen.getAllByRole("option");
  expect(rows).toHaveLength(1);
  expect(rows[0].textContent).toContain("bastion");
});

test("Ctrl+Shift+F focuses and selects the search box", async () => {
  setup();
  const box = screen.getByRole("searchbox") as HTMLInputElement;
  await fireEvent.input(box, { target: { value: "web" } });
  const row = screen.getByRole("option", { name: /web/ });
  row.focus();
  // Keys bubble from wherever focus is (the terminal included) to the window.
  const e = new KeyboardEvent("keydown", {
    key: "F",
    ctrlKey: true,
    shiftKey: true,
    bubbles: true,
    cancelable: true,
  });
  row.dispatchEvent(e);
  expect(document.activeElement).toBe(box);
  expect([box.selectionStart, box.selectionEnd]).toEqual([0, 3]);
  expect(e.defaultPrevented).toBe(true);
});

test("Enter on focused row emits open", async () => {
  const { onopen } = setup();
  const row = screen.getByRole("option", { name: /db/ });
  row.focus();
  await fireEvent.keyDown(row, { key: "Enter" });
  expect(onopen).toHaveBeenCalledWith("b");
});

test("delete asks confirmation listing dependents", async () => {
  vi.mocked(api.connectionDependents).mockResolvedValue(["b", "c"]);
  setup();
  await fireEvent.contextMenu(screen.getByRole("option", { name: /web/ }));
  await fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
  const dialog = await screen.findByRole("alertdialog");
  expect(dialog.textContent).toContain("db");
  expect(dialog.textContent).toContain("bastion");
  expect(api.connectionDelete).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole("button", { name: "Delete" }));
  await waitFor(() => expect(api.connectionDelete).toHaveBeenCalledWith("a"));
});

test("duplicate creates new id and suffixed name", async () => {
  setup();
  await fireEvent.contextMenu(screen.getByRole("option", { name: /web/ }));
  await fireEvent.click(screen.getByRole("menuitem", { name: "Duplicate" }));
  await waitFor(() => expect(api.connectionSave).toHaveBeenCalledOnce());
  const [copy] = vi.mocked(api.connectionSave).mock.calls[0];
  expect(copy.id).not.toBe("a");
  expect(copy.name).toBe("web (copy)");
  expect(copy.host).toBe("web.example.com");
});

test("a connection whose group was deleted lands under No group", () => {
  library.connections = [{ ...web, group_id: "gone" }];
  setup();
  expect(screen.getByText("No group")).toBeTruthy();
  expect(screen.getAllByRole("option")).toHaveLength(1);
});

test("keyboard: Move to group submenu keeps focus and moves the connection", async () => {
  library.groups = groups = [{ id: "g", name: "Servers" }];
  setup();
  const row = screen.getByRole("option", { name: /web/ });
  row.focus();
  await fireEvent.keyDown(row, { key: "F10", shiftKey: true });
  const menu = screen.getByRole("menu");
  await waitFor(() =>
    expect(document.activeElement).toBe(
      screen.getByRole("menuitem", { name: "Edit" }),
    ),
  );
  await fireEvent.keyDown(menu, { key: "ArrowDown" });
  await fireEvent.keyDown(menu, { key: "ArrowDown" });
  const move = screen.getByRole("menuitem", { name: "Move to group" });
  expect(document.activeElement).toBe(move);
  await fireEvent.click(move); // what Enter does on a focused button
  const first = await screen.findByRole("menuitem", { name: "Servers" });
  await waitFor(() => expect(document.activeElement).toBe(first));
  await fireEvent.click(first);
  await waitFor(() => expect(api.connectionSave).toHaveBeenCalledOnce());
  expect(vi.mocked(api.connectionSave).mock.calls[0][0]).toMatchObject({
    id: "a",
    group_id: "g",
  });
});

test("create group appears in list", async () => {
  setup();
  await fireEvent.click(screen.getByRole("button", { name: "New group" }));
  await fireEvent.input(screen.getByLabelText("Group name"), {
    target: { value: " Servers " },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(api.groupSave).toHaveBeenCalledOnce());
  const [g] = vi.mocked(api.groupSave).mock.calls[0];
  expect(g.name).toBe("Servers");
  expect(g.id).toBeTruthy();
  expect(await screen.findByRole("button", { name: /Servers/ })).toBeTruthy();
});

test("group dialog rejects a blank name", async () => {
  setup();
  await fireEvent.click(screen.getByRole("button", { name: "New group" }));
  await fireEvent.input(screen.getByLabelText("Group name"), {
    target: { value: "   " },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Save" }));
  const input = screen.getByLabelText("Group name");
  expect(input.getAttribute("aria-invalid")).toBe("true");
  expect(
    document.getElementById(input.getAttribute("aria-describedby")!),
  ).toBeTruthy();
  expect(api.groupSave).not.toHaveBeenCalled();
});

test("rename group calls groupSave with same id and new name", async () => {
  library.groups = groups = [{ id: "g", name: "Servers" }];
  setup();
  await fireEvent.contextMenu(screen.getByRole("button", { name: /Servers/ }));
  await fireEvent.click(screen.getByRole("menuitem", { name: "Rename" }));
  const input = screen.getByLabelText("Group name") as HTMLInputElement;
  expect(input.value).toBe("Servers");
  await fireEvent.input(input, { target: { value: "Prod" } });
  await fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() =>
    expect(api.groupSave).toHaveBeenCalledWith({ id: "g", name: "Prod" }),
  );
});

test("delete group confirms, then calls groupDelete", async () => {
  library.groups = groups = [{ id: "g", name: "Servers" }];
  setup();
  const header = screen.getByRole("button", { name: /Servers/ });
  header.focus();
  await fireEvent.keyDown(header, { key: "F10", shiftKey: true });
  await fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
  const dialog = await screen.findByRole("alertdialog");
  expect(dialog.textContent).toContain("No group");
  expect(api.groupDelete).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole("button", { name: "Delete" }));
  await waitFor(() => expect(api.groupDelete).toHaveBeenCalledWith("g"));
});
