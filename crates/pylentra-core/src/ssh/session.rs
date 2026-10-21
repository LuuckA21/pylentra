use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use russh::client::{Handle, Msg};
use russh::{Channel, ChannelMsg, ChannelReadHalf, ChannelWriteHalf, Disconnect};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use uuid::Uuid;

use super::events::{CloseReason, PromptAnswer, SessionEvent, Stage};
use super::forward::{Forwards, auto_start};
use super::handler::ClientHandler;
use super::jump::{Chain, ChainCtx, connect_chain};
use super::{PromptBroker, SessionId, SessionSink};
use crate::error::CoreError;
use crate::known_hosts::KnownHosts;
use crate::model::{Connection, ForwardRule};
use crate::secrets::SecretStore;

const TERM: &str = "xterm-256color";
/// How long `close` waits for one session to wind down before aborting it.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub struct SessionOptions {
    pub connect_timeout: Duration,
    pub keepalive_interval: Duration,
    pub keepalive_max: usize,
    pub max_passphrase_attempts: u8,
    pub agent_path: Option<PathBuf>,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
            keepalive_interval: Duration::from_secs(30),
            keepalive_max: 3,
            max_passphrase_attempts: 3,
            agent_path: None,
        }
    }
}

enum Command {
    Write(Vec<u8>),
    Resize(u16, u16),
    ForwardStart(ForwardRule, oneshot::Sender<Result<u32, CoreError>>),
    ForwardStop(u32, oneshot::Sender<Result<(), CoreError>>),
    Close,
}

struct SessionHandle {
    tx: mpsc::Sender<Command>,
    task: JoinHandle<()>,
    sink: Arc<dyn SessionSink>,
}

type Sessions = Arc<Mutex<HashMap<SessionId, SessionHandle>>>;

fn lock(
    sessions: &Mutex<HashMap<SessionId, SessionHandle>>,
) -> MutexGuard<'_, HashMap<SessionId, SessionHandle>> {
    // A poisoned map is still a valid map.
    sessions.lock().unwrap_or_else(|e| e.into_inner())
}

fn not_found(id: SessionId) -> CoreError {
    CoreError::NotFound {
        what: format!("session {id}"),
    }
}

pub struct SessionManager {
    secrets: Arc<dyn SecretStore>,
    known_hosts: Arc<KnownHosts>,
    opts: SessionOptions,
    broker: Arc<PromptBroker>,
    sessions: Sessions,
}

impl SessionManager {
    pub fn new(
        secrets: Arc<dyn SecretStore>,
        known_hosts: Arc<KnownHosts>,
        opts: SessionOptions,
    ) -> Self {
        Self {
            secrets,
            known_hosts,
            opts,
            broker: Arc::default(),
            sessions: Arc::default(),
        }
    }

    /// Starts the session in the background; progress arrives on `sink`, ending with one `Closed`.
    /// `chain` lists the jump hosts first and the target last.
    pub fn open(
        &self,
        chain: Vec<Connection>,
        cols: u16,
        rows: u16,
        sink: Arc<dyn SessionSink>,
    ) -> SessionId {
        let id = Uuid::new_v4();
        let (tx, rx) = mpsc::channel(256);
        let ctx = Ctx {
            id,
            secrets: self.secrets.clone(),
            known_hosts: self.known_hosts.clone(),
            broker: self.broker.clone(),
            opts: self.opts.clone(),
            sink: sink.clone(),
            sessions: self.sessions.clone(),
        };
        // Locked across the spawn so the task cannot deregister before it is registered.
        let mut sessions = lock(&self.sessions);
        let task = tokio::spawn(run(ctx, chain, cols, rows, rx));
        sessions.insert(id, SessionHandle { tx, task, sink });
        id
    }

    pub async fn write(&self, id: SessionId, bytes: Vec<u8>) -> Result<(), CoreError> {
        self.send(id, Command::Write(bytes)).await
    }

    pub async fn resize(&self, id: SessionId, cols: u16, rows: u16) -> Result<(), CoreError> {
        self.send(id, Command::Resize(cols, rows)).await
    }

    /// Resolves the session's pending prompt; `NotFound` if there is none.
    pub fn answer(&self, id: SessionId, answer: PromptAnswer) -> Result<(), CoreError> {
        self.broker.answer(id, answer)
    }

