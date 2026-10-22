import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";

vi.mock("../lib/api", () => ({
  api: {
    libraryGet: vi.fn(),
    startupWarnings: vi.fn(),
    connectionSave: vi.fn(),
  },
}));

import { api } from "../lib/api";
import { library } from "../lib/stores/library.svelte";
import ConnectionForm from "./ConnectionForm.svelte";

function setup() {
  const props = { onsaved: vi.fn(), oncancel: vi.fn() };
  render(ConnectionForm, props);
  return props;
}

const type = (label: string, value: string) =>
  fireEvent.input(screen.getByLabelText(label), { target: { value } });
const save = () =>
  fireEvent.click(screen.getByRole("button", { name: "Save" }));

beforeEach(() => {
  vi.resetAllMocks();
  library.groups = [];
  library.connections = [];
  vi.mocked(api.libraryGet).mockResolvedValue({
    version: 1,
    groups: [],
    connections: [],
  });
  vi.mocked(api.startupWarnings).mockResolvedValue([]);
  vi.mocked(api.connectionSave).mockResolvedValue([]);
});

test("form inputs have accessible labels", () => {
  setup();
  expect(screen.getByLabelText("Host")).toBeTruthy();
  expect(screen.getByLabelText("Port")).toBeTruthy();
  expect(screen.getByLabelText("Username")).toBeTruthy();
});

test("form shows inline error for empty host and does not call connectionSave", async () => {
  setup();
  await type("Name", "web");
  await type("Username", "luca");
  await save();
  const host = screen.getByLabelText("Host");
  expect(host.getAttribute("aria-invalid")).toBe("true");
  const msg = document.getElementById(host.getAttribute("aria-describedby")!);
  expect(msg?.textContent).toMatch(/required|empty/i);
  expect(api.connectionSave).not.toHaveBeenCalled();
});

test("server-side config.invalid error lands under field", async () => {
  vi.mocked(api.connectionSave).mockRejectedValue({
    code: "config.invalid",
    params: { field: "host", reason: "bad host" },
  });
  const { onsaved } = setup();
  await type("Name", "web");
  await type("Host", "example.com");
  await type("Username", "luca");
  await save();
  const host = screen.getByLabelText("Host");
  await waitFor(() => expect(host.getAttribute("aria-invalid")).toBe("true"));
  const msg = document.getElementById(host.getAttribute("aria-describedby")!);
  expect(msg?.textContent).toContain("bad host");
  expect(onsaved).not.toHaveBeenCalled();
});

test("saves a trimmed connection and never sends an empty secret", async () => {
  const { onsaved } = setup();
  await type("Name", " web ");
  await type("Host", "example.com");
  await type("Username", "luca");
  await save();
  await waitFor(() => expect(onsaved).toHaveBeenCalled());
  const [c, password, passphrase] = vi.mocked(api.connectionSave).mock.calls[0];
  expect(c).toMatchObject({
    name: "web",
    host: "example.com",
    port: 22,
    auth: { type: "Password", save: false },
  });
  expect(password).toBeNull();
  expect(passphrase).toBeNull();
});

test("local forward needs an IP bind address", async () => {
  setup();
  await type("Name", "web");
  await type("Host", "example.com");
  await type("Username", "luca");
  await fireEvent.click(
    screen.getByRole("button", { name: "Add local forward" }),
  );
  await type("Bind address", "localhost");
  await save();
  expect(
    screen.getByLabelText("Bind address").getAttribute("aria-invalid"),
  ).toBe("true");
  expect(api.connectionSave).not.toHaveBeenCalled();
});

const secrets = () => {
  const [, password, passphrase] = vi.mocked(api.connectionSave).mock.calls[0];
  return { password, passphrase };
};

async function fillBasics() {
  await type("Name", "web");
  await type("Host", "example.com");
  await type("Username", "luca");
}

test("password saved but left empty is sent as null", async () => {
  setup();
  await fillBasics();
  await fireEvent.click(screen.getByLabelText("Save password in the keychain"));
  await save();
  await waitFor(() => expect(api.connectionSave).toHaveBeenCalled());
  expect(secrets()).toEqual({ password: null, passphrase: null });
});

test("a typed password is passed through", async () => {
  setup();
  await fillBasics();
  await fireEvent.click(screen.getByLabelText("Save password in the keychain"));
  await type("Password", "s3cret");
  await save();
  await waitFor(() => expect(api.connectionSave).toHaveBeenCalled());
  expect(secrets()).toEqual({ password: "s3cret", passphrase: null });
});

test("key passphrase: empty is null, typed is passed through", async () => {
  setup();
  await fillBasics();
  await fireEvent.change(screen.getByLabelText("Method"), {
    target: { value: "Key" },
  });
  await fireEvent.click(
    screen.getByLabelText("Save passphrase in the keychain"),
  );
  await save();
  await waitFor(() => expect(api.connectionSave).toHaveBeenCalledTimes(1));
  expect(secrets()).toEqual({ password: null, passphrase: null });

  await type("Passphrase", "hunter2");
  await save();
  await waitFor(() => expect(api.connectionSave).toHaveBeenCalledTimes(2));
  expect(vi.mocked(api.connectionSave).mock.calls[1].slice(1)).toEqual([
    null,
    "hunter2",
  ]);
});
