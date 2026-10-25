import type { CloseReason } from "./bindings/CloseReason";

/** Wait before each automatic retry, in ms; one entry per attempt. */
export const RECONNECT_DELAYS = [1000, 2000, 5000, 10000, 30000] as const;

/** Only a session that was up and lost its connection is retried; auth, host key and user choices are not. */
export function shouldReconnect(
  reason: CloseReason,
  wasReady: boolean,
  enabled: boolean,
): boolean {
  if (!enabled || !wasReady || reason.type !== "Error") return false;
  const { code } = reason.error;
  return (
    code.startsWith("network.") || code === "ssh.protocol" || code === "io"
  );
}
