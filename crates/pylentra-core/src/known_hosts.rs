use crate::error::CoreError;
use crate::store::atomic_write;
use russh::keys::known_hosts::{
    check_known_hosts_path, known_host_keys_path, learn_known_hosts_path,
};
use russh::keys::ssh_key::{HashAlg, PublicKey};
use serde::Serialize;
use std::path::PathBuf;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub enum KeySource {
    Pylentra,
    OpenSsh,
}

#[derive(Debug, PartialEq)]
pub enum HostKeyStatus {
    Known,
    Unknown,
    Mismatch {
        source: KeySource,
        expected_fp: String,
    },
}

pub struct KnownHosts {
    pylentra: PathBuf,
    openssh: Option<PathBuf>,
}

pub fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

impl KnownHosts {
    /// `openssh` is the read-only `~/.ssh/known_hosts`; only `pylentra` is ever written.
    pub fn new(pylentra: PathBuf, openssh: Option<PathBuf>) -> Self {
        Self { pylentra, openssh }
    }

    /// Pylentra's file first, then OpenSSH's; any recorded key that differs
    /// for the same algorithm is a mismatch, even if the other file knows it.
    pub fn check(
        &self,
        host: &str,
        port: u16,
        key: &PublicKey,
    ) -> Result<HostKeyStatus, CoreError> {
        let files = [
            (KeySource::Pylentra, Some(&self.pylentra)),
            (KeySource::OpenSsh, self.openssh.as_ref()),
        ];
        let mut known = false;
        for (source, path) in files {
            let Some(path) = path else { continue };
            match check_known_hosts_path(host, port, key, path) {
                Ok(found) => known |= found,
                Err(russh::keys::Error::KeyChanged { line }) => {
                    let recorded = known_host_keys_path(host, port, path).map_err(ssh_err)?;
                    let expected_fp = recorded
                        .iter()
                        .find(|(l, _)| *l == line)
                        .map(|(_, k)| fingerprint(k))
                        .unwrap_or_default();
                    return Ok(HostKeyStatus::Mismatch {
                        source,
                        expected_fp,
                    });
                }
                Err(e) => return Err(ssh_err(e)),
            }
        }
        Ok(if known {
            HostKeyStatus::Known
        } else {
            HostKeyStatus::Unknown
        })
    }

    /// Records the key in the Pylentra file only.
    pub fn learn(&self, host: &str, port: u16, key: &PublicKey) -> Result<(), CoreError> {
        learn_known_hosts_path(host, port, key, &self.pylentra).map_err(ssh_err)
    }

    /// Removes every line of the Pylentra file matching `host:port`; returns how many.
    pub fn forget(&self, host: &str, port: u16) -> Result<usize, CoreError> {
        let hits: Vec<usize> = known_host_keys_path(host, port, &self.pylentra)
            .map_err(ssh_err)?
            .into_iter()
            .map(|(line, _)| line)
            .collect();
        if hits.is_empty() {
            return Ok(0);
        }
        let text = std::fs::read_to_string(&self.pylentra)?;
        // russh numbers lines from 1 but does not count '#' comment lines.
        let mut n = 0;
        let kept: String = text
            .split_inclusive('\n')
            .filter(|l| {
                if l.starts_with('#') {
                    return true;
                }
                n += 1;
                !hits.contains(&n)
            })
            .collect();
        atomic_write(&self.pylentra, &kept)?;
        Ok(hits.len())
    }
}

