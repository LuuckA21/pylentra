import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";

vi.mock("../lib/api", () => ({
  api: {
    libraryGet: vi.fn(),
    startupWarnings: vi.fn(),
    groupSave: vi.fn(),
    sshConfigScan: vi.fn(),
    connectionsImport: vi.fn(),
  },
}));

import { api } from "../lib/api";
import type { Candidate } from "../lib/bindings/Candidate";
import type { ScanResult } from "../lib/bindings/ScanResult";
import { library } from "../lib/stores/library.svelte";
import { toasts } from "../lib/stores/toasts.svelte";
import ImportDialog from "./ImportDialog.svelte";

const cand = (
  name: string,
  extra: Partial<Candidate> = {},
  username = "luca",
): Candidate => ({
  connection: {
    id: name,
    name,
    group_id: null,
    host: `${name}.example`,
    port: 22,
    username,
    auth: { type: "Auto" },
    jump_via: null,
    forwards: [],
  },
  notes: [],
  error: null,
  exists: false,
  needs_user: username === "",
  ...extra,
});
const scan = (candidates: Candidate[], extra: Partial<ScanResult> = {}) =>
  vi.mocked(api.sshConfigScan).mockResolvedValue({
    path: "/home/u/.ssh/config",
    missing: false,
    candidates,
    skipped: [],
    ...extra,
  });

async function open() {
  const onclose = vi.fn();
  render(ImportDialog, { onclose });
  await screen.findByRole("dialog");
  return onclose;
}
const box = (name: string) =>
  screen.getByRole("checkbox", { name: new RegExp(`\\b${name}\\b`) });
const importBtn = () =>
  screen.getByRole("button", { name: /^Import \d+ connections?$/ });

beforeEach(() => {
  vi.resetAllMocks();
  toasts.length = 0;
  library.groups = [];
  library.connections = [];
  library.warnings = [];
  vi.mocked(api.libraryGet).mockResolvedValue({
    version: 1,
    groups: [],
    connections: [],
  });
  vi.mocked(api.startupWarnings).mockResolvedValue([]);
  vi.mocked(api.groupSave).mockResolvedValue(undefined);
  vi.mocked(api.connectionsImport).mockResolvedValue(2);
});

test("existing hosts are disabled and unchecked", async () => {
  scan([cand("web"), cand("old", { exists: true })]);
  await open();
  await screen.findByText("already present");
  const old = box("old") as HTMLInputElement;
  expect(old.disabled).toBe(true);
  expect(old.checked).toBe(false);
  expect((box("web") as HTMLInputElement).checked).toBe(true);
});

test("candidates with an error are disabled and show the reason", async () => {
  scan([cand("bad", { error: { type: "InvalidPort", value: "99999" } })]);
  await open();
  expect(
    ((await screen.findByRole("checkbox")) as HTMLInputElement).disabled,
  ).toBe(true);
  expect(screen.getByText("Invalid port: 99999.")).toBeTruthy();
  const ids = screen.getByRole("checkbox").getAttribute("aria-describedby");
  expect(document.getElementById(ids!)?.textContent).toBe(
    "Invalid port: 99999.",
  );
});

test("user field appears and is required when a selected host has no User", async () => {
  scan([cand("web", {}, ""), cand("db")]);
  await open();
  const user = (await screen.findByLabelText(
    "User for hosts without User",
  )) as HTMLInputElement;
  expect(user.required).toBe(true);
  expect((importBtn() as HTMLButtonElement).disabled).toBe(true);
  await fireEvent.input(user, { target: { value: "root" } });
  expect((importBtn() as HTMLButtonElement).disabled).toBe(false);
  await fireEvent.click(box("web"));
  expect(screen.queryByLabelText("User for hosts without User")).toBeNull();
});

test("Import sends the prepared connections and the chosen group, then toasts", async () => {
  library.groups = [{ id: "g1", name: "prod" }];
  scan([cand("web", {}, ""), cand("db")]);
  const onclose = await open();
  await fireEvent.input(
    await screen.findByLabelText("User for hosts without User"),
    {
      target: { value: "root" },
    },
  );
  await fireEvent.change(screen.getByLabelText("Destination group"), {
    target: { value: "g1" },
  });
  await fireEvent.click(
    screen.getByRole("button", { name: "Import 2 connections" }),
  );
  await waitFor(() => expect(onclose).toHaveBeenCalled());
  const [sent, group] = vi.mocked(api.connectionsImport).mock.calls[0]!;
  expect(sent.map((c) => [c.name, c.username])).toEqual([
    ["web", "root"],
    ["db", "luca"],
  ]);
  expect(group).toBe("g1");
  expect(toasts.at(-1)?.message).toBe("Imported 2 connections");
  expect(toasts.at(-1)?.kind).toBe("success");
});

test("new 'ssh config' group is created before importing", async () => {
  vi.mocked(api.connectionsImport).mockResolvedValue(1);
  scan([cand("web")]);
  await open();
  await fireEvent.change(await screen.findByLabelText("Destination group"), {
    target: { value: "__new__" },
  });
  await fireEvent.click(importBtn());
  await waitFor(() => expect(api.connectionsImport).toHaveBeenCalled());
  const saved = vi.mocked(api.groupSave).mock.calls[0]![0];
  expect(saved.name).toBe("ssh config");
  expect(vi.mocked(api.connectionsImport).mock.calls[0]![1]).toBe(saved.id);
  expect(vi.mocked(api.groupSave).mock.invocationCallOrder[0]).toBeLessThan(
    vi.mocked(api.connectionsImport).mock.invocationCallOrder[0]!,
  );
  await waitFor(() =>
    expect(toasts.at(-1)?.message).toBe("Imported 1 connection"),
  );
});

test("an import error keeps the dialog open", async () => {
  vi.mocked(api.connectionsImport).mockRejectedValue({
    code: "config.read_only",
    params: {},
  });
  scan([cand("web")]);
  const onclose = await open();
  await fireEvent.click(
    await screen.findByRole("button", { name: "Import 1 connection" }),
  );
  await waitFor(() => expect(toasts.at(-1)?.kind).toBe("error"));
  expect(onclose).not.toHaveBeenCalled();
});

test("missing file shows the searched path", async () => {
  scan([], { missing: true });
  await open();
  expect(await screen.findByText(/\/home\/u\/\.ssh\/config/)).toBeTruthy();
  expect((importBtn() as HTMLButtonElement).disabled).toBe(true);
});

test("read-only library disables Import", async () => {
  library.warnings = [{ code: "config.read_only", params: {} }];
  scan([cand("web")]);
  await open();
  await screen.findByRole("checkbox");
  expect((importBtn() as HTMLButtonElement).disabled).toBe(true);
});

test("a retry after a failed import reuses the new group", async () => {
  vi.mocked(api.connectionsImport)
    .mockRejectedValueOnce({ code: "io", params: { reason: "x" } })
    .mockResolvedValueOnce(1);
  scan([cand("web")]);
  const onclose = await open();
  await fireEvent.change(await screen.findByLabelText("Destination group"), {
    target: { value: "__new__" },
  });
  await fireEvent.click(importBtn());
  await waitFor(() => expect(toasts.at(-1)?.kind).toBe("error"));
  await fireEvent.click(importBtn());
  await waitFor(() => expect(onclose).toHaveBeenCalled());
  expect(api.groupSave).toHaveBeenCalledTimes(1);
  const id = vi.mocked(api.groupSave).mock.calls[0]![0].id;
  expect(vi.mocked(api.connectionsImport).mock.calls.map((c) => c[1])).toEqual([
    id,
    id,
  ]);
});
