//! The SFTP channel of a session: its own task, so a long transfer never slows the terminal.

mod copy;
pub(crate) mod names;
pub mod scan;
mod transfer;

use std::collections::{HashSet, VecDeque};
use std::future::{self, Future};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use russh::ChannelMsg;
use russh::client::Handle;
use russh_sftp::client::RawSftpSession;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::protocol::{File, StatusCode};
use serde::Serialize;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use ts_rs::TS;

use super::events::{SessionEvent, TransferState};
use super::handler::ClientHandler;
use super::session::CLOSE_TIMEOUT;
use super::{PromptBroker, SessionId, SessionSink};
use crate::error::CoreError;
use names::{LOCAL_WINDOWS, valid_name};

/// At most this many names are read from one directory (spec §4.6).
const MAX_LISTING: usize = 100_000;
const POSIX_RENAME: &str = "posix-rename@openssh.com";
const PROGRESS_EVERY: Duration = Duration::from_millis(200);

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub enum EntryKind {
    File,
    Dir,
    Link,
    Other,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, TS)]
#[ts(export)]
pub enum Direction {
    Up,
    Down,
}

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    #[ts(type = "number | null")]
    pub size: Option<u64>,
    /// Unix seconds.
    #[ts(type = "number | null")]
    pub modified: Option<u64>,
}

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct Listing {
    pub dir: String,
    pub entries: Vec<Entry>,
    pub truncated: bool,
}

/// Files dropped on the window; the paths stay in the backend, keyed by `drop_id`.
#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct FilesDropped {
    #[ts(type = "number")]
    pub drop_id: u64,
    pub names: Vec<String>,
    pub x: f64,
    pub y: f64,
}

pub(crate) enum SftpCmd {
    List {
        dir: String,
        reply: oneshot::Sender<Result<Listing, CoreError>>,
    },
    /// Queues a download of remote `items` into `local_dir`; replies with its transfer id.
    Download {
        items: Vec<String>,
        local_dir: PathBuf,
        reply: oneshot::Sender<Result<u32, CoreError>>,
    },
    /// Queues an upload of local `items` into `remote_dir`; replies with its transfer id.
    Upload {
        items: Vec<PathBuf>,
        remote_dir: String,
        reply: oneshot::Sender<Result<u32, CoreError>>,
    },
    /// Drops a queued transfer, or stops the running one.
    Cancel {
        transfer_id: u32,
        reply: oneshot::Sender<Result<(), CoreError>>,
    },
    /// Test hook: runs `scan_remote` on the open channel.
    #[cfg(feature = "test-util")]
    Scan {
        items: Vec<String>,
        reply: oneshot::Sender<Result<scan::Plan, CoreError>>,
    },
}

impl SftpCmd {
    pub(crate) fn fail(self, e: CoreError) {
        match self {
            Self::List { reply, .. } => {
                let _ = reply.send(Err(e));
            }
            Self::Download { reply, .. } | Self::Upload { reply, .. } => {
                let _ = reply.send(Err(e));
            }
            Self::Cancel { reply, .. } => {
                let _ = reply.send(Err(e));
            }
            #[cfg(feature = "test-util")]
            Self::Scan { reply, .. } => {
                let _ = reply.send(Err(e));
            }
        }
    }
}

/// The open SFTP channel: dropping the queue ends the task, which closes the channel.
pub(crate) struct Sftp {
    tx: mpsc::Sender<SftpCmd>,
    task: Task,
}

/// Aborted when dropped, so the task (and the connection it holds) never outlives its
/// session, even when the session task itself is aborted.
struct Task(JoinHandle<()>);

impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Below the session's own close deadline, so cleanup ends before the session is aborted.
const SHUTDOWN_BUDGET: Duration = CLOSE_TIMEOUT.saturating_sub(Duration::from_secs(1));

struct Server {
    raw: RawSftpSession,
    id: SessionId,
    sink: Arc<dyn SessionSink>,
    broker: Arc<PromptBroker>,
    /// Per session, so ids stay unique when a lost channel is reopened.
    transfer_ids: Arc<AtomicU32>,
    posix_rename: bool,
    /// From `limits@openssh.com`; `None` when not advertised or unlimited.
    max_read: Option<u64>,
    max_write: Option<u64>,
}

