use crate::error::CoreError;
use crate::model::{Connection, Group, MAX_JUMP_DEPTH, Settings, Warning, lenient_extra};
use crate::secrets::{SecretKind, SecretStore};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use ts_rs::TS;
use uuid::Uuid;

pub const CONNECTIONS_VERSION: u32 = 3;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct Library {
    pub version: u32,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(flatten, deserialize_with = "lenient_extra")]
    #[ts(skip)]
    pub extra: toml::Table,
}

impl Default for Library {
    fn default() -> Self {
        Self {
            version: CONNECTIONS_VERSION,
            groups: vec![],
            connections: vec![],
            extra: toml::Table::new(),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum LoadOutcome {
    Ok,
    CreatedEmpty,
    RecoveredFromCorrupt { backup: PathBuf },
    ReadOnlyNewerVersion { found: u32 },
}

/// Writes `<path>.tmp`, syncs it and renames it over `path`, so readers never
/// see a half-written file.
pub(crate) fn atomic_write(path: &Path, contents: &str) -> Result<(), CoreError> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = OsString::from(path);
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let write = || -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    };
    write().inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })?;
    Ok(())
}

/// Moves an unreadable file to `<name>.corrupt-<unix_secs>` (never onto an
/// existing file) and returns the new path.
pub(crate) fn quarantine(path: &Path) -> Result<PathBuf, CoreError> {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let mut backup = path.with_file_name(format!("{name}.corrupt-{secs}"));
    for n in 1.. {
        if !backup.exists() {
            break;
        }
        backup = path.with_file_name(format!("{name}.corrupt-{secs}-{n}"));
    }
    fs::rename(path, &backup)?;
    Ok(backup)
}

pub(crate) fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Option<T> {
    toml::from_str(std::str::from_utf8(bytes).ok()?).ok()
}

#[derive(Deserialize)]
pub(crate) struct VersionOnly {
    pub(crate) version: u32,
}

pub struct ConnectionStore {
    path: PathBuf,
    library: Library,
    read_only: bool,
}

