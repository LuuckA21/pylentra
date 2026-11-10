//! One transfer from scan to its last event (spec §3 "Trasferimento"); the queue is in `mod.rs`.
//! Both directions share this flow; `Side` holds what differs: where the scan reads, how a
//! folder is made, how a destination is checked, and which way a file is copied.

use std::collections::HashMap;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::Instant;

use russh_sftp::client::error::Error as SftpError;
use russh_sftp::protocol::{File, FileAttributes, StatusCode};
use tokio::sync::watch;

use super::names::{remote_join, remote_split};
use super::scan::{Plan, PlannedFile};
use super::{
    Direction, EntryKind, Job, PROGRESS_EVERY, Server, StepError, copy, entry, lost, named,
    or_cancel, scan, step_error,
};
use crate::error::CoreError;
use crate::ssh::events::{
    ConflictChoice, PromptAnswer, SessionEvent, SkipReason, Skipped, TransferState,
};

pub(super) fn failed(e: &CoreError, files: u32, skipped: Vec<Skipped>) -> TransferState {
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

/// The items of a transfer and where they go.
pub(super) enum Side {
    /// Remote absolute paths into a local folder.
    Down {
        items: Vec<String>,
        local_dir: PathBuf,
    },
    /// Local paths into a remote folder.
    Up {
        items: Vec<PathBuf>,
        remote_dir: String,
    },
}

impl Side {
    pub(super) fn direction(&self) -> Direction {
        match self {
            Side::Down { .. } => Direction::Down,
            Side::Up { .. } => Direction::Up,
        }
    }

    /// The items' own names, for the `Queued` event.
    pub(super) fn label(&self) -> String {
        let names: Vec<_> = match self {
            Side::Down { items, .. } => items.iter().map(|i| remote_split(i).1.into()).collect(),
            Side::Up { items, .. } => items
                .iter()
                .map(|i| i.file_name().unwrap_or_default().to_string_lossy())
                .collect(),
        };
        names.join(", ")
    }

    /// Item name to item index; a local name that is not UTF-8 is skipped by the scan anyway.
    fn index(&self) -> HashMap<&str, usize> {
        let names: Vec<Option<&str>> = match self {
            Side::Down { items, .. } => items.iter().map(|i| Some(remote_split(i).1)).collect(),
            Side::Up { items, .. } => items
                .iter()
                .map(|i| i.file_name().and_then(|n| n.to_str()))
                .collect(),
        };
        // Two items of one name land on one destination anyway.
        names
            .into_iter()
            .enumerate()
            .filter_map(|(i, n)| Some((n?, i)))
            .collect()
    }

    async fn scan(&self, server: &Server) -> Result<Plan, StepError> {
        match self {
            Side::Down { items, .. } => scan::scan_remote(&server.raw, items).await,
            Side::Up { items, .. } => {
                // A walk of the local disk, off the runtime.
                let items = items.clone();
                let plan = tokio::task::spawn_blocking(move || scan::scan_local(&items));
                Ok(plan.await.map_err(io::Error::other)??)
            }
        }
    }

    /// Makes the folder `rel` unless it is already a real one; `Some` skips it with all it
    /// holds (spec §3 step 2, §4.4).
    async fn mkdir(&self, server: &Server, rel: &str) -> Result<Option<SkipReason>, StepError> {
        let reason = match self {
            Side::Down { local_dir, .. } => {
                let path = local_path(local_dir, rel);
                match fs::create_dir(&path) {
                    Ok(()) => return Ok(None),
                    Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                        match fs::symlink_metadata(&path) {
                            Ok(m) if m.is_dir() => return Ok(None),
                            Ok(m) if m.file_type().is_symlink() => SkipReason::Symlink,
                            Ok(_) => SkipReason::Exists,
                            Err(e) => error_skip(rel, &e.into()).reason,
                        }
                    }
                    Err(e) => error_skip(rel, &e.into()).reason,
                }
            }
            Side::Up { remote_dir, .. } => {
                let path = remote_join(remote_dir, rel);
                let made = server.raw.mkdir(path.as_str(), FileAttributes::default());
                let e = match made.await {
                    Ok(_) => return Ok(None),
                    Err(e) if lost(&e) => return Err(step_error(e, &path)),
                    Err(e) => e,
                };
                // SFTPv3 has no "exists" status, so a failed mkdir is looked at.
                match self.existing(server, rel).await? {
                    Some(EntryKind::Dir) => return Ok(None),
                    Some(EntryKind::Link) => SkipReason::Symlink,
                    Some(_) => SkipReason::Exists,
                    None => error_skip(rel, &named(e, &path)).reason,
                }
            }
        };
        Ok(Some(reason))
    }

    /// What is at the destination of `rel`, without following a link.
    async fn existing(&self, server: &Server, rel: &str) -> Result<Option<EntryKind>, StepError> {
        match self {
            Side::Down { local_dir, .. } => {
                let m = fs::symlink_metadata(local_path(local_dir, rel));
                Ok(m.ok().map(|m| {
                    let t = m.file_type();
                    if t.is_symlink() {
                        EntryKind::Link
                    } else if t.is_dir() {
                        EntryKind::Dir
                    } else {
                        EntryKind::File
                    }
                }))
            }
            Side::Up { remote_dir, .. } => {
                let path = remote_join(remote_dir, rel);
                match server.raw.lstat(path.as_str()).await {
                    Ok(a) => Ok(Some(entry(File::new("", a.attrs)).kind)),
                    Err(SftpError::Status(s)) if s.status_code == StatusCode::NoSuchFile => {
                        Ok(None)
                    }
                    Err(e) => Err(step_error(e, &path)),
                }
            }
        }
    }

    async fn copy(
        &self,
        server: &Server,
        item: usize,
        f: &PlannedFile,
        cancel: &watch::Receiver<bool>,
        progress: &mut impl FnMut(u64),
    ) -> Result<(), StepError> {
        match self {
            Side::Down { items, local_dir } => {
                let remote = remote_join(remote_split(&items[item]).0, &f.rel);
                let local = local_path(local_dir, &f.rel);
                let (size, modified) = (f.size, f.modified);
                copy::download_file(server, &remote, &local, size, modified, cancel, progress).await
            }
            Side::Up { items, remote_dir } => {
                let parent = items[item].parent().unwrap_or(Path::new(""));
                let local = local_path(parent, &f.rel);
                let remote = remote_join(remote_dir, &f.rel);
                let (size, modified) = (f.size, f.modified);
                copy::upload_file(server, &local, &remote, size, modified, cancel, progress).await
            }
        }
    }
}

