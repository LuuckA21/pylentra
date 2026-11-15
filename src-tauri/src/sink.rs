use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use pylentra_core::ssh::SessionSink;
use pylentra_core::ssh::events::SessionEvent;
use tauri::ipc::{Channel, InvokeResponseBody};

/// Forwards a session's output to the webview over two IPC channels.
pub struct ChannelSink {
    pub data: Channel<InvokeResponseBody>,
    pub events: Channel<SessionEvent>,
    /// Output bytes sent, and the most the terminal has reported rendered (`session_ack`).
    sent: AtomicUsize,
    acked: AtomicUsize,
}

impl ChannelSink {
    pub fn new(data: Channel<InvokeResponseBody>, events: Channel<SessionEvent>) -> Self {
        Self {
            data,
            events,
            sent: AtomicUsize::new(0),
            acked: AtomicUsize::new(0),
        }
    }
}

// Send fails only once the webview is gone, when nobody is left to tell.
impl SessionSink for ChannelSink {
    fn data(&self, bytes: Vec<u8>) {
        let n = bytes.len();
        // Counted before the send, so no acknowledgement can come before its bytes.
        self.sent.fetch_add(n, Relaxed);
        if let Err(e) = self.data.send(InvokeResponseBody::Raw(bytes)) {
            self.sent.fetch_sub(n, Relaxed);
            log::debug!("dropping session data: {e}");
        }
    }

    fn event(&self, ev: SessionEvent) {
        if let Err(e) = self.events.send(ev) {
            log::debug!("dropping session event: {e}");
        }
    }

    // Saturating: a send that failed after its ack can leave `acked` past `sent`.
    fn unacked(&self) -> usize {
        self.sent
            .load(Relaxed)
            .saturating_sub(self.acked.load(Relaxed))
    }

    // Clamped to what was sent: a huge total must not lift backpressure for later output.
    fn ack(&self, total: usize) {
        self.acked
            .fetch_max(total.min(self.sent.load(Relaxed)), Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sink(send_ok: bool) -> ChannelSink {
        let data = Channel::new(move |_| {
            if send_ok {
                Ok(())
            } else {
                Err(tauri::Error::WebviewNotFound)
            }
        });
        ChannelSink::new(data, Channel::new(|_| Ok(())))
    }

    #[test]
    fn late_or_lost_acks_do_not_stall() {
        let s = sink(true);
        for _ in 0..3 {
            s.data(vec![0; 10]);
        }
        // The ack for 10 arrives after the one for 20: it changes nothing.
        s.ack(20);
        s.ack(10);
        assert_eq!(s.unacked(), 10);
        // The ack for 30 is lost; the next one covers it.
        s.data(vec![0; 5]);
        assert_eq!(s.unacked(), 15);
        s.ack(35);
        assert_eq!(s.unacked(), 0);
    }

    #[test]
    fn acks_beyond_what_was_sent_do_not_lift_backpressure() {
        let s = sink(true);
        s.data(vec![0; 10]);
        s.ack(usize::MAX);
        assert_eq!(s.unacked(), 0);
        s.data(vec![0; 5]);
        assert_eq!(s.unacked(), 5);
    }

    #[test]
    fn unsent_bytes_are_not_counted() {
        let s = sink(false);
        s.data(vec![0; 10]);
        assert_eq!(s.unacked(), 0);
    }
}
