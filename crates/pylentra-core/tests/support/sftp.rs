//! SFTP subsystem of the test server: serves a real folder as "/", never anything outside it.

use std::collections::HashMap;
use std::fs::FileTimes;
use std::fs::{self, Metadata};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use russh::Disconnect;
use russh_sftp::protocol::{
    Attrs, Data, File, FileAttributes, Handle, Name, OpenFlags, Packet, Status, StatusCode, Version,
};

const POSIX_RENAME: &str = "posix-rename@openssh.com";

/// Ways the server misbehaves, for the client's defences.
#[derive(Clone, Debug, Default)]
pub enum Hostile {
    #[default]
    None,
    /// Extra names added to every directory listing.
    Names(Vec<String>),
    /// Every directory contains a subdirectory `d`, which is another such directory.
    EndlessDirs,
    /// The "sftp" subsystem is accepted, but no SFTP reply ever comes.
    NeverAnswer,
    /// Every readdir answers a batch of "." and ".." and never ends.
    EndlessDots,
    /// Every read returns half the bytes asked for (at least one).
    HalfReads,
    /// Every file ends, when read, at half the size it is listed with.
    ShortFile,
    /// Reads of files with this name answer a plain "failure" status (an EIO, say).
    FailRead(String),
    /// No read is ever answered, and nothing after it either: the server is stuck.
    StallReads,
    /// `posix-rename@openssh.com` is neither advertised nor served.
    NoPosixRename,
    /// The whole SSH connection is closed once this many bytes of writes have arrived.
    DropAfterWrite(u64),
}

pub struct SftpFs {
    /// Canonical, so followed paths can be checked against it.
    root: PathBuf,
    hostile: Hostile,
    /// Open directory handles and the entries not yet returned.
    dirs: HashMap<String, Vec<File>>,
    /// Open file handles, with the length reads stop at and whether reads fail.
    files: HashMap<String, (fs::File, u64, bool)>,
    next_handle: u64,
    /// The connection, for `Hostile::DropAfterWrite`, and the bytes written so far.
    conn: russh::server::Handle,
    written: u64,
}

impl SftpFs {
    pub fn new(root: &Path, hostile: Hostile, conn: russh::server::Handle) -> Self {
        Self {
            root: root.canonicalize().expect("sftp root"),
            hostile,
            dirs: HashMap::new(),
            files: HashMap::new(),
            next_handle: 0,
            conn,
            written: 0,
        }
    }

    fn handle(&mut self, f: fs::File, len: u64, fail: bool) -> String {
        self.next_handle += 1;
        let handle = self.next_handle.to_string();
        self.files.insert(handle.clone(), (f, len, fail));
        handle
    }

    /// The canonical remote path ("/a/b") and its place on disk. Relative paths start at the
    /// home, which is the root; `..` past the root and platform path syntax are refused.
    fn resolve(&self, path: &str) -> Result<(String, PathBuf), StatusCode> {
        let mut parts = Vec::new();
        for c in path.split('/') {
            match c {
                "" | "." => {}
                ".." => {
                    parts.pop().ok_or(StatusCode::PermissionDenied)?;
                }
                c if c.contains(['\\', ':']) => return Err(StatusCode::PermissionDenied),
                c => parts.push(c),
            }
        }
        let endless = matches!(self.hostile, Hostile::EndlessDirs);
        let real = parts
            .iter()
            .filter(|c| !(endless && **c == "d"))
            .fold(self.root.clone(), |p, c| p.join(c));
        Ok((format!("/{}", parts.join("/")), real))
    }

    /// `real` with every link followed; refused when that leaves the root.
    fn followed(&self, real: &Path) -> Result<PathBuf, StatusCode> {
        let c = real.canonicalize().map_err(status)?;
        if c.starts_with(&self.root) {
            Ok(c)
        } else {
            Err(StatusCode::PermissionDenied)
        }
    }

    /// `real` itself is not followed, but the folders leading to it are.
    fn lstat_path(&self, path: &str) -> Result<PathBuf, StatusCode> {
        let (_, real) = self.resolve(path)?;
        match real.parent() {
            Some(parent) if real != self.root => Ok(self
                .followed(parent)?
                .join(real.file_name().expect("named"))),
            _ => Ok(real),
        }
    }

    fn entries(&self, dir: &Path) -> Result<Vec<File>, StatusCode> {
        let mut dir_attrs = FileAttributes::default();
        dir_attrs.set_dir(true);
        let mut files = vec![
            File::new(".", dir_attrs.clone()),
            File::new("..", dir_attrs.clone()),
        ];
        for e in fs::read_dir(dir).map_err(status)? {
            let e = e.map_err(status)?;
            let meta = e.path().symlink_metadata().map_err(status)?;
            files.push(File::new(e.file_name().to_string_lossy(), attrs(&meta)));
        }
        match &self.hostile {
            Hostile::None
            | Hostile::NeverAnswer
            | Hostile::EndlessDots
            | Hostile::HalfReads
            | Hostile::ShortFile
            | Hostile::FailRead(_)
            | Hostile::StallReads
            | Hostile::NoPosixRename
            | Hostile::DropAfterWrite(_) => {}
            Hostile::Names(names) => {
                let mut file = FileAttributes::default();
                file.set_regular(true);
                file.size = Some(0);
                files.extend(names.iter().map(|n| File::new(n.as_str(), file.clone())));
            }
            Hostile::EndlessDirs => {
                if !files.iter().any(|f| f.filename == "d") {
                    files.push(File::new("d", dir_attrs));
                }
            }
        }
        Ok(files)
    }
}

