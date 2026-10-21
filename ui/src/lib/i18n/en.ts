export const en = {
  "app.title": "Pylentra",

  "error.auth.agent_unavailable": "SSH agent unavailable: {reason}",
  "error.auth.all_failed": "Authentication failed (tried: {tried}).",
  "error.auth.key_unreadable": "Cannot read key {path}: {reason}",
  "error.cancelled": "Cancelled.",
  "error.config.invalid": "Invalid value for {field}: {reason}",
  "error.config.jump_cycle": "The jump host chain contains a cycle.",
  "error.config.jump_too_deep": "The jump host chain is too deep (maximum 5).",
  "error.config.read_only":
    "The configuration was written by a newer version and is read-only.",
  "error.config.unsupported_version":
    "Unsupported configuration version {found}.",
  "error.config.recovered_corrupt":
    "The configuration file was unreadable. It was kept as {backup} and a new one was started.",
  "error.forward.permission_denied": "Permission denied for port {port}.",
  "error.forward.port_in_use": "Port {port} is already in use.",
  "error.forward.rejected": "The server rejected the port forward.",
  "error.hostkey.mismatch":
    "The host key of {host}:{port} has changed. The connection was refused.",
  "error.hostkey.rejected": "Host key rejected.",
  "error.io": "I/O error: {reason}",
  "error.network.dns": "Cannot resolve host {host}.",
  "error.network.refused": "Connection refused by {host}:{port}.",
  "error.network.timeout": "Connection to {host}:{port} timed out.",
  "error.network.unreachable": "{host}:{port} is unreachable: {reason}",
  "error.not_found": "Not found: {what}",
  "error.secret.unavailable": "Keychain unavailable: {reason}",
  "error.ssh.protocol": "SSH error: {reason}",
  "error.unknown": "Something went wrong.",
} as const;

export type MessageKey = keyof typeof en;
