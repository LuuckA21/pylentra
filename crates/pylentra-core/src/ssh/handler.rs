use std::sync::Arc;

use russh::Channel;
use russh::client::{self, ChannelOpenHandle, Msg};
use russh::keys::PublicKeyOrCertificate;
use tokio::sync::watch;

use super::events::{PromptAnswer, SessionEvent};
use super::forward::{RemoteRoutes, accept_remote};
use super::{PromptBroker, SessionId, SessionSink};
use crate::error::CoreError;
use crate::known_hosts::{HostKeyStatus, KnownHosts, fingerprint};

/// russh client callbacks for one hop: verifies the host key against `KnownHosts`.
pub(crate) struct ClientHandler {
    pub id: SessionId,
    /// Already lowercased: known_hosts matching is case-sensitive.
    pub host: String,
    pub port: u16,
    pub known_hosts: Arc<KnownHosts>,
    pub broker: Arc<PromptBroker>,
    pub sink: Arc<dyn SessionSink>,
    /// True while the user is answering the host-key prompt (the connect timeout pauses).
    pub prompting: watch::Sender<bool>,
    pub routes: RemoteRoutes,
}

impl client::Handler for ClientHandler {
    type Error = CoreError;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, CoreError> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = key else {
            return Err(CoreError::Ssh("host certificates are not supported".into()));
        };
        match self.known_hosts.check(&self.host, self.port, key)? {
            HostKeyStatus::Known => Ok(true),
            HostKeyStatus::Mismatch {
                source,
                expected_fp,
            } => {
                self.sink.event(SessionEvent::HostKeyMismatch {
                    host: self.host.clone(),
                    port: self.port,
                    expected: expected_fp,
                    got: fingerprint(key),
                    source,
                });
                Err(CoreError::HostKeyMismatch {
                    host: self.host.clone(),
                    port: self.port,
                })
            }
            HostKeyStatus::Unknown => {
                let prompt = SessionEvent::HostKeyPrompt {
                    host: self.host.clone(),
                    port: self.port,
                    algo: key.algorithm().to_string(),
                    fingerprint: fingerprint(key),
                };
                self.prompting.send_replace(true);
                let answer = self.broker.ask(self.id, &*self.sink, prompt).await;
                self.prompting.send_replace(false);
                match answer {
                    PromptAnswer::HostKey { accept: true } => {
                        self.known_hosts.learn(&self.host, self.port, key)?;
                        Ok(true)
                    }
                    _ => Err(CoreError::HostKeyRejected),
                }
            }
        }
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: Channel<Msg>,
        _connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), CoreError> {
        tokio::spawn(accept_remote(
            self.routes.clone(),
            connected_port,
            channel,
            reply,
        ));
        Ok(())
    }
}
