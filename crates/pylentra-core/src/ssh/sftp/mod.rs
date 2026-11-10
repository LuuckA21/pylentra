//! The SFTP channel of a session: its own task, so a long transfer never slows the terminal.

mod copy;
pub(crate) mod names;
pub mod scan;

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::future::{self, Future};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use russh::ChannelMsg;
use russh::client::Handle;
use russh_sftp::client::RawSftpSession;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::protocol::{File, StatusCode};
use serde::Serialize;
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use ts_rs::TS;

use super::events::{
    ConflictChoice, PromptAnswer, SessionEvent, SkipReason, Skipped, TransferState,
};
use super::handler::ClientHandler;
use super::session::CLOSE_TIMEOUT;
use super::{PromptBroker, SessionId, SessionSink};
use crate::error::CoreError;
use names::{LOCAL_WINDOWS, remote_join, remote_split, valid_name};

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
            Self::Download { reply, .. } => {
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
    task: JoinHandle<()>,
}

struct Server {
    raw: RawSftpSession,
    id: SessionId,
    sink: Arc<dyn SessionSink>,
    broker: Arc<PromptBroker>,
    /// Per session, so ids stay unique when a lost channel is reopened.
    transfer_ids: Arc<AtomicU32>,
    #[allow(dead_code)] // Used by uploads (Task 6).
    posix_rename: bool,
    /// From `limits@openssh.com`; `None` when not advertised or unlimited.
    max_read: Option<u64>,
    #[allow(dead_code)] // Used by uploads (Task 6).
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
        Sftp { tx, task }
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
    pub(crate) async fn shutdown(self) {
        drop(self.tx);
        let mut task = self.task;
        if tokio::time::timeout(CLOSE_TIMEOUT, &mut task)
            .await
            .is_err()
        {
            task.abort();
        }
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

struct Job {
    id: u32,
    items: Vec<String>,
    local_dir: PathBuf,
}

/// The transfer in progress: ends `true` when it lost the channel.
type Running<'a> = (
    u32,
    Arc<AtomicBool>,
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
            let cancel = Arc::new(AtomicBool::new(false));
            let id = job.id;
            running = Some((id, cancel.clone(), Box::pin(download(&server, job, cancel))));
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
                        let r = list(&server.raw, &dir).await;
                        let lost = matches!(&r, Err(e) if lost(e));
                        let _ = reply.send(r.map_err(|e| named(e, &dir)));
                        lost
                    }
                    SftpCmd::Download { items, local_dir, reply } => {
                        let id = server.transfer_ids.fetch_add(1, Ordering::Relaxed);
                        let names: Vec<_> = items.iter().map(|i| remote_split(i).1).collect();
                        server.emit(id, TransferState::Queued {
                            direction: Direction::Down,
                            name: names.join(", "),
                        });
                        queue.push_back(Job { id, items, local_dir });
                        let _ = reply.send(Ok(id));
                        false
                    }
                    SftpCmd::Cancel { transfer_id, reply } => {
                        let _ = reply.send(cancel(&server, &mut queue, &running, transfer_id));
                        false
                    }
                    #[cfg(feature = "test-util")]
                    SftpCmd::Scan { items, reply } => {
                        let r = scan::scan_remote(&server.raw, &items).await;
                        let lost = matches!(&r, Err(e) if channel_error(e));
                        let _ = reply.send(r);
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
    if let Some((_, flag, t)) = running {
        if !gone {
            flag.store(true, Ordering::Relaxed);
            server.broker.cancel(server.id);
        }
        t.await;
    }
    if gone {
        for job in queue {
            server.emit(job.id, failed(&closed(), 0, vec![]));
        }
    }
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
    } else if let Some((_, flag, _)) = running.as_ref().filter(|r| r.0 == transfer_id) {
        flag.store(true, Ordering::Relaxed);
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

// shortcut: status replies other than "no such file" and "permission denied" also become
// `Ssh`, so they end the queue as if the channel were lost; upgrade if real servers send them.
fn channel_error(e: &CoreError) -> bool {
    matches!(e, CoreError::Ssh(_))
}

fn failed(e: &CoreError, files: u32, skipped: Vec<Skipped>) -> TransferState {
    TransferState::Failed {
        error: e.to_api(),
        files,
        skipped,
    }
}

fn error_skip(path: &str, e: &CoreError) -> Skipped {
    Skipped {
        path: path.into(),
        reason: SkipReason::Error { error: e.to_api() },
    }
}

/// `rel` (`/`-separated, every name validated by the scan) under `dir`.
fn local_path(dir: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(dir.to_path_buf(), |p, n| p.join(n))
}

fn under(rel: &str, dir: &str) -> bool {
    rel.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
}

/// Runs one download to its last event; `true` when the channel was lost.
async fn download(server: &Server, job: Job, cancel: Arc<AtomicBool>) -> bool {
    let id = job.id;
    let plan = match scan::scan_remote(&server.raw, &job.items).await {
        Ok(p) => p,
        Err(e) => {
            server.emit(id, failed(&e, 0, vec![]));
            return channel_error(&e);
        }
    };
    let mut skipped = plan.skipped;
    // Folders that are links or files locally: skipped with all they hold (spec §4.4).
    let mut skipped_dirs: Vec<&str> = vec![];
    for rel in &plan.dirs {
        if skipped_dirs.iter().any(|d| under(rel, d)) {
            continue;
        }
        let path = local_path(&job.local_dir, rel);
        let reason = match fs::create_dir(&path) {
            Ok(()) => continue,
            Err(e) if e.kind() == ErrorKind::AlreadyExists => match fs::symlink_metadata(&path) {
                Ok(m) if m.is_dir() => continue,
                Ok(m) if m.file_type().is_symlink() => SkipReason::Symlink,
                Ok(_) => SkipReason::Exists,
                Err(e) => error_skip(rel, &e.into()).reason,
            },
            Err(e) => error_skip(rel, &e.into()).reason,
        };
        skipped.push(Skipped {
            path: rel.clone(),
            reason,
        });
        skipped_dirs.push(rel);
    }
    // Item name to its remote parent; two items of one name land on one local path anyway.
    let parents: HashMap<_, _> = job
        .items
        .iter()
        .map(|i| {
            let (parent, name) = remote_split(i);
            (name, parent)
        })
        .collect();
    let (mut files, mut done) = (0, 0);
    let mut every: Option<ConflictChoice> = None;
    let mut last: Option<Instant> = None;
    let running = |done_bytes, files_done, current: &str| TransferState::Running {
        done_bytes,
        total_bytes: Some(plan.total_bytes),
        files_done,
        current: current.into(),
    };
    for f in &plan.files {
        if skipped_dirs.iter().any(|d| under(&f.rel, d)) {
            done += f.size;
            continue;
        }
        if cancel.load(Ordering::Relaxed) {
            server.emit(id, TransferState::Cancelled { files });
            return false;
        }
        let local = local_path(&job.local_dir, &f.rel);
        if let Ok(m) = fs::symlink_metadata(&local) {
            let choice = if m.file_type().is_symlink() {
                ConflictChoice::Skip
            } else if let Some(c) = every {
                c
            } else {
                let ask = TransferState::Conflict {
                    name: f.rel.clone(),
                };
                let ask = SessionEvent::Transfer {
                    transfer_id: id,
                    state: ask,
                };
                match server.broker.ask(server.id, &*server.sink, ask).await {
                    PromptAnswer::Conflict { choice, all } => {
                        every = all.then_some(choice);
                        choice
                    }
                    _ => ConflictChoice::Cancel,
                }
            };
            match choice {
                ConflictChoice::Overwrite => {}
                ConflictChoice::Skip => {
                    let reason = if m.file_type().is_symlink() {
                        SkipReason::Symlink
                    } else {
                        SkipReason::Exists
                    };
                    skipped.push(Skipped {
                        path: f.rel.clone(),
                        reason,
                    });
                    done += f.size;
                    continue;
                }
                ConflictChoice::Cancel => {
                    server.emit(id, TransferState::Cancelled { files });
                    return false;
                }
            }
        }
        let first = f.rel.split('/').next().unwrap_or_default();
        let remote = remote_join(parents.get(first).copied().unwrap_or("."), &f.rel);
        let mut progress = |n: u64| {
            if last.is_none_or(|t| t.elapsed() >= PROGRESS_EVERY) {
                last = Some(Instant::now());
                server.emit(id, running(done + n, files, &f.rel));
            }
        };
        let r = copy::download_file(
            server,
            &remote,
            &local,
            f.size,
            f.modified,
            &cancel,
            &mut progress,
        )
        .await;
        done += f.size;
        match r {
            Ok(()) => files += 1,
            Err(CoreError::Cancelled) => {
                server.emit(id, TransferState::Cancelled { files });
                return false;
            }
            Err(e) if channel_error(&e) => {
                server.emit(id, failed(&e, files, skipped));
                return true;
            }
            Err(e) => skipped.push(error_skip(&f.rel, &e)),
        }
        last = Some(Instant::now());
        server.emit(id, running(done, files, &f.rel));
    }
    server.emit(id, TransferState::Finished { files, skipped });
    false
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
    let read = read_dir(raw, &handle).await;
    let _ = raw.close(handle).await;
    let read = read?;
    Ok(Listing {
        dir,
        entries: read.entries,
        truncated: read.truncated,
    })
}

/// What one directory holds: usable entries, names unsafe here, and whether it was cut short.
struct Dir {
    entries: Vec<Entry>,
    invalid: Vec<String>,
    truncated: bool,
}

async fn read_dir(raw: &RawSftpSession, handle: &str) -> Result<Dir, SftpError> {
    let mut dir = Dir {
        entries: Vec::new(),
        invalid: Vec::new(),
        truncated: false,
    };
    // Every name counts, valid or not, so a hostile server cannot keep this loop going.
    let mut seen = 0;
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
            if matches!(f.filename.as_str(), "." | "..") {
                continue;
            }
            if valid_name(&f.filename, LOCAL_WINDOWS) {
                dir.entries.push(entry(f));
            } else {
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
