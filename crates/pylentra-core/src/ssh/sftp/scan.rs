//! Walks the items of a transfer before it starts (spec §4, §3 "Trasferimento" step 1).
//! Reads only, never follows links, and walks with a stack so depth is a counter, not recursion.

use std::fs::{self, Metadata};
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use russh_sftp::client::RawSftpSession;
use russh_sftp::protocol::File;

use super::names::{LOCAL_WINDOWS, remote_join, remote_split, valid_name};
use super::{EntryKind, StepError, entry, lost, named, read_dir, step_error};
use crate::error::CoreError;
use crate::ssh::events::{SkipReason, Skipped};

const MAX_DEPTH: usize = 32;
const MAX_ENTRIES: usize = 100_000;

#[derive(Debug)]
pub struct Plan {
    /// Parents first, relative to the item's parent, `/`-separated.
    pub dirs: Vec<String>,
    pub files: Vec<PlannedFile>,
    pub skipped: Vec<Skipped>,
    pub total_bytes: u64,
    max: usize,
}

#[derive(Debug)]
pub struct PlannedFile {
    pub rel: String,
    pub size: u64,
    pub modified: Option<u64>,
}

fn limit(what: &str) -> CoreError {
    CoreError::SftpLimit { what: what.into() }
}

impl Plan {
    fn new(max: usize) -> Self {
        Plan {
            dirs: vec![],
            files: vec![],
            skipped: vec![],
            total_bytes: 0,
            max,
        }
    }

    fn skip(&mut self, path: String, reason: SkipReason) -> Result<(), CoreError> {
        self.skipped.push(Skipped { path, reason });
        self.check(0)
    }

    fn file(&mut self, rel: String, size: u64, modified: Option<u64>) -> Result<(), CoreError> {
        self.total_bytes += size;
        self.files.push(PlannedFile {
            rel,
            size,
            modified,
        });
        self.check(0)
    }

    fn dir(&mut self, rel: String, depth: usize) -> Result<(), CoreError> {
        if depth > MAX_DEPTH {
            return Err(limit("depth"));
        }
        self.dirs.push(rel);
        self.check(0)
    }

    /// `pending` items are already known but not yet counted; they count too, so a wide tree
    /// cannot fill memory before the limit trips.
    fn check(&self, pending: usize) -> Result<(), CoreError> {
        if self.dirs.len() + self.files.len() + self.skipped.len() + pending > self.max {
            return Err(limit("entries"));
        }
        Ok(())
    }
}

fn join(rel: &str, name: &str) -> String {
    format!("{rel}/{name}")
}

#[allow(dead_code)] // Used by uploads (Task 6).
pub(crate) fn scan_local(items: &[PathBuf]) -> Result<Plan, CoreError> {
    local(items, MAX_ENTRIES)
}

fn local(items: &[PathBuf], max: usize) -> Result<Plan, CoreError> {
    let mut plan = Plan::new(max);
    for item in items {
        let name = item.file_name().and_then(|n| n.to_str());
        let Some(name) = name.filter(|n| valid_name(n, false)) else {
            let shown = item.file_name().unwrap_or_default().to_string_lossy();
            plan.skip(shown.into_owned(), SkipReason::InvalidName)?;
            continue;
        };
        let meta = fs::symlink_metadata(item)?;
        let mut stack = vec![(item.clone(), name.to_string(), 1, meta)];
        while let Some((path, rel, depth, meta)) = stack.pop() {
            let ty = meta.file_type();
            if ty.is_symlink() {
                plan.skip(rel, SkipReason::Symlink)?;
            } else if ty.is_file() {
                plan.file(rel, meta.len(), mtime(&meta))?;
            } else if !ty.is_dir() {
                plan.skip(rel, SkipReason::Special)?;
            } else {
                plan.dir(rel.clone(), depth)?;
                let children = fs::read_dir(&path).and_then(|r| r.collect::<Result<Vec<_>, _>>());
                let children = match children {
                    Ok(c) => c,
                    Err(e) => {
                        plan.skip(rel, error(CoreError::Io(e)))?;
                        continue;
                    }
                };
                for c in children {
                    let file_name = c.file_name();
                    let child = match file_name.to_str().filter(|n| valid_name(n, false)) {
                        Some(n) => join(&rel, n),
                        None => {
                            plan.skip(
                                join(&rel, &file_name.to_string_lossy()),
                                SkipReason::InvalidName,
                            )?;
                            continue;
                        }
                    };
                    match fs::symlink_metadata(c.path()) {
                        Ok(m) => stack.push((c.path(), child, depth + 1, m)),
                        Err(e) => plan.skip(child, error(CoreError::Io(e)))?,
                    }
                    plan.check(stack.len())?;
                }
            }
        }
    }
    Ok(plan)
}

#[allow(dead_code)] // Used by uploads (Task 6).
fn mtime(m: &Metadata) -> Option<u64> {
    let t = m.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(t.as_secs())
}

fn error(e: CoreError) -> SkipReason {
    SkipReason::Error { error: e.to_api() }
}

