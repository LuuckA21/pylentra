use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use russh::client::{self, Handle, Msg};
use russh::{Channel, ChannelMsg, ChannelReadHalf, ChannelWriteHalf, Disconnect};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use uuid::Uuid;

use super::auth::{AuthCtx, authenticate};
use super::events::{CloseReason, PromptAnswer, SessionEvent, Stage};
use super::handler::ClientHandler;
use super::{PromptBroker, SessionId, SessionSink};
use crate::error::CoreError;
use crate::known_hosts::KnownHosts;
use crate::model::Connection;
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
    let reason = drive(&ctx, &chain, cols, rows, &mut rx).await;
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
) -> CloseReason {
    let Some(target) = chain.last() else {
        return error_reason(CoreError::InvalidInput {
            field: "chain".into(),
            reason: "empty".into(),
        });
    };
    ctx.sink.event(SessionEvent::Status {
        stage: Stage::Connecting,
    });
    let setup = open_shell(ctx, target, cols, rows);
    tokio::pin!(setup);
    let mut new_size = None;
    let (handle, channel) = loop {
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
            },
        }
    };
    if let Some((c, r)) = new_size {
        let _ = channel.window_change(c.into(), r.into(), 0, 0).await;
    }
    io_loop(handle, channel, &*ctx.sink, rx).await
}

async fn open_shell(
    ctx: &Ctx,
    target: &Connection,
    cols: u16,
    rows: u16,
) -> Result<(Handle<ClientHandler>, Channel<Msg>), CoreError> {
    let mut handle = connect(ctx, target).await?;
    ctx.sink.event(SessionEvent::Status {
        stage: Stage::Authenticating,
    });
    let auth = AuthCtx {
        secrets: &*ctx.secrets,
        prompts: &ctx.broker,
        session: ctx.id,
        sink: &*ctx.sink,
        opts: &ctx.opts,
    };
    authenticate(&mut handle, target, &auth).await?;
    let channel = handle.channel_open_session().await?;
    channel
        .request_pty(false, TERM, cols.into(), rows.into(), 0, 0, &[])
        .await?;
    channel.request_shell(false).await?;
    ctx.sink.event(SessionEvent::Status {
        stage: Stage::Ready,
    });
    Ok((handle, channel))
}

/// TCP plus SSH handshake (host key included), bounded by `connect_timeout`.
async fn connect(ctx: &Ctx, target: &Connection) -> Result<Handle<ClientHandler>, CoreError> {
    let host = target.host.to_lowercase();
    let port = target.port;
    let (prompting, paused) = watch::channel(false);
    let handler = ClientHandler {
        id: ctx.id,
        host: host.clone(),
        port,
        known_hosts: ctx.known_hosts.clone(),
        broker: ctx.broker.clone(),
        sink: ctx.sink.clone(),
        prompting,
    };
    let config = Arc::new(client::Config {
        keepalive_interval: Some(ctx.opts.keepalive_interval),
        keepalive_max: ctx.opts.keepalive_max,
        ..Default::default()
    });
    let attempt = async {
        let tcp = tcp_connect(&host, port).await?;
        client::connect_stream(config, tcp, handler).await
    };
    match timeout_unless_paused(ctx.opts.connect_timeout, paused, attempt).await {
        Some(r) => r,
        None => Err(CoreError::Timeout { host, port }),
    }
}

async fn tcp_connect(host: &str, port: u16) -> Result<TcpStream, CoreError> {
    let dns = || CoreError::Dns { host: host.into() };
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| dns())?
        .collect();
    if addrs.is_empty() {
        return Err(dns());
    }
    let tcp = TcpStream::connect(&addrs[..])
        .await
        .map_err(|e| match e.kind() {
            io::ErrorKind::ConnectionRefused => CoreError::Refused {
                host: host.into(),
                port,
            },
            _ => CoreError::Unreachable {
                host: host.into(),
                port,
                reason: e.to_string(),
            },
        })?;
    // Keystrokes should not wait for Nagle.
    let _ = tcp.set_nodelay(true);
    Ok(tcp)
}

/// `tokio::time::timeout`, except that the clock stops while `paused` is true
/// (the user is reading a prompt) and restarts in full afterwards.
async fn timeout_unless_paused<F: Future>(
    limit: Duration,
    mut paused: watch::Receiver<bool>,
    fut: F,
) -> Option<F::Output> {
    tokio::pin!(fut);
    let sleep = tokio::time::sleep(limit);
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            out = &mut fut => return Some(out),
            () = &mut sleep, if !*paused.borrow() => return None,
            Ok(()) = paused.changed() => {
                if !*paused.borrow() {
                    sleep.as_mut().reset(Instant::now() + limit);
                }
            }
        }
    }
}

async fn io_loop(
    handle: Handle<ClientHandler>,
    channel: Channel<Msg>,
    sink: &dyn SessionSink,
    rx: &mut mpsc::Receiver<Command>,
) -> CloseReason {
    let (mut reader, writer) = channel.split();
    // Polled together: a write waiting for window space never stops output from draining.
    tokio::select! {
        r = read_loop(&mut reader, sink) => r,
        r = write_loop(&handle, &writer, rx) => r,
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
    handle: &Handle<ClientHandler>,
    writer: &ChannelWriteHalf<Msg>,
    rx: &mut mpsc::Receiver<Command>,
) -> CloseReason {
    loop {
        let sent = match rx.recv().await {
            Some(Command::Write(bytes)) => writer.data_bytes(bytes).await,
            Some(Command::Resize(cols, rows)) => {
                writer.window_change(cols.into(), rows.into(), 0, 0).await
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