impl ConnectionStore {
    pub fn load(path: PathBuf) -> Result<(Self, LoadOutcome), CoreError> {
        let mut store = Self {
            path,
            library: Library::default(),
            read_only: false,
        };
        let bytes = match fs::read(&store.path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((store, LoadOutcome::CreatedEmpty));
            }
            Err(e) => return Err(e.into()),
        };
        let outcome = match parse::<VersionOnly>(&bytes) {
            None => {
                let backup = quarantine(&store.path)?;
                LoadOutcome::RecoveredFromCorrupt { backup }
            }
            Some(VersionOnly { version }) if version > CONNECTIONS_VERSION => {
                // Show what we can, but never write a format we do not know.
                store.read_only = true;
                store.library = parse(&bytes).unwrap_or_default();
                LoadOutcome::ReadOnlyNewerVersion { found: version }
            }
            Some(_) => match parse::<Library>(&bytes) {
                Some(lib) => {
                    store.library = lib;
                    LoadOutcome::Ok
                }
                None => {
                    let backup = quarantine(&store.path)?;
                    LoadOutcome::RecoveredFromCorrupt { backup }
                }
            },
        };
        Ok((store, outcome))
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    pub fn get(&self, id: Uuid) -> Result<&Connection, CoreError> {
        self.library
            .connections
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| CoreError::NotFound {
                what: "connection".into(),
            })
    }

    fn writable(&self) -> Result<(), CoreError> {
        if self.read_only {
            return Err(CoreError::ReadOnly);
        }
        Ok(())
    }

    /// Saves `candidate` and only then makes it the live library.
    fn commit(&mut self, mut candidate: Library) -> Result<(), CoreError> {
        self.writable()?;
        candidate.version = CONNECTIONS_VERSION;
        let text = toml::to_string_pretty(&candidate).map_err(|e| CoreError::InvalidInput {
            field: "library".into(),
            reason: e.to_string(),
        })?;
        atomic_write(&self.path, &text)?;
        self.library = candidate;
        Ok(())
    }

    /// Every check `upsert_connection` makes, without writing anything.
    pub fn check_connection(&self, c: &Connection) -> Result<Vec<Warning>, CoreError> {
        self.candidate_with(c.clone()).map(|(_, warnings)| warnings)
    }

    pub fn upsert_connection(&mut self, c: Connection) -> Result<Vec<Warning>, CoreError> {
        let (candidate, warnings) = self.candidate_with(c)?;
        self.commit(candidate)?;
        Ok(warnings)
    }

    /// The library with `c` inserted or replaced, once it passes every check.
    fn candidate_with(&self, mut c: Connection) -> Result<(Library, Vec<Warning>), CoreError> {
        self.writable()?;
        if c.notes.as_deref().is_some_and(|n| n.trim().is_empty()) {
            c.notes = None;
        }
        let warnings = c.validate()?;
        let mut candidate = self.library.clone();
        let id = c.id;
        // The store owns `extra`: the UI sends back what it received (move,
        // duplicate) or builds a connection from scratch; neither is the file's.
        match candidate.connections.iter_mut().find(|x| x.id == id) {
            Some(slot) => {
                c.extra = std::mem::take(&mut slot.extra);
                *slot = c;
            }
            None => {
                c.extra = toml::Table::new();
                candidate.connections.push(c);
            }
        }
        if let Some(j) = candidate
            .connections
            .iter()
            .find(|x| x.id == id)
            .and_then(|x| x.jump_via)
            && !candidate.connections.iter().any(|x| x.id == j)
        {
            return Err(CoreError::InvalidInput {
                field: "jump_via".into(),
                reason: "unknown jump host".into(),
            });
        }
        // Only chains through the edited connection can have changed; a broken
        // chain elsewhere (hand-edited file) must not block this edit.
        let mut affected = vec![id];
        let mut i = 0;
        while i < affected.len() {
            for d in dependents_in(&candidate.connections, affected[i]) {
                if !affected.contains(&d) {
                    affected.push(d);
                }
            }
            i += 1;
        }
        for a in affected {
            chain_in(&candidate.connections, a)?;
        }
        Ok((candidate, warnings))
    }

    /// Adds `conns` with a single write; any invalid one cancels them all.
    /// Names are not deduplicated: the caller leaves out the ones it has.
    pub fn import(
        &mut self,
        mut conns: Vec<Connection>,
        group_id: Option<Uuid>,
    ) -> Result<usize, CoreError> {
        self.writable()?;
        if conns.is_empty() {
            return Ok(0);
        }
        let mut candidate = self.library.clone();
        let mut seen: HashSet<Uuid> = candidate.connections.iter().map(|x| x.id).collect();
        for c in &mut conns {
            c.group_id = group_id;
            c.extra = toml::Table::new();
            c.validate()?;
            if !seen.insert(c.id) {
                return Err(CoreError::InvalidInput {
                    field: "id".into(),
                    reason: "already exists".into(),
                });
            }
        }
        let n = conns.len();
        let ids: Vec<Uuid> = conns.iter().map(|c| c.id).collect();
        candidate.connections.extend(conns);
        for id in ids {
            if let Some(j) = candidate
                .connections
                .iter()
                .find(|x| x.id == id)
                .and_then(|x| x.jump_via)
                && !candidate.connections.iter().any(|x| x.id == j)
            {
                return Err(CoreError::InvalidInput {
                    field: "jump_via".into(),
                    reason: "unknown jump host".into(),
                });
            }
            chain_in(&candidate.connections, id)?;
        }
        self.commit(candidate)?;
        Ok(n)
    }

    pub fn dependents(&self, id: Uuid) -> Vec<Uuid> {
        dependents_in(&self.library.connections, id)
    }

    /// The TOML change is authoritative: a secret that cannot be deleted
    /// (keychain locked or missing) is logged, not an error.
    pub fn delete_connection(
        &mut self,
        id: Uuid,
        secrets: &dyn SecretStore,
    ) -> Result<(), CoreError> {
        self.writable()?;
        self.get(id)?;
        let mut candidate = self.library.clone();
        candidate.connections.retain(|c| c.id != id);
        for c in &mut candidate.connections {
            if c.jump_via == Some(id) {
                c.jump_via = None;
            }
        }
        self.commit(candidate)?;
        for kind in [SecretKind::Password, SecretKind::Passphrase] {
            if let Err(e) = secrets.delete(id, kind) {
                log::warn!("could not delete {kind:?} of connection {id}: {e}");
            }
        }
        Ok(())
    }

    pub fn upsert_group(&mut self, mut g: Group) -> Result<(), CoreError> {
        self.writable()?;
        if g.name.trim().is_empty() {
            return Err(CoreError::InvalidInput {
                field: "name".into(),
                reason: "must not be empty".into(),
            });
        }
        let mut candidate = self.library.clone();
        match candidate.groups.iter_mut().find(|x| x.id == g.id) {
            Some(slot) => {
                g.extra = std::mem::take(&mut slot.extra);
                *slot = g;
            }
            None => {
                g.extra = toml::Table::new();
                candidate.groups.push(g);
            }
        }
        self.commit(candidate)
    }

    pub fn delete_group(&mut self, id: Uuid) -> Result<(), CoreError> {
        self.writable()?;
        let mut candidate = self.library.clone();
        let before = candidate.groups.len();
        candidate.groups.retain(|g| g.id != id);
        if candidate.groups.len() == before {
            return Err(CoreError::NotFound {
                what: "group".into(),
            });
        }
        for c in &mut candidate.connections {
            if c.group_id == Some(id) {
                c.group_id = None;
            }
        }
        self.commit(candidate)
    }

    /// `[first hop, ..., target]`.
    pub fn jump_chain(&self, id: Uuid) -> Result<Vec<Connection>, CoreError> {
        chain_in(&self.library.connections, id)
    }
}

fn dependents_in(conns: &[Connection], id: Uuid) -> Vec<Uuid> {
    conns
        .iter()
        .filter(|c| c.jump_via == Some(id))
        .map(|c| c.id)
        .collect()
}

fn chain_in(conns: &[Connection], id: Uuid) -> Result<Vec<Connection>, CoreError> {
    let find = |id: Uuid, what: &str| {
        conns
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| CoreError::NotFound { what: what.into() })
    };
    let mut chain = vec![find(id, "connection")?.clone()];
    while let Some(next) = chain.last().and_then(|c| c.jump_via) {
        if chain.iter().any(|c| c.id == next) {
            return Err(CoreError::JumpCycle);
        }
        chain.push(find(next, "jump host")?.clone());
        // Jump hosts plus the target.
        if chain.len() > MAX_JUMP_DEPTH + 1 {
            return Err(CoreError::JumpTooDeep);
        }
    }
    chain.reverse();
    Ok(chain)
}

