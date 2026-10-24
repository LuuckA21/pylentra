import { Channel, invoke } from "@tauri-apps/api/core";
import type { ApiError } from "./bindings/ApiError";
import type { Connection } from "./bindings/Connection";
import type { ForwardRule } from "./bindings/ForwardRule";
import type { Group } from "./bindings/Group";
import type { KnownHostEntry } from "./bindings/KnownHostEntry";
import type { Library } from "./bindings/Library";
import type { PromptAnswer } from "./bindings/PromptAnswer";
import type { SessionEvent } from "./bindings/SessionEvent";
import type { ScanResult } from "./bindings/ScanResult";
import type { Settings } from "./bindings/Settings";
import type { Warning } from "./bindings/Warning";

// One wrapper per Rust command; a failed command rejects with an ApiError.
// Tauri converts camelCase argument names to the snake_case ones in Rust.

export const libraryGet = () => invoke<Library>("library_get");
export const startupWarnings = () => invoke<ApiError[]>("startup_warnings");
export const connectionSave = (
  connection: Connection,
  password: string | null = null,
  passphrase: string | null = null,
) => invoke<Warning[]>("connection_save", { connection, password, passphrase });
export const connectionDependents = (id: string) =>
  invoke<string[]>("connection_dependents", { id });
export const connectionDelete = (id: string) =>
  invoke<void>("connection_delete", { id });
export const groupSave = (group: Group) =>
  invoke<void>("group_save", { group });
export const groupDelete = (id: string) => invoke<void>("group_delete", { id });
export const sshConfigScan = () => invoke<ScanResult>("ssh_config_scan");
export const connectionsImport = (
  connections: Connection[],
  groupId: string | null,
) => invoke<number>("connections_import", { connections, groupId });
export const settingsGet = () => invoke<Settings>("settings_get");
export const settingsSave = (settings: Settings) =>
  invoke<void>("settings_save", { settings });

// Raw channel bodies arrive as an ArrayBuffer (InvokeResponseBody::Raw); a
// number[] is accepted too, in case the body went through JSON.
const toBytes = (m: ArrayBuffer | ArrayLike<number>): Uint8Array =>
  m instanceof ArrayBuffer ? new Uint8Array(m) : Uint8Array.from(m);

function openWithChannels(
  command: string,
  args: Record<string, unknown>,
  onData: (bytes: Uint8Array) => void,
  onEvent: (e: SessionEvent) => void,
): Promise<string> {
  const data = new Channel<ArrayBuffer | number[]>();
  data.onmessage = (m) => onData(toBytes(m));
  const events = new Channel<SessionEvent>();
  events.onmessage = onEvent;
  return invoke<string>(command, { ...args, data, events });
}

export const sessionOpen = (
  connectionId: string,
  cols: number,
  rows: number,
  onData: (bytes: Uint8Array) => void,
  onEvent: (e: SessionEvent) => void,
) =>
  openWithChannels(
    "session_open",
    { connectionId, cols, rows },
    onData,
    onEvent,
  );

// Opens a session for a connection that is not saved in the library.
export const sessionOpenQuick = (
  connection: Connection,
  cols: number,
  rows: number,
  onData: (bytes: Uint8Array) => void,
  onEvent: (e: SessionEvent) => void,
) =>
  openWithChannels(
    "session_open_quick",
    { connection, cols, rows },
    onData,
    onEvent,
  );

export function sessionWrite(
  id: string,
  data: string | Uint8Array,
): Promise<void> {
  const bytes =
    typeof data === "string" ? new TextEncoder().encode(data) : data;
  return invoke<void>("session_write", { id, data: Array.from(bytes) });
}

export const sessionResize = (id: string, cols: number, rows: number) =>
  invoke<void>("session_resize", { id, cols, rows });
export const sessionAnswer = (id: string, answer: PromptAnswer) =>
  invoke<void>("session_answer", { id, answer });
export const sessionClose = (id: string) =>
  invoke<void>("session_close", { id });
export const forwardStart = (sessionId: string, rule: ForwardRule) =>
  invoke<number>("forward_start", { sessionId, rule });
export const forwardStop = (sessionId: string, forwardId: number) =>
  invoke<void>("forward_stop", { sessionId, forwardId });
export const knownHostForget = (host: string, port: number) =>
  invoke<number>("known_host_forget", { host, port });
export const knownHostsList = () =>
  invoke<KnownHostEntry[]>("known_hosts_list");
export const logDir = () => invoke<string>("log_dir");

export const api = {
  libraryGet,
  startupWarnings,
  connectionSave,
  connectionDependents,
  connectionDelete,
  groupSave,
  groupDelete,
  sshConfigScan,
  connectionsImport,
  settingsGet,
  settingsSave,
  sessionOpen,
  sessionOpenQuick,
  sessionWrite,
  sessionResize,
  sessionAnswer,
  sessionClose,
  forwardStart,
  forwardStop,
  knownHostForget,
  knownHostsList,
  logDir,
};
