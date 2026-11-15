use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use pylentra_core::ssh::SessionSink;
use pylentra_core::ssh::events::SessionEvent;
use tauri::ipc::{Channel, InvokeResponseBody};

/// Forwards a session's output to the webview over two IPC channels.
pub struct ChannelSink {
    pub data: Channel<InvokeResponseBody>,
    pub events: Channel<SessionEvent>,
    /// Bytes sent that the terminal has not rendered yet (`session_ack`).
    pub unacked: AtomicUsize,
}

// Send fails only once the webview is gone, when nobody is left to tell.
impl SessionSink for ChannelSink {
    fn data(&self, bytes: Vec<u8>) {
        let n = bytes.len();
        match self.data.send(InvokeResponseBody::Raw(bytes)) {
            // Counted once sent: a gone webview acknowledges nothing.
            Ok(()) => {
                self.unacked.fetch_add(n, Relaxed);
            }
            Err(e) => log::debug!("dropping session data: {e}"),
        }
    }

    fn event(&self, ev: SessionEvent) {
        if let Err(e) = self.events.send(ev) {
            log::debug!("dropping session event: {e}");
        }
    }

    fn unacked(&self) -> usize {
        self.unacked.load(Relaxed)
    }

    // Saturating: acknowledging more than was sent only lifts the webview's own backpressure.
    fn ack(&self, n: usize) {
        let _ = self
            .unacked
            .fetch_update(Relaxed, Relaxed, |u| Some(u.saturating_sub(n)));
    }
}
