import type { ForwardRule } from "./bindings/ForwardRule";
import type { MessageKey } from "./i18n";

// A forward being edited: every field of every type, ports empty until typed.
export type ForwardDraft = {
  type: ForwardRule["type"];
  bind_host: string;
  bind_port: number | null;
  dest_host: string;
  dest_port: number | null;
  auto_start: boolean;
};

// Client-side mirror of Connection::validate and ForwardRule::validate in
// crates/pylentra-core/src/model.rs. Keys are the field names the core reports.
export type Draft = {
  name: string;
  host: string;
  port: number | null;
  username: string;
  forwards: ForwardDraft[];
};

// A number field's value; empty is null, so it fails as a port.
export const num = (v: string) => (v === "" ? null : Number(v));

// Defaults of a new forward: loopback bind, SOCKS on 1080.
export const newForward = (type: ForwardDraft["type"]): ForwardDraft => ({
  type,
  bind_host: type === "Remote" ? "" : "127.0.0.1",
  bind_port: type === "Dynamic" ? 1080 : null,
  dest_host: "localhost",
  dest_port: null,
  auto_start: false,
});

// The rule a draft that passed `validate` stands for.
export function toRule(r: ForwardDraft): ForwardRule {
  const { auto_start } = r;
  const bind_port = r.bind_port!;
  if (r.type === "Dynamic")
    return {
      type: "Dynamic",
      bind_host: r.bind_host.trim(),
      bind_port,
      auto_start,
    };
  const dest = { dest_host: r.dest_host.trim(), dest_port: r.dest_port! };
  return r.type === "Local"
    ? {
        type: "Local",
        bind_host: r.bind_host.trim(),
        bind_port,
        ...dest,
        auto_start,
      }
    : { type: "Remote", bind_port, ...dest, auto_start };
}

const validPort = (p: number | null) =>
  p !== null && Number.isInteger(p) && p >= 1 && p <= 65535;

const OCTET = "(?:25[0-5]|2[0-4]\\d|1\\d\\d|[1-9]?\\d)"; // no leading zeros, like Rust
const IPV4 = new RegExp(`^${OCTET}(?:\\.${OCTET}){3}$`);

// IPv4 dotted quad, or anything with a colon that the URL parser takes as an IPv6 literal.
function isIp(s: string): boolean {
  if (IPV4.test(s)) return true;
  if (!s.includes(":")) return false;
  try {
    return new URL(`http://[${s}]`).hostname !== "";
  } catch {
    return false;
  }
}

export function validate(d: Draft): Record<string, MessageKey> {
  const errors: Record<string, MessageKey> = {};
  for (const f of ["name", "host", "username"] as const)
    if (!d[f].trim()) errors[f] = "form.error.required";
  if (!validPort(d.port)) errors.port = "form.error.port";
  d.forwards.forEach((r, i) => {
    if (r.type !== "Remote" && !isIp(r.bind_host))
      errors[`forwards[${i}].bind_host`] = "form.error.ip";
    if (!validPort(r.bind_port))
      errors[`forwards[${i}].bind_port`] = "form.error.port";
    if (r.type === "Dynamic") return;
    if (!r.dest_host.trim())
      errors[`forwards[${i}].dest_host`] = "form.error.required";
    if (!validPort(r.dest_port))
      errors[`forwards[${i}].dest_port`] = "form.error.port";
  });
  return errors;
}
