import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";

vi.mock("../lib/api", () => ({
  api: {
    libraryGet: vi.fn(),
    startupWarnings: vi.fn(),
    connectionSave: vi.fn(),
    connectionDependents: vi.fn(),
    connectionDelete: vi.fn(),
  },
}));

import { api } from "../lib/api";
import { library } from "../lib/stores/library.svelte";
import type { Connection } from "../lib/bindings/Connection";
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

beforeEach(() => {
  vi.resetAllMocks();
  library.groups = [];
  library.connections = [web, db, bastion];
  vi.mocked(api.libraryGet).mockImplementation(async () => ({
    version: 1,
    groups: [],
    connections: [web, db, bastion],
  }));
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
