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

test("sessionWrite sends UTF-8 bytes as the raw body, the id as a header", async () => {
  invoke.mockResolvedValue(undefined);
  await sessionWrite("s", "è");
  const [cmd, body, options] = invoke.mock.calls[0];
  expect([cmd, Array.from(body as Uint8Array), options]).toEqual([
    "session_write",
    [0xc3, 0xa8],
    { headers: { id: "s" } },
  ]);
});

test("sessionWrite sends a Uint8Array as it is", async () => {
  invoke.mockResolvedValue(undefined);
  const bytes = new Uint8Array([1, 2]);
  await sessionWrite("s", bytes);
  expect(invoke).toHaveBeenCalledWith("session_write", bytes, {
    headers: { id: "s" },
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
