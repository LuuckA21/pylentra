//! One transfer from scan to its last event (spec §3 "Trasferimento"); the queue is in `mod.rs`.

use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Instant;

use tokio::sync::watch;

use super::names::{remote_join, remote_split};
use super::{Job, PROGRESS_EVERY, Server, StepError, copy, or_cancel, scan};
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

/// Runs one download to its last event; `true` when the channel was lost.
pub(super) async fn download(server: &Server, job: Job, cancel: watch::Receiver<bool>) -> bool {
    let id = job.id;
    let cancelled = || *cancel.borrow();
    let plan = match or_cancel(&cancel, scan::scan_remote(&server.raw, &job.items)).await {
        Ok(p) => p,
        Err(StepError {
            error: CoreError::Cancelled,
            ..
        }) => {
            server.emit(id, TransferState::Cancelled { files: 0 });
            return false;
        }
        Err(e) => {
            server.emit(id, failed(&e.error, 0, vec![]));
            return e.lost;
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
        if cancelled() {
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
                // A cancel drops the question too (`mod.rs` calls `broker.cancel`).
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
            Err(StepError {
                error: CoreError::Cancelled,
                ..
            }) => {
                server.emit(id, TransferState::Cancelled { files });
                return false;
            }
            Err(e) if e.lost => {
                server.emit(id, failed(&e.error, files, skipped));
                return true;
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
