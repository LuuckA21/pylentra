import { afterEach, expect, test, vi } from "vitest";
import type { SessionEvent } from "./bindings/SessionEvent";
import {
  initialPane,
  keygenHint,
  reduce,
  type PaneState,
} from "./sessionReducer";

const run = (...events: SessionEvent[]): PaneState =>
  events.reduce(reduce, initialPane("t1", "c1"));

const hostKey: SessionEvent = {
  type: "HostKeyPrompt",
  host: "h",
  port: 22,
  algo: "ssh-ed25519",
  fingerprint: "SHA256:abc",
};
const local = {
  type: "Local",
  bind_host: "127.0.0.1",
  bind_port: 8080,
  dest_host: "db",
  dest_port: 5432,
  auto_start: false,
} as const;

test("status events advance phase", () => {
  expect(initialPane("t1", "c1").phase).toBe("connecting");
  const s = run({ type: "Status", stage: "Connecting" });
  expect(s.phase).toBe("connecting");
  const a = reduce(s, { type: "Status", stage: "Authenticating" });
  expect(a.phase).toBe("authenticating");
  expect(reduce(a, { type: "Status", stage: "Ready" }).phase).toBe("ready");
});

test("prompt set then cleared by Ready", () => {
  const s = run({ type: "Status", stage: "Connecting" }, hostKey);
  expect(s.prompt).toEqual(hostKey);
  expect(reduce(s, { type: "Status", stage: "Ready" }).prompt).toBeUndefined();
});

test("mismatch stored and phase closed on Closed", () => {
  const mismatch: SessionEvent = {
    type: "HostKeyMismatch",
    host: "h",
    port: 22,
    expected: "SHA256:old",
    got: "SHA256:new",
    source: "Pylentra",
  };
  const error = { code: "hostkey.mismatch", params: { host: "h", port: "22" } };
  const s = run(mismatch, {
    type: "Closed",
    reason: { type: "Error", error },
  });
  expect(s.mismatch).toEqual(mismatch);
  expect(s.phase).toBe("closed");
  expect(s.closed).toEqual({ type: "Error", error });
});

test("forward events upsert by forward_id", () => {
  const error = { code: "forward.port_in_use", params: { port: "8080" } };
  const s = run(
    {
      type: "Forward",
      forward_id: 3,
      rule: local,
      state: { type: "Active", bound_port: 8080 },
    },
    {
      type: "Forward",
      forward_id: 3,
      rule: local,
      state: { type: "Failed", error },
    },
  );
  expect(Object.keys(s.forwards)).toEqual(["3"]);
  expect(s.forwards[3]).toEqual({
    rule: local,
    state: { type: "Failed", error },
  });
});

test("Closed clears pending prompt", () => {
  const s = run(hostKey, { type: "Closed", reason: { type: "UserClosed" } });
  expect(s.prompt).toBeUndefined();
  expect(s.phase).toBe("closed");
});

test("reduce does not mutate its input", () => {
  const s = initialPane("t1", "c1");
  reduce(s, hostKey);
  expect(s.prompt).toBeUndefined();
});

test("ssh-keygen hint uses [host]:port for non-22", () => {
  expect(keygenHint("example.com", 22)).toBe("ssh-keygen -R example.com");
  expect(keygenHint("example.com", 2222)).toBe(
    'ssh-keygen -R "[example.com]:2222"',
  );
});

afterEach(() => vi.useRealTimers());

test("Ready sets readyAt once", () => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(1000);
  let s = run({ type: "Status", stage: "Ready" });
  expect(s.readyAt).toBe(1000);
  vi.setSystemTime(5000);
  s = reduce(s, { type: "Status", stage: "Ready" });
  expect(s.readyAt).toBe(1000);
  s = reduce(s, { type: "Closed", reason: { type: "UserClosed" } });
  expect(s.readyAt).toBe(1000);
});

test("a Transfer event sets that transfer's state", () => {
  const queued = run({
    type: "Transfer",
    transfer_id: 4,
    state: { type: "Queued", direction: "Down", name: "a.txt" },
  });
  const running = reduce(queued, {
    type: "Transfer",
    transfer_id: 4,
    state: {
      type: "Running",
      done_bytes: 10,
      total_bytes: 20,
      files_done: 0,
      current: "a.txt",
    },
  });
  // The name only comes with Queued; the row keeps it.
  expect(running.transfers[4]).toMatchObject({
    name: "a.txt",
    state: { type: "Running" },
  });
  expect(initialPane("t1", "c1").transfers).toEqual({});
});

test("Closed cancels the transfers still in progress, keeps the final ones", () => {
  const s = run(
    {
      type: "Transfer",
      transfer_id: 1,
      state: { type: "Queued", direction: "Up", name: "q" },
    },
    {
      type: "Transfer",
      transfer_id: 2,
      state: {
        type: "Running",
        done_bytes: 1,
        total_bytes: null,
        files_done: 3,
        current: "r",
      },
    },
    {
      type: "Transfer",
      transfer_id: 3,
      state: { type: "Conflict", name: "c" },
    },
    {
      type: "Transfer",
      transfer_id: 4,
      state: { type: "Finished", files: 2, skipped: [] },
    },
    { type: "Closed", reason: { type: "UserClosed" } },
  );
  expect(Object.values(s.transfers).map((t) => t.state)).toEqual([
    { type: "Cancelled", files: 0 },
    { type: "Cancelled", files: 3 },
    { type: "Cancelled", files: 0 },
    { type: "Finished", files: 2, skipped: [] },
  ]);
  expect(s.transfers[1].name).toBe("q");
});