/// Type bits set explicitly: russh-sftp's own `From<&Metadata>` marks links as regular files.
fn attrs(m: &Metadata) -> FileAttributes {
    let mut a = FileAttributes {
        size: Some(m.len()),
        mtime: m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .and_then(|d| u32::try_from(d.as_secs()).ok()),
        ..Default::default()
    };
    let t = m.file_type();
    if t.is_symlink() {
        a.set_symlink(true);
    } else if t.is_dir() {
        a.set_dir(true);
    } else if t.is_file() {
        a.set_regular(true);
    }
    a
}

fn ok(id: u32) -> Status {
    Status {
        id,
        status_code: StatusCode::Ok,
        error_message: "Ok".into(),
        language_tag: "en-US".into(),
    }
}

/// SFTP strings (u32 length, bytes) back to back, as extended requests carry them.
fn strings(mut d: &[u8]) -> Option<Vec<String>> {
    let mut out = vec![];
    while !d.is_empty() {
        let (len, rest) = d.split_first_chunk::<4>()?;
        let len = u32::from_be_bytes(*len) as usize;
        out.push(String::from_utf8(rest.get(..len)?.to_vec()).ok()?);
        d = &rest[len..];
    }
    Some(out)
}

/// Like OpenSSH, an existing file is a plain failure: SFTPv3 has no "exists" status.
fn status(e: io::Error) -> StatusCode {
    match e.kind() {
        io::ErrorKind::NotFound => StatusCode::NoSuchFile,
        io::ErrorKind::PermissionDenied => StatusCode::PermissionDenied,
        _ => StatusCode::Failure,
    }
}

impl russh_sftp::server::Handler for SftpFs {
    type Error = StatusCode;

    fn unimplemented(&self) -> Self::Error {
        StatusCode::OpUnsupported
    }

    async fn init(
        &mut self,
        _version: u32,
        _extensions: HashMap<String, String>,
    ) -> Result<Version, Self::Error> {
        let mut v = Version::new();
        if !matches!(self.hostile, Hostile::NoPosixRename) {
            v.extensions.insert(POSIX_RENAME.into(), "1".into());
        }
        Ok(v)
    }

