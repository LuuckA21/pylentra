import { getPref, setPref } from "./prefs";

const KEY = "pylentra.recent";
const MAX = 5;

function stored(): string[] {
  const v = getPref<unknown>(KEY, []);
  return Array.isArray(v) ? v.filter((x) => typeof x === "string") : [];
}

export function recordRecent(id: string): void {
  setPref(KEY, [id, ...stored().filter((x) => x !== id)].slice(0, MAX));
}

/** Recent ids, newest first, without connections that no longer exist. */
export function recentIds(existing: Set<string>): string[] {
  return stored().filter((id) => existing.has(id));
}
