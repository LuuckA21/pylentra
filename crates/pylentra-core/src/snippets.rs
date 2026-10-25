use crate::error::CoreError;
use crate::store::{LoadOutcome, VersionOnly, atomic_write, parse, quarantine};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use ts_rs::TS;
use uuid::Uuid;

pub const SNIPPETS_VERSION: u32 = 1;
const MAX_NAME_CHARS: usize = 80;
const MAX_TEXT_BYTES: usize = 65_536;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct Snippet {
    pub id: Uuid,
    pub name: String,
    pub text: String,
    #[serde(default)]
    pub run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub connection_id: Option<Uuid>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SnippetFile {
    pub version: u32,
    #[serde(default)]
    pub snippets: Vec<Snippet>,
}

impl Default for SnippetFile {
    fn default() -> Self {
        Self {
            version: SNIPPETS_VERSION,
            snippets: vec![],
        }
    }
}

impl Snippet {
    pub fn validate(&self) -> Result<(), CoreError> {
        let invalid = |field: &str, reason: &str| CoreError::InvalidInput {
            field: field.into(),
            reason: reason.into(),
        };
        if self.name.trim().is_empty() {
            return Err(invalid("name", "must not be empty"));
        }
        if self.name.chars().count() > MAX_NAME_CHARS {
            return Err(invalid("name", "too long"));
        }
        if self.text.trim().is_empty() {
            return Err(invalid("text", "must not be empty"));
        }
        if self.text.len() > MAX_TEXT_BYTES {
            return Err(invalid("text", "too long"));
        }
        Ok(())
    }
}

pub struct SnippetStore {
    path: PathBuf,
    snippets: Vec<Snippet>,
    read_only: bool,
}

impl SnippetStore {
    pub fn load(path: PathBuf) -> Result<(Self, LoadOutcome), CoreError> {
        let mut store = Self {
            path,
            snippets: vec![],
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
            Some(VersionOnly { version }) if version > SNIPPETS_VERSION => {
                // Show what we can, but never write a format we do not know.
                store.read_only = true;
                store.snippets = parse::<SnippetFile>(&bytes).unwrap_or_default().snippets;
                LoadOutcome::ReadOnlyNewerVersion { found: version }
            }
            Some(_) => match parse::<SnippetFile>(&bytes) {
                Some(file) => {
                    store.snippets = file.snippets;
                    LoadOutcome::Ok
                }
                None => LoadOutcome::RecoveredFromCorrupt {
                    backup: quarantine(&store.path)?,
                },
            },
            None => LoadOutcome::RecoveredFromCorrupt {
                backup: quarantine(&store.path)?,
            },
        };
        Ok((store, outcome))
    }

    /// An empty, read-only store for a file that could not be read, so the
    /// app still starts and never overwrites it.
    pub fn unavailable(path: PathBuf) -> Self {
        Self {
            path,
            snippets: vec![],
            read_only: true,
        }
    }

    pub fn list(&self) -> &[Snippet] {
        &self.snippets
    }

    fn writable(&self) -> Result<(), CoreError> {
        if self.read_only {
            return Err(CoreError::ReadOnly);
        }
        Ok(())
    }

    /// Saves `candidate` and only then makes it the live list.
    fn commit(&mut self, candidate: Vec<Snippet>) -> Result<(), CoreError> {
        self.writable()?;
        let file = SnippetFile {
            version: SNIPPETS_VERSION,
            snippets: candidate,
        };
        let text = toml::to_string_pretty(&file).map_err(|e| CoreError::InvalidInput {
            field: "snippets".into(),
            reason: e.to_string(),
        })?;
        atomic_write(&self.path, &text)?;
        self.snippets = file.snippets;
        Ok(())
    }

    pub fn upsert(&mut self, s: Snippet) -> Result<(), CoreError> {
        self.writable()?;
        s.validate()?;
        let mut candidate = self.snippets.clone();
        match candidate.iter_mut().find(|x| x.id == s.id) {
            Some(slot) => *slot = s,
            None => candidate.push(s),
        }
        self.commit(candidate)
    }

    pub fn delete(&mut self, id: Uuid) -> Result<(), CoreError> {
        self.writable()?;
        let mut candidate = self.snippets.clone();
        candidate.retain(|s| s.id != id);
        if candidate.len() == self.snippets.len() {
            return Err(CoreError::NotFound {
                what: "snippet".into(),
            });
        }
        self.commit(candidate)
    }

    /// Drops the snippets tied to a deleted connection; writes only if any.
    pub fn remove_for_connection(&mut self, id: Uuid) -> Result<usize, CoreError> {
        self.writable()?;
        let mut candidate = self.snippets.clone();
        candidate.retain(|s| s.connection_id != Some(id));
        let removed = self.snippets.len() - candidate.len();
        if removed > 0 {
            self.commit(candidate)?;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::LoadOutcome;
    use std::fs;
    use tempfile::TempDir;

    fn snip(name: &str, text: &str) -> Snippet {
        Snippet {
            id: Uuid::new_v4(),
            name: name.into(),
            text: text.into(),
            run: false,
            connection_id: None,
        }
    }

    fn store() -> (TempDir, SnippetStore) {
        let dir = TempDir::new().unwrap();
        let (s, outcome) = SnippetStore::load(dir.path().join("snippets.toml")).unwrap();
        assert_eq!(outcome, LoadOutcome::CreatedEmpty);
        (dir, s)
    }

    fn reload(d: &TempDir) -> SnippetStore {
        let (s, outcome) = SnippetStore::load(d.path().join("snippets.toml")).unwrap();
        assert_eq!(outcome, LoadOutcome::Ok);
        s
    }

    fn assert_invalid(r: Result<(), CoreError>, want: &str) {
        match r {
            Err(CoreError::InvalidInput { field, .. }) => assert_eq!(field, want),
            other => panic!("expected InvalidInput({want}), got {other:?}"),
        }
    }

    #[test]
    fn missing_file_is_empty() {
        let (_d, s) = store();
        assert!(s.list().is_empty());
    }

    #[test]
    fn upsert_then_reload_roundtrip() {
        let (d, mut s) = store();
        let mut a = snip("a", "ls");
        a.connection_id = Some(Uuid::new_v4());
        a.run = true;
        s.upsert(a).unwrap();
        s.upsert(snip("b", "pwd")).unwrap();
        assert_eq!(reload(&d).list(), s.list());
        assert_eq!(s.list().len(), 2);
    }

    #[test]
    fn multiline_text_survives() {
        let (d, mut s) = store();
        s.upsert(snip("m", "cd /srv\n  ls -la\n\"q\" \\ fine"))
            .unwrap();
        assert_eq!(
            reload(&d).list()[0].text,
            "cd /srv\n  ls -la\n\"q\" \\ fine"
        );
    }

    #[test]
    fn upsert_replaces_same_id() {
        let (_d, mut s) = store();
        let mut a = snip("a", "ls");
        s.upsert(a.clone()).unwrap();
        a.name = "renamed".into();
        s.upsert(a).unwrap();
        assert_eq!(s.list().len(), 1);
        assert_eq!(s.list()[0].name, "renamed");
    }

    #[test]
    fn validate_rejects() {
        assert_invalid(snip("  ", "ls").validate(), "name");
        assert_invalid(snip(&"é".repeat(81), "ls").validate(), "name");
        snip(&"é".repeat(80), "ls").validate().unwrap();
        assert_invalid(snip("n", "\n ").validate(), "text");
        assert_invalid(snip("n", &"x".repeat(65_537)).validate(), "text");
        snip("n", &"x".repeat(65_536)).validate().unwrap();
    }

    #[test]
    fn invalid_snippet_is_not_saved() {
        let (d, mut s) = store();
        assert_invalid(s.upsert(snip("", "ls")), "name");
        assert!(s.list().is_empty());
        assert!(!d.path().join("snippets.toml").exists());
    }

    #[test]
    fn corrupt_file_quarantined() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("snippets.toml");
        fs::write(&path, "not = [valid").unwrap();
        let (s, outcome) = SnippetStore::load(path).unwrap();
        let LoadOutcome::RecoveredFromCorrupt { backup } = outcome else {
            panic!("expected RecoveredFromCorrupt");
        };
        assert!(backup.exists());
        assert!(s.list().is_empty());
    }

    #[test]
    fn newer_version_read_only() {
        let d = TempDir::new().unwrap();
        let path = d.path().join("snippets.toml");
        let id = Uuid::new_v4();
        let text =
            format!("version = 2\n\n[[snippets]]\nid = \"{id}\"\nname = \"n\"\ntext = \"ls\"\n");
        fs::write(&path, &text).unwrap();
        let (mut s, outcome) = SnippetStore::load(path.clone()).unwrap();
        assert_eq!(outcome, LoadOutcome::ReadOnlyNewerVersion { found: 2 });
        assert_eq!(s.list().len(), 1);
        assert!(matches!(s.upsert(snip("x", "y")), Err(CoreError::ReadOnly)));
        assert!(matches!(s.delete(id), Err(CoreError::ReadOnly)));
        assert!(matches!(
            s.remove_for_connection(id),
            Err(CoreError::ReadOnly)
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn remove_for_connection_only_that_connection() {
        let (d, mut s) = store();
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let mut sa = snip("a", "1");
        sa.connection_id = Some(a);
        let mut sb = snip("b", "2");
        sb.connection_id = Some(b);
        let global = snip("g", "3");
        for x in [&sa, &sb, &global] {
            s.upsert(x.clone()).unwrap();
        }
        assert_eq!(s.remove_for_connection(a).unwrap(), 1);
        assert_eq!(s.list(), [sb.clone(), global.clone()]);
        assert_eq!(reload(&d).list(), [sb, global]);
    }

    #[test]
    fn remove_for_connection_without_matches_writes_nothing() {
        let (d, mut s) = store();
        assert_eq!(s.remove_for_connection(Uuid::new_v4()).unwrap(), 0);
        assert!(!d.path().join("snippets.toml").exists());
    }

    #[test]
    fn delete_unknown_is_not_found() {
        let (_d, mut s) = store();
        assert!(matches!(
            s.delete(Uuid::new_v4()),
            Err(CoreError::NotFound { what }) if what == "snippet"
        ));
    }

    #[test]
    fn delete_removes_and_persists() {
        let (d, mut s) = store();
        let a = snip("a", "1");
        s.upsert(a.clone()).unwrap();
        s.upsert(snip("b", "2")).unwrap();
        s.delete(a.id).unwrap();
        assert_eq!(reload(&d).list().len(), 1);
    }
}