    /// Returns once the session has emitted its `Closed`.
    pub async fn close(&self, id: SessionId) -> Result<(), CoreError> {
        let handle = lock(&self.sessions)
            .remove(&id)
            .ok_or_else(|| not_found(id))?;
        shutdown(
            vec![(id, handle)],
            Instant::now() + CLOSE_TIMEOUT,
            &self.broker,
        )
        .await;
        Ok(())
    }

    /// Closes every session, aborting those still running after `timeout`.
    pub async fn close_all(&self, timeout: Duration) {
        let all: Vec<_> = lock(&self.sessions).drain().collect();
        shutdown(all, Instant::now() + timeout, &self.broker).await;
    }

    /// Starts a forward on a ready session and returns its id. The outcome, failure included,
    /// is also reported as a `Forward` event; a failed forward never closes the session.
    pub async fn forward_start(&self, id: SessionId, rule: ForwardRule) -> Result<u32, CoreError> {
        let (reply, answer) = oneshot::channel();
        self.send(id, Command::ForwardStart(rule, reply)).await?;
        // No answer: the session ended first.
        answer.await.map_err(|_| not_found(id))?
    }

    pub async fn forward_stop(&self, id: SessionId, forward_id: u32) -> Result<(), CoreError> {
        let (reply, answer) = oneshot::channel();
        self.send(id, Command::ForwardStop(forward_id, reply))
            .await?;
        answer.await.map_err(|_| not_found(id))?
    }

    pub fn pending_prompts(&self) -> usize {
        self.broker.len()
    }

    async fn send(&self, id: SessionId, cmd: Command) -> Result<(), CoreError> {
        let tx = lock(&self.sessions)
            .get(&id)
            .map(|h| h.tx.clone())
            .ok_or_else(|| not_found(id))?;
        // The task may have just finished on its own.
        tx.send(cmd).await.map_err(|_| not_found(id))
    }
}

/// Asks each session to close, then aborts whatever is still running at `deadline`.
async fn shutdown(
    handles: Vec<(SessionId, SessionHandle)>,
    deadline: Instant,
    broker: &PromptBroker,
) {
    for (_, h) in &handles {
        let _ = tokio::time::timeout_at(deadline, h.tx.send(Command::Close)).await;
    }
    for (id, mut h) in handles {
        if tokio::time::timeout_at(deadline, &mut h.task).await.is_ok() {
            continue;
        }
        h.task.abort();
        // Cancelled means the task never reached its own `Closed`; finished means it did.
        if matches!(h.task.await, Err(e) if e.is_cancelled()) {
            broker.cancel(id);
            h.sink.event(SessionEvent::Closed {
                reason: CloseReason::UserClosed,
            });
        }
    }
}

/// What a session task shares with its manager.
struct Ctx {
    id: SessionId,
    secrets: Arc<dyn SecretStore>,
    known_hosts: Arc<KnownHosts>,
    broker: Arc<PromptBroker>,
    opts: SessionOptions,
    sink: Arc<dyn SessionSink>,
    sessions: Sessions,
}

fn error_reason(e: CoreError) -> CloseReason {
    CloseReason::Error { error: e.to_api() }
}

async fn run(
    ctx: Ctx,
    chain: Vec<Connection>,
    cols: u16,
    rows: u16,
    mut rx: mpsc::Receiver<Command>,
) {
    let mut forwards = Forwards::new(ctx.sink.clone());
    let reason = drive(&ctx, &chain, cols, rows, &mut rx, &mut forwards).await;
    // Listeners are gone before `Closed` is seen.
    forwards.stop_all().await;
    ctx.broker.cancel(ctx.id);
    // Deregistered first, so commands sent after `Closed` get `NotFound`.
    lock(&ctx.sessions).remove(&ctx.id);
    ctx.sink.event(SessionEvent::Closed { reason });
}

