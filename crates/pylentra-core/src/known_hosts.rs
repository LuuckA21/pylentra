use crate::error::CoreError;
use crate::store::atomic_write;
use data_encoding::BASE64_MIME;
use hmac::{Hmac, KeyInit, Mac};
use russh::keys::known_hosts::{
    check_known_hosts_path, known_host_keys_path, learn_known_hosts_path,
};
use russh::keys::parse_public_key_base64;
use russh::keys::ssh_key::{HashAlg, PublicKey};
use serde::Serialize;
use sha1::Sha1;
use std::path::{Path, PathBuf};
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

/// One host name of one line of Pylentra's known_hosts; `host: None` is a hashed entry.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct KnownHostEntry {
    pub host: Option<String>,
    pub port: u16,
    pub algo: String,
    pub fingerprint: String,
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
        let mut known = match check_known_hosts_path(host, port, key, &self.pylentra) {
            Ok(found) => found,
            Err(russh::keys::Error::KeyChanged { line }) => {
                let expected_fp = known_host_keys_path(host, port, &self.pylentra)
                    .map_err(ssh_err)?
                    .iter()
                    .find(|(l, _)| *l == line)
                    .map(|(_, k)| fingerprint(k))
                    .unwrap_or_default();
                return Ok(HostKeyStatus::Mismatch {
                    source: KeySource::Pylentra,
                    expected_fp,
                });
            }
            Err(e) => return Err(ssh_err(e)),
        };
        if let Some(path) = &self.openssh {
            match scan_openssh(path, host, port, key)? {
                Scan::Changed(expected_fp) => {
                    return Ok(HostKeyStatus::Mismatch {
                        source: KeySource::OpenSsh,
                        expected_fp,
                    });
                }
                Scan::Found => known = true,
                Scan::Absent => {}
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

    /// Entries of the Pylentra file in file order, one per host name; a missing
    /// file is empty and unreadable lines are skipped.
    pub fn list(&self) -> Result<Vec<KnownHostEntry>, CoreError> {
        let bytes = match std::fs::read(&self.pylentra) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut entries = Vec::new();
        for (i, line) in String::from_utf8_lossy(&bytes).lines().enumerate() {
            if line.trim().is_empty() || line.starts_with(['#', '@']) {
                continue;
            }
            let mut fields = line.split_ascii_whitespace();
            let (Some(hosts), Some(algo), Some(b64)) =
                (fields.next(), fields.next(), fields.next())
            else {
                log::warn!("skipping malformed line {} of known_hosts", i + 1);
                continue;
            };
            let Ok(key) = parse_public_key_base64(b64) else {
                log::warn!("skipping unusable key on line {} of known_hosts", i + 1);
                continue;
            };
            let fingerprint = fingerprint(&key);
            for name in hosts.split(',') {
                let (host, port) = split_host_port(name);
                entries.push(KnownHostEntry {
                    host,
                    port,
                    algo: algo.to_owned(),
                    fingerprint: fingerprint.clone(),
                });
            }
        }
        Ok(entries)
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

enum Scan {
    Absent,
    Found,
    /// Fingerprint of the recorded key that differs from the presented one.
    Changed(String),
}

/// Scans the user's own known_hosts line by line instead of using russh's
/// helper, which fails on the first unparseable matching line (ssh-dss,
/// unsupported types) and would hide the valid entries around it.
/// Same semantics as russh otherwise: a differing key of the same algorithm
/// wins over a match. A missing file means no entries.
fn scan_openssh(path: &Path, host: &str, port: u16, key: &PublicKey) -> Result<Scan, CoreError> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Scan::Absent),
        Err(e) => return Err(e.into()),
    };
    let host_port = if port == 22 {
        host.to_owned()
    } else {
        format!("[{host}]:{port}")
    };
    let mut found = false;
    for line in String::from_utf8_lossy(&bytes).lines() {
        // '@cert-authority' / '@revoked' markers are not plain host keys.
        if line.starts_with(['#', '@']) {
            continue;
        }
        let mut fields = line.split_ascii_whitespace();
        let (Some(hosts), Some(_), Some(b64)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if !hosts.split(',').any(|p| host_matches(&host_port, p)) {
            continue;
        }
        let recorded = match parse_public_key_base64(b64) {
            Ok(k) => k,
            Err(e) => {
                log::warn!("ignoring unusable entry in OpenSSH known_hosts: {e}");
                continue;
            }
        };
        if recorded.algorithm() == key.algorithm() {
            if recorded == *key {
                found = true;
            } else {
                return Ok(Scan::Changed(fingerprint(&recorded)));
            }
        }
    }
    Ok(if found { Scan::Found } else { Scan::Absent })
}

/// `[h]:p` -> (h, p), `h` -> (h, 22); a hashed name hides both (port 22 is a placeholder).
fn split_host_port(name: &str) -> (Option<String>, u16) {
    if name.starts_with("|1|") {
        return (None, 22);
    }
    if let Some((h, p)) = name.strip_prefix('[').and_then(|r| r.split_once("]:"))
        && let Ok(port) = p.parse()
    {
        return (Some(h.to_owned()), port);
    }
    (Some(name.to_owned()), 22)
}

/// Plain pattern or hashed `|1|salt|hash` (HMAC-SHA1 of the host string).
fn host_matches(host_port: &str, pattern: &str) -> bool {
    let Some(hashed) = pattern.strip_prefix("|1|") else {
        return pattern == host_port;
    };
    let mut parts = hashed.split('|');
    let (Some(Ok(salt)), Some(Ok(hash))) = (
        parts.next().map(|p| BASE64_MIME.decode(p.as_bytes())),
        parts.next().map(|p| BASE64_MIME.decode(p.as_bytes())),
    ) else {
        return false;
    };
    Hmac::<Sha1>::new_from_slice(&salt)
        .is_ok_and(|mac| mac.chain_update(host_port).verify_slice(&hash).is_ok())
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

    #[test]
    fn unparseable_openssh_entry_is_ignored() {
        let (d, kh) = setup();
        let k = key();
        fs::write(
            d.path().join("ssh_known_hosts"),
            "h ssh-dss AAAAB3NzaC1kc3MAAAA=\n",
        )
        .unwrap();
        assert_eq!(kh.check("h", 22, &k).unwrap(), HostKeyStatus::Unknown);
        kh.learn("h", 22, &k).unwrap();
        assert_eq!(kh.check("h", 22, &k).unwrap(), HostKeyStatus::Known);
    }

    #[test]
    fn openssh_file_noise_does_not_error() {
        let (d, kh) = setup();
        let (k, other) = (key(), key());
        let pk = other.to_openssh().unwrap();
        let ssh = d.path().join("ssh_known_hosts");
        fs::write(
            &ssh,
            format!(
                "# comment\n\n@cert-authority *.x ssh-rsa AAAAB3NzaC1yc2E=\n@revoked h {pk}\n*.example.com {pk}\nwild.example.com {pk}\n"
            ),
        )
        .unwrap();
        assert_eq!(kh.check("h", 22, &k).unwrap(), HostKeyStatus::Unknown);
        assert_eq!(
            kh.check("a.example.com", 22, &k).unwrap(),
            HostKeyStatus::Unknown
        );
        assert_eq!(
            kh.check("wild.example.com", 22, &other).unwrap(),
            HostKeyStatus::Known
        );
        assert!(matches!(
            kh.check("wild.example.com", 22, &k).unwrap(),
            HostKeyStatus::Mismatch {
                source: KeySource::OpenSsh,
                ..
            }
        ));
    }

    #[test]
    fn unparseable_openssh_line_does_not_hide_valid_entries() {
        let (d, kh) = setup();
        let (k1, k2) = (key(), key());
        fs::write(
            d.path().join("ssh_known_hosts"),
            format!(
                "h ssh-dss AAAAB3NzaC1kc3MAAAA=\nh {}\n",
                k1.to_openssh().unwrap()
            ),
        )
        .unwrap();
        assert_eq!(kh.check("h", 22, &k1).unwrap(), HostKeyStatus::Known);
        assert_eq!(
            kh.check("h", 22, &k2).unwrap(),
            HostKeyStatus::Mismatch {
                source: KeySource::OpenSsh,
                expected_fp: fingerprint(&k1)
            }
        );
    }

    #[test]
    fn hashed_openssh_entry_known_and_mismatch() {
        let (d, kh) = setup();
        let (k1, k2) = (key(), key());
        // hashed "example.com" (salt/hash from russh's own tests)
        fs::write(
            d.path().join("ssh_known_hosts"),
            format!(
                "|1|O33ESRMWPVkMYIwJ1Uw+n877jTo=|nuuC5vEqXlEZ/8BXQR7m619W6Ak= {}\n",
                k1.to_openssh().unwrap()
            ),
        )
        .unwrap();
        assert_eq!(
            kh.check("example.com", 22, &k1).unwrap(),
            HostKeyStatus::Known
        );
        assert_eq!(
            kh.check("other.com", 22, &k1).unwrap(),
            HostKeyStatus::Unknown
        );
        assert_eq!(
            kh.check("example.com", 22, &k2).unwrap(),
            HostKeyStatus::Mismatch {
                source: KeySource::OpenSsh,
                expected_fp: fingerprint(&k1)
            }
        );
    }

    #[test]
    fn list_shows_learned_hosts() {
        let (_d, kh) = setup();
        let (k1, k2) = (key(), key());
        kh.learn("a", 22, &k1).unwrap();
        kh.learn("b", 2222, &k2).unwrap();
        assert_eq!(
            kh.list().unwrap(),
            vec![
                KnownHostEntry {
                    host: Some("a".into()),
                    port: 22,
                    algo: "ssh-ed25519".into(),
                    fingerprint: fingerprint(&k1)
                },
                KnownHostEntry {
                    host: Some("b".into()),
                    port: 2222,
                    algo: "ssh-ed25519".into(),
                    fingerprint: fingerprint(&k2)
                },
            ]
        );
    }

    #[test]
    fn list_marks_hashed_lines() {
        let (d, kh) = setup();
        let k = key();
        fs::write(
            d.path().join("known_hosts"),
            format!(
                "|1|O33ESRMWPVkMYIwJ1Uw+n877jTo=|nuuC5vEqXlEZ/8BXQR7m619W6Ak= {}\n",
                k.to_openssh().unwrap()
            ),
        )
        .unwrap();
        let list = kh.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].host, None);
        assert_eq!(list[0].fingerprint, fingerprint(&k));
    }

    #[test]
    fn list_missing_file_is_empty() {
        let (_d, kh) = setup();
        assert!(kh.list().unwrap().is_empty());
    }

    #[test]
    fn list_after_forget() {
        let (_d, kh) = setup();
        kh.learn("a", 22, &key()).unwrap();
        kh.learn("b", 2222, &key()).unwrap();
        kh.forget("a", 22).unwrap();
        let list = kh.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].host.as_deref(), list[0].port), (Some("b"), 2222));
    }

    #[test]
    fn list_skips_noise_splits_names_and_parses_ipv6() {
        let (d, kh) = setup();
        let k = key();
        let pk = k.to_openssh().unwrap();
        fs::write(
            d.path().join("known_hosts"),
            format!(
                "# c\n\n@revoked r {pk}\n@cert-authority *.x {pk}\nbroken ssh-ed25519 !!!\nshort\nx,[y]:2200 {pk}\n[::1]:2222 {pk}\n"
            ),
        )
        .unwrap();
        let got: Vec<_> = kh
            .list()
            .unwrap()
            .into_iter()
            .map(|e| (e.host.unwrap(), e.port))
            .collect();
        assert_eq!(
            got,
            [("x".into(), 22), ("y".into(), 2200), ("::1".into(), 2222)]
        );
    }
}
