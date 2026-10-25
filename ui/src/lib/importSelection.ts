import type { Candidate } from "./bindings/Candidate";
import type { Connection } from "./bindings/Connection";

// Mirrors MAX_JUMP_DEPTH in the core: the most jump hosts a chain may have.
const MAX_JUMP_DEPTH = 5;

/** Candidates the user can pick: not already saved and without a parse error. */
export const selectable = (c: Candidate): boolean => !c.exists && !c.error;

const byId = (candidates: Candidate[]) =>
  new Map(candidates.map((c) => [c.connection.id, c]));

/** Checking a host also checks its candidate jump hosts; unchecking touches only that host. */
export function toggle(
  selected: Set<string>,
  candidates: Candidate[],
  id: string,
): Set<string> {
  const next = new Set(selected);
  if (next.delete(id)) return next;
  const all = byId(candidates);
  for (
    let cur = all.get(id);
    cur && selectable(cur) && !next.has(cur.connection.id);
    cur = all.get(cur.connection.jump_via ?? "")
  )
    next.add(cur.connection.id);
  return next;
}

/** Names of the selected hosts that use `id` as their jump host. */
export const requiredBy = (
  candidates: Candidate[],
  selected: Set<string>,
  id: string,
): string[] =>
  candidates
    .filter(
      (c) => selected.has(c.connection.id) && c.connection.jump_via === id,
    )
    .map((c) => c.connection.name);

/**
 * The connections to import. A jump host that is not imported is dropped, as is
 * any jump that would form a loop or a chain the core rejects, so one bad
 * chain cannot cancel the whole import.
 */
export function prepare(
  candidates: Candidate[],
  selected: Set<string>,
  user: string,
): Connection[] {
  const out = candidates
    .filter((c) => selected.has(c.connection.id))
    .map(({ connection: c }) => ({
      ...c,
      username: c.username || user,
    }));
  const inSet = new Map(out.map((c) => [c.id, c]));
  const all = byId(candidates);
  for (const c of out)
    if (c.jump_via && all.has(c.jump_via) && !inSet.has(c.jump_via))
      c.jump_via = null;
  for (const c of out) {
    const seen = new Set([c.id]);
    let hops = 0;
    for (let n = c.jump_via; n; n = inSet.get(n)?.jump_via ?? null) {
      if (seen.has(n) || ++hops > MAX_JUMP_DEPTH) {
        c.jump_via = null;
        break;
      }
      seen.add(n);
    }
  }
  return out;
}