impl Sftp {
    /// Returns at once: the task opens the channel itself, within `open_timeout`, and keeps
    /// the commands sent meanwhile. A failed open answers them all and ends the task.
    pub(crate) fn start(
        target: Arc<Handle<ClientHandler>>,
        id: SessionId,
        sink: Arc<dyn SessionSink>,
        broker: Arc<PromptBroker>,
        transfer_ids: Arc<AtomicU32>,
        open_timeout: Duration,
    ) -> Sftp {
        let (tx, mut rx) = mpsc::channel(16);
        let task = tokio::spawn(async move {
            let opened = open(&target, id, sink, broker, transfer_ids);
            let opened = tokio::time::timeout(open_timeout, opened).await;
            match opened.unwrap_or(Err(CoreError::SftpUnavailable)) {
                Ok(server) => serve(server, rx).await,
                Err(e) => refuse(&mut rx, || again(&e)).await,
            }
        });
        Sftp {
            tx,
            task: Task(task),
        }
    }

    /// The task is gone (failed open, lost channel): the next command needs a new one.
    pub(crate) fn ended(&self) -> bool {
        self.tx.is_closed()
    }

    /// Never waits, so the session task never stalls behind SFTP work.
    pub(crate) fn send(&self, cmd: SftpCmd) {
        match self.tx.try_send(cmd) {
            Ok(()) => {}
            Err(TrySendError::Full(cmd)) => cmd.fail(CoreError::Ssh("sftp busy".into())),
            Err(TrySendError::Closed(cmd)) => cmd.fail(closed()),
        }
    }

    /// Stops the running transfer, which removes its temp file; queued ones are dropped.
    /// Aborted once `SHUTDOWN_BUDGET` is spent: a cancel wakes every wait except a remote temp
    /// create and a `replace`, never cut short, so only those or a hung local disk reach it.
    pub(crate) async fn shutdown(self) {
        let Sftp { tx, mut task } = self;
        drop(tx);
        let _ = tokio::time::timeout(SHUTDOWN_BUDGET, &mut task.0).await;
    }
}

async fn open(
    target: &Handle<ClientHandler>,
    id: SessionId,
    sink: Arc<dyn SessionSink>,
    broker: Arc<PromptBroker>,
    transfer_ids: Arc<AtomicU32>,
) -> Result<Server, CoreError> {
    let mut channel = target.channel_open_session().await?;
    channel.request_subsystem(true, "sftp").await?;
    loop {
        match channel.wait().await {
            Some(ChannelMsg::Success) => break,
            Some(ChannelMsg::Failure | ChannelMsg::Close) | None => {
                return Err(CoreError::SftpUnavailable);
            }
            Some(_) => {}
        }
    }
    let raw = RawSftpSession::new(channel.into_stream());
    let version = raw.init().await?;
    // A server that advertises limits but cannot state them gets the defaults.
    let limits = if version
        .extensions
        .contains_key(russh_sftp::extensions::LIMITS)
    {
        raw.limits().await.ok()
    } else {
        None
    };
    // 0 means "no limit" in the extension.
    let nonzero = |n: u64| (n > 0).then_some(n);
    Ok(Server {
        posix_rename: version.extensions.contains_key(POSIX_RENAME),
        max_read: limits.as_ref().and_then(|l| nonzero(l.max_read_len)),
        max_write: limits.as_ref().and_then(|l| nonzero(l.max_write_len)),
        raw,
        id,
        sink,
        broker,
        transfer_ids,
    })
}

fn closed() -> CoreError {
    CoreError::Ssh("sftp channel closed".into())
}

/// `CoreError` is not `Clone` (it can hold an `io::Error`), so each queued command gets a copy.
fn again(e: &CoreError) -> CoreError {
    match e {
        CoreError::SftpUnavailable => CoreError::SftpUnavailable,
        CoreError::Ssh(m) => CoreError::Ssh(m.clone()),
        other => CoreError::Ssh(other.to_string()),
    }
}

/// Closed first, so `ended` is true before any queued command is answered.
async fn refuse(rx: &mut mpsc::Receiver<SftpCmd>, err: impl Fn() -> CoreError) {
    rx.close();
    while let Some(cmd) = rx.recv().await {
        cmd.fail(err());
    }
}

/// A failed step of a transfer, and whether it lost the channel: only that ends the queue
/// (spec §3 step 5), which `CoreError` alone cannot tell (status replies also become `Ssh`).
pub(crate) struct StepError {
    pub(crate) error: CoreError,
    pub(crate) lost: bool,
}

impl From<CoreError> for StepError {
    fn from(error: CoreError) -> Self {
        Self { error, lost: false }
    }
}

