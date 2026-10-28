import { api } from "../api";
import type { PromptAnswer } from "../bindings/PromptAnswer";
import type { SessionEvent } from "../bindings/SessionEvent";
import type { Connection } from "../bindings/Connection";
import type { QuickTarget } from "../quickTarget";
import {
  count,
  firstPane,
  MAX_PANES,
  rects,
  remove,
  split,
  type Dir,
  type Node,
} from "../layout";
import { recordRecent } from "../recent";
import { initialPane, reduce, type PaneState } from "../sessionReducer";
import { isApiError, toastError } from "./toasts.svelte";

// A tab lays out its panes in `root`; `focus` is one of them. `active` is a tabId.
export type Tab = { tabId: string; root: Node; focus: string };

export const sessions = $state<{
  tabs: Tab[];
  panes: PaneState[];
  active: string | null;
}>({
  tabs: [],
  panes: [],
  active: null,
});

// paneId → id of the pane's current session; a promise because events (and the
// user) can act before session_open has returned.
// Not reactive on purpose: nothing renders from it.
// eslint-disable-next-line svelte/prefer-svelte-reactivity
const sessionIds = new Map<string, Promise<string>>();

const indexOf = (paneId: string) =>
  sessions.panes.findIndex((p) => p.paneId === paneId);

export const paneOf = (paneId: string): PaneState | undefined =>
  sessions.panes.find((p) => p.paneId === paneId);

const tabIndexOf = (paneId: string) =>
  sessions.tabs.findIndex((t) => paneId in rects(t.root).panes);

export const tabOf = (paneId: string): Tab | undefined =>
  sessions.tabs[tabIndexOf(paneId)];

function addTab(pane: PaneState): string {
  const tabId = crypto.randomUUID();
  sessions.panes.push(pane);
  sessions.tabs.push({
    tabId,
    root: { pane: pane.paneId },
    focus: pane.paneId,
  });
  sessions.active = tabId;
  return pane.paneId;
}

// `target`: a saved connection's id, or a quick connection's target.
function newPane(target: string | QuickTarget): PaneState {
  if (typeof target === "string")
    return initialPane(crypto.randomUUID(), target);
  const { user, host, port } = target;
  const quick: Connection = {
    id: crypto.randomUUID(),
    name: `${user}@${host}`,
    host,
    port,
    username: user,
    auth: { type: "Auto" },
    group_id: null,
    jump_via: null,
    forwards: [],
  };
  return initialPane(crypto.randomUUID(), quick.id, quick);
}

export const openTab = (connectionId: string): string =>
  addTab(newPane(connectionId));

export const openQuickTab = (target: QuickTarget): string =>
  addTab(newPane(target));

// The new pane takes the focus; null, and nothing changes, when the tab is full.
export function splitPane(
  paneId: string,
  dir: Dir,
  target: string | QuickTarget,
): string | null {
  const tab = tabOf(paneId);
  if (!tab || count(tab.root) >= MAX_PANES) return null;
  const pane = newPane(target);
  sessions.panes.push(pane);
  tab.root = split(tab.root, paneId, dir, pane.paneId);
  tab.focus = pane.paneId;
  return pane.paneId;
}

// The open session keeps running; only the pane's identity changes.
export function linkQuickPane(paneId: string, saved: Connection): void {
  const i = indexOf(paneId);
  if (i < 0) return;
  sessions.panes[i] = {
    ...sessions.panes[i],
    connectionId: saved.id,
    quick: undefined,
  };
}