fn ssh_err(e: russh::keys::Error) -> CoreError {
    match e {
        russh::keys::Error::IO(e) => CoreError::Io(e),
        e => CoreError::Ssh(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::keys::{Algorithm, PrivateKey};
    use std::fs;

    fn key() -> PublicKey {
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
            .unwrap()
            .public_key()
            .clone()
    }

    fn setup() -> (tempfile::TempDir, KnownHosts) {
        let dir = tempfile::tempdir().unwrap();
        let kh = KnownHosts::new(
            dir.path().join("known_hosts"),
            Some(dir.path().join("ssh_known_hosts")),
        );
        (dir, kh)
    }

    #[test]
    fn unknown_then_learn_then_known() {
        let (_d, kh) = setup();
        let k = key();
        assert_eq!(
            kh.check("example.com", 22, &k).unwrap(),
            HostKeyStatus::Unknown
        );
        kh.learn("example.com", 22, &k).unwrap();
        assert_eq!(
            kh.check("example.com", 22, &k).unwrap(),
            HostKeyStatus::Known
        );
    }

    #[test]
    fn openssh_file_is_trusted_but_never_written() {
        let (d, kh) = setup();
        let k = key();
        let ssh = d.path().join("ssh_known_hosts");
        learn_known_hosts_path("a.example", 22, &k, &ssh).unwrap();
        let before = fs::read(&ssh).unwrap();
        assert_eq!(kh.check("a.example", 22, &k).unwrap(), HostKeyStatus::Known);
        kh.learn("b.example", 22, &key()).unwrap();
        assert_eq!(kh.forget("a.example", 22).unwrap(), 0);
        assert_eq!(fs::read(&ssh).unwrap(), before);
    }

    #[test]
    fn mismatch_reports_source_and_expected_fp() {
        let (_d, kh) = setup();
        let (k1, k2) = (key(), key());
        kh.learn("h", 22, &k1).unwrap();
        assert_eq!(
            kh.check("h", 22, &k2).unwrap(),
            HostKeyStatus::Mismatch {
                source: KeySource::Pylentra,
                expected_fp: fingerprint(&k1)
            }
        );
    }

    #[test]
    fn mismatch_in_openssh_file() {
        let (d, kh) = setup();
        let (k1, k2) = (key(), key());
        learn_known_hosts_path("h", 22, &k1, d.path().join("ssh_known_hosts")).unwrap();
        assert_eq!(
            kh.check("h", 22, &k2).unwrap(),
            HostKeyStatus::Mismatch {
                source: KeySource::OpenSsh,
                expected_fp: fingerprint(&k1)
            }
        );
    }

    #[test]
    fn missing_files_mean_unknown() {
        let (_d, kh) = setup();
        assert_eq!(kh.check("h", 22, &key()).unwrap(), HostKeyStatus::Unknown);
        assert_eq!(
            KnownHosts::new("/nonexistent/dir/kh".into(), None)
                .check("h", 22, &key())
                .unwrap(),
            HostKeyStatus::Unknown
        );
        assert_eq!(kh.forget("h", 22).unwrap(), 0);
    }

    #[test]
    fn ipv6_with_port_learn_check_forget() {
        let (_d, kh) = setup();
        let (k, k2) = (key(), key());
        kh.learn("::1", 2222, &k).unwrap();
        kh.learn("::1", 22, &k2).unwrap();
        assert_eq!(kh.check("::1", 2222, &k).unwrap(), HostKeyStatus::Known);
        assert_eq!(kh.forget("::1", 2222).unwrap(), 1);
        assert_eq!(kh.check("::1", 2222, &k).unwrap(), HostKeyStatus::Unknown);
        assert_eq!(kh.check("::1", 22, &k2).unwrap(), HostKeyStatus::Known);
    }

    #[test]
    fn forget_hashed_entry() {
        let (d, kh) = setup();
        let k = key();
        // hashed form of "example.com" (salt/hash from russh's own tests)
        let line = format!(
            "|1|O33ESRMWPVkMYIwJ1Uw+n877jTo=|nuuC5vEqXlEZ/8BXQR7m619W6Ak= {}\n",
            k.to_openssh().unwrap()
        );
        let keep = format!("other {}\n", k.to_openssh().unwrap());
        fs::write(d.path().join("known_hosts"), format!("{keep}{line}")).unwrap();
        assert_eq!(
            kh.check("example.com", 22, &k).unwrap(),
            HostKeyStatus::Known
        );
        assert_eq!(kh.forget("example.com", 22).unwrap(), 1);
        assert_eq!(
            fs::read_to_string(d.path().join("known_hosts")).unwrap(),
            keep
        );
    }

    #[test]
    fn forget_keeps_other_lines_despite_comments_and_blanks() {
        let (d, kh) = setup();
        let k = key();
        let pk = k.to_openssh().unwrap();
        let path = d.path().join("known_hosts");
        let content = format!("# note\n\nkeep1 {pk}\n# another\ngone {pk}\nkeep2 {pk}\n");
        fs::write(&path, content).unwrap();
        assert_eq!(kh.forget("gone", 22).unwrap(), 1);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("# note\n\nkeep1 {pk}\n# another\nkeep2 {pk}\n")
        );
    }
}