pub struct SettingsStore {
    path: PathBuf,
    settings: Settings,
}

impl SettingsStore {
    /// Settings are cosmetic, so loading never fails: anything unreadable
    /// falls back to the defaults.
    pub fn load(path: PathBuf) -> (Self, LoadOutcome) {
        let (settings, outcome) = match fs::read(&path) {
            Ok(bytes) => match parse::<Settings>(&bytes) {
                Some(s) => (s, LoadOutcome::Ok),
                None => match quarantine(&path) {
                    Ok(backup) => (
                        Settings::default(),
                        LoadOutcome::RecoveredFromCorrupt { backup },
                    ),
                    Err(e) => {
                        log::warn!("cannot move corrupt settings aside: {e}");
                        (Settings::default(), LoadOutcome::CreatedEmpty)
                    }
                },
            },
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    log::warn!("cannot read settings: {e}");
                }
                (Settings::default(), LoadOutcome::CreatedEmpty)
            }
        };
        (Self { path, settings }, outcome)
    }

    pub fn get(&self) -> &Settings {
        &self.settings
    }

    pub fn save(&mut self, s: Settings) -> Result<(), CoreError> {
        s.validate()?;
        // The store owns `extra`; the UI's copy is ignored.
        let s = Settings {
            extra: self.settings.extra.clone(),
            ..s
        };
        let text = toml::to_string_pretty(&s).map_err(|e| CoreError::InvalidInput {
            field: "settings".into(),
            reason: e.to_string(),
        })?;
        atomic_write(&self.path, &text)?;
        self.settings = s;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AuthMethod, ConnIcon, TagColor, Theme};
    use crate::secrets::{MemorySecretStore, SecretKind};
    use tempfile::TempDir;

    fn conn(name: &str) -> Connection {
        Connection {
            id: Uuid::new_v4(),
            name: name.into(),
            group_id: None,
            host: "example.com".into(),
            port: 22,
            username: "luca".into(),
            auth: AuthMethod::Agent,
            jump_via: None,
            forwards: vec![],
            color: None,
            icon: None,
            notes: None,
            extra: Default::default(),
        }
    }

    fn via(name: &str, jump: &Connection) -> Connection {
        Connection {
            jump_via: Some(jump.id),
            ..conn(name)
        }
    }

    fn store() -> (TempDir, ConnectionStore) {
        let dir = TempDir::new().unwrap();
        let (s, outcome) = ConnectionStore::load(dir.path().join("connections.toml")).unwrap();
        assert!(matches!(outcome, LoadOutcome::CreatedEmpty));
        (dir, s)
    }

    fn assert_invalid(r: Result<Vec<Warning>, CoreError>, want: &str) {
        match r {
            Err(CoreError::InvalidInput { field, .. }) => assert_eq!(field, want),
            other => panic!("expected InvalidInput({want}), got {other:?}"),
        }
    }

    #[test]
    fn missing_file_creates_empty() {
        let (_d, s) = store();
        assert_eq!(s.library().version, 3);
        assert!(s.library().groups.is_empty() && s.library().connections.is_empty());
    }

    #[test]
    fn save_then_reload_roundtrip() {
        let (d, mut s) = store();
        let g = Group {
            id: Uuid::new_v4(),
            name: "prod".into(),
            color: None,
            extra: Default::default(),
        };
        s.upsert_group(g.clone()).unwrap();
        let mut a = conn("a");
        a.group_id = Some(g.id);
        s.upsert_connection(a).unwrap();
        s.upsert_connection(conn("b")).unwrap();
        let (again, outcome) = ConnectionStore::load(d.path().join("connections.toml")).unwrap();
        assert!(matches!(outcome, LoadOutcome::Ok));
        assert_eq!(again.library(), s.library());
        assert_eq!(again.library().connections.len(), 2);
    }

    #[test]
    fn notes_roundtrip() {
        let (d, mut s) = store();
        let mut a = conn("a");
        a.notes = Some("line one\nline two\n".into());
        s.upsert_connection(a.clone()).unwrap();
        assert_eq!(s.get(a.id).unwrap().notes, a.notes);
        let (again, _) = ConnectionStore::load(d.path().join("connections.toml")).unwrap();
        assert_eq!(again.get(a.id).unwrap().notes, a.notes);
    }

    #[test]
    fn blank_notes_saved_as_none() {
        let (d, mut s) = store();
        let mut a = conn("a");
        a.notes = Some("  \n".into());
        s.upsert_connection(a.clone()).unwrap();
        assert_eq!(s.get(a.id).unwrap().notes, None);
        let text = fs::read_to_string(d.path().join("connections.toml")).unwrap();
        assert!(!text.contains("notes"), "{text}");
    }

    #[test]
    fn file_without_notes_loads() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("connections.toml");
        let id = Uuid::new_v4();
        fs::write(
            &path,
            format!(
                "version = 2\n\n[[connections]]\nid = \"{id}\"\nname = \"n\"\nhost = \"h\"\nusername = \"u\"\nauth = {{ type = \"Agent\" }}\n"
            ),
        )
        .unwrap();
        let (s, outcome) = ConnectionStore::load(path).unwrap();
        assert!(matches!(outcome, LoadOutcome::Ok));
        assert_eq!(s.get(id).unwrap().notes, None);
    }

    #[test]
    fn non_text_notes_read_as_none() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("connections.toml");
        let id = Uuid::new_v4();
        fs::write(
            &path,
            format!(
                "version = 2\n\n[[connections]]\nid = \"{id}\"\nname = \"n\"\nhost = \"h\"\nusername = \"u\"\nnotes = 5\nauth = {{ type = \"Agent\" }}\n"
            ),
        )
        .unwrap();
        let (s, outcome) = ConnectionStore::load(path).unwrap();
        assert!(matches!(outcome, LoadOutcome::Ok));
        assert_eq!(s.get(id).unwrap().notes, None);
    }

    fn table(path: &Path) -> toml::Table {
        toml::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn extra_survives_roundtrip() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("connections.toml");
        let (id, other, gid) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        fs::write(
            &path,
            format!(
                "version = 2\nfuturo = \"x\"\n\n[futuro_tabella]\na = 1\n\n[[groups]]\nid = \"{gid}\"\nname = \"g\"\nfuturo_g = true\n\n[[connections]]\nid = \"{id}\"\nname = \"n\"\nhost = \"h\"\nusername = \"u\"\nauth = {{ type = \"Key\", path = \"k\", save_passphrase = false }}\ncolor = 5\nfuturo_c = [1, 2]\n\n[[connections]]\nid = \"{other}\"\nname = \"o\"\nhost = \"h\"\nusername = \"u\"\nauth = {{ type = \"Agent\" }}\n"
            ),
        )
        .unwrap();
        let before = table(&path);
        let (mut s, outcome) = ConnectionStore::load(path.clone()).unwrap();
        assert!(matches!(outcome, LoadOutcome::Ok));
        assert_eq!(s.get(id).unwrap().color, None);
        let mut o = s.get(other).unwrap().clone();
        o.name = "renamed".into();
        s.upsert_connection(o).unwrap();
        let after = table(&path);
        for key in ["futuro", "futuro_tabella"] {
            assert_eq!(after.get(key), before.get(key), "{key}");
        }
        assert_eq!(
            after["groups"][0].get("futuro_g"),
            before["groups"][0].get("futuro_g")
        );
        assert_eq!(
            after["connections"][0].get("futuro_c"),
            before["connections"][0].get("futuro_c")
        );
        assert!(before["connections"][0].get("futuro_c").is_some());
        assert_eq!(after["version"].as_integer(), Some(3));
        let (again, outcome) = ConnectionStore::load(path).unwrap();
        assert!(matches!(outcome, LoadOutcome::Ok));
        assert_eq!(again.library(), s.library());
        assert_eq!(again.get(id).unwrap().color, None);
        assert_eq!(again.library().version, 3);
    }

    fn with_extra(path: &Path, id: Uuid, gid: Uuid) {
        fs::write(
            path,
            format!(
                "version = 3\n\n[[groups]]\nid = \"{gid}\"\nname = \"g\"\nfuturo_g = 1\n\n[[connections]]\nid = \"{id}\"\nname = \"n\"\nhost = \"h\"\nusername = \"u\"\nauth = {{ type = \"Agent\" }}\nfuturo_c = 1\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn ui_save_keeps_connection_extra() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("connections.toml");
        let (id, gid) = (Uuid::new_v4(), Uuid::new_v4());
        with_extra(&path, id, gid);
        let (mut s, _) = ConnectionStore::load(path.clone()).unwrap();
        let c = Connection {
            extra: toml::Table::new(),
            ..s.get(id).unwrap().clone()
        };
        s.upsert_connection(c).unwrap();
        let g = Group {
            extra: toml::Table::new(),
            ..s.library().groups[0].clone()
        };
        s.upsert_group(g).unwrap();
        let t = table(&path);
        assert_eq!(t["connections"][0]["futuro_c"].as_integer(), Some(1));
        assert_eq!(t["groups"][0]["futuro_g"].as_integer(), Some(1));
    }

    #[test]
    fn incoming_extra_is_ignored_on_update() {
        let (d, mut s) = store();
        let mut a = conn("a");
        s.upsert_connection(a.clone()).unwrap();
        a.extra.insert("iniettato".into(), 1.into());
        s.upsert_connection(a.clone()).unwrap();
        let mut g = Group {
            id: Uuid::new_v4(),
            name: "g".into(),
            color: None,
            extra: Default::default(),
        };
        s.upsert_group(g.clone()).unwrap();
        g.extra.insert("iniettato".into(), 1.into());
        s.upsert_group(g).unwrap();
        let text = fs::read_to_string(d.path().join("connections.toml")).unwrap();
        assert!(!text.contains("iniettato"), "{text}");
        assert!(s.get(a.id).unwrap().extra.is_empty());
    }

    #[test]
    fn delete_drops_extra() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("connections.toml");
        let (id, gid) = (Uuid::new_v4(), Uuid::new_v4());
        with_extra(&path, id, gid);
        let (mut s, _) = ConnectionStore::load(path.clone()).unwrap();
        s.delete_connection(id, &MemorySecretStore::default())
            .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("futuro_c"), "{text}");
    }

    #[test]
    fn unknown_field_in_forward_rule_loads() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("connections.toml");
        let id = Uuid::new_v4();
        fs::write(
            &path,
            format!(
                "version = 3\n\n[[connections]]\nid = \"{id}\"\nname = \"n\"\nhost = \"h\"\nusername = \"u\"\nauth = {{ type = \"Agent\" }}\n\n[[connections.forwards]]\ntype = \"Local\"\nbind_host = \"127.0.0.1\"\nbind_port = 8080\ndest_host = \"d\"\ndest_port = 80\nauto_start = false\nfuturo = 1\n"
            ),
        )
        .unwrap();
        let (s, outcome) = ConnectionStore::load(path).unwrap();
        assert!(matches!(outcome, LoadOutcome::Ok));
        assert_eq!(s.get(id).unwrap().forwards.len(), 1);
    }

    #[test]
    fn import_starts_with_empty_extra() {
        let (d, mut s) = store();
        let mut a = conn("a");
        a.extra.insert("iniettato".into(), 1.into());
        s.import(vec![a.clone()], None).unwrap();
        assert!(s.get(a.id).unwrap().extra.is_empty());
        let text = fs::read_to_string(d.path().join("connections.toml")).unwrap();
        assert!(!text.contains("iniettato"), "{text}");
    }

    #[test]
    fn upsert_replaces_existing_id() {
        let (_d, mut s) = store();
        let mut a = conn("a");
        s.upsert_connection(a.clone()).unwrap();
        a.name = "renamed".into();
        s.upsert_connection(a.clone()).unwrap();
        assert_eq!(s.library().connections.len(), 1);
        assert_eq!(s.get(a.id).unwrap().name, "renamed");
    }

    #[test]
    fn get_unknown_is_not_found() {
        let (_d, s) = store();
        assert!(matches!(
            s.get(Uuid::new_v4()),
            Err(CoreError::NotFound { what }) if what == "connection"
        ));
    }

    #[test]
    fn atomic_write_leaves_no_tmp() {
        let (d, mut s) = store();
        s.upsert_connection(conn("a")).unwrap();
        assert!(d.path().join("connections.toml").exists());
        assert!(!d.path().join("connections.toml.tmp").exists());
    }

    #[test]
    fn atomic_write_creates_parent_dir() {
        let d = TempDir::new().unwrap();
        let p = d.path().join("nested/dir/f.toml");
        atomic_write(&p, "x = 1").unwrap();
        assert_eq!(fs::read_to_string(p).unwrap(), "x = 1");
    }

    #[test]
    fn corrupt_file_is_renamed_not_overwritten() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("connections.toml");
        fs::write(&path, "{{not toml").unwrap();
        let (s, outcome) = ConnectionStore::load(path.clone()).unwrap();
        let LoadOutcome::RecoveredFromCorrupt { backup } = outcome else {
            panic!("expected RecoveredFromCorrupt");
        };
        assert_eq!(fs::read_to_string(&backup).unwrap(), "{{not toml");
        assert!(
            backup
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("connections.toml.corrupt-")
        );
        assert!(!path.exists());
        assert!(s.library().connections.is_empty());
    }

    #[test]
    fn corrupt_backup_never_clobbers_an_earlier_one() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("connections.toml");
        let mut backups = Vec::new();
        for i in 0..2 {
            fs::write(&path, format!("{{{{bad {i}")).unwrap();
            let (_, outcome) = ConnectionStore::load(path.clone()).unwrap();
            let LoadOutcome::RecoveredFromCorrupt { backup } = outcome else {
                panic!("expected RecoveredFromCorrupt");
            };
            backups.push(backup);
        }
        assert_ne!(backups[0], backups[1]);
        assert_eq!(fs::read_to_string(&backups[0]).unwrap(), "{{bad 0");
    }

    fn newer_file(dir: &TempDir) -> (PathBuf, String) {
        let path = dir.path().join("connections.toml");
        let id = Uuid::new_v4();
        let text = format!(
            "version = 4\n\n[[connections]]\nid = \"{id}\"\nname = \"n\"\nhost = \"h\"\nusername = \"u\"\nauth = {{ type = \"Agent\" }}\n"
        );
        fs::write(&path, &text).unwrap();
        (path, text)
    }

    #[test]
    fn v4_is_read_only() {
        let d = TempDir::new().unwrap();
        let (path, text) = newer_file(&d);
        let (mut s, outcome) = ConnectionStore::load(path.clone()).unwrap();
        assert!(matches!(
            outcome,
            LoadOutcome::ReadOnlyNewerVersion { found: 4 }
        ));
        assert_eq!(s.library().connections.len(), 1);
        let id = s.library().connections[0].id;
        assert!(matches!(
            s.upsert_connection(conn("x")),
            Err(CoreError::ReadOnly)
        ));
        assert!(matches!(
            s.delete_connection(id, &MemorySecretStore::default()),
            Err(CoreError::ReadOnly)
        ));
        assert!(matches!(
            s.upsert_group(Group {
                id: Uuid::new_v4(),
                name: "g".into(),
                color: None,
                extra: Default::default(),
            }),
            Err(CoreError::ReadOnly)
        ));
        assert!(matches!(
            s.delete_group(Uuid::new_v4()),
            Err(CoreError::ReadOnly)
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn colors_and_icons_round_trip() {
        let (d, mut s) = store();
        let g = Group {
            id: Uuid::new_v4(),
            name: "prod".into(),
            color: Some(TagColor::Teal),
            extra: Default::default(),
        };
        s.upsert_group(g).unwrap();
        let mut a = conn("a");
        a.color = Some(TagColor::Red);
        a.icon = Some(ConnIcon::Database);
        s.upsert_connection(a).unwrap();
        let (again, outcome) = ConnectionStore::load(d.path().join("connections.toml")).unwrap();
        assert!(matches!(outcome, LoadOutcome::Ok));
        assert_eq!(again.library(), s.library());
        assert_eq!(again.library().groups[0].color, Some(TagColor::Teal));
        assert_eq!(again.library().connections[0].color, Some(TagColor::Red));
        assert_eq!(
            again.library().connections[0].icon,
            Some(ConnIcon::Database)
        );
    }

    #[test]
    fn unknown_color_and_icon_read_as_none() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("connections.toml");
        let (id, gid) = (Uuid::new_v4(), Uuid::new_v4());
        fs::write(
            &path,
            format!(
                "version = 2\n\n[[groups]]\nid = \"{gid}\"\nname = \"g\"\ncolor = \"pink\"\n\n[[connections]]\nid = \"{id}\"\nname = \"n\"\nhost = \"h\"\nusername = \"u\"\ncolor = \"pink\"\nicon = \"rocket\"\nauth = {{ type = \"Agent\" }}\n"
            ),
        )
        .unwrap();
        let (s, outcome) = ConnectionStore::load(path).unwrap();
        assert!(matches!(outcome, LoadOutcome::Ok));
        assert_eq!(s.library().groups[0].color, None);
        let c = s.get(id).unwrap();
        assert_eq!((c.color, c.icon), (None, None));
    }

    #[test]
    fn non_text_color_and_icon_read_as_none() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("connections.toml");
        let (id, gid) = (Uuid::new_v4(), Uuid::new_v4());
        fs::write(
            &path,
            format!(
                "version = 2\n\n[[groups]]\nid = \"{gid}\"\nname = \"g\"\ncolor = 5\n\n[[connections]]\nid = \"{id}\"\nname = \"n\"\nhost = \"h\"\nusername = \"u\"\ncolor = {{ r = 1 }}\nicon = [\"x\"]\nauth = {{ type = \"Agent\" }}\n"
            ),
        )
        .unwrap();
        let (s, outcome) = ConnectionStore::load(path).unwrap();
        assert!(matches!(outcome, LoadOutcome::Ok));
        assert_eq!(s.library().groups[0].color, None);
        let c = s.get(id).unwrap();
        assert_eq!((c.color, c.icon), (None, None));
    }

    #[test]
    fn empty_appearance_fields_not_written() {
        let (d, mut s) = store();
        s.upsert_connection(conn("a")).unwrap();
        let text = fs::read_to_string(d.path().join("connections.toml")).unwrap();
        assert!(!text.contains("color") && !text.contains("icon"), "{text}");
    }

    #[test]
    fn v1_file_loads_and_is_rewritten_as_v3() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("connections.toml");
        let id = Uuid::new_v4();
        fs::write(
            &path,
            format!(
                "version = 1\n\n[[connections]]\nid = \"{id}\"\nname = \"n\"\nhost = \"h\"\nusername = \"u\"\nauth = {{ type = \"Agent\" }}\n"
            ),
        )
        .unwrap();
        let (mut s, outcome) = ConnectionStore::load(path.clone()).unwrap();
        assert!(matches!(outcome, LoadOutcome::Ok));
        s.upsert_connection(conn("x")).unwrap();
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .starts_with("version = 3")
        );
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("connections.toml");
        let id = Uuid::new_v4();
        fs::write(
            &path,
            format!(
                "version = 1\n\n[[connections]]\nid = \"{id}\"\nname = \"n\"\nhost = \"h\"\nusername = \"u\"\ncolor = 'red'\nauth = {{ type = \"Agent\" }}\n"
            ),
        )
        .unwrap();
        let (s, outcome) = ConnectionStore::load(path).unwrap();
        assert!(matches!(outcome, LoadOutcome::Ok));
        assert_eq!(s.get(id).unwrap().name, "n");
    }

    #[test]
    fn delete_group_ungroups_connections() {
        let (_d, mut s) = store();
        let g = Group {
            id: Uuid::new_v4(),
            name: "g".into(),
            color: None,
            extra: Default::default(),
        };
        s.upsert_group(g.clone()).unwrap();
        let mut a = conn("a");
        a.group_id = Some(g.id);
        s.upsert_connection(a.clone()).unwrap();
        s.delete_group(g.id).unwrap();
        assert!(s.library().groups.is_empty());
        assert_eq!(s.get(a.id).unwrap().group_id, None);
        assert!(matches!(
            s.delete_group(g.id),
            Err(CoreError::NotFound { .. })
        ));
    }

    #[test]
    fn delete_connection_clears_jump_refs_and_secrets() {
        let (d, mut s) = store();
        let a = conn("a");
        let b = via("b", &a);
        s.upsert_connection(a.clone()).unwrap();
        s.upsert_connection(b.clone()).unwrap();
        assert_eq!(s.dependents(a.id), vec![b.id]);
        let secrets = MemorySecretStore::default();
        for kind in [SecretKind::Password, SecretKind::Passphrase] {
            secrets.set(a.id, kind, &"s".to_string().into()).unwrap();
        }
        s.delete_connection(a.id, &secrets).unwrap();
        assert_eq!(s.get(b.id).unwrap().jump_via, None);
        assert!(s.get(a.id).is_err());
        for kind in [SecretKind::Password, SecretKind::Passphrase] {
            assert!(secrets.get(a.id, kind).unwrap().is_none());
        }
        let (again, _) = ConnectionStore::load(d.path().join("connections.toml")).unwrap();
        assert_eq!(again.library(), s.library());
    }

    #[test]
    fn delete_connection_survives_secret_store_failure() {
        let (_d, mut s) = store();
        let a = conn("a");
        s.upsert_connection(a.clone()).unwrap();
        s.delete_connection(a.id, &crate::secrets::FailingSecretStore)
            .unwrap();
        assert!(s.get(a.id).is_err());
        assert!(matches!(
            s.delete_connection(a.id, &MemorySecretStore::default()),
            Err(CoreError::NotFound { .. })
        ));
    }

    #[test]
    fn jump_chain_order() {
        let (_d, mut s) = store();
        let a = conn("a");
        let b = via("b", &a);
        let c = via("c", &b);
        for x in [&a, &b, &c] {
            s.upsert_connection(x.clone()).unwrap();
        }
        let ids: Vec<Uuid> = s.jump_chain(c.id).unwrap().iter().map(|x| x.id).collect();
        assert_eq!(ids, vec![a.id, b.id, c.id]);
        assert_eq!(s.jump_chain(a.id).unwrap().len(), 1);
    }

    #[test]
    fn jump_cycle_detected() {
        let (_d, mut s) = store();
        let mut a = conn("a");
        let b = via("b", &a);
        s.upsert_connection(a.clone()).unwrap();
        s.upsert_connection(b.clone()).unwrap();
        a.jump_via = Some(b.id);
        assert!(matches!(
            s.upsert_connection(a.clone()),
            Err(CoreError::JumpCycle)
        ));
        let mut selfish = conn("s");
        selfish.jump_via = Some(selfish.id);
        assert!(s.upsert_connection(selfish).is_err());
        // The rejected edit must not have been applied.
        assert_eq!(s.get(a.id).unwrap().jump_via, None);
        // jump_chain also reports cycles already present on disk.
        s.library.connections[0].jump_via = Some(b.id);
        assert!(matches!(s.jump_chain(b.id), Err(CoreError::JumpCycle)));
    }

    #[test]
    fn jump_depth_limit() {
        let (_d, mut s) = store();
        let mut prev = conn("0");
        s.upsert_connection(prev.clone()).unwrap();
        for i in 1..6 {
            prev = via(&i.to_string(), &prev);
            s.upsert_connection(prev.clone()).unwrap();
        }
        assert_eq!(s.jump_chain(prev.id).unwrap().len(), 6);
        let too_deep = via("6", &prev);
        assert!(matches!(
            s.upsert_connection(too_deep.clone()),
            Err(CoreError::JumpTooDeep)
        ));
        assert!(s.get(too_deep.id).is_err());
    }

    #[test]
    fn editing_a_jump_host_cannot_deepen_dependents_past_the_limit() {
        let (_d, mut s) = store();
        let root = conn("root");
        let mut h = conn("h");
        s.upsert_connection(root.clone()).unwrap();
        s.upsert_connection(h.clone()).unwrap();
        let mut prev = h.clone();
        for i in 0..5 {
            prev = via(&i.to_string(), &prev);
            s.upsert_connection(prev.clone()).unwrap();
        }
        h.jump_via = Some(root.id);
        assert!(matches!(
            s.upsert_connection(h),
            Err(CoreError::JumpTooDeep)
        ));
    }

    #[test]
    fn jump_via_missing_rejected() {
        let (_d, mut s) = store();
        let mut c = conn("c");
        c.jump_via = Some(Uuid::new_v4());
        assert_invalid(s.upsert_connection(c), "jump_via");
    }

    #[test]
    fn invalid_connection_is_not_saved() {
        let (d, mut s) = store();
        let mut c = conn("c");
        c.host = String::new();
        assert_invalid(s.upsert_connection(c), "host");
        assert!(!d.path().join("connections.toml").exists());
    }

    #[test]
    fn upsert_returns_validation_warnings() {
        let (_d, mut s) = store();
        let mut c = conn("c");
        let path = PathBuf::from("/nonexistent/key");
        c.auth = AuthMethod::Key {
            path: path.clone(),
            save_passphrase: false,
        };
        assert_eq!(
            s.upsert_connection(c).unwrap(),
            vec![Warning::KeyFileMissing { path }]
        );
    }

    #[test]
    fn settings_default_when_missing_or_corrupt() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("settings.toml");
        let (s, outcome) = SettingsStore::load(path.clone());
        assert!(matches!(outcome, LoadOutcome::CreatedEmpty));
        assert_eq!(s.get(), &Settings::default());

        fs::write(&path, "{{nope").unwrap();
        let (s, outcome) = SettingsStore::load(path.clone());
        let LoadOutcome::RecoveredFromCorrupt { backup } = outcome else {
            panic!("expected RecoveredFromCorrupt");
        };
        assert_eq!(fs::read_to_string(backup).unwrap(), "{{nope");
        assert_eq!(s.get(), &Settings::default());
    }

    #[test]
    fn save_rejects_invalid_settings() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("settings.toml");
        let (mut s, _) = SettingsStore::load(path.clone());
        s.save(Settings::default()).unwrap();
        let before = fs::read_to_string(&path).unwrap();
        let bad = Settings {
            scrollback: 10,
            ..Settings::default()
        };
        assert!(matches!(
            s.save(bad),
            Err(CoreError::InvalidInput { field, .. }) if field == "scrollback"
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        assert_eq!(s.get(), &Settings::default());
    }

    #[test]
    fn settings_save_keeps_extra() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("settings.toml");
        fs::write(&path, "futuro = \"x\"\nfont_size = 15\n").unwrap();
        let (mut s, outcome) = SettingsStore::load(path.clone());
        assert!(matches!(outcome, LoadOutcome::Ok));
        let new = Settings {
            font_size: 16,
            extra: toml::Table::new(),
            ..s.get().clone()
        };
        s.save(new).unwrap();
        let t = table(&path);
        assert_eq!(t["futuro"].as_str(), Some("x"));
        assert_eq!(t["font_size"].as_integer(), Some(16));
        assert_eq!(s.get().extra.len(), 1);
    }

    #[test]
    fn settings_non_text_theme_still_ignored() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("settings.toml");
        fs::write(&path, "terminal_theme = 5\nfuturo = 1\n").unwrap();
        let (s, outcome) = SettingsStore::load(path);
        assert!(matches!(outcome, LoadOutcome::Ok));
        assert_eq!(s.get().terminal_theme, Settings::default().terminal_theme);
        assert_eq!(s.get().extra.len(), 1);
    }

    #[test]
    fn settings_save_then_reload() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("settings.toml");
        let (mut s, _) = SettingsStore::load(path.clone());
        let new = Settings {
            theme: Theme::Dark,
            font_size: 18,
            language: Some("it".into()),
            ..Settings::default()
        };
        s.save(new.clone()).unwrap();
        let (again, outcome) = SettingsStore::load(path);
        assert!(matches!(outcome, LoadOutcome::Ok));
        assert_eq!(again.get(), &new);
    }
    #[test]
    fn import_writes_once_with_jumps_between_new_connections() {
        let (d, mut s) = store();
        let bastion = conn("bastion");
        let web = via("web", &bastion);
        assert_eq!(s.import(vec![web, bastion], None).unwrap(), 2);
        let (again, _) = ConnectionStore::load(d.path().join("connections.toml")).unwrap();
        assert_eq!(again.library(), s.library());
        assert_eq!(again.library().connections.len(), 2);
    }

    #[test]
    fn import_with_one_invalid_connection_changes_nothing() {
        let (d, mut s) = store();
        s.upsert_connection(conn("old")).unwrap();
        let path = d.path().join("connections.toml");
        let before = fs::read_to_string(&path).unwrap();
        let mut bad = conn("bad");
        bad.host = String::new();
        let err = s.import(vec![conn("ok"), bad], None).unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput { field, .. } if field == "host"));
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        assert_eq!(s.library().connections.len(), 1);
    }

    #[test]
    fn import_rejects_unknown_jump_and_taken_ids() {
        let (_d, mut s) = store();
        let saved = conn("saved");
        s.upsert_connection(saved.clone()).unwrap();
        let orphan = Connection {
            jump_via: Some(Uuid::new_v4()),
            ..conn("orphan")
        };
        assert!(matches!(
            s.import(vec![orphan], None),
            Err(CoreError::InvalidInput { field, .. }) if field == "jump_via"
        ));
        assert!(matches!(
            s.import(vec![saved], None),
            Err(CoreError::InvalidInput { field, .. }) if field == "id"
        ));
        assert_eq!(s.library().connections.len(), 1);
    }

    #[test]
    fn import_sets_the_group() {
        let (_d, mut s) = store();
        let g = Uuid::new_v4();
        s.import(vec![conn("a"), conn("b")], Some(g)).unwrap();
        assert!(
            s.library()
                .connections
                .iter()
                .all(|c| c.group_id == Some(g))
        );
    }

    #[test]
    fn import_empty_writes_nothing() {
        let (d, mut s) = store();
        assert_eq!(s.import(vec![], None).unwrap(), 0);
        assert!(!d.path().join("connections.toml").exists());
    }

    #[test]
    fn import_into_read_only_library_fails() {
        let d = TempDir::new().unwrap();
        let (path, _) = newer_file(&d);
        let (mut s, _) = ConnectionStore::load(path).unwrap();
        assert!(matches!(
            s.import(vec![conn("a")], None),
            Err(CoreError::ReadOnly)
        ));
    }
    #[test]
    fn import_rejects_duplicate_ids_in_one_batch() {
        let (d, mut s) = store();
        s.upsert_connection(conn("old")).unwrap();
        let path = d.path().join("connections.toml");
        let before = fs::read_to_string(&path).unwrap();
        let a = conn("a");
        let err = s.import(vec![a.clone(), a], None).unwrap_err();
        assert!(matches!(err, CoreError::InvalidInput { field, reason }
            if field == "id" && reason == "already exists"));
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn import_empty_into_read_only_library_fails() {
        let d = TempDir::new().unwrap();
        let (path, _) = newer_file(&d);
        let (mut s, _) = ConnectionStore::load(path).unwrap();
        assert!(matches!(s.import(vec![], None), Err(CoreError::ReadOnly)));
    }
}
