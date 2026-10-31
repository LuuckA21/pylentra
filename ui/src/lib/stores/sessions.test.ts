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
import { rects } from "../layout";
import type { SessionEvent } from "../bindings/SessionEvent";
import {
  answer,
  closePane,
  closeTab,
  connect,
  cycleTab,
  detachPane,
  focusPane,
  linkQuickPane,
  mergeTab,
  moveFocus,
  openQuickTab,
  openTab,
  sessions,
  setTabRatio,
  splitPane,
  tabOf,
  write,
} from "./sessions.svelte";

const flush = () => new Promise((r) => setTimeout(r));
let emit: (e: SessionEvent) => void;

beforeEach(() => {
  sessions.tabs.splice(0);
  sessions.panes.splice(0);
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
  expect(sessions.panes[0].prompt?.type).toBe("SecretPrompt");
  await answer(id, { type: "Secret", value: "pw", save: false });
  expect(sessions.panes[0].prompt).toBeUndefined();
  expect(sessions.panes[0].sessionId).toBe("s1");
  expect(api.sessionAnswer).toHaveBeenCalledWith("s1", {
    type: "Secret",
    value: "pw",
    save: false,
  });
});

test("openTab adds a tab holding one pane, focused and active", () => {
  const id = openTab("c1");
  const tab = sessions.tabs[0];
  expect(tab).toMatchObject({ root: { pane: id }, focus: id });
  expect(sessions.active).toBe(tab.tabId);
  expect(sessions.panes.map((p) => p.paneId)).toEqual([id]);
  expect(tabOf(id)).toBe(tab);
});

test("closing the only pane closes its session and the tab, activating a neighbor", async () => {
  const a = openTab("c1");
  const b = openTab("c2");
  connect(
    b,
    80,
    24,
    () => {},
    () => {},
  );
  closePane(b);
  await flush();
  expect(api.sessionClose).toHaveBeenCalledWith("s1");
  expect(sessions.tabs.map((t) => t.root)).toEqual([{ pane: a }]);
  expect(sessions.active).toBe(sessions.tabs[0].tabId);
  emit({ type: "Status", stage: "Ready" }); // late event of the closed pane is dropped
  expect(sessions.panes.map((p) => p.paneId)).toEqual([a]);
});

test("closing a pane whose session is still opening closes it once open", async () => {
  vi.mocked(api.sessionClose).mockClear();
  let open!: (id: string) => void;
  vi.mocked(api.sessionOpen).mockReturnValue(new Promise((r) => (open = r)));
  const id = openTab("c1");
  connect(
    id,
    80,
    24,
    () => {},
    () => {},
  );
  closePane(id);
  await flush();
  expect(api.sessionClose).not.toHaveBeenCalled();
  open("s1");
  await flush();
  expect(api.sessionClose).toHaveBeenCalledWith("s1");
});

test("closeTab closes every pane of the tab", async () => {
  vi.mocked(api.sessionClose).mockClear();
  const a = openTab("c1");
  const b = openTab("c2");
  connect(
    a,
    80,
    24,
    () => {},
    () => {},
  );
  vi.mocked(api.sessionOpen).mockResolvedValueOnce("s2");
  connect(
    b,
    80,
    24,
    () => {},
    () => {},
  );
  // One tab holding both panes, side by side.
  sessions.tabs.splice(1);
  sessions.active = sessions.tabs[0].tabId;
  sessions.tabs[0].root = {
    dir: "row",
    ratio: 0.5,
    a: { pane: a },
    b: { pane: b },
  };
  closeTab(sessions.tabs[0].tabId);
  await flush();
  expect(sessions.tabs).toEqual([]);
  expect(sessions.panes).toEqual([]);
  expect(sessions.active).toBeNull();
  expect(vi.mocked(api.sessionClose).mock.calls.sort()).toEqual([
    ["s1"],
    ["s2"],
  ]);
});

