import { beforeEach, expect, test, vi } from "vitest";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/svelte";

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
    onsettings: vi.fn(),
    onimport: vi.fn(),
  };
  render(Sidebar, props);
  return props;
}

// What the mocked backend returns; group tests change it like the real store would.
let groups: Group[] = [];

beforeEach(() => {
  vi.resetAllMocks();
  vi.unstubAllGlobals();
  localStorage.clear();
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

// The "more" menu holds New group and Import.
async function openMore() {
  await fireEvent.click(screen.getByRole("button", { name: "More" }));
}

test("search filters list", async () => {
  setup();
  expect(screen.getAllByRole("option")).toHaveLength(3);
  await fireEvent.input(screen.getByRole("textbox", { name: "Search" }), {
    target: { value: "bast" },
  });
  const rows = screen.getAllByRole("option");
  expect(rows).toHaveLength(1);
  expect(rows[0].textContent).toContain("bastion");
});

test("Ctrl+Shift+F focuses and selects the search box", async () => {
  setup();
  const box = screen.getByRole("textbox", {
    name: "Search",
  }) as HTMLInputElement;
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

test("Ctrl+Shift+F does nothing from inside a dialog", () => {
  setup();
  const dialog = document.createElement("div");
  dialog.setAttribute("aria-modal", "true");
  const input = document.createElement("input");
  dialog.append(input);
  document.body.append(dialog);
  input.focus();
  const e = new KeyboardEvent("keydown", {
    key: "F",
    ctrlKey: true,
    shiftKey: true,
    bubbles: true,
    cancelable: true,
  });
  input.dispatchEvent(e);
  expect(document.activeElement).toBe(input);
  expect(e.defaultPrevented).toBe(false);
  dialog.remove();
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
  expect(
    screen.getByRole("button", { name: /No group/, expanded: true }),
  ).toBeTruthy();
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
  await openMore();
  await fireEvent.click(screen.getByRole("menuitem", { name: "New group" }));
  await fireEvent.input(screen.getByLabelText("Group name"), {
    target: { value: " Servers " },
  });
  await fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(api.groupSave).toHaveBeenCalledOnce());
  const [g] = vi.mocked(api.groupSave).mock.calls[0];
  expect(g.name).toBe("Servers");
  expect(g.id).toBeTruthy();
  expect(
    await screen.findByRole("button", { name: /Servers/, expanded: true }),
  ).toBeTruthy();
});

test("group dialog rejects a blank name", async () => {
  setup();
  await openMore();
  await fireEvent.click(screen.getByRole("menuitem", { name: "New group" }));
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
  await fireEvent.contextMenu(
    screen.getByRole("button", { name: /Servers/, expanded: true }),
  );
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
  const header = screen.getByRole("button", {
    name: /Servers/,
    expanded: true,
  });
  header.focus();
  await fireEvent.keyDown(header, { key: "F10", shiftKey: true });
  await fireEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
  const dialog = await screen.findByRole("alertdialog");
  expect(dialog.textContent).toContain("No group");
  expect(api.groupDelete).not.toHaveBeenCalled();
  await fireEvent.click(screen.getByRole("button", { name: "Delete" }));
  await waitFor(() => expect(api.groupDelete).toHaveBeenCalledWith("g"));
});

const grouped = () => {
  library.groups = groups = [{ id: "g", name: "Servers" }];
  library.connections = [{ ...web, group_id: "g" }, db, bastion];
};

test("group pill shows only that group, without headings", async () => {
  grouped();
  setup();
  await fireEvent.click(
    screen.getByRole("button", { name: "Servers", pressed: false }),
  );
  const rows = screen.getAllByRole("option");
  expect(rows).toHaveLength(1);
  expect(rows[0].textContent).toContain("web");
  expect(screen.queryByRole("button", { expanded: true })).toBeNull();
});

test("All shows every group with its heading", () => {
  grouped();
  setup();
  expect(
    screen.getByRole("button", { name: "All", pressed: true }),
  ).toBeTruthy();
  expect(screen.getAllByRole("option")).toHaveLength(3);
  expect(screen.getAllByRole("button", { expanded: true })).toHaveLength(2);
});

test("No group pill shows ungrouped connections", async () => {
  grouped();
  setup();
  await fireEvent.click(
    screen.getByRole("button", { name: "No group", pressed: false }),
  );
  const names = screen.getAllByRole("option").map((r) => r.textContent);
  expect(names).toHaveLength(2);
  expect(names.join()).toContain("db");
  expect(names.join()).toContain("bastion");
});

test("a collapsed group stays collapsed after a reload", async () => {
  grouped();
  const first = render(Sidebar, {
    openConnectionIds: new Set<string>(),
    onopen: vi.fn(),
    onedit: vi.fn(),
    onsettings: vi.fn(),
    onimport: vi.fn(),
  });
  await fireEvent.click(
    screen.getByRole("button", { name: /Servers/, expanded: true }),
  );
  expect(screen.getAllByRole("option")).toHaveLength(2);
  first.unmount();
  setup();
  expect(screen.getAllByRole("option")).toHaveLength(2);
  expect(
    screen.getByRole("button", { name: /Servers/, expanded: false }),
  ).toBeTruthy();
});

test("more menu offers New group and Import from ~/.ssh/config", async () => {
  const { onimport } = setup();
  await openMore();
  expect(screen.getByRole("menuitem", { name: "New group" })).toBeTruthy();
  await fireEvent.click(
    screen.getByRole("menuitem", { name: "Import from ~/.ssh/config" }),
  );
  expect(onimport).toHaveBeenCalledOnce();
  expect(screen.queryByRole("menu")).toBeNull();
});

test("empty library offers new connection and import", async () => {
  library.connections = [];
  const { onedit, onimport } = setup();
  expect(screen.getByText("No connections yet.")).toBeTruthy();
  const empty = screen.getByText("No connections yet.").parentElement!;
  await fireEvent.click(
    within(empty).getByRole("button", { name: "New connection" }),
  );
  expect(onedit).toHaveBeenCalledWith();
  await fireEvent.click(
    within(empty).getByRole("button", { name: "Import from ~/.ssh/config" }),
  );
  expect(onimport).toHaveBeenCalledOnce();
});

test("row shows initials avatar, name, user@host and the open label", () => {
  render(Sidebar, {
    openConnectionIds: new Set(["a"]),
    onopen: vi.fn(),
    onedit: vi.fn(),
    onsettings: vi.fn(),
    onimport: vi.fn(),
  });
  const row = screen.getByRole("option", { name: /web/ });
  expect(row.textContent).toContain("WE");
  expect(row.querySelector(".name")?.textContent).toBe("web");
  expect(row.textContent).toContain("luca@web.example.com");
  expect(row.textContent).toContain("active");
  expect(screen.getByRole("option", { name: /db/ }).textContent).not.toContain(
    "active",
  );
});

test("the collapse button folds the sidebar to avatars and remembers it", async () => {
  const { onopen } = setup();
  await fireEvent.click(
    screen.getByRole("button", { name: "Collapse sidebar" }),
  );
  expect(screen.queryByRole("textbox", { name: "Search" })).toBeNull();
  expect(localStorage.getItem("pylentra.sidebar.collapsed")).toBe("true");
  await fireEvent.click(screen.getByRole("button", { name: "web" }));
  expect(onopen).toHaveBeenCalledWith("a");
  await fireEvent.click(screen.getByRole("button", { name: "Expand sidebar" }));
  expect(screen.getByRole("textbox", { name: "Search" })).toBeTruthy();
});

test("on a narrow window the sidebar opens over the sessions and closes on Escape or open", async () => {
  vi.stubGlobal("matchMedia", () => ({
    matches: true,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
  const { onopen } = setup();
  expect(screen.queryByRole("textbox", { name: "Search" })).toBeNull();
  await fireEvent.click(screen.getByRole("button", { name: "Expand sidebar" }));
  expect(screen.getAllByRole("option")).toHaveLength(3);
  await fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("textbox", { name: "Search" })).toBeNull();
  await fireEvent.click(screen.getByRole("button", { name: "Expand sidebar" }));
  await fireEvent.keyDown(screen.getByRole("option", { name: /db/ }), {
    key: "Enter",
  });
  expect(onopen).toHaveBeenCalledWith("b");
  expect(screen.queryByRole("textbox", { name: "Search" })).toBeNull();
  // The narrow overlay never touches the saved wide-window preference.
  expect(localStorage.getItem("pylentra.sidebar.collapsed")).toBeNull();
});

test.each(["null", "{}", "5", '"x"', "{oops"])(
  "a junk stored sidebar state (%s) does not break rendering",
  (junk) => {
    localStorage.setItem("pylentra.sidebar.groups", junk);
    localStorage.setItem("pylentra.sidebar.collapsed", junk);
    setup();
    expect(screen.getAllByRole("option")).toHaveLength(3);
  },
);

test("the More button reports expanded while its menu is open", async () => {
  setup();
  const more = screen.getByRole("button", { name: "More" });
  expect(more.getAttribute("aria-expanded")).toBe("false");
  await fireEvent.click(more);
  expect(more.getAttribute("aria-expanded")).toBe("true");
  await fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
  expect(more.getAttribute("aria-expanded")).toBe("false");
});
