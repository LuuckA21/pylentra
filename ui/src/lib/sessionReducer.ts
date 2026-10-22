import type { CloseReason } from "./bindings/CloseReason";
import type { ForwardRule } from "./bindings/ForwardRule";
import type { ForwardState } from "./bindings/ForwardState";
import type { SessionEvent } from "./bindings/SessionEvent";

export type TabState = {
  tabId: string;
  connectionId: string;
  sessionId?: string;
  phase: "connecting" | "authenticating" | "ready" | "closed";
  prompt?: Extract<SessionEvent, { type: "HostKeyPrompt" | "SecretPrompt" }>;
  mismatch?: Extract<SessionEvent, { type: "HostKeyMismatch" }>;
  closed?: CloseReason;
  forwards: Record<number, { rule: ForwardRule; state: ForwardState }>;
};

export const initialTab = (tabId: string, connectionId: string): TabState => ({
  tabId,
  connectionId,
  phase: "connecting",
  forwards: {},
});

const PHASE = {
  Connecting: "connecting",
  Authenticating: "authenticating",
  Ready: "ready",
} as const;

// The core waits for each answer before going on, so any later Status or
// Closed means the pending prompt is settled.
export function reduce(s: TabState, e: SessionEvent): TabState {
  switch (e.type) {
    case "Status":
      return { ...s, phase: PHASE[e.stage], prompt: undefined };
    case "HostKeyPrompt":
    case "SecretPrompt":
      return { ...s, prompt: e };
    case "HostKeyMismatch":
      return { ...s, mismatch: e };
    case "Forward":
      return {
        ...s,
        forwards: {
          ...s.forwards,
          [e.forward_id]: { rule: e.rule, state: e.state },
        },
      };
    case "Closed":
      return { ...s, phase: "closed", closed: e.reason, prompt: undefined };
  }
}

// Double quotes keep `[host]:port` from being globbed by bash/zsh and work in cmd and PowerShell too.
export const keygenHint = (host: string, port: number): string =>
  port === 22 ? `ssh-keygen -R ${host}` : `ssh-keygen -R "[${host}]:${port}"`;