test("splitPane adds a focused pane beside the one split", () => {
  const p = openTab("c1");
  const id = splitPane(p, "row", "c2")!;
  expect(sessions.panes.map((x) => x.paneId)).toEqual([p, id]);
  expect(sessions.tabs).toHaveLength(1);
  expect(sessions.tabs[0]).toMatchObject({
    root: { dir: "row", ratio: 0.5, a: { pane: p }, b: { pane: id } },
    focus: id,
  });
  connect(
    id,
    80,
    24,
    () => {},
    () => {},
  );
  expect(api.sessionOpen).toHaveBeenLastCalledWith(
    "c2",
    80,
    24,
    expect.any(Function),
    expect.any(Function),
  );
});

test("splitPane with a quick target makes a quick pane", () => {
  const p = openTab("c1");
  const id = splitPane(p, "column", { user: "u", host: "h", port: 22 })!;
  expect(sessions.panes[1]).toMatchObject({
    paneId: id,
    quick: { name: "u@h", username: "u", host: "h", port: 22 },
  });
  expect(sessions.tabs[0].root).toMatchObject({ dir: "column" });
});

test("splitPane refuses a fifth pane and changes nothing", () => {
  const p = openTab("c1");
  splitPane(p, "row", "c2");
  splitPane(p, "column", "c3");
  splitPane(p, "column", "c4");
  const before = JSON.parse(JSON.stringify(sessions));
  expect(splitPane(p, "row", "c5")).toBeNull();
  expect(JSON.parse(JSON.stringify(sessions))).toEqual(before);
});

// A | (B / C), focus on C.
const threePanes = () => {
  const a = openTab("c1");
  const b = splitPane(a, "row", "c2")!;
  const c = splitPane(b, "column", "c3")!;
  return { a, b, c, tab: sessions.tabs[0] };
};

test("moveFocus follows the neighbor on the active tab, and stops at the edge", () => {
  const { a, tab } = threePanes();
  moveFocus("left");
  expect(tab.focus).toBe(a);
  moveFocus("left");
  expect(tab.focus).toBe(a);
});

test("focusPane focuses the pane and activates its tab", () => {
  const { b, tab } = threePanes();
  openTab("c4");
  focusPane(b);
  expect(tab.focus).toBe(b);
  expect(sessions.active).toBe(tab.tabId);
});

test("closing the focused pane focuses the first pane of its sibling", () => {
  const { a, b, c, tab } = threePanes();
  focusPane(a);
  closePane(a);
  expect(tab.root).toEqual({
    dir: "column",
    ratio: 0.5,
    a: { pane: b },
    b: { pane: c },
  });
  expect(tab.focus).toBe(b);
});

test("closing the focused pane in a nested split focuses its sibling, not the tab's first pane", () => {
  const { a, b, c, tab } = threePanes();
  closePane(c);
  expect(tab.root).toEqual({
    dir: "row",
    ratio: 0.5,
    a: { pane: a },
    b: { pane: b },
  });
  expect(tab.focus).toBe(b);
});

test("closing a pane without the focus leaves the focus where it is", () => {
  const { a, b, c, tab } = threePanes();
  focusPane(b);
  closePane(c);
  expect(tab.focus).toBe(b);
  expect(tab.root).toMatchObject({ a: { pane: a }, b: { pane: b } });
});

test("closing a pane missing from the panes still removes its leaf", () => {
  const { a, b, c, tab } = threePanes();
  sessions.panes.splice(
    sessions.panes.findIndex((p) => p.paneId === c),
    1,
  );
  closePane(c);
  expect(tab.root).toMatchObject({ a: { pane: a }, b: { pane: b } });
  expect(tab.focus).toBe(b);
});

