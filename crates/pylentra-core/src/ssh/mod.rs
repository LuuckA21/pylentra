mod auth;
pub mod events;
mod forward;
mod handler;
mod jump;
mod session;
pub mod sftp;
mod socks;

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use events::{PromptAnswer, SessionEvent};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::error::CoreError;

pub use session::{CLOSE_TIMEOUT, SessionManager, SessionOptions};

pub type SessionId = Uuid;

/// Terminal output a sink may hold unacknowledged before the session stops reading it.
pub const MAX_UNACKED: usize = 4 << 20;

/// Receiver of everything a session produces: terminal bytes and lifecycle events.
pub trait SessionSink: Send + Sync + 'static {
    fn data(&self, bytes: Vec<u8>);
    fn event(&self, ev: SessionEvent);
    /// Bytes given to `data` that the consumer has not acknowledged with `ack` yet. Above
    /// [`MAX_UNACKED`] the session stops reading, so the server slows down instead of the
    /// output piling up. A sink that consumes the bytes at once keeps the default.
    fn unacked(&self) -> usize {
        0
    }
    /// `total`: every byte the consumer has processed so far, so a lost or late
    /// acknowledgement is made good by the next one.
    fn ack(&self, _total: usize) {}
}

/// Prompts waiting for the UI, at most one per session.
#[derive(Default)]
pub struct PromptBroker {
    pending: Mutex<HashMap<SessionId, oneshot::Sender<PromptAnswer>>>,
}

impl PromptBroker {
    fn lock(&self) -> MutexGuard<'_, HashMap<SessionId, oneshot::Sender<PromptAnswer>>> {
        // A poisoned map is still a valid map.
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Emits `prompt` and waits for its answer; a dropped prompt reads as `Cancel`.
    pub async fn ask(
        &self,
        id: SessionId,
        sink: &dyn SessionSink,
        prompt: SessionEvent,
    ) -> PromptAnswer {
        let (tx, rx) = oneshot::channel();
        // Registered before the event goes out, so an immediate answer finds it.
        self.lock().insert(id, tx);
        sink.event(prompt);
        rx.await.unwrap_or(PromptAnswer::Cancel)
    }

    pub fn answer(&self, id: SessionId, answer: PromptAnswer) -> Result<(), CoreError> {
        let not_found = || CoreError::NotFound {
            what: format!("prompt for session {id}"),
        };
        let tx = self.lock().remove(&id).ok_or_else(not_found)?;
        // The asker is gone (session closing): same as no pending prompt.
        tx.send(answer).map_err(|_| not_found())
    }

    /// Drops the pending prompt, if any; its asker sees `Cancel`.
    pub fn cancel(&self, id: SessionId) {
        self.lock().remove(&id);
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }
}