    async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
        let (canonical, real) = self.resolve(&path)?;
        self.followed(&real)?;
        Ok(Name {
            id,
            files: vec![File::dummy(canonical)],
        })
    }

    async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
        let (_, real) = self.resolve(&path)?;
        let files = self.entries(&self.followed(&real)?)?;
        self.next_handle += 1;
        let handle = self.next_handle.to_string();
        self.dirs.insert(handle.clone(), files);
        Ok(Handle { id, handle })
    }

    async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
        let files = self.dirs.get_mut(&handle).ok_or(StatusCode::Failure)?;
        if matches!(self.hostile, Hostile::EndlessDots) {
            // A big batch, so the client's entry limit trips quickly.
            let dots = [".", ".."].map(|n| File {
                filename: n.into(),
                longname: String::new(),
                attrs: FileAttributes::empty(),
            });
            return Ok(Name {
                id,
                files: dots.iter().cycle().take(1000).cloned().collect(),
            });
        }
        if files.is_empty() {
            return Err(StatusCode::Eof);
        }
        Ok(Name {
            id,
            files: std::mem::take(files),
        })
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        let dir = self.dirs.remove(&handle).is_some();
        if !dir {
            self.files.remove(&handle).ok_or(StatusCode::Failure)?;
        }
        Ok(ok(id))
    }

    /// Reads follow links, as a real server does, but never out of the root. Writes create
    /// the last component itself: with EXCLUDE, an existing name (a link included) fails.
    async fn open(
        &mut self,
        id: u32,
        filename: String,
        pflags: OpenFlags,
        _attrs: FileAttributes,
    ) -> Result<Handle, Self::Error> {
        if pflags.contains(OpenFlags::WRITE) {
            let mut o = fs::OpenOptions::new();
            o.write(true).truncate(pflags.contains(OpenFlags::TRUNCATE));
            if pflags.contains(OpenFlags::CREATE | OpenFlags::EXCLUDE) {
                o.create_new(true);
            } else {
                o.create(pflags.contains(OpenFlags::CREATE));
            }
            let f = o.open(self.lstat_path(&filename)?).map_err(status)?;
            let handle = self.handle(f, 0, false);
            return Ok(Handle { id, handle });
        }
        let (_, real) = self.resolve(&filename)?;
        let f = fs::File::open(self.followed(&real)?).map_err(status)?;
        let mut len = f.metadata().map_err(status)?.len();
        if matches!(self.hostile, Hostile::ShortFile) {
            len /= 2;
        }
        let fail =
            matches!(&self.hostile, Hostile::FailRead(n) if filename.rsplit('/').next() == Some(n));
        let handle = self.handle(f, len, fail);
        Ok(Handle { id, handle })
    }

    async fn write(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        data: Vec<u8>,
    ) -> Result<Status, Self::Error> {
        if let Hostile::DropAfterWrite(n) = self.hostile {
            self.written += data.len() as u64;
            if self.written >= n {
                let bye = Disconnect::ByApplication;
                let _ = self
                    .conn
                    .disconnect(bye, String::new(), String::new())
                    .await;
                // No reply: the client must see the connection end, not a failed write.
                std::future::pending::<()>().await;
            }
        }
        let (f, ..) = self.files.get_mut(&handle).ok_or(StatusCode::Failure)?;
        f.seek(SeekFrom::Start(offset)).map_err(status)?;
        f.write_all(&data).map_err(status)?;
        Ok(ok(id))
    }

    async fn fsetstat(
        &mut self,
        id: u32,
        handle: String,
        attrs: FileAttributes,
    ) -> Result<Status, Self::Error> {
        let (f, ..) = self.files.get(&handle).ok_or(StatusCode::Failure)?;
        let at = |t: Option<u32>| t.map(|s| UNIX_EPOCH + Duration::from_secs(s.into()));
        let mut times = FileTimes::new();
        if let Some(t) = at(attrs.mtime) {
            times = times.set_modified(t);
        }
        if let Some(t) = at(attrs.atime) {
            times = times.set_accessed(t);
        }
        f.set_times(times).map_err(status)?;
        Ok(ok(id))
    }

    async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
        fs::remove_file(self.lstat_path(&filename)?).map_err(status)?;
        Ok(ok(id))
    }

    async fn mkdir(
        &mut self,
        id: u32,
        path: String,
        _attrs: FileAttributes,
    ) -> Result<Status, Self::Error> {
        fs::create_dir(self.lstat_path(&path)?).map_err(status)?;
        Ok(ok(id))
    }

    /// SFTPv3 rename, as OpenSSH serves it: an existing target is a failure, never replaced.
    async fn rename(
        &mut self,
        id: u32,
        oldpath: String,
        newpath: String,
    ) -> Result<Status, Self::Error> {
        let (from, to) = (self.lstat_path(&oldpath)?, self.lstat_path(&newpath)?);
        if to.symlink_metadata().is_ok() {
            return Err(StatusCode::Failure);
        }
        fs::rename(from, to).map_err(status)?;
        Ok(ok(id))
    }

    /// `posix-rename@openssh.com`: replaces the target in one step.
    async fn extended(
        &mut self,
        id: u32,
        request: String,
        data: Vec<u8>,
    ) -> Result<Packet, Self::Error> {
        if request != POSIX_RENAME || matches!(self.hostile, Hostile::NoPosixRename) {
            return Err(StatusCode::OpUnsupported);
        }
        let Some([old, new]) = strings(&data).and_then(|s| <[String; 2]>::try_from(s).ok()) else {
            return Err(StatusCode::BadMessage);
        };
        fs::rename(self.lstat_path(&old)?, self.lstat_path(&new)?).map_err(status)?;
        Ok(Packet::Status(ok(id)))
    }

    async fn read(
        &mut self,
        id: u32,
        handle: String,
        offset: u64,
        len: u32,
    ) -> Result<Data, Self::Error> {
        if matches!(self.hostile, Hostile::StallReads) {
            std::future::pending::<()>().await;
        }
        let (f, end, fail) = self.files.get_mut(&handle).ok_or(StatusCode::Failure)?;
        if *fail {
            return Err(StatusCode::Failure);
        }
        if offset >= *end {
            return Err(StatusCode::Eof);
        }
        let mut len = u64::from(len).min(*end - offset);
        if matches!(self.hostile, Hostile::HalfReads) {
            len = (len / 2).max(1);
        }
        f.seek(SeekFrom::Start(offset)).map_err(status)?;
        let mut data = vec![];
        f.take(len).read_to_end(&mut data).map_err(status)?;
        Ok(Data { id, data })
    }

    async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let meta = self.lstat_path(&path)?.symlink_metadata().map_err(status)?;
        Ok(Attrs {
            id,
            attrs: attrs(&meta),
        })
    }

    async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
        let (_, real) = self.resolve(&path)?;
        let meta = self.followed(&real)?.metadata().map_err(status)?;
        Ok(Attrs {
            id,
            attrs: attrs(&meta),
        })
    }
}
