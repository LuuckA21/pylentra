import type { Connection } from "./bindings/Connection";
import type { Group } from "./bindings/Group";

export const fold = (s: string) =>
  s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

export function matches(
  c: Connection,
  query: string,
  groups: Group[],
): boolean {
  const q = fold(query.trim());
  const group = groups.find((g) => g.id === c.group_id)?.name;
  return [c.name, c.host, c.username, c.notes, group].some(
    (f) => f && fold(f).includes(q),
  );
}
