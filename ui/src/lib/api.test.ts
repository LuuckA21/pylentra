import { beforeEach, expect, test, vi } from "vitest";

const { invoke, channels } = vi.hoisted(() => ({
  invoke: vi.fn(),
  channels: [] as { onmessage: (m: unknown) => void }[],
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke,
  Channel: class {
    onmessage: (m: unknown) => void = () => {};
    constructor() {
      channels.push(this);
    }
  },
}));

import { api, sessionOpen, sessionWrite } from "./api";

beforeEach(() => {
  invoke.mockReset();
  channels.length = 0;
});

test("sessionWrite encodes strings as UTF-8 bytes", async () => {
  invoke.mockResolvedValue(undefined);
  await sessionWrite("s", "è");
  expect(invoke).toHaveBeenCalledWith("session_write", {
    id: "s",
    data: [0xc3, 0xa8],
  });
});

test("sessionWrite sends Uint8Array as a plain number array", async () => {
  invoke.mockResolvedValue(undefined);
  await sessionWrite("s", new Uint8Array([1, 2]));
  expect(invoke).toHaveBeenCalledWith("session_write", {
    id: "s",
    data: [1, 2],
  });
});

test("sessionOpen passes raw ArrayBuffer to onData as Uint8Array", async () => {
  invoke.mockResolvedValue("sid");
  const onData = vi.fn();
  const onEvent = vi.fn();
  const id = await sessionOpen("c", 80, 24, onData, onEvent);
  expect(id).toBe("sid");
  const [data, events] = channels;
  expect(invoke).toHaveBeenCalledWith("session_open", {
    connectionId: "c",
    cols: 80,
    rows: 24,
    data,
    events,
  });
  data.onmessage(new Uint8Array([104, 105]).buffer);
  data.onmessage([1, 2, 3]);
  expect(onData.mock.calls).toEqual([
    [new Uint8Array([104, 105])],
    [new Uint8Array([1, 2, 3])],
  ]);
  const ev = { type: "Status", stage: "Ready" };
  events.onmessage(ev);
  expect(onEvent).toHaveBeenCalledWith(ev);
});

test("wrappers use snake_case command names and camelCase args", async () => {
  invoke.mockResolvedValue(undefined);
  await api.connectionSave({} as never, "pw", null);
  expect(invoke).toHaveBeenCalledWith("connection_save", {
    connection: {},
    password: "pw",
    passphrase: null,
  });
  await api.forwardStop("s", 3);
  expect(invoke).toHaveBeenCalledWith("forward_stop", {
    sessionId: "s",
    forwardId: 3,
  });
  await api.logDir();
  expect(invoke).toHaveBeenCalledWith("log_dir");
});

test("errors from invoke reject unchanged", async () => {
  const err = { code: "io", params: {} };
  invoke.mockRejectedValue(err);
  await expect(api.libraryGet()).rejects.toBe(err);
});
