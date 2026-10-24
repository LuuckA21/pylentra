use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::ApiError;
use crate::known_hosts::KeySource;
use crate::model::ForwardRule;

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub enum Stage {
    Connecting,
    Authenticating,
    Ready,
}

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct KbdPrompt {
    pub text: String,
    pub echo: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[serde(tag = "type")]
#[ts(export)]
pub enum PromptKind {
    Password,
    Passphrase {
        attempt: u8,
    },
    KeyboardInteractive {
        name: String,
        instructions: String,
        prompts: Vec<KbdPrompt>,
    },
}

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[serde(tag = "type")]
#[ts(export)]
pub enum ForwardState {
    Active { bound_port: u16 },
    Stopped,
    Failed { error: ApiError },
}

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[serde(tag = "type")]
#[ts(export)]
pub enum CloseReason {
    Exited { code: u32 },
    Error { error: ApiError },
    UserClosed,
}

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[serde(tag = "type")]
#[ts(export)]
pub enum SessionEvent {
    Status {
        stage: Stage,
    },
    HostKeyPrompt {
        host: String,
        port: u16,
        algo: String,
        fingerprint: String,
    },
    HostKeyMismatch {
        host: String,
        port: u16,
        expected: String,
        got: String,
        source: KeySource,
    },
    SecretPrompt {
        host: String,
        port: u16,
        kind: PromptKind,
        /// false: the answer is never saved, so the UI hides its "save" option.
        can_save: bool,
    },
    Forward {
        forward_id: u32,
        rule: ForwardRule,
        state: ForwardState,
    },
    Closed {
        reason: CloseReason,
    },
}

#[derive(Deserialize, Clone, PartialEq, TS)]
#[serde(tag = "type")]
#[ts(export)]
pub enum PromptAnswer {
    HostKey { accept: bool },
    Secret { value: String, save: bool },
    KeyboardInteractive { responses: Vec<String>, save: bool },
    Cancel,
}

// Hand-written so that secrets never reach logs through `{:?}`.
impl fmt::Debug for PromptAnswer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HostKey { accept } => f.debug_struct("HostKey").field("accept", accept).finish(),
            Self::Secret { save, .. } => f
                .debug_struct("Secret")
                .field("value", &"<redacted>")
                .field("save", save)
                .finish(),
            Self::KeyboardInteractive { save, .. } => f
                .debug_struct("KeyboardInteractive")
                .field("responses", &"<redacted>")
                .field("save", save)
                .finish(),
            Self::Cancel => f.write_str("Cancel"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_tagged_and_secrets_are_redacted() {
        let ev = SessionEvent::Status {
            stage: Stage::Ready,
        };
        let out = toml::to_string(&ev).unwrap();
        assert!(out.contains("type = \"Status\""), "{out}");
        assert!(out.contains("stage = \"Ready\""), "{out}");

        let a: PromptAnswer =
            toml::from_str("type = \"Secret\"\nvalue = \"hunter2\"\nsave = true").unwrap();
        assert!(!format!("{a:?}").contains("hunter2"));
    }
}