test("cycleTab wraps around", () => {
  const a = openTab("c1");
  const b = openTab("c1");
  cycleTab(1);
  expect(sessions.active).toBe(tabOf(a)?.tabId);
  cycleTab(-1);
  expect(sessions.active).toBe(tabOf(b)?.tabId);
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
  expect(sessions.panes[0].phase).toBe("closed");
  expect(sessions.panes[0].closed).toEqual({
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
  const tab = sessions.panes[0];
  expect(sessions.active).toBe(tabOf(id)?.tabId);
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
  const quick = sessions.panes[0].quick;
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
  expect(sessions.panes[0].quick).toEqual(quick); // survives the restart
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
    ...sessions.panes[0].quick!,
    id: "saved",
    name: "Example.com",
  });
  expect(sessions.panes[0].connectionId).toBe("saved");
  expect(sessions.panes[0].quick).toBeUndefined();
  expect(sessions.panes[0].sessionId).toBe("q1");
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
  write(sessions.panes[0], "ls");
  write(sessions.panes[0], "\r");
  await flush();
  expect(api.sessionWrite).toHaveBeenCalledTimes(1);
  fail();
  await flush();
  expect(vi.mocked(api.sessionWrite).mock.calls).toEqual([
    ["s1", "ls"],
    ["s1", "\r"],
  ]);
});

test("setTabRatio sets the ratio at a path, clamped, and ignores an unknown tab", () => {
  const { tab } = threePanes();
  setTabRatio(tab.tabId, [], 0.7);
  expect((tab.root as { ratio: number }).ratio).toBe(0.7);
  setTabRatio(tab.tabId, ["b"], 0.99);
  expect(rects(tab.root).dividers.find((d) => d.path[0] === "b")!.ratio).toBe(
    0.9,
  );
  setTabRatio("nope", [], 0.2);
  expect((tab.root as { ratio: number }).ratio).toBe(0.7);
});

const tabIdOf = (paneId: string) => tabOf(paneId)!.tabId;

test("mergeTab puts the whole tab beside the target's focused pane and removes it", () => {
  const a = openTab("c1");
  const b = openTab("c2");
  const c = splitPane(b, "column", "c3")!;
  const from = tabIdOf(b);
  expect(mergeTab(from, tabIdOf(a))).toBe(true);
  expect(sessions.tabs).toHaveLength(1);
  const tab = sessions.tabs[0];
  expect(tab.root).toEqual({
    dir: "row",
    ratio: 0.5,
    a: { pane: a },
    b: { dir: "column", ratio: 0.5, a: { pane: b }, b: { pane: c } },
  });
  // The moved tab's focused pane keeps the focus; the panes and sessions stay.
  expect(tab.focus).toBe(c);
  expect(sessions.active).toBe(tab.tabId);
  expect(sessions.panes.map((p) => p.paneId)).toEqual([a, b, c]);
  expect(api.sessionClose).not.toHaveBeenCalled();
});

test("mergeTab refuses more than 4 panes, the same tab or an unknown one", () => {
  const a = openTab("c1");
  splitPane(a, "row", "c2");
  splitPane(a, "row", "c3");
  const b = openTab("c4");
  splitPane(b, "row", "c5");
  const before = JSON.parse(JSON.stringify(sessions));
  expect(mergeTab(tabIdOf(b), tabIdOf(a))).toBe(false);
  expect(mergeTab(tabIdOf(a), tabIdOf(a))).toBe(false);
  expect(mergeTab("nope", tabIdOf(a))).toBe(false);
  expect(JSON.parse(JSON.stringify(sessions))).toEqual(before);
});

test("detachPane moves a pane to a new tab after its own, focused and active", () => {
  const a = openTab("c1");
  const b = splitPane(a, "row", "c2")!;
  const other = openTab("c3");
  const from = tabIdOf(a);
  focusPane(b);
  detachPane(b);
  expect(sessions.tabs.map((t) => Object.keys(rects(t.root).panes))).toEqual([
    [a],
    [b],
    [other],
  ]);
  expect(tabOf(a)!.focus).toBe(a);
  expect(tabOf(a)!.tabId).toBe(from);
  expect(sessions.active).toBe(tabIdOf(b));
  expect(tabOf(b)!.focus).toBe(b);
  expect(api.sessionClose).not.toHaveBeenCalled();
});

test("detachPane leaves a lone pane alone", () => {
  const a = openTab("c1");
  const before = JSON.parse(JSON.stringify(sessions));
  detachPane(a);
  expect(JSON.parse(JSON.stringify(sessions))).toEqual(before);
});