async fn drive(
    ctx: &Ctx,
    chain: &[Connection],
    cols: u16,
    rows: u16,
    rx: &mut mpsc::Receiver<Command>,
    forwards: &mut Forwards,
) -> CloseReason {
    ctx.sink.event(SessionEvent::Status {
        stage: Stage::Connecting,
    });
    let mut new_size = None;
    // A block, so `setup` lets go of `forwards` before the I/O loop takes it.
    let (chain, channel) = {
        let setup = open_shell(ctx, chain, cols, rows, forwards);
        tokio::pin!(setup);
        loop {
            tokio::select! {
                r = &mut setup => match r {
                    Ok(v) => break v,
                    Err(CoreError::Cancelled) => return CloseReason::UserClosed,
                    Err(e) => return error_reason(e),
                },
                cmd = rx.recv() => match cmd {
                    Some(Command::Close) | None => return CloseReason::UserClosed,
                    Some(Command::Resize(c, r)) => new_size = Some((c, r)),
                    // Nothing to type into yet.
                    Some(Command::Write(_)) => {}
                    Some(Command::ForwardStart(_, reply)) => {
                        let _ = reply.send(Err(CoreError::Ssh("session is not ready".into())));
                    }
                    Some(Command::ForwardStop(_, reply)) => {
                        let _ = reply.send(Err(CoreError::Ssh("session is not ready".into())));
                    }
                },
            }
        }
    };
    if let Some((c, r)) = new_size {
        let _ = channel.window_change(c.into(), r.into(), 0, 0).await;
    }
    io_loop(chain, channel, &*ctx.sink, rx, forwards).await
}

async fn open_shell(
    ctx: &Ctx,
    hops: &[Connection],
    cols: u16,
    rows: u16,
    forwards: &mut Forwards,
) -> Result<(Chain, Channel<Msg>), CoreError> {
    let chain_ctx = ChainCtx {
        id: ctx.id,
        secrets: &*ctx.secrets,
        known_hosts: &ctx.known_hosts,
        broker: &ctx.broker,
        sink: &ctx.sink,
        opts: &ctx.opts,
        routes: &forwards.routes,
    };
    let chain = connect_chain(hops, &chain_ctx).await?;
    let channel = chain.target.channel_open_session().await?;
    channel
        .request_pty(false, TERM, cols.into(), rows.into(), 0, 0, &[])
        .await?;
    channel.request_shell(false).await?;
    // Failures are reported as events and leave the session up.
    // `connect_chain` accepted `hops`, so it has a target.
    let rules = hops
        .last()
        .map(|c| c.forwards.as_slice())
        .unwrap_or_default();
    for rule in rules.iter().filter(|r| auto_start(r)) {
        let _ = forwards.start(&chain.target, rule.clone()).await;
    }
    ctx.sink.event(SessionEvent::Status {
        stage: Stage::Ready,
    });
    Ok((chain, channel))
}

async fn io_loop(
    chain: Chain,
    channel: Channel<Msg>,
    sink: &dyn SessionSink,
    rx: &mut mpsc::Receiver<Command>,
    forwards: &mut Forwards,
) -> CloseReason {
    let (mut reader, writer) = channel.split();
    // Polled together: a write waiting for window space never stops output from draining.
    tokio::select! {
        r = read_loop(&mut reader, sink) => r,
        r = write_loop(&chain.target, &writer, rx, forwards) => r,
    }
}

async fn read_loop(reader: &mut ChannelReadHalf, sink: &dyn SessionSink) -> CloseReason {
    let mut exit = None;
    loop {
        match reader.wait().await {
            Some(ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. }) => {
                sink.data(data.to_vec());
            }
            Some(ChannelMsg::ExitStatus { exit_status }) => exit = Some(exit_status),
            // Eof only ends the output: servers may send exit-status after it, then Close.
            Some(ChannelMsg::Close) | None => {
                return match exit {
                    Some(code) => CloseReason::Exited { code },
                    None => error_reason(CoreError::Ssh(
                        "session ended without an exit status".into(),
                    )),
                };
            }
            Some(_) => {}
        }
    }
}

/// Runs until the user closes; write failures mean the channel is gone, which `read_loop` reports.
async fn write_loop(
    handle: &Arc<Handle<ClientHandler>>,
    writer: &ChannelWriteHalf<Msg>,
    rx: &mut mpsc::Receiver<Command>,
    forwards: &mut Forwards,
) -> CloseReason {
    loop {
        let sent = match rx.recv().await {
            Some(Command::Write(bytes)) => writer.data_bytes(bytes).await,
            Some(Command::Resize(cols, rows)) => {
                writer.window_change(cols.into(), rows.into(), 0, 0).await
            }
            Some(Command::ForwardStart(rule, reply)) => {
                let _ = reply.send(forwards.start(handle, rule).await);
                continue;
            }
            Some(Command::ForwardStop(forward_id, reply)) => {
                let _ = reply.send(forwards.stop(handle, forward_id).await);
                continue;
            }
            Some(Command::Close) | None => {
                let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
                return CloseReason::UserClosed;
            }
        };
        if let Err(e) = sent {
            log::debug!("channel write failed: {e}");
        }
    }
}