// Starts (or restarts, in the same pane) the session. Events of a pane that was
// closed meanwhile are dropped.
export function connect(
  paneId: string,
  cols: number,
  rows: number,
  onData: (bytes: Uint8Array) => void,
  onEvent: (e: SessionEvent) => void,
): void {
  const i = indexOf(paneId);
  if (i < 0) return;
  const update = (f: (t: PaneState) => PaneState) => {
    const j = indexOf(paneId);
    if (j >= 0) sessions.panes[j] = f(sessions.panes[j]);
  };
  const event = (e: SessionEvent) => {
    if (indexOf(paneId) < 0) return;
    update((t) => reduce(t, e));
    const pane = sessions.panes[indexOf(paneId)];
    if (e.type === "Status" && e.stage === "Ready" && !pane.quick)
      recordRecent(pane.connectionId);
    onEvent(e);
  };
  const { connectionId, quick } = sessions.panes[i];
  sessions.panes[i] = initialPane(paneId, connectionId, quick);
  const onBytes = (bytes: Uint8Array) => indexOf(paneId) >= 0 && onData(bytes);
  const id = quick
    ? api.sessionOpenQuick(quick, cols, rows, onBytes, event)
    : api.sessionOpen(connectionId, cols, rows, onBytes, event);
  sessionIds.set(paneId, id);
  id.then(
    (sessionId) => update((t) => ({ ...t, sessionId })),
    (e: unknown) =>
      event({
        type: "Closed",
        reason: {
          type: "Error",
          error: isApiError(e) ? e : { code: "unknown", params: {} },
        },
      }),
  );
}

// sessionId → its last write. Tauri runs each call on its own task, so two
// calls in flight can reach the session swapped: each write waits for the one
// before it. Not reactive on purpose: nothing renders from it.
// eslint-disable-next-line svelte/prefer-svelte-reactivity
const writes = new Map<string, Promise<void>>();

// Write and resize errors are not reported: a dead session sends Closed.
export function write(pane: PaneState, data: string | Uint8Array): void {
  const id = pane.sessionId;
  if (!id || pane.phase !== "ready") return;
  const last = (writes.get(id) ?? Promise.resolve())
    .then(() => api.sessionWrite(id, data))
    .catch(() => {})
    .finally(() => {
      if (writes.get(id) === last) writes.delete(id);
    });
  writes.set(id, last);
}

export function resize(pane: PaneState, cols: number, rows: number): void {
  if (pane.sessionId && pane.phase !== "closed")
    api.sessionResize(pane.sessionId, cols, rows).catch(() => {});
}

export async function answer(paneId: string, a: PromptAnswer): Promise<void> {
  const i = indexOf(paneId);
  const id = sessionIds.get(paneId);
  if (i < 0 || !id) return;
  sessions.panes[i] = { ...sessions.panes[i], prompt: undefined };
  try {
    await api.sessionAnswer(await id, a);
  } catch (e) {
    toastError(e);
  }
}

// Closes the pane's session if it is live; the last pane takes its tab along.
export function closePane(paneId: string): void {
  const i = indexOf(paneId);
  if (i < 0) return;
  const [pane] = sessions.panes.splice(i, 1);
  const id = sessionIds.get(paneId);
  sessionIds.delete(paneId);
  if (pane.phase !== "closed")
    id?.then((sid) => api.sessionClose(sid)).catch(() => {});
  const t = tabIndexOf(paneId);
  if (t < 0) return;
  const tab = sessions.tabs[t];
  const root = remove(tab.root, paneId);
  if (root) {
    tab.root = root;
    if (tab.focus === paneId) tab.focus = firstPane(root);
    return;
  }
  sessions.tabs.splice(t, 1);
  if (sessions.active === tab.tabId)
    sessions.active =
      sessions.tabs[Math.min(t, sessions.tabs.length - 1)]?.tabId ?? null;
}

export function closeTab(tabId: string): void {
  const tab = sessions.tabs.find((t) => t.tabId === tabId);
  if (tab) Object.keys(rects(tab.root).panes).forEach(closePane);
}

export function cycleTab(delta: 1 | -1): void {
  const n = sessions.tabs.length;
  if (!n) return;
  const i = sessions.active
    ? sessions.tabs.findIndex((t) => t.tabId === sessions.active)
    : 0;
  sessions.active = sessions.tabs[(i + delta + n) % n].tabId;
}