impl From<std::io::Error> for StepError {
    fn from(e: std::io::Error) -> Self {
        CoreError::Io(e).into()
    }
}

/// `e` with `path` filled in, judged before the conversion loses the difference.
fn step_error(e: SftpError, path: &str) -> StepError {
    StepError {
        lost: lost(&e),
        error: named(e, path),
    }
}

/// `io`, unless the transfer is cancelled first. Polled once before the cancel is checked,
/// so a request (a close, say) still goes out even when cancelled already.
async fn or_cancel<T>(
    cancel: &watch::Receiver<bool>,
    io: impl Future<Output = Result<T, StepError>>,
) -> Result<T, StepError> {
    let mut cancel = cancel.clone();
    tokio::select! {
        biased;
        r = io => r,
        // A dropped sender (the queue gone) cancels too.
        _ = cancel.wait_for(|c| *c) => Err(CoreError::Cancelled.into()),
    }
}

struct Job {
    id: u32,
    side: transfer::Side,
}

/// The transfer in progress: ends `true` when it lost the channel.
type Running<'a> = (
    u32,
    watch::Sender<bool>,
    Pin<Box<dyn Future<Output = bool> + Send + 'a>>,
);

/// Commands and the running transfer share one `select!`, so a listing never waits on a copy.
async fn serve(server: Server, mut rx: mpsc::Receiver<SftpCmd>) {
    let mut queue: VecDeque<Job> = VecDeque::new();
    let mut running: Option<Running> = None;
    // Once lost, the remaining commands are refused and nothing new starts.
    let mut gone = false;
    loop {
        if running.is_none()
            && !gone
            && let Some(job) = queue.pop_front()
        {
            let (cancel, cancelled) = watch::channel(false);
            let id = job.id;
            let t = Box::pin(transfer::run(&server, job, cancelled));
            running = Some((id, cancel, t));
        }
        let ended = async {
            match &mut running {
                Some((_, _, t)) => t.await,
                None => future::pending().await,
            }
        };
        tokio::select! {
            lost = ended => {
                running = None;
                if lost {
                    gone = true;
                    rx.close();
                }
            }
            cmd = rx.recv() => {
                let Some(cmd) = cmd else { break };
                if gone {
                    cmd.fail(closed());
                    continue;
                }
                let lost = match cmd {
                    SftpCmd::List { dir, reply } => {
                        // shortcut: a listing in flight delays cancel and close handling up to
                        // the SFTP request timeout; upgrade by serving List concurrently.
                        let r = list(&server.raw, &dir).await;
                        let lost = matches!(&r, Err(e) if lost(e));
                        let _ = reply.send(r.map_err(|e| named(e, &dir)));
                        lost
                    }
                    SftpCmd::Download { items, local_dir, reply } => {
                        let side = transfer::Side::Down { items, local_dir };
                        let _ = reply.send(Ok(enqueue(&server, &mut queue, side)));
                        false
                    }
                    SftpCmd::Upload { items, remote_dir, reply } => {
                        let side = transfer::Side::Up { items, remote_dir };
                        let _ = reply.send(Ok(enqueue(&server, &mut queue, side)));
                        false
                    }
                    SftpCmd::Cancel { transfer_id, reply } => {
                        let _ = reply.send(cancel(&server, &mut queue, &running, transfer_id));
                        false
                    }
                    #[cfg(feature = "test-util")]
                    SftpCmd::Scan { items, reply } => {
                        let r = scan::scan_remote(&server.raw, &items).await;
                        let lost = matches!(&r, Err(e) if e.lost);
                        let _ = reply.send(r.map_err(|e| e.error));
                        lost
                    }
                };
                if lost {
                    gone = true;
                    rx.close();
                }
            }
        }
    }
    // The queue is closed: a lost channel fails what waits, a shutdown drops it.
    // Stopped even on a lost channel, so it never asks over the next channel's question.
    if let Some((_, cancel, t)) = running {
        cancel.send_replace(true);
        server.broker.cancel(server.id);
        t.await;
    }
    if gone {
        for job in queue {
            server.emit(job.id, transfer::failed(&closed(), 0, vec![]));
        }
    }
}

fn enqueue(server: &Server, queue: &mut VecDeque<Job>, side: transfer::Side) -> u32 {
    let id = server.transfer_ids.fetch_add(1, Ordering::Relaxed);
    server.emit(
        id,
        TransferState::Queued {
            direction: side.direction(),
            name: side.label(),
        },
    );
    queue.push_back(Job { id, side });
    id
}

