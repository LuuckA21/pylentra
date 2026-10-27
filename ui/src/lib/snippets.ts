import type { Snippet } from "./bindings/Snippet";
import { fold } from "./search";

export const firstLine = (text: string): string =>
  text
    .split(/\r?\n/)
    .find((l) => l.trim())
    ?.trim() ?? "";

export const body = (text: string): string => text.replace(/[\r\n]+$/, "");

// Multi-line text sent as-is would run each line; ask first unless the
// shell gets it as one bracketed paste or the snippet is meant to run.
export const needsConfirm = (
  text: string,
  run: boolean,
  bracketed: boolean,
): boolean => !run && !bracketed && /[\r\n]/.test(body(text));

export function groupSnippets(
  list: Snippet[],
  query: string,
  connectionId: string | null,
  known: Set<string>,
): { own: Snippet[]; global: Snippet[] } {
  const q = fold(query.trim());
  const hit = (s: Snippet) =>
    fold(s.name).includes(q) || fold(s.text).includes(q);
  const sorted = (xs: Snippet[]) =>
    xs.sort((a, b) => a.name.localeCompare(b.name));
  const found = list.filter(hit);
  return {
    own: sorted(
      connectionId === null
        ? []
        : found.filter((s) => s.connection_id === connectionId),
    ),
    // A snippet whose connection is gone stays reachable as a global one.
    global: sorted(
      found.filter((s) => !s.connection_id || !known.has(s.connection_id)),
    ),
  };
}

// `{{{{` is the escape for a literal `{{`; both helpers share this one regex
// so they always agree on what counts as a variable.
const VAR = /\{\{\{\{|\{\{\s*([A-Za-z0-9_-]{1,40})\s*\}\}/g;

export const snippetVars = (text: string): string[] => [
  ...new Set([...text.matchAll(VAR)].flatMap((m) => (m[1] ? [m[1]] : []))),
];

// A replacer function inserts values literally: no `$` patterns, and a value
// containing `{{x}}` is not expanded again.
export const fillVars = (
  text: string,
  values: Record<string, string>,
): string =>
  text.replace(VAR, (_, name?: string) =>
    name ? (Object.hasOwn(values, name) ? values[name] : "") : "{{",
  );

const remembered = new Map<string, string>();

export const recallVar = (snippetId: string, name: string): string =>
  remembered.get(`${snippetId}:${name}`) ?? "";

export function rememberVars(
  snippetId: string,
  values: Record<string, string>,
): void {
  for (const [name, v] of Object.entries(values))
    remembered.set(`${snippetId}:${name}`, v);
}
