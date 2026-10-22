use pylentra_core::ssh::SessionSink;
use pylentra_core::ssh::events::SessionEvent;
use tauri::ipc::{Channel, InvokeResponseBody};

/// Forwards a session's output to the webview over two IPC channels.
pub struct ChannelSink {
    pub data: Channel<InvokeResponseBody>,
    pub events: Channel<SessionEvent>,
}

// Send fails only once the webview is gone, when nobody is left to tell.
impl SessionSink for ChannelSink {
    fn data(&self, bytes: Vec<u8>) {
        if let Err(e) = self.data.send(InvokeResponseBody::Raw(bytes)) {
            log::debug!("dropping session data: {e}");
        }
    }

    fn event(&self, ev: SessionEvent) {
        if let Err(e) = self.events.send(ev) {
            log::debug!("dropping session event: {e}");
        }
    }
}
