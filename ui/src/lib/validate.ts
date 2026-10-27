import type { MessageKey } from "./i18n";

// Client-side mirror of Connection::validate and ForwardRule::validate in
// crates/pylentra-core/src/model.rs. Keys are the field names the core reports.
export type Draft = {
  name: string;
  host: string;
  port: number | null;
  username: string;
  forwards: {
    type: "Local" | "Remote" | "Dynamic";
    bind_host: string;
    bind_port: number | null;
    dest_port: number | null;
  }[];
};

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
    if (r.type !== "Dynamic" && !validPort(r.dest_port))
      errors[`forwards[${i}].dest_port`] = "form.error.port";
  });
  return errors;
}
