//! The SFTP channel of a session: its own task, so a long transfer never slows the terminal.

pub(crate) mod names;
pub mod scan;

use std::sync::Arc;
use std::time::Duration;

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

use super::handler::ClientHandler;
use super::{PromptBroker, SessionId, SessionSink};
use crate::error::CoreError;
use names::{LOCAL_WINDOWS, valid_name};

/// At most this many names are read from one directory (spec §4.6).
const MAX_LISTING: usize = 100_000;
const POSIX_RENAME: &str = "posix-rename@openssh.com";

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

// All but `raw` are used by transfers (Tasks 4-6).
#[allow(dead_code)]
struct Server {
    raw: RawSftpSession,
    id: SessionId,
    sink: Arc<dyn SessionSink>,
    broker: Arc<PromptBroker>,
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
        open_timeout: Duration,
    ) -> Sftp {
        let (tx, mut rx) = mpsc::channel(16);
        let task = tokio::spawn(async move {
            let opened = tokio::time::timeout(open_timeout, open(&target, id, sink, broker)).await;
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

    pub(crate) async fn shutdown(self) {
        drop(self.tx);
        let _ = self.task.await;
    }
}

async fn open(
    target: &Handle<ClientHandler>,
    id: SessionId,
    sink: Arc<dyn SessionSink>,
    broker: Arc<PromptBroker>,
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

async fn serve(server: Server, mut rx: mpsc::Receiver<SftpCmd>) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            SftpCmd::List { dir, reply } => {
                let r = list(&server.raw, &dir).await;
                let lost = matches!(&r, Err(e) if lost(e));
                let _ = reply.send(r.map_err(|e| named(e, &dir)));
                if lost {
                    return refuse(&mut rx, closed).await;
                }
            }
            #[cfg(feature = "test-util")]
            SftpCmd::Scan { items, reply } => {
                let _ = reply.send(scan::scan_remote(&server.raw, &items).await);
            }
        }
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
