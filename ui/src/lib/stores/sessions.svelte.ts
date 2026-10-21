import { api } from "../api";
import type { PromptAnswer } from "../bindings/PromptAnswer";
import type { SessionEvent } from "../bindings/SessionEvent";
import { initialTab, reduce, type TabState } from "../sessionReducer";
import { isApiError, toastError } from "./toasts.svelte";

export const sessions = $state<{ tabs: TabState[]; active: string | null }>({
  tabs: [],
  active: null,
});

// tabId → id of the tab's current session; a promise because events (and the
// user) can act before session_open has returned.
// Not reactive on purpose: nothing renders from it.
// eslint-disable-next-line svelte/prefer-svelte-reactivity
const sessionIds = new Map<string, Promise<string>>();

const indexOf = (tabId: string) =>
  sessions.tabs.findIndex((t) => t.tabId === tabId);

export function openTab(connectionId: string): string {
  const tabId = crypto.randomUUID();
  sessions.tabs.push(initialTab(tabId, connectionId));
  sessions.active = tabId;
  return tabId;
}

// Starts (or restarts, in the same tab) the session. Events of a tab that was
// closed meanwhile are dropped.
export function connect(
  tabId: string,
  cols: number,
  rows: number,
  onData: (bytes: Uint8Array) => void,
  onEvent: (e: SessionEvent) => void,
): void {
  const i = indexOf(tabId);
  if (i < 0) return;
  const update = (f: (t: TabState) => TabState) => {
    const j = indexOf(tabId);
    if (j >= 0) sessions.tabs[j] = f(sessions.tabs[j]);
  };
  const event = (e: SessionEvent) => {
    if (indexOf(tabId) < 0) return;
    update((t) => reduce(t, e));
    onEvent(e);
  };
  sessions.tabs[i] = initialTab(tabId, sessions.tabs[i].connectionId);
  const id = api.sessionOpen(
    sessions.tabs[i].connectionId,
    cols,
    rows,
    (bytes) => indexOf(tabId) >= 0 && onData(bytes),
    event,
  );
  sessionIds.set(tabId, id);
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

// Write and resize errors are not reported: a dead session sends Closed.
export function write(tab: TabState, data: string | Uint8Array): void {
  if (tab.sessionId && tab.phase === "ready")
    api.sessionWrite(tab.sessionId, data).catch(() => {});
}

export function resize(tab: TabState, cols: number, rows: number): void {
  if (tab.sessionId && tab.phase !== "closed")
    api.sessionResize(tab.sessionId, cols, rows).catch(() => {});
}

export async function answer(tabId: string, a: PromptAnswer): Promise<void> {
  const i = indexOf(tabId);
  const id = sessionIds.get(tabId);
  if (i < 0 || !id) return;
  sessions.tabs[i] = { ...sessions.tabs[i], prompt: undefined };
  try {
    await api.sessionAnswer(await id, a);
  } catch (e) {
    toastError(e);
  }
}

export function closeTab(tabId: string): void {
  const i = indexOf(tabId);
  if (i < 0) return;
  const [tab] = sessions.tabs.splice(i, 1);
  const id = sessionIds.get(tabId);
  sessionIds.delete(tabId);
  if (tab.phase !== "closed")
    id?.then((sid) => api.sessionClose(sid)).catch(() => {});
  if (sessions.active === tabId)
    sessions.active =
      sessions.tabs[Math.min(i, sessions.tabs.length - 1)]?.tabId ?? null;
}

export function cycleTab(delta: 1 | -1): void {
  const n = sessions.tabs.length;
  if (!n) return;
  const i = sessions.active ? indexOf(sessions.active) : 0;
  sessions.active = sessions.tabs[(i + delta + n) % n].tabId;
}
