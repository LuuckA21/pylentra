import type { Connection } from "./bindings/Connection";

export const fold = (s: string) =>
  s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

export function matches(c: Connection, query: string): boolean {
  const q = fold(query.trim());
  return [c.name, c.host, c.username].some((f) => fold(f).includes(q));
}
