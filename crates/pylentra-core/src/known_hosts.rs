use crate::error::CoreError;
use crate::model::check_host_name;
use crate::store::atomic_write;
use data_encoding::BASE64_MIME;
use hmac::{Hmac, KeyInit, Mac};
use russh::Preferred;
use russh::keys::known_hosts::{known_host_keys_path, learn_known_hosts_path};
use russh::keys::parse_public_key_base64;
use russh::keys::ssh_key::{Algorithm, HashAlg, PublicKey};
use serde::Serialize;
use sha1::Sha1;
use std::path::{Path, PathBuf};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub enum KeySource {
    Pylentra,
    OpenSsh,
    /// The system-wide `ssh_known_hosts`.
    System,
}

#[derive(Debug, PartialEq)]
pub enum HostKeyStatus {
    Known,
    Unknown,
    Mismatch {
        source: KeySource,
        expected_fp: String,
        /// The known key is of another type than the presented one.
        type_changed: bool,
    },
    /// An `@revoked` line records this very key.
    Revoked {
        source: KeySource,
        fingerprint: String,
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
    /// Never written.
    read_only: Vec<(KeySource, PathBuf)>,
}

pub fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

impl KnownHosts {
    /// `openssh` is the read-only `~/.ssh/known_hosts`; only `pylentra` is ever written.
    pub fn new(pylentra: PathBuf, openssh: Option<PathBuf>) -> Self {
        let read_only = openssh
            .map(|p| (KeySource::OpenSsh, p))
            .into_iter()
            .collect();
        Self {
            pylentra,
            read_only,
        }
    }

    /// Also reads the system-wide `ssh_known_hosts` (never written).
    pub fn with_system(mut self, path: Option<PathBuf>) -> Self {
        self.read_only.extend(path.map(|p| (KeySource::System, p)));
        self
    }

    /// Pylentra's file first, then the read-only ones; any recorded key that differs
    /// for the same algorithm is a mismatch only if no line in any file records the
    /// presented key (as OpenSSH). A key of a type none of the files has for this
    /// host, while they have keys of other types, is a mismatch too: connections
    /// prefer the known types (see `algorithms`), so an honest server presents one
    /// of them.
    pub fn check(
        &self,
        host: &str,
        port: u16,
        key: &PublicKey,
    ) -> Result<HostKeyStatus, CoreError> {
        // Before russh is asked: a revoked key must lose even to an accepting line.
        if let Some(source) = self.revoked_by(host, port, key)? {
            return Ok(HostKeyStatus::Revoked {
                source,
                fingerprint: fingerprint(key),
            });
        }
        let mut recorded: Vec<(KeySource, PublicKey)> =
            known_host_keys_path(host, port, &self.pylentra)
                .map_err(ssh_err)?
                .into_iter()
                .map(|(_, k)| (KeySource::Pylentra, k))
                .collect();
        for (source, path) in &self.read_only {
            for k in recorded_keys(source, path, host, port)? {
                recorded.push((source.clone(), k));
            }
        }
        // As OpenSSH: any line recording the key makes it known, whatever else matches.
        if recorded.iter().any(|(_, k)| k == key) {
            return Ok(HostKeyStatus::Known);
        }
        let mismatch = |(source, k): &(KeySource, PublicKey), type_changed| {
            Ok(HostKeyStatus::Mismatch {
                source: source.clone(),
                expected_fp: fingerprint(k),
                type_changed,
            })
        };
        if let Some(same) = recorded
            .iter()
            .find(|(_, k)| k.algorithm() == key.algorithm())
        {
            return mismatch(same, false);
        }
        // Only types Pylentra can negotiate count: an honest server could never present the others.
        match recorded.iter().find(|(_, k)| negotiable(&k.algorithm())) {
            Some(other) => mismatch(other, true),
            None => Ok(HostKeyStatus::Unknown),
        }
    }

