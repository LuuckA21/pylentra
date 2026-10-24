import type { ConnIcon } from "./bindings/ConnIcon";
import type { Connection } from "./bindings/Connection";
import type { Group } from "./bindings/Group";
import type { TagColor } from "./bindings/TagColor";

export const TAG_COLORS: readonly TagColor[] = [
  "red",
  "orange",
  "amber",
  "green",
  "teal",
  "blue",
  "violet",
  "gray",
];

export const CONN_ICONS: readonly ConnIcon[] = [
  "server",
  "database",
  "web",
  "nas",
  "raspberry",
  "cloud",
  "container",
  "terminal",
];

/** The connection's own color, else its group's; null means the neutral tag. */
export function colorOf(
  c: Pick<Connection, "color" | "group_id">,
  groups: Group[],
): TagColor | null {
  return c.color ?? groups.find((g) => g.id === c.group_id)?.color ?? null;
}

/** Up to two uppercase letters/digits: one per word, or the first two of a single word. */
export function initialsOf(name: string): string {
  const words = name
    .split(/[-_.\s]+/)
    .map((w) => Array.from(w.replace(/[^\p{L}\p{N}]/gu, "")))
    .filter((w) => w.length > 0);
  if (words.length === 0) return "?";
  const letters =
    words.length === 1 ? words[0].slice(0, 2) : [words[0][0], words[1][0]];
  return letters.join("").toUpperCase();
}
