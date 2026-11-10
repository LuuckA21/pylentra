//! SFTP subsystem of the test server: serves a real folder as "/", never anything outside it.

use std::collections::HashMap;
use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use russh_sftp::protocol::{
    Attrs, File, FileAttributes, Handle, Name, Status, StatusCode, Version,
};

/// Ways the server misbehaves, for the client's defences.
#[derive(Clone, Debug, Default)]
pub enum Hostile {
    #[default]
    None,
    /// Extra names added to every directory listing.
    Names(Vec<String>),
    /// Every directory contains a subdirectory `d`, which is another such directory.
    EndlessDirs,
}

pub struct SftpFs {
    /// Canonical, so followed paths can be checked against it.
    root: PathBuf,
    hostile: Hostile,
    /// Open directory handles and the entries not yet returned.
    dirs: HashMap<String, Vec<File>>,
    next_handle: u64,
}

impl SftpFs {
    pub fn new(root: &Path, hostile: Hostile) -> Self {
        Self {
            root: root.canonicalize().expect("sftp root"),
            hostile,
            dirs: HashMap::new(),
            next_handle: 0,
        }
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
            Hostile::None => {}
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
        v.extensions
            .insert("posix-rename@openssh.com".into(), "1".into());
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
        if files.is_empty() {
            return Err(StatusCode::Eof);
        }
        Ok(Name {
            id,
            files: std::mem::take(files),
        })
    }

    async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
        self.dirs.remove(&handle).ok_or(StatusCode::Failure)?;
        Ok(Status {
            id,
            status_code: StatusCode::Ok,
            error_message: "Ok".into(),
            language_tag: "en-US".into(),
        })
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