    /// The first file with an `@revoked` line for `host:port` recording `key`; patterns
    /// and hashed names count in every file, Pylentra's own included.
    fn revoked_by(
        &self,
        host: &str,
        port: u16,
        key: &PublicKey,
    ) -> Result<Option<KeySource>, CoreError> {
        let host_port = host_port(host, port);
        let own = (KeySource::Pylentra, &self.pylentra);
        for (source, path) in
            std::iter::once(own).chain(self.read_only.iter().map(|(s, p)| (s.clone(), p)))
        {
            let Some(text) = read_text(&source, path)? else {
                continue;
            };
            let hit = key_lines(&text).any(|(_, marker, hosts, _, b64)| {
                marker == Marker::Revoked
                    && host_field_matches(&host_port, hosts)
                    && match parse_public_key_base64(b64) {
                        Ok(k) => k == *key,
                        Err(e) => {
                            log::warn!("ignoring unusable @revoked entry: {e}");
                            false
                        }
                    }
            });
            if hit {
                return Ok(Some(source));
            }
        }
        Ok(None)
    }

    /// Whether any file has a `@cert-authority` line covering `host:port`. The key is not
    /// parsed: the line only matters as a warning, since Pylentra ignores certificates.
    pub fn cert_authority(&self, host: &str, port: u16) -> Result<bool, CoreError> {
        let host_port = host_port(host, port);
        let own = (KeySource::Pylentra, &self.pylentra);
        for (source, path) in
            std::iter::once(own).chain(self.read_only.iter().map(|(s, p)| (s.clone(), p)))
        {
            let Some(text) = read_text(&source, path)? else {
                continue;
            };
            if key_lines(&text).any(|(_, marker, hosts, _, _)| {
                marker == Marker::CertAuthority && host_field_matches(&host_port, hosts)
            }) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Algorithms of every key any file records for `host:port`, to put first in negotiation.
    pub fn algorithms(&self, host: &str, port: u16) -> Result<Vec<Algorithm>, CoreError> {
        let mut keys: Vec<PublicKey> = known_host_keys_path(host, port, &self.pylentra)
            .map_err(ssh_err)?
            .into_iter()
            .map(|(_, k)| k)
            .collect();
        for (source, path) in &self.read_only {
            keys.extend(recorded_keys(source, path, host, port)?);
        }
        Ok(keys.iter().map(PublicKey::algorithm).collect())
    }

    /// Records the key in the Pylentra file only.
    pub fn learn(&self, host: &str, port: u16, key: &PublicKey) -> Result<(), CoreError> {
        // russh writes the name unescaped; saved connections are checked, this covers the rest.
        check_host_name(host)?;
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
        for (n, _, hosts, algo, b64) in
            key_lines(&String::from_utf8_lossy(&bytes)).filter(|l| l.1 == Marker::Plain)
        {
            let Ok(key) = parse_public_key_base64(b64) else {
                log::warn!("skipping unusable key on line {n} of known_hosts");
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

    /// Empties Pylentra's own file; the user's OpenSSH file is never touched.
    pub fn clear(&self) -> Result<(), CoreError> {
        if !self.pylentra.exists() {
            return Ok(());
        }
        atomic_write(&self.pylentra, "")
    }
}

/// The text of a known_hosts file; `None` for a missing file (no entries).
fn read_text(source: &KeySource, path: &Path) -> Result<Option<String>, CoreError> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(String::from_utf8_lossy(&b).into_owned())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        // As OpenSSH does: a root-only system file must not stop every connection.
        // The user's own file still fails, so a changed key there is never skipped.
        Err(e)
            if *source == KeySource::System && e.kind() == std::io::ErrorKind::PermissionDenied =>
        {
            static WARNED: std::sync::Once = std::sync::Once::new();
            WARNED.call_once(|| log::warn!("cannot read the system known_hosts: {e}"));
            Ok(None)
        }
        Err(e) => Err(e.into()),
    }
}

fn host_port(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_owned()
    } else {
        format!("[{host}]:{port}")
    }
}

/// Keys a read-only (OpenSSH) known_hosts records for `host:port`, scanned line by
/// line instead of with russh's helper, which fails on the first unparseable
/// matching line (ssh-dss, unsupported types) and would hide the valid entries
/// around it. A missing file means no entries.
fn recorded_keys(
    source: &KeySource,
    path: &Path,
    host: &str,
    port: u16,
) -> Result<Vec<PublicKey>, CoreError> {
    let Some(text) = read_text(source, path)? else {
        return Ok(Vec::new());
    };
    let host_port = host_port(host, port);
    let mut keys = Vec::new();
    for (_, marker, hosts, _, b64) in key_lines(&text) {
        if marker != Marker::Plain || !host_field_matches(&host_port, hosts) {
            continue;
        }
        match parse_public_key_base64(b64) {
            Ok(k) => keys.push(k),
            Err(e) => log::warn!("ignoring unusable entry in OpenSSH known_hosts: {e}"),
        }
    }
    Ok(keys)
}

/// Host key algorithms Pylentra offers, in russh's order, without SHA-1 `ssh-rsa`
/// signatures (as OpenSSH 8.8+); rsa-sha2-* stay.
pub(crate) fn host_key_algorithms() -> Vec<Algorithm> {
    Preferred::DEFAULT
        .key
        .iter()
        .filter(|a| **a != Algorithm::Rsa { hash: None })
        .cloned()
        .collect()
}

/// Whether a recorded key of type `recorded` is used by `offered`: a recorded RSA key is
/// `Rsa { hash: None }` whatever signature hash it is used with.
pub(crate) fn same_key_type(recorded: &Algorithm, offered: &Algorithm) -> bool {
    recorded == offered
        || matches!(
            (recorded, offered),
            (Algorithm::Rsa { .. }, Algorithm::Rsa { .. })
        )
}

fn negotiable(recorded: &Algorithm) -> bool {
    host_key_algorithms()
        .iter()
        .any(|a| same_key_type(recorded, a))
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Marker {
    Plain,
    Revoked,
    CertAuthority,
}

/// `(line number, marker, hosts, algorithm, base64 key)` of each line that can
/// hold a key; `@revoked` and `@cert-authority` lines are yielded too, for callers
/// to filter. Comments, unknown '@' markers and lines with too few fields are
/// skipped.
fn key_lines(text: &str) -> impl Iterator<Item = (usize, Marker, &str, &str, &str)> {
    text.lines().enumerate().filter_map(|(i, line)| {
        if line.starts_with('#') {
            return None;
        }
        let mut fields = line.split_ascii_whitespace();
        let mut first = fields.next()?;
        let marker = match first {
            "@revoked" => Marker::Revoked,
            "@cert-authority" => Marker::CertAuthority,
            _ if first.starts_with('@') => return None,
            _ => Marker::Plain,
        };
        if marker != Marker::Plain {
            first = fields.next()?;
        }
        Some((i + 1, marker, first, fields.next()?, fields.next()?))
    })
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

/// A hosts field: one hashed `|1|salt|hash` entry (HMAC-SHA1 of the host
/// string), matched exactly, or a comma list of patterns with `!` negation.
pub(crate) fn host_field_matches(host_port: &str, field: &str) -> bool {
    let Some(hashed) = field.strip_prefix("|1|") else {
        return crate::pattern::list_matches(field.split(','), host_port);
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
                expected_fp: fingerprint(&k1),
                type_changed: false
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
                expected_fp: fingerprint(&k1),
                type_changed: false
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
        assert!(matches!(
            kh.check("a.example.com", 22, &k).unwrap(),
            HostKeyStatus::Mismatch {
                source: KeySource::OpenSsh,
                ..
            }
        ));
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
                expected_fp: fingerprint(&k1),
                type_changed: false
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
                expected_fp: fingerprint(&k1),
                type_changed: false
            }
        );
    }

    fn ecdsa() -> PublicKey {
        let curve = russh::keys::EcdsaCurve::NistP256;
        PrivateKey::random(&mut rand::rng(), Algorithm::Ecdsa { curve })
            .unwrap()
            .public_key()
            .clone()
    }

    fn write_ssh(d: &tempfile::TempDir, text: String) {
        fs::write(d.path().join("ssh_known_hosts"), text).unwrap();
    }

    fn is_openssh_mismatch(s: HostKeyStatus) -> bool {
        matches!(
            s,
            HostKeyStatus::Mismatch {
                source: KeySource::OpenSsh,
                type_changed: false,
                ..
            }
        )
    }

    #[test]
    fn wildcard_line_is_known_and_blocks_other_keys() {
        let (d, kh) = setup();
        let (k, other) = (key(), key());
        write_ssh(&d, format!("*.example.com {}\n", k.to_openssh().unwrap()));
        assert_eq!(
            kh.check("a.example.com", 22, &k).unwrap(),
            HostKeyStatus::Known
        );
        assert!(is_openssh_mismatch(
            kh.check("a.example.com", 22, &other).unwrap()
        ));
    }

    #[test]
    fn wildcard_and_exact_line_either_key_is_known() {
        for reversed in [false, true] {
            let (d, kh) = setup();
            let (k1, k2, k3) = (key(), key(), key());
            let mut lines = [
                format!("*.corp.example {}", k1.to_openssh().unwrap()),
                format!("a.corp.example {}", k2.to_openssh().unwrap()),
            ];
            if reversed {
                lines.reverse();
            }
            write_ssh(&d, lines.join("\n") + "\n");
            for k in [&k1, &k2] {
                assert_eq!(
                    kh.check("a.corp.example", 22, k).unwrap(),
                    HostKeyStatus::Known
                );
            }
            assert!(matches!(
                kh.check("a.corp.example", 22, &k3).unwrap(),
                HostKeyStatus::Mismatch {
                    source: KeySource::OpenSsh,
                    type_changed: false,
                    ..
                }
            ));
        }
    }

    #[test]
    fn key_known_in_another_file_is_known() {
        let (d, kh) = setup();
        let (k1, k2, k3) = (key(), key(), key());
        kh.learn("h", 22, &k1).unwrap();
        write_ssh(&d, format!("h {}\n", k2.to_openssh().unwrap()));
        assert_eq!(kh.check("h", 22, &k2).unwrap(), HostKeyStatus::Known);
        assert_eq!(kh.check("h", 22, &k1).unwrap(), HostKeyStatus::Known);
        assert_eq!(
            kh.check("h", 22, &k3).unwrap(),
            HostKeyStatus::Mismatch {
                source: KeySource::Pylentra,
                expected_fp: fingerprint(&k1),
                type_changed: false
            }
        );
    }

    #[test]
    fn equal_key_plus_another_type_is_known() {
        let (d, kh) = setup();
        let (k, e) = (key(), ecdsa());
        write_ssh(
            &d,
            format!(
                "h {}\nh {}\n",
                e.to_openssh().unwrap(),
                k.to_openssh().unwrap()
            ),
        );
        assert_eq!(kh.check("h", 22, &k).unwrap(), HostKeyStatus::Known);
        assert_eq!(kh.check("h", 22, &e).unwrap(), HostKeyStatus::Known);
    }

    #[test]
    fn mismatch_points_at_first_differing_line() {
        let (d, kh) = setup();
        let (k1, k2, k3, e) = (key(), key(), key(), ecdsa());
        // Same-type mismatch: Pylentra's line wins over the read-only file's.
        kh.learn("h", 22, &k1).unwrap();
        write_ssh(&d, format!("h {}\n", k2.to_openssh().unwrap()));
        assert_eq!(
            kh.check("h", 22, &k3).unwrap(),
            HostKeyStatus::Mismatch {
                source: KeySource::Pylentra,
                expected_fp: fingerprint(&k1),
                type_changed: false
            }
        );
        // Other-type mismatch: only the read-only file has a negotiable key.
        let (d, kh) = setup();
        write_ssh(&d, format!("h {}\n", e.to_openssh().unwrap()));
        assert_eq!(
            kh.check("h", 22, &k3).unwrap(),
            HostKeyStatus::Mismatch {
                source: KeySource::OpenSsh,
                expected_fp: fingerprint(&e),
                type_changed: true
            }
        );
    }

    #[test]
    fn two_keys_in_pylentra_file_either_is_known() {
        let (d, kh) = setup();
        let (k1, k2) = (key(), key());
        fs::write(
            d.path().join("known_hosts"),
            format!(
                "h {}\nh {}\n",
                k1.to_openssh().unwrap(),
                k2.to_openssh().unwrap()
            ),
        )
        .unwrap();
        for k in [&k1, &k2] {
            assert_eq!(kh.check("h", 22, k).unwrap(), HostKeyStatus::Known);
        }
    }

    #[test]
    fn question_mark_and_mixed_case() {
        let (d, kh) = setup();
        let (k, other) = (key(), key());
        write_ssh(
            &d,
            format!("web?.Example.COM {}\n", k.to_openssh().unwrap()),
        );
        assert_eq!(
            kh.check("web1.example.com", 22, &k).unwrap(),
            HostKeyStatus::Known
        );
        assert_eq!(
            kh.check("web10.example.com", 22, &other).unwrap(),
            HostKeyStatus::Unknown
        );
    }

    #[test]
    fn negation_excludes_a_host() {
        let (d, kh) = setup();
        let (k, other) = (key(), key());
        write_ssh(
            &d,
            format!(
                "*.example.com,!bad.example.com {}\n",
                k.to_openssh().unwrap()
            ),
        );
        assert_eq!(
            kh.check("bad.example.com", 22, &other).unwrap(),
            HostKeyStatus::Unknown
        );
        assert!(is_openssh_mismatch(
            kh.check("good.example.com", 22, &other).unwrap()
        ));
    }

    #[test]
    fn negation_only_field_matches_nothing() {
        let (d, kh) = setup();
        let (k, other) = (key(), key());
        write_ssh(
            &d,
            format!("!bad.example.com {}\n", k.to_openssh().unwrap()),
        );
        assert_eq!(
            kh.check("x.example.com", 22, &other).unwrap(),
            HostKeyStatus::Unknown
        );
        assert_eq!(
            kh.check("bad.example.com", 22, &k).unwrap(),
            HostKeyStatus::Unknown
        );
    }

    #[test]
    fn bracketed_port_patterns() {
        let (d, kh) = setup();
        let (k, other) = (key(), key());
        let pk = k.to_openssh().unwrap();
        write_ssh(&d, format!("[*.example.com]:2222 {pk}\n"));
        assert_eq!(
            kh.check("a.example.com", 2222, &k).unwrap(),
            HostKeyStatus::Known
        );
        write_ssh(&d, format!("*.example.com {pk}\n"));
        assert_eq!(
            kh.check("a.example.com", 2222, &other).unwrap(),
            HostKeyStatus::Unknown
        );
        write_ssh(&d, format!("* {pk}\n"));
        assert!(is_openssh_mismatch(kh.check("h", 2222, &other).unwrap()));
    }

    #[test]
    fn algorithms_include_pattern_lines() {
        let (d, kh) = setup();
        let ec = ecdsa();
        write_ssh(&d, format!("*.example.com {}\n", ec.to_openssh().unwrap()));
        assert_eq!(
            kh.algorithms("a.example.com", 22).unwrap(),
            [ec.algorithm()]
        );
    }

    #[test]
    fn only_other_key_types_known_is_a_mismatch() {
        let (d, kh) = setup();
        let (ed, ec) = (key(), ecdsa());
        kh.learn("p", 22, &ed).unwrap();
        assert_eq!(
            kh.check("p", 22, &ec).unwrap(),
            HostKeyStatus::Mismatch {
                source: KeySource::Pylentra,
                expected_fp: fingerprint(&ed),
                type_changed: true
            }
        );
        // OpenSSH's file holds only an ECDSA key: that host still checks as Known for it.
        learn_known_hosts_path("o", 22, &ec, d.path().join("ssh_known_hosts")).unwrap();
        assert_eq!(kh.check("o", 22, &ec).unwrap(), HostKeyStatus::Known);
        assert_eq!(
            kh.check("o", 22, &ed).unwrap(),
            HostKeyStatus::Mismatch {
                source: KeySource::OpenSsh,
                expected_fp: fingerprint(&ec),
                type_changed: true
            }
        );
        // A same-type match anywhere wins over other types recorded elsewhere.
        kh.learn("o", 22, &ed).unwrap();
        assert_eq!(kh.check("o", 22, &ed).unwrap(), HostKeyStatus::Known);
    }

    #[test]
    fn only_types_pylentra_cannot_negotiate_known_is_unknown() {
        let (d, kh) = setup();
        let ed = match key().key_data() {
            russh::keys::ssh_key::public::KeyData::Ed25519(k) => *k,
            _ => unreachable!(),
        };
        let sk = PublicKey::from(russh::keys::ssh_key::public::SkEd25519::new(ed, "ssh:"));
        learn_known_hosts_path("s", 22, &sk, d.path().join("ssh_known_hosts")).unwrap();
        kh.learn("p", 22, &sk).unwrap();
        assert_eq!(kh.check("s", 22, &key()).unwrap(), HostKeyStatus::Unknown);
        assert_eq!(kh.check("p", 22, &key()).unwrap(), HostKeyStatus::Unknown);
    }

    #[test]
    fn algorithms_come_from_every_file() {
        let (d, kh) = setup();
        let kh = kh.with_system(Some(d.path().join("global")));
        kh.learn("h", 2222, &key()).unwrap();
        learn_known_hosts_path("h", 2222, &ecdsa(), d.path().join("global")).unwrap();
        learn_known_hosts_path("h", 22, &ecdsa(), d.path().join("ssh_known_hosts")).unwrap();
        assert_eq!(
            kh.algorithms("h", 2222).unwrap(),
            [Algorithm::Ed25519, ecdsa().algorithm()]
        );
        assert!(kh.algorithms("other", 22).unwrap().is_empty());
    }

    #[test]
    fn system_file_is_read_only_with_its_own_source() {
        let (d, kh) = setup();
        let global = d.path().join("global");
        let kh = kh.with_system(Some(global.clone()));
        let (k1, k2) = (key(), key());
        learn_known_hosts_path("g", 22, &k1, &global).unwrap();
        let before = fs::read(&global).unwrap();
        assert_eq!(kh.check("g", 22, &k1).unwrap(), HostKeyStatus::Known);
        assert_eq!(
            kh.check("g", 22, &k2).unwrap(),
            HostKeyStatus::Mismatch {
                source: KeySource::System,
                expected_fp: fingerprint(&k1),
                type_changed: false
            }
        );
        kh.learn("g", 2222, &k2).unwrap();
        kh.clear().unwrap();
        assert_eq!(fs::read(&global).unwrap(), before);
        // A missing system file has no entries.
        let (_d2, kh) = setup();
        let kh = kh.with_system(Some("/nonexistent/ssh_known_hosts".into()));
        assert_eq!(kh.check("g", 22, &k1).unwrap(), HostKeyStatus::Unknown);
    }

    #[test]
    fn learn_refuses_names_known_hosts_would_split() {
        let (d, kh) = setup();
        for bad in ["a,victim", "x\nvictim ssh-ed25519 AAAA\ny", "a b", "[a]"] {
            assert!(matches!(
                kh.learn(bad, 22, &key()),
                Err(CoreError::InvalidInput { .. })
            ));
        }
        assert!(!d.path().join("known_hosts").exists());
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
    fn clear_empties_own_file_only() {
        let (d, kh) = setup();
        let ssh = d.path().join("ssh_known_hosts");
        let line = format!("o {}\n", key().to_openssh().unwrap());
        fs::write(&ssh, &line).unwrap();
        kh.learn("a", 22, &key()).unwrap();
        kh.learn("b", 2222, &key()).unwrap();
        kh.clear().unwrap();
        assert!(kh.list().unwrap().is_empty());
        assert_eq!(fs::read_to_string(&ssh).unwrap(), line);
        kh.clear().unwrap();
    }

    #[test]
    fn clear_missing_file_is_ok() {
        let (d, kh) = setup();
        kh.clear().unwrap();
        assert!(!d.path().join("known_hosts").exists());
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

    fn revoked(k: &PublicKey) -> HostKeyStatus {
        HostKeyStatus::Revoked {
            source: KeySource::OpenSsh,
            fingerprint: fingerprint(k),
        }
    }

    #[test]
    fn cert_authority_flag() {
        let (d, kh) = setup();
        assert!(!kh.cert_authority("a.corp.example", 22).unwrap());
        fs::write(
            d.path().join("ssh_known_hosts"),
            format!(
                "@cert-authority *.corp.example {}\n",
                key().to_openssh().unwrap()
            ),
        )
        .unwrap();
        assert!(kh.cert_authority("a.corp.example", 22).unwrap());
        assert!(!kh.cert_authority("other.example", 22).unwrap());
    }

    #[test]
    fn revoked_in_openssh_file_wins_over_pylentra() {
        let (d, kh) = setup();
        let k = key();
        kh.learn("h", 22, &k).unwrap();
        fs::write(
            d.path().join("ssh_known_hosts"),
            format!("@revoked h {}\n", k.to_openssh().unwrap()),
        )
        .unwrap();
        assert_eq!(kh.check("h", 22, &k).unwrap(), revoked(&k));
    }

    #[test]
    fn revoked_wins_over_another_accepting_line() {
        let (d, kh) = setup();
        let k = key();
        let o = k.to_openssh().unwrap();
        fs::write(
            d.path().join("ssh_known_hosts"),
            format!("h {o}\n@revoked h {o}\n"),
        )
        .unwrap();
        assert_eq!(kh.check("h", 22, &k).unwrap(), revoked(&k));
    }

    #[test]
    fn revoked_in_pylentra_file() {
        let (d, kh) = setup();
        let k = key();
        fs::write(
            d.path().join("known_hosts"),
            format!("@revoked h {}\n", k.to_openssh().unwrap()),
        )
        .unwrap();
        assert_eq!(
            kh.check("h", 22, &k).unwrap(),
            HostKeyStatus::Revoked {
                source: KeySource::Pylentra,
                fingerprint: fingerprint(&k)
            }
        );
    }

    #[test]
    fn revoked_in_system_file() {
        let (d, kh) = setup();
        let k = key();
        let global = d.path().join("global");
        fs::write(&global, format!("@revoked h {}\n", k.to_openssh().unwrap())).unwrap();
        let kh = kh.with_system(Some(global));
        assert_eq!(
            kh.check("h", 22, &k).unwrap(),
            HostKeyStatus::Revoked {
                source: KeySource::System,
                fingerprint: fingerprint(&k)
            }
        );
    }

    #[test]
    fn revoked_star_covers_every_host_and_port() {
        let (d, kh) = setup();
        let k = key();
        fs::write(
            d.path().join("ssh_known_hosts"),
            format!("@revoked * {}\n", k.to_openssh().unwrap()),
        )
        .unwrap();
        assert_eq!(kh.check("a", 22, &k).unwrap(), revoked(&k));
        assert_eq!(kh.check("b", 2222, &k).unwrap(), revoked(&k));
    }

    #[test]
    fn revoked_hashed_host() {
        let (d, kh) = setup();
        let k = key();
        fs::write(
            d.path().join("ssh_known_hosts"),
            format!(
                "@revoked |1|O33ESRMWPVkMYIwJ1Uw+n877jTo=|nuuC5vEqXlEZ/8BXQR7m619W6Ak= {}\n",
                k.to_openssh().unwrap()
            ),
        )
        .unwrap();
        assert_eq!(kh.check("example.com", 22, &k).unwrap(), revoked(&k));
    }

    #[test]
    fn revoked_other_key_does_not_block() {
        let (d, kh) = setup();
        let (k, other) = (key(), key());
        fs::write(
            d.path().join("ssh_known_hosts"),
            format!("@revoked h {}\n", other.to_openssh().unwrap()),
        )
        .unwrap();
        assert_eq!(kh.check("h", 22, &k).unwrap(), HostKeyStatus::Unknown);
    }

    #[test]
    fn revoked_line_with_unparseable_key_is_skipped() {
        let (d, kh) = setup();
        let k = key();
        fs::write(
            d.path().join("ssh_known_hosts"),
            format!(
                "@revoked * ssh-ed25519 !!!\nh {}\n",
                k.to_openssh().unwrap()
            ),
        )
        .unwrap();
        assert_eq!(kh.check("h", 22, &k).unwrap(), HostKeyStatus::Known);
    }
}
