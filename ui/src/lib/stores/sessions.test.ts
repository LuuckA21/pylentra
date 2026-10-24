import { beforeEach, expect, test, vi } from "vitest";

vi.mock("../api", () => ({
  api: {
    sessionOpen: vi.fn(),
    sessionClose: vi.fn(),
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
  openTab,
  sessions,
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
  expect(sessions.tabs.map((t) => t.tabId)).toEqual([a]);
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
