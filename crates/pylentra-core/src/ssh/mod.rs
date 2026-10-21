pub mod events;

use events::SessionEvent;
use uuid::Uuid;

pub type SessionId = Uuid;

/// Receiver of everything a session produces: terminal bytes and lifecycle events.
pub trait SessionSink: Send + Sync + 'static {
    fn data(&self, bytes: Vec<u8>);
    fn event(&self, ev: SessionEvent);
}
