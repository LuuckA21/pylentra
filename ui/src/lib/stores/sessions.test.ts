import { beforeEach, expect, test, vi } from "vitest";

vi.mock("../api", () => ({
  api: {
    sessionOpen: vi.fn(),
    sessionOpenQuick: vi.fn(),
    sessionClose: vi.fn(),
    sessionWrite: vi.fn(),
    sessionAnswer: vi.fn(),
  },
}));

import { api } from "../api";
import { recentIds } from "../recent";
import type { SessionEvent } from "../bindings/SessionEvent";
import {
  answer,
  closeTab,
  connect,
  cycleTab,
  linkQuickPane,
  openQuickTab,
  openTab,
  sessions,
  write,
} from "./sessions.svelte";

const flush = () => new Promise((r) => setTimeout(r));
let emit: (e: SessionEvent) => void;

beforeEach(() => {
  sessions.tabs.splice(0);
  sessions.active = null;
  vi.mocked(api.sessionOpen).mockImplementation((_c, _w, _h, _d, onEvent) => {
    emit = onEvent;
    return Promise.resolve("s1");
  });
  vi.mocked(api.sessionOpenQuick).mockResolvedValue("q1");
  vi.mocked(api.sessionClose).mockResolvedValue();
  vi.mocked(api.sessionAnswer).mockResolvedValue();
});

test("events reach the tab and the answer clears the prompt", async () => {
  const id = openTab("c1");
  connect(
    id,
    80,
    24,
    () => {},
    () => {},
  );
  emit({
    type: "SecretPrompt",
    host: "h",
    port: 22,
    kind: { type: "Password" },
    can_save: true,
  });
  expect(sessions.tabs[0].prompt?.type).toBe("SecretPrompt");
  await answer(id, { type: "Secret", value: "pw", save: false });
  expect(sessions.tabs[0].prompt).toBeUndefined();
  expect(sessions.tabs[0].sessionId).toBe("s1");
  expect(api.sessionAnswer).toHaveBeenCalledWith("s1", {
    type: "Secret",
    value: "pw",
    save: false,
  });
});

test("closing the active tab closes its session and activates a neighbor", async () => {
  const a = openTab("c1");
  const b = openTab("c2");
  connect(
    b,
    80,
    24,
    () => {},
    () => {},
  );
  closeTab(b);
  await flush();
  expect(api.sessionClose).toHaveBeenCalledWith("s1");
  expect(sessions.active).toBe(a);
  emit({ type: "Status", stage: "Ready" }); // late event of the closed tab is dropped
  expect(sessions.tabs.map((t) => t.paneId)).toEqual([a]);
});

test("cycleTab wraps around", () => {
  const a = openTab("c1");
  const b = openTab("c1");
  cycleTab(1);
  expect(sessions.active).toBe(a);
  cycleTab(-1);
  expect(sessions.active).toBe(b);
});

test("a failed session_open closes the tab with the error", async () => {
  vi.mocked(api.sessionOpen).mockRejectedValue({
    code: "not_found",
    params: {},
  });
  const id = openTab("c1");
  connect(
    id,
    80,
    24,
    () => {},
    () => {},
  );
  await flush();
  expect(sessions.tabs[0].phase).toBe("closed");
  expect(sessions.tabs[0].closed).toEqual({
    type: "Error",
    error: { code: "not_found", params: {} },
  });
});

test("a tab that becomes ready is recorded as recent", () => {
  localStorage.clear();
  const id = openTab("c1");
  connect(
    id,
    80,
    24,
    () => {},
    () => {},
  );
  emit({ type: "Status", stage: "Authenticating" });
  expect(recentIds(new Set(["c1"]))).toEqual([]);
  emit({ type: "Status", stage: "Ready" });
  expect(recentIds(new Set(["c1"]))).toEqual(["c1"]);
});

const target = { user: "luca", host: "Example.com", port: 2222 };

test("openQuickTab opens a tab named user@host with Auto auth", () => {
  const id = openQuickTab(target);
  const tab = sessions.tabs[0];
  expect(sessions.active).toBe(id);
  expect(tab.quick).toMatchObject({
    name: "luca@Example.com",
    host: "Example.com",
    port: 2222,
    username: "luca",
    auth: { type: "Auto" },
    group_id: null,
    jump_via: null,
    forwards: [],
  });
  expect(tab.connectionId).toBe(tab.quick?.id);
});

test("connect uses sessionOpenQuick for quick tabs and does not record them as recent", async () => {
  localStorage.clear();
  const id = openQuickTab(target);
  const quick = sessions.tabs[0].quick;
  let onEvent: (e: SessionEvent) => void = () => {};
  vi.mocked(api.sessionOpenQuick).mockImplementation((_c, _w, _h, _d, ev) => {
    onEvent = ev;
    return Promise.resolve("q1");
  });
  connect(
    id,
    80,
    24,
    () => {},
    () => {},
  );
  await flush();
  expect(api.sessionOpenQuick).toHaveBeenCalledWith(
    quick,
    80,
    24,
    expect.any(Function),
    expect.any(Function),
  );
  expect(api.sessionOpen).not.toHaveBeenCalled();
  expect(sessions.tabs[0].quick).toEqual(quick); // survives the restart
  onEvent({ type: "Status", stage: "Ready" });
  expect(recentIds(new Set([quick!.id]))).toEqual([]);
});

test("linkQuickPane points the tab at the saved connection", async () => {
  const id = openQuickTab(target);
  connect(
    id,
    80,
    24,
    () => {},
    () => {},
  );
  await flush();
  linkQuickPane(id, {
    ...sessions.tabs[0].quick!,
    id: "saved",
    name: "Example.com",
  });
  expect(sessions.tabs[0].connectionId).toBe("saved");
  expect(sessions.tabs[0].quick).toBeUndefined();
  expect(sessions.tabs[0].sessionId).toBe("q1");
});

test("a write waits for the one before it, even a failed one", async () => {
  const id = openTab("c1");
  connect(
    id,
    80,
    24,
    () => {},
    () => {},
  );
  await flush();
  emit({ type: "Status", stage: "Ready" });
  let fail!: () => void;
  vi.mocked(api.sessionWrite)
    .mockReset()
    .mockReturnValueOnce(new Promise((_, reject) => (fail = reject)))
    .mockResolvedValue();
  write(sessions.tabs[0], "ls");
  write(sessions.tabs[0], "\r");
  await flush();
  expect(api.sessionWrite).toHaveBeenCalledTimes(1);
  fail();
  await flush();
  expect(vi.mocked(api.sessionWrite).mock.calls).toEqual([
    ["s1", "ls"],
    ["s1", "\r"],
  ]);
});