/// `items` are absolute remote paths; a failure of the channel itself ends the scan.
pub(crate) async fn scan_remote(
    sftp: &RawSftpSession,
    items: &[String],
) -> Result<Plan, StepError> {
    remote(sftp, items, MAX_ENTRIES).await
}

async fn remote(sftp: &RawSftpSession, items: &[String], max: usize) -> Result<Plan, StepError> {
    let mut plan = Plan::new(max);
    for item in items {
        let name = remote_split(item).1;
        if !valid_name(name, LOCAL_WINDOWS) {
            plan.skip(name.into(), SkipReason::InvalidName)?;
            continue;
        }
        let attrs = sftp
            .lstat(item.as_str())
            .await
            .map_err(|e| step_error(e, item))?;
        let root = entry(File::new(name, attrs.attrs));
        let mut stack = vec![(item.clone(), name.to_string(), 1, root)];
        while let Some((path, rel, depth, e)) = stack.pop() {
            match e.kind {
                EntryKind::Link => plan.skip(rel, SkipReason::Symlink)?,
                EntryKind::Other => plan.skip(rel, SkipReason::Special)?,
                EntryKind::File => plan.file(rel, e.size.unwrap_or(0), e.modified)?,
                EntryKind::Dir => {
                    plan.dir(rel.clone(), depth)?;
                    let read = match sftp.opendir(path.as_str()).await {
                        Ok(h) => {
                            let r = read_dir(sftp, &h.handle).await;
                            let _ = sftp.close(h.handle).await;
                            r
                        }
                        Err(e) => Err(e),
                    };
                    let dir = match read {
                        Ok(d) => d,
                        Err(e) if lost(&e) => return Err(step_error(e, &path)),
                        Err(e) => {
                            plan.skip(rel, error(named(e, &path)))?;
                            continue;
                        }
                    };
                    if dir.truncated {
                        return Err(limit("entries").into());
                    }
                    for bad in dir.invalid {
                        plan.skip(join(&rel, &bad), SkipReason::InvalidName)?;
                    }
                    for c in dir.entries {
                        let child = (remote_join(&path, &c.name), join(&rel, &c.name));
                        stack.push((child.0, child.1, depth + 1, c));
                    }
                    plan.check(stack.len())?;
                }
            }
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &std::path::Path, body: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    #[test]
    fn local_tree() {
        let t = tempfile::tempdir().unwrap();
        write(&t.path().join("a/b/c.txt"), "abc");
        write(&t.path().join("a/x.txt"), "xy");
        let plan = scan_local(&[t.path().join("a")]).unwrap();
        assert_eq!(plan.dirs, ["a", "a/b"]);
        let mut files: Vec<_> = plan.files.iter().map(|f| f.rel.as_str()).collect();
        files.sort();
        assert_eq!(files, ["a/b/c.txt", "a/x.txt"]);
        assert_eq!(plan.total_bytes, 5);
        assert!(plan.skipped.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn local_skips_links_and_fifos() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("a");
        write(&d.join("f"), "1");
        std::os::unix::fs::symlink("f", d.join("link")).unwrap();
        assert!(
            std::process::Command::new("mkfifo")
                .arg(d.join("fifo"))
                .status()
                .unwrap()
                .success()
        );
        let plan = scan_local(&[d]).unwrap();
        assert_eq!(plan.files.len(), 1);
        let mut skipped: Vec<_> = plan
            .skipped
            .iter()
            .map(|s| (s.path.as_str(), &s.reason))
            .collect();
        skipped.sort_by_key(|s| s.0);
        assert_eq!(
            skipped,
            [
                ("a/fifo", &SkipReason::Special),
                ("a/link", &SkipReason::Symlink)
            ]
        );
    }

    #[test]
    fn local_pending_counts() {
        let t = tempfile::tempdir().unwrap();
        for d in 0..5 {
            for f in 0..5 {
                write(&t.path().join(format!("a/d{d}/f{f}")), "");
            }
        }
        let err = local(&[t.path().join("a")], 10).unwrap_err();
        assert!(
            matches!(&err, CoreError::SftpLimit { what } if what == "entries"),
            "{err:?}"
        );
        // 1 + 5 dirs + 25 files fit.
        assert_eq!(local(&[t.path().join("a")], 31).unwrap().files.len(), 25);
    }

    #[test]
    fn local_depth_limit() {
        let t = tempfile::tempdir().unwrap();
        let mut p = t.path().to_path_buf();
        for _ in 0..33 {
            p.push("d");
        }
        fs::create_dir_all(&p).unwrap();
        let err = scan_local(&[t.path().join("d")]).unwrap_err();
        assert!(
            matches!(&err, CoreError::SftpLimit { what } if what == "depth"),
            "{err:?}"
        );
        // 32 levels is still fine.
        let t = tempfile::tempdir().unwrap();
        let mut p = t.path().to_path_buf();
        for _ in 0..32 {
            p.push("d");
        }
        fs::create_dir_all(&p).unwrap();
        assert_eq!(scan_local(&[t.path().join("d")]).unwrap().dirs.len(), 32);
    }
}
