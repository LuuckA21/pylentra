//! Copies one file between the server and this machine (spec §3 "Trasferimento", steps 3-4).

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use futures_util::{StreamExt, stream};
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::protocol::{Data, FileAttributes, OpenFlags, Packet, StatusCode};
use tokio::sync::watch;

use super::names::{TEMP_SUFFIX, remote_join, remote_split, temp_name};
use super::{POSIX_RENAME, Server, StepError, lost, or_cancel, step_error};
use crate::error::CoreError;
use crate::store::create_temp;

const BLOCK: u64 = 32 * 1024;
/// Requests in flight per file, so the server's latency does not cap the speed.
const IN_FLIGHT: usize = 8;
/// How long a failed or cancelled upload waits for its remote temp file to be removed.
const CLEANUP: Duration = Duration::from_millis(500);

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
    // Local TOCTOU: the folder checked by the transfer could be swapped for a link before this
    // create, but only by a local user who can already write there.
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

/// Uploads `local` (`size` bytes, as scanned) over `remote` through a fresh remote temp file.
/// The temp file is removed on any failure or cancel while the channel is up; when the channel
/// is gone it may remain, but `remote` is only ever replaced by a complete file.
pub(crate) async fn upload_file(
    sftp: &Server,
    local: &Path,
    remote: &str,
    size: u64,
    modified: Option<u64>,
    cancel: &watch::Receiver<bool>,
    progress: &mut impl FnMut(u64),
) -> Result<(), StepError> {
    // shortcut: opening and reading the local file block the runtime thread for one 32 KiB
    // read at a time; upgrade to spawn_blocking if users report stalls on slow disks.
    let mut file = fs::File::open(local)?;
    // Never cut short by a cancel: the temp would be created with no one to remove it.
    let (tmp, handle) = create_remote_temp(sftp, remote).await?;
    let wrote = async {
        if *cancel.borrow() {
            return Err(CoreError::Cancelled.into());
        }
        write_from(sftp, &handle, remote, &mut file, size, cancel, progress).await?;
        if let Some(t) = modified.and_then(|m| u32::try_from(m).ok()) {
            let attrs = FileAttributes {
                mtime: Some(t),
                atime: Some(t),
                ..Default::default()
            };
            let set = sftp.raw.fsetstat(handle.as_str(), attrs).await;
            set.map_err(|e| step_error(e, remote))?;
        }
        Ok(())
    };
    let wrote = or_cancel(cancel, wrote).await;
    // Sent even after a cancel, but never waited for then.
    let close = async {
        sftp.raw
            .close(handle)
            .await
            .map_err(|e| step_error(e, remote))
    };
    let close = or_cancel(cancel, close).await;
    let done = match wrote.and(close) {
        // Not cut short either: between its remove and its rename, a cancel would lose both.
        Ok(_) => replace(sftp, &tmp, remote).await,
        Err(e) => Err(e),
    };
    let Err(e) = done else { return Ok(()) };
    match tokio::time::timeout(CLEANUP, sftp.raw.remove(tmp)).await {
        // The channel is gone: the temp file stays, so this is a failure, not a clean cancel.
        Ok(Err(r)) if lost(&r) => Err(step_error(r, remote)),
        _ => Err(e),
    }
}

/// `<name>.<n>.pylentra-tmp` next to `remote`, created exclusively; returns it and its handle.
async fn create_remote_temp(sftp: &Server, remote: &str) -> Result<(String, String), StepError> {
    let (dir, name) = remote_split(remote);
    let flags = OpenFlags::CREATE | OpenFlags::EXCLUDE | OpenFlags::WRITE;
    let mut n = 0;
    loop {
        let tmp = remote_join(dir, &temp_name(name, n));
        let e = match sftp.raw.open(tmp.as_str(), flags, Default::default()).await {
            Ok(h) => return Ok((tmp, h.handle)),
            Err(e) => e,
        };
        // SFTPv3 has no "exists" status (OpenSSH answers a plain failure), so look.
        let taken = matches!(&e, SftpError::Status(s) if s.status_code == StatusCode::Failure)
            && sftp.raw.lstat(tmp.as_str()).await.is_ok();
        if !taken || n == 100 {
            return Err(step_error(e, remote));
        }
        n += 1;
    }
}

/// Local blocks read in order, written with up to `IN_FLIGHT` requests outstanding.
async fn write_from(
    sftp: &Server,
    handle: &str,
    remote: &str,
    file: &mut fs::File,
    size: u64,
    cancel: &watch::Receiver<bool>,
    progress: &mut impl FnMut(u64),
) -> Result<(), StepError> {
    let block = sftp.max_write.map_or(BLOCK, |m| m.min(BLOCK));
    let mut blocks = stream::iter((0..size).step_by(block as usize))
        .map(|offset| {
            let mut data = vec![0; block.min(size - offset) as usize];
            let read = file.read_exact(&mut data);
            async move {
                read.map_err(|e| match e.kind() {
                    io::ErrorKind::UnexpectedEof => changed(),
                    _ => e.into(),
                })?;
                let end = offset + data.len() as u64;
                let w = sftp.raw.write(handle, offset, data).await;
                w.map_err(|e| step_error(e, remote))?;
                Ok::<_, StepError>(end)
            }
        })
        .buffered(IN_FLIGHT);
    while let Some(end) = blocks.next().await {
        if *cancel.borrow() {
            return Err(CoreError::Cancelled.into());
        }
        progress(end?);
    }
    Ok(())
}

/// Puts the complete `tmp` in place of `remote`: in one step with `posix-rename@openssh.com`,
/// otherwise by removing `remote` first, since a plain SFTP rename never replaces.
async fn replace(sftp: &Server, tmp: &str, remote: &str) -> Result<(), StepError> {
    let r = if sftp.posix_rename {
        let mut data = vec![];
        for s in [tmp, remote] {
            // Remote paths come from validated names, far below 4 GiB.
            data.extend((s.len() as u32).to_be_bytes());
            data.extend(s.as_bytes());
        }
        match sftp.raw.extended(POSIX_RENAME, data).await {
            Ok(Packet::Status(s)) if s.status_code == StatusCode::Ok => Ok(()),
            Ok(Packet::Status(s)) => Err(SftpError::Status(s)),
            Ok(_) => Err(SftpError::UnexpectedPacket),
            Err(e) => Err(e),
        }
    } else {
        match sftp.raw.remove(remote).await {
            Ok(_) => sftp.raw.rename(tmp, remote).await.map(drop),
            Err(SftpError::Status(s)) if s.status_code == StatusCode::NoSuchFile => {
                sftp.raw.rename(tmp, remote).await.map(drop)
            }
            Err(e) => Err(e),
        }
    };
    r.map_err(|e| step_error(e, remote))
}
