import { api } from "../api";
import type { ApiError } from "../bindings/ApiError";
import type { Connection } from "../bindings/Connection";
import type { Group } from "../bindings/Group";
import type { Warning } from "../bindings/Warning";

export const library = $state<{
  groups: Group[];
  connections: Connection[];
  warnings: ApiError[];
}>({ groups: [], connections: [], warnings: [] });

export const isReadOnly = (): boolean =>
  library.warnings.some((w) => w.code === "config.read_only");

export async function refresh(): Promise<void> {
  const [lib, warnings] = await Promise.all([
    api.libraryGet(),
    api.startupWarnings(),
  ]);
  library.groups = lib.groups;
  library.connections = lib.connections;
  library.warnings = warnings;
}

// Each save/delete refreshes afterwards, so the Rust side stays the source of truth.
async function then<T>(op: Promise<T>): Promise<T> {
  const result = await op;
  await refresh();
  return result;
}

export const saveConnection = (
  c: Connection,
  password?: string,
  passphrase?: string,
): Promise<Warning[]> =>
  then(api.connectionSave(c, password ?? null, passphrase ?? null));
export const deleteConnection = (id: string) => then(api.connectionDelete(id));
export const saveGroup = (g: Group) => then(api.groupSave(g));
export const deleteGroup = (id: string) => then(api.groupDelete(id));