fn cancel(
    server: &Server,
    queue: &mut VecDeque<Job>,
    running: &Option<Running>,
    transfer_id: u32,
) -> Result<(), CoreError> {
    if let Some(i) = queue.iter().position(|j| j.id == transfer_id) {
        queue.remove(i);
        server.emit(transfer_id, TransferState::Cancelled { files: 0 });
    } else if let Some((_, cancel, _)) = running.as_ref().filter(|r| r.0 == transfer_id) {
        cancel.send_replace(true);
        // After `Ready` the only prompt a session asks is this transfer's conflict question.
        server.broker.cancel(server.id);
    } else {
        return Err(CoreError::NotFound {
            what: format!("transfer {transfer_id}"),
        });
    }
    Ok(())
}

impl Server {
    fn emit(&self, transfer_id: u32, state: TransferState) {
        self.sink
            .event(SessionEvent::Transfer { transfer_id, state });
    }
}

/// Not a status reply: the channel is gone or the server broke the protocol.
fn lost(e: &SftpError) -> bool {
    !matches!(e, SftpError::Status(_) | SftpError::Limited(_))
}

fn home(dir: &str) -> &str {
    if dir.is_empty() { "." } else { dir }
}

/// The error with the path asked for, where the UI can show it.
fn named(e: SftpError, dir: &str) -> CoreError {
    match CoreError::from(e) {
        CoreError::SftpPermissionDenied { .. } => CoreError::SftpPermissionDenied {
            path: home(dir).into(),
        },
        CoreError::NotFound { .. } => CoreError::NotFound {
            what: home(dir).into(),
        },
        other => other,
    }
}

/// `dir` empty is the home. Entries with names unsafe here are left out (spec §4.2).
async fn list(raw: &RawSftpSession, dir: &str) -> Result<Listing, SftpError> {
    let dir = raw
        .realpath(home(dir))
        .await?
        .files
        .into_iter()
        .next()
        .ok_or_else(|| SftpError::UnexpectedBehavior("empty realpath reply".into()))?
        .filename;
    let handle = raw.opendir(dir.as_str()).await?.handle;
    let read = read_dir(raw, &handle, false).await;
    let _ = raw.close(handle).await;
    let read = read?;
    Ok(Listing {
        dir,
        entries: read.entries,
        truncated: read.truncated,
    })
}

/// What one directory holds: usable entries, names unsafe here (when asked for), and whether
/// it was cut short.
struct Dir {
    entries: Vec<Entry>,
    invalid: Vec<String>,
    truncated: bool,
}

async fn read_dir(
    raw: &RawSftpSession,
    handle: &str,
    keep_invalid: bool,
) -> Result<Dir, SftpError> {
    let mut dir = Dir {
        entries: Vec::new(),
        invalid: Vec::new(),
        truncated: false,
    };
    // Every name counts, valid or not, so a hostile server cannot keep this loop going.
    let mut seen = 0;
    let mut names = HashSet::new();
    loop {
        let files = match raw.readdir(handle).await {
            Ok(name) => name.files,
            Err(SftpError::Status(s)) if s.status_code == StatusCode::Eof => break,
            Err(e) => return Err(e),
        };
        // An empty batch would repeat forever.
        if files.is_empty() {
            break;
        }
        for f in files {
            if seen == MAX_LISTING {
                dir.truncated = true;
                return Ok(dir);
            }
            seen += 1;
            // A repeated name would break the UI's keyed list and plan one file twice.
            if matches!(f.filename.as_str(), "." | "..") || !names.insert(f.filename.clone()) {
                continue;
            }
            if valid_name(&f.filename, LOCAL_WINDOWS) {
                dir.entries.push(entry(f));
            } else if keep_invalid {
                dir.invalid.push(f.filename);
            }
        }
    }
    Ok(dir)
}

/// The type comes from `readdir`'s own attributes, which describe links, not their targets.
pub(crate) fn entry(f: File) -> Entry {
    let kind = match f.attrs.permissions.map(|p| p & 0o170_000) {
        Some(0o040_000) => EntryKind::Dir,
        Some(0o100_000) => EntryKind::File,
        Some(0o120_000) => EntryKind::Link,
        _ => EntryKind::Other,
    };
    Entry {
        name: f.filename,
        kind,
        size: f.attrs.size,
        modified: f.attrs.mtime.map(u64::from),
    }
}
