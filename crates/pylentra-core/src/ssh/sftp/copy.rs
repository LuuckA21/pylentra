//! Copies one file between the server and this machine (spec §3 "Trasferimento", steps 3-4).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use futures_util::{StreamExt, stream};
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::protocol::{Data, FileAttributes, OpenFlags, StatusCode};
use tokio::sync::watch;

use super::names::TEMP_SUFFIX;
use super::{Server, StepError, or_cancel, step_error};
use crate::error::CoreError;
use crate::store::create_temp;

const BLOCK: u64 = 32 * 1024;
/// Requests in flight per file, so the server's latency does not cap the speed.
const IN_FLIGHT: usize = 8;

fn changed() -> StepError {
    CoreError::NotFound {
        what: "file changed".into(),
    }
    .into()
}

/// Removes the temp file when dropped, unless it was renamed: every exit, an aborted task
/// included, leaves nothing behind.
struct Temp(Option<PathBuf>);

impl Drop for Temp {
    fn drop(&mut self) {
        if let Some(p) = &self.0 {
            let _ = fs::remove_file(p);
        }
    }
}

/// Downloads `remote` (`size` bytes, as scanned) over `local` through a fresh temp file, which
/// any failure or cancel removes. `progress` gets the bytes done in this file. A cancel ends
/// any wait on the server at once.
pub(crate) async fn download_file(
    sftp: &Server,
    remote: &str,
    local: &Path,
    size: u64,
    modified: Option<u64>,
    cancel: &watch::Receiver<bool>,
    progress: &mut impl FnMut(u64),
) -> Result<(), StepError> {
    let (tmp, mut file) = create_temp(local, TEMP_SUFFIX)?;
    let mut guard = Temp(Some(tmp.clone()));
    let open = async {
        let flags = OpenFlags::READ;
        let opened = sftp
            .raw
            .open(remote, flags, FileAttributes::default())
            .await;
        opened.map_err(|e| step_error(e, remote))
    };
    let handle = or_cancel(cancel, open).await?.handle;
    let read = read_into(sftp, &handle, remote, &mut file, size, cancel, progress);
    let read = or_cancel(cancel, read).await;
    // Sent even after a cancel, but never waited for then.
    let close = async { Ok(sftp.raw.close(handle).await) };
    let _ = or_cancel(cancel, close).await;
    read?;
    // Off the runtime: a slow disk must not stall the session (spec §3).
    let local = local.to_path_buf();
    let finish = move || -> io::Result<()> {
        if let Some(secs) = modified {
            file.set_modified(UNIX_EPOCH + Duration::from_secs(secs))?;
        }
        // On disk before it replaces anything, so a crash never trades a good file for a hole.
        file.sync_all()?;
        fs::rename(&tmp, &local)
    };
    tokio::task::spawn_blocking(finish)
        .await
        .map_err(io::Error::other)??;
    guard.0 = None;
    Ok(())
}

/// Writes in offset order, so the temp file never has holes.
async fn read_into(
    sftp: &Server,
    handle: &str,
    remote: &str,
    file: &mut fs::File,
    size: u64,
    cancel: &watch::Receiver<bool>,
    progress: &mut impl FnMut(u64),
) -> Result<(), StepError> {
    let block = sftp.max_read.map_or(BLOCK, |m| m.min(BLOCK));
    let read = |offset: u64, len: u64| async move {
        // `len` is at most one block, so it fits.
        let r = sftp.raw.read(handle, offset, len as u32).await;
        bytes(r, len, remote)
    };
    let mut blocks = stream::iter((0..size).step_by(block as usize))
        .map(|offset| {
            let len = block.min(size - offset);
            async move { (offset, len, read(offset, len).await) }
        })
        .buffered(IN_FLIGHT);
    while let Some((offset, len, data)) = blocks.next().await {
        if *cancel.borrow() {
            return Err(CoreError::Cancelled.into());
        }
        let mut done = 0;
        let mut data = data?;
        // A short read: the rest of this block comes before the next one is written.
        loop {
            // shortcut: 32 KiB writes on the runtime thread, which land in the page cache;
            // upgrade to spawn_blocking if users report stalls on slow disks.
            file.write_all(&data)?;
            done += data.len() as u64;
            if done == len {
                break;
            }
            data = read(offset + done, len - done).await?;
        }
        progress(offset + len);
    }
    Ok(())
}

/// The bytes of one reply, never more than asked; an end before `size` means the file changed.
fn bytes(r: Result<Data, SftpError>, len: u64, remote: &str) -> Result<Vec<u8>, StepError> {
    match r {
        Ok(Data { mut data, .. }) if !data.is_empty() => {
            data.truncate(len as usize);
            Ok(data)
        }
        Ok(_) => Err(changed()),
        Err(SftpError::Status(s)) if s.status_code == StatusCode::Eof => Err(changed()),
        Err(e) => Err(step_error(e, remote)),
    }
}
