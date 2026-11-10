//! Copies one file between the server and this machine (spec §3 "Trasferimento", steps 3-4).

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, UNIX_EPOCH};

use futures_util::{StreamExt, stream};
use russh_sftp::client::error::Error as SftpError;
use russh_sftp::protocol::{Data, FileAttributes, OpenFlags, StatusCode};

use super::names::TEMP_SUFFIX;
use super::{Server, named};
use crate::error::CoreError;
use crate::store::create_temp;

const BLOCK: u64 = 32 * 1024;
/// Requests in flight per file, so the server's latency does not cap the speed.
const IN_FLIGHT: usize = 8;

fn changed() -> CoreError {
    CoreError::NotFound {
        what: "file changed".into(),
    }
}

/// Downloads `remote` (`size` bytes, as scanned) over `local` through a fresh temp file, which
/// any failure or cancel removes. `progress` gets the bytes done in this file.
pub(crate) async fn download_file(
    sftp: &Server,
    remote: &str,
    local: &Path,
    size: u64,
    modified: Option<u64>,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(u64),
) -> Result<(), CoreError> {
    let (tmp, mut file) = create_temp(local, TEMP_SUFFIX)?;
    let r: Result<(), CoreError> = async {
        let handle = sftp
            .raw
            .open(remote, OpenFlags::READ, FileAttributes::default())
            .await
            .map_err(|e| named(e, remote))?
            .handle;
        let read = read_into(sftp, &handle, remote, &mut file, size, cancel, progress).await;
        let _ = sftp.raw.close(handle).await;
        read?;
        if let Some(secs) = modified {
            file.set_modified(UNIX_EPOCH + Duration::from_secs(secs))?;
        }
        // On disk before it replaces anything, so a crash never trades a good file for a hole.
        file.sync_all()?;
        Ok(fs::rename(&tmp, local)?)
    }
    .await;
    if r.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    r
}

/// Writes in offset order, so the temp file never has holes.
async fn read_into(
    sftp: &Server,
    handle: &str,
    remote: &str,
    file: &mut fs::File,
    size: u64,
    cancel: &AtomicBool,
    progress: &mut impl FnMut(u64),
) -> Result<(), CoreError> {
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
        if cancel.load(Ordering::Relaxed) {
            return Err(CoreError::Cancelled);
        }
        let mut done = 0;
        let mut data = data?;
        // A short read: the rest of this block comes before the next one is written.
        loop {
            // Local writes are small and sequential; the runtime tolerates them.
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
fn bytes(r: Result<Data, SftpError>, len: u64, remote: &str) -> Result<Vec<u8>, CoreError> {
    match r {
        Ok(Data { mut data, .. }) if !data.is_empty() => {
            data.truncate(len as usize);
            Ok(data)
        }
        Ok(_) => Err(changed()),
        Err(SftpError::Status(s)) if s.status_code == StatusCode::Eof => Err(changed()),
        Err(e) => Err(named(e, remote)),
    }
}
