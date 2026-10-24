import type { MessageKey } from "./i18n";

export type QuickTarget = { user: string; host: string; port: number };

type Result =
  { ok: true; target: QuickTarget } | { ok: false; error: MessageKey };

const fail = (error: MessageKey): Result => ({ ok: false, error });

// shortcut: no hostname/IP grammar check, the connection attempt reports bad hosts.
export function parseTarget(
  text: string,
  defaults: { user: string | null; port: number },
): Result {
  const s = text.trim();
  if (!s || /\s/.test(s)) return fail("quick.error.format");

  const parts = s.split("@");
  if (parts.length > 2 || parts[0] === "" || parts.at(-1) === "")
    return fail("quick.error.format");
  const user = parts.length === 2 ? parts[0] : defaults.user;
  // user is shown in the tab title: keep passwords, URLs and option-like text out
  if (parts.length === 2 && /^-|[:/[]/.test(parts[0]))
    return fail("quick.error.format");
  const rest = parts.at(-1)!;

  let host = rest;
  let portText: string | null = null;
  if (rest.startsWith("[")) {
    const end = rest.indexOf("]");
    if (end < 0) return fail("quick.error.format");
    host = rest.slice(1, end);
    const after = rest.slice(end + 1);
    if (after) {
      if (!after.startsWith(":")) return fail("quick.error.format");
      portText = after.slice(1);
    }
  } else if (rest.split(":").length === 2) {
    [host, portText] = rest.split(":");
  } // more than one ":" without brackets: all host (bare IPv6)
  if (!host) return fail("quick.error.format");
  if (user === null) return fail("quick.error.user");

  let port = defaults.port;
  if (portText !== null) {
    port = /^\d+$/.test(portText) ? Number(portText) : 0;
    if (port < 1 || port > 65535) return fail("quick.error.port");
  }
  return { ok: true, target: { user, host, port } };
}
