use serde::Serialize;
use std::collections::BTreeMap;
use ts_rs::TS;

#[derive(thiserror::Error, Debug)]
pub enum CoreError {
    #[error("cannot resolve host {host}")]
    Dns { host: String },
    #[error("connection refused by {host}:{port}")]
    Refused { host: String, port: u16 },
    #[error("connection to {host}:{port} timed out")]
    Timeout { host: String, port: u16 },
    #[error("{host}:{port} is unreachable: {reason}")]
    Unreachable {
        host: String,
        port: u16,
        reason: String,
    },
    #[error("host key of {host}:{port} changed")]
    HostKeyMismatch { host: String, port: u16 },
    #[error("host key rejected")]
    HostKeyRejected,
    #[error("authentication failed (tried: {})", tried.join(", "))]
    AuthFailed { tried: Vec<String> },
    #[error("cannot read key {path}: {reason}")]
    KeyUnreadable { path: String, reason: String },
    #[error("SSH agent unavailable: {reason}")]
    AgentUnavailable { reason: String },
    #[error("secret store unavailable: {reason}")]
    SecretUnavailable { reason: String },
    #[error("port {port} is already in use")]
    PortInUse { port: u16 },
    #[error("permission denied for port {port}")]
    PermissionDenied { port: u16 },
    #[error("forward rejected by the server")]
    ForwardRejected,
    #[error("invalid {field}: {reason}")]
    InvalidInput { field: String, reason: String },
    #[error("jump hosts form a cycle")]
    JumpCycle,
    #[error("jump host chain is too deep")]
    JumpTooDeep,
    #[error("unsupported file version {found}")]
    UnsupportedVersion { found: u32 },
    #[error("configuration is read-only")]
    ReadOnly,
    #[error("{what} not found")]
    NotFound { what: String },
    #[error("SSH error: {0}")]
    Ssh(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Error as seen by the UI: a stable code plus params, translated on the UI side.
#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct ApiError {
    pub code: String,
    pub params: BTreeMap<String, String>,
}

impl CoreError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Dns { .. } => "network.dns",
            Self::Refused { .. } => "network.refused",
            Self::Timeout { .. } => "network.timeout",
            Self::Unreachable { .. } => "network.unreachable",
            Self::HostKeyMismatch { .. } => "hostkey.mismatch",
            Self::HostKeyRejected => "hostkey.rejected",
            Self::AuthFailed { .. } => "auth.all_failed",
            Self::KeyUnreadable { .. } => "auth.key_unreadable",
            Self::AgentUnavailable { .. } => "auth.agent_unavailable",
            Self::SecretUnavailable { .. } => "secret.unavailable",
            Self::PortInUse { .. } => "forward.port_in_use",
            Self::PermissionDenied { .. } => "forward.permission_denied",
            Self::ForwardRejected => "forward.rejected",
            Self::InvalidInput { .. } => "config.invalid",
            Self::JumpCycle => "config.jump_cycle",
            Self::JumpTooDeep => "config.jump_too_deep",
            Self::UnsupportedVersion { .. } => "config.unsupported_version",
            Self::ReadOnly => "config.read_only",
            Self::NotFound { .. } => "not_found",
            Self::Ssh(_) => "ssh.protocol",
            Self::Io(_) => "io",
        }
    }

    pub fn to_api(&self) -> ApiError {
        let params: Vec<(&str, String)> = match self {
            Self::Dns { host } => vec![("host", host.clone())],
            Self::Refused { host, port }
            | Self::Timeout { host, port }
            | Self::HostKeyMismatch { host, port } => {
                vec![("host", host.clone()), ("port", port.to_string())]
            }
            Self::Unreachable { host, port, reason } => vec![
                ("host", host.clone()),
                ("port", port.to_string()),
                ("reason", reason.clone()),
            ],
            Self::AuthFailed { tried } => vec![("tried", tried.join(","))],
            Self::KeyUnreadable { path, reason } => {
                vec![("path", path.clone()), ("reason", reason.clone())]
            }
            Self::AgentUnavailable { reason } | Self::SecretUnavailable { reason } => {
                vec![("reason", reason.clone())]
            }
            Self::PortInUse { port } | Self::PermissionDenied { port } => {
                vec![("port", port.to_string())]
            }
            Self::InvalidInput { field, reason } => {
                vec![("field", field.clone()), ("reason", reason.clone())]
            }
            Self::UnsupportedVersion { found } => vec![("found", found.to_string())],
            Self::NotFound { what } => vec![("what", what.clone())],
            Self::Ssh(reason) => vec![("reason", reason.clone())],
            Self::Io(e) => vec![("reason", e.to_string())],
            Self::HostKeyRejected
            | Self::ForwardRejected
            | Self::JumpCycle
            | Self::JumpTooDeep
            | Self::ReadOnly => vec![],
        };
        ApiError {
            code: self.code().into(),
            params: params.into_iter().map(|(k, v)| (k.into(), v)).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_and_params() {
        assert_eq!(
            CoreError::AuthFailed {
                tried: vec!["publickey".into(), "password".into()]
            }
            .to_api(),
            ApiError {
                code: "auth.all_failed".into(),
                params: [("tried".into(), "publickey,password".into())].into()
            }
        );
        assert_eq!(CoreError::JumpCycle.code(), "config.jump_cycle");
    }
}
