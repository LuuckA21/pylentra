//! The SFTP channel of a session: its own task, so a long transfer never slows the terminal.

pub(crate) mod names;

use std::sync::Arc;

use russh::ChannelMsg;
use russh::client::Handle;
use russh_sftp::client::RawSftpSession;
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::protocol::{File, StatusCode};
use serde::Serialize;
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
}

impl SftpCmd {
    pub(crate) fn fail(self, e: CoreError) {
        match self {
            Self::List { reply, .. } => {
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
    pub(crate) async fn open(
        target: Arc<Handle<ClientHandler>>,
        id: SessionId,
        sink: Arc<dyn SessionSink>,
        broker: Arc<PromptBroker>,
    ) -> Result<Sftp, CoreError> {
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
        let server = Server {
            posix_rename: version.extensions.contains_key(POSIX_RENAME),
            max_read: limits.as_ref().and_then(|l| nonzero(l.max_read_len)),
            max_write: limits.as_ref().and_then(|l| nonzero(l.max_write_len)),
            raw,
            id,
            sink,
            broker,
        };
        let (tx, rx) = mpsc::channel(16);
        let task = tokio::spawn(serve(server, rx));
        Ok(Sftp { tx, task })
    }

    /// A command the task can no longer take is dropped, so its caller sees no answer.
    pub(crate) async fn send(&self, cmd: SftpCmd) {
        let _ = self.tx.send(cmd).await;
    }

    pub(crate) async fn shutdown(self) {
        drop(self.tx);
        let _ = self.task.await;
    }
}

async fn serve(server: Server, mut rx: mpsc::Receiver<SftpCmd>) {
    while let Some(cmd) = rx.recv().await {
        match cmd {
            SftpCmd::List { dir, reply } => {
                let _ = reply.send(list(&server.raw, dir).await);
            }
        }
    }
}

/// `dir` empty is the home. Entries with names unsafe here are left out (spec §4.2).
async fn list(raw: &RawSftpSession, dir: String) -> Result<Listing, CoreError> {
    let asked = if dir.is_empty() { "." } else { dir.as_str() };
    let named = |e: SftpError| match CoreError::from(e) {
        CoreError::SftpPermissionDenied { .. } => {
            CoreError::SftpPermissionDenied { path: asked.into() }
        }
        CoreError::NotFound { .. } => CoreError::NotFound { what: asked.into() },
        other => other,
    };
    let dir = raw
        .realpath(asked)
        .await
        .map_err(named)?
        .files
        .into_iter()
        .next()
        .ok_or_else(|| CoreError::Ssh("empty realpath reply".into()))?
        .filename;
    let handle = raw.opendir(dir.as_str()).await.map_err(named)?.handle;
    let read = read_dir(raw, &handle).await;
    let _ = raw.close(handle).await;
    let (entries, truncated) = read.map_err(named)?;
    Ok(Listing {
        dir,
        entries,
        truncated,
    })
}

async fn read_dir(raw: &RawSftpSession, handle: &str) -> Result<(Vec<Entry>, bool), SftpError> {
    let mut entries = Vec::new();
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
                return Ok((entries, true));
            }
            seen += 1;
            if valid_name(&f.filename, LOCAL_WINDOWS) {
                entries.push(entry(f));
            }
        }
    }
    Ok((entries, false))
}

/// The type comes from `readdir`'s own attributes, which describe links, not their targets.
fn entry(f: File) -> Entry {
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