/// The last event of a transfer stopped by `e`: true when it lost the channel.
fn end(server: &Server, id: u32, e: StepError, files: u32, skipped: Vec<Skipped>) -> bool {
    let state = match e.error {
        CoreError::Cancelled => TransferState::Cancelled { files },
        _ => failed(&e.error, files, skipped),
    };
    server.emit(id, state);
    e.lost
}

/// Runs one transfer to its last event; `true` when the channel was lost.
pub(super) async fn run(server: &Server, job: Job, cancel: watch::Receiver<bool>) -> bool {
    let Job { id, side } = job;
    let cancelled = || *cancel.borrow();
    let plan = match or_cancel(&cancel, side.scan(server)).await {
        Ok(p) => p,
        Err(e) => return end(server, id, e, 0, vec![]),
    };
    let mut skipped = plan.skipped;
    let mut skipped_dirs: Vec<&str> = vec![];
    for rel in &plan.dirs {
        if skipped_dirs.iter().any(|d| under(rel, d)) {
            continue;
        }
        match or_cancel(&cancel, side.mkdir(server, rel)).await {
            Ok(None) => {}
            Ok(Some(reason)) => {
                skipped.push(Skipped {
                    path: rel.clone(),
                    reason,
                });
                skipped_dirs.push(rel);
            }
            Err(e) => return end(server, id, e, 0, skipped),
        }
    }
    let index = side.index();
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
        if cancelled() {
            server.emit(id, TransferState::Cancelled { files });
            return false;
        }
        // `Ok(false)`: skipped, and already listed.
        let copied = async {
            if let Some(kind) = or_cancel(&cancel, side.existing(server, &f.rel)).await? {
                let link = kind == EntryKind::Link;
                let choice = match every {
                    _ if link => ConflictChoice::Skip,
                    Some(c) => c,
                    None => {
                        let ask = SessionEvent::Transfer {
                            transfer_id: id,
                            state: TransferState::Conflict {
                                name: f.rel.clone(),
                            },
                        };
                        // A cancel drops the question too (`mod.rs` calls `broker.cancel`).
                        match server.broker.ask(server.id, &*server.sink, ask).await {
                            PromptAnswer::Conflict { choice, all } => {
                                every = all.then_some(choice);
                                choice
                            }
                            _ => ConflictChoice::Cancel,
                        }
                    }
                };
                match choice {
                    ConflictChoice::Overwrite => {}
                    ConflictChoice::Skip => {
                        let reason = if link {
                            SkipReason::Symlink
                        } else {
                            SkipReason::Exists
                        };
                        skipped.push(Skipped {
                            path: f.rel.clone(),
                            reason,
                        });
                        return Ok(false);
                    }
                    ConflictChoice::Cancel => return Err(CoreError::Cancelled.into()),
                }
            }
            let first = f.rel.split('/').next().unwrap_or_default();
            let Some(&item) = index.get(first) else {
                return Err(CoreError::NotFound {
                    what: f.rel.clone(),
                }
                .into());
            };
            let mut progress = |n: u64| {
                if last.is_none_or(|t| t.elapsed() >= PROGRESS_EVERY) {
                    last = Some(Instant::now());
                    server.emit(id, running(done + n, files, &f.rel));
                }
            };
            side.copy(server, item, f, &cancel, &mut progress).await?;
            Ok(true)
        };
        let copied: Result<bool, StepError> = copied.await;
        done += f.size;
        match copied {
            Ok(true) => files += 1,
            Ok(false) => continue,
            Err(e) if e.lost || matches!(e.error, CoreError::Cancelled) => {
                return end(server, id, e, files, skipped);
            }
            // Spec §3 step 5: one bad file (denied, vanished, unreadable) never stops the rest.
            Err(e) => skipped.push(error_skip(&f.rel, &e.error)),
        }
        last = Some(Instant::now());
        server.emit(id, running(done, files, &f.rel));
    }
    server.emit(id, TransferState::Finished { files, skipped });
    false
}
