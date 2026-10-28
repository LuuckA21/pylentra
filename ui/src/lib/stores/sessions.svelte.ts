import { api } from "../api";
import type { PromptAnswer } from "../bindings/PromptAnswer";
import type { SessionEvent } from "../bindings/SessionEvent";
import type { Connection } from "../bindings/Connection";
import type { QuickTarget } from "../quickTarget";
import { recordRecent } from "../recent";
import { initialPane, reduce, type PaneState } from "../sessionReducer";
import { isApiError, toastError } from "./toasts.svelte";

export const sessions = $state<{ tabs: PaneState[]; active: string | null }>({
  tabs: [],
  active: null,
});

// paneId → id of the pane's current session; a promise because events (and the
// user) can act before session_open has returned.
// Not reactive on purpose: nothing renders from it.
// eslint-disable-next-line svelte/prefer-svelte-reactivity
const sessionIds = new Map<string, Promise<string>>();

const indexOf = (paneId: string) =>
  sessions.tabs.findIndex((t) => t.paneId === paneId);

export function openTab(connectionId: string): string {
  const paneId = crypto.randomUUID();
  sessions.tabs.push(initialPane(paneId, connectionId));
  sessions.active = paneId;
  return paneId;
}

export function openQuickTab({ user, host, port }: QuickTarget): string {
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
  const paneId = crypto.randomUUID();
  sessions.tabs.push(initialPane(paneId, quick.id, quick));
  sessions.active = paneId;
  return paneId;
}

// The open session keeps running; only the tab's identity changes.
export function linkQuickPane(paneId: string, saved: Connection): void {
  const i = indexOf(paneId);
  if (i < 0) return;
  sessions.tabs[i] = {
    ...sessions.tabs[i],
    connectionId: saved.id,
    quick: undefined,
  };
}

// Starts (or restarts, in the same tab) the session. Events of a tab that was
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
    if (j >= 0) sessions.tabs[j] = f(sessions.tabs[j]);
  };
  const event = (e: SessionEvent) => {
    if (indexOf(paneId) < 0) return;
    update((t) => reduce(t, e));
    const pane = sessions.tabs[indexOf(paneId)];
    if (e.type === "Status" && e.stage === "Ready" && !pane.quick)
      recordRecent(pane.connectionId);
    onEvent(e);
  };
  const { connectionId, quick } = sessions.tabs[i];
  sessions.tabs[i] = initialPane(paneId, connectionId, quick);
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
  sessions.tabs[i] = { ...sessions.tabs[i], prompt: undefined };
  try {
    await api.sessionAnswer(await id, a);
  } catch (e) {
    toastError(e);
  }
}

export function closeTab(paneId: string): void {
  const i = indexOf(paneId);
  if (i < 0) return;
  const [pane] = sessions.tabs.splice(i, 1);
  const id = sessionIds.get(paneId);
  sessionIds.delete(paneId);
  if (pane.phase !== "closed")
    id?.then((sid) => api.sessionClose(sid)).catch(() => {});
  if (sessions.active === paneId)
    sessions.active =
      sessions.tabs[Math.min(i, sessions.tabs.length - 1)]?.paneId ?? null;
}

export function cycleTab(delta: 1 | -1): void {
  const n = sessions.tabs.length;
  if (!n) return;
  const i = sessions.active ? indexOf(sessions.active) : 0;
  sessions.active = sessions.tabs[(i + delta + n) % n].paneId;
}
