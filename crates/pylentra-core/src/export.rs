use crate::error::CoreError;
use crate::model::{Connection, Group};
use crate::snippets::Snippet;
use crate::store::Library;
use serde::{Deserialize, Serialize};

pub const EXPORT_FORMAT: &str = "pylentra-export";
pub const EXPORT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ExportFile {
    pub format: String,
    pub version: u32,
    pub exported_at: String,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub connections: Vec<Connection>,
    #[serde(default)]
    pub snippets: Vec<Snippet>,
}

fn invalid_file(e: impl ToString) -> CoreError {
    CoreError::InvalidInput {
        field: "file".into(),
        reason: e.to_string(),
    }
}

/// Serializes the library and snippets; unknown (`extra`) fields are never written.
pub fn export(
    library: &Library,
    snippets: &[Snippet],
    exported_at: &str,
) -> Result<String, CoreError> {
    let file = ExportFile {
        format: EXPORT_FORMAT.into(),
        version: EXPORT_VERSION,
        exported_at: exported_at.into(),
        groups: library
            .groups
            .iter()
            .cloned()
            .map(|mut g| {
                g.extra.clear();
                g
            })
            .collect(),
        connections: library
            .connections
            .iter()
            .cloned()
            .map(|mut c| {
                c.extra.clear();
                c
            })
            .collect(),
        snippets: snippets
            .iter()
            .cloned()
            .map(|mut s| {
                s.extra.clear();
                s
            })
            .collect(),
    };
    toml::to_string_pretty(&file).map_err(invalid_file)
}

/// Checks `format` and `version` on the raw table before the full parse, so a newer file reports its version.
pub fn parse(text: &str) -> Result<ExportFile, CoreError> {
    let table: toml::Table = text.parse().map_err(invalid_file)?;
    if table.get("format").and_then(|v| v.as_str()) != Some(EXPORT_FORMAT) {
        return Err(CoreError::InvalidInput {
            field: "format".into(),
            reason: "not a Pylentra export".into(),
        });
    }
    match table.get("version").and_then(|v| v.as_integer()) {
        Some(v) if v > i64::from(EXPORT_VERSION) => {
            return Err(CoreError::UnsupportedVersion {
                found: u32::try_from(v).unwrap_or(u32::MAX),
            });
        }
        _ => {}
    }
    table.try_into().map_err(invalid_file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AuthMethod;
    use uuid::Uuid;

    fn conn(name: &str, auth: AuthMethod) -> Connection {
        Connection {
            id: Uuid::new_v4(),
            name: name.into(),
            group_id: None,
            host: "h".into(),
            port: 22,
            username: "u".into(),
            auth,
            jump_via: None,
            forwards: vec![],
            color: None,
            icon: None,
            notes: None,
            extra: toml::Table::new(),
        }
    }

    fn sample() -> (Library, Vec<Snippet>) {
        let group = Group {
            id: Uuid::new_v4(),
            name: "Prod".into(),
            color: None,
            extra: toml::Table::new(),
        };
        let mut a = conn("a", AuthMethod::Password { save: true });
        a.group_id = Some(group.id);
        let mut b = conn(
            "b",
            AuthMethod::Key {
                path: "/k".into(),
                save_passphrase: false,
            },
        );
        b.jump_via = Some(a.id);
        let snippet = Snippet {
            id: Uuid::new_v4(),
            name: "s".into(),
            text: "ls".into(),
            run: true,
            connection_id: Some(b.id),
            extra: toml::Table::new(),
        };
        let library = Library {
            groups: vec![group],
            connections: vec![a, b],
            ..Library::default()
        };
        (library, vec![snippet])
    }

    fn future() -> toml::Table {
        let mut t = toml::Table::new();
        t.insert("futuro".into(), 1.into());
        t
    }

    #[test]
    fn roundtrip_keeps_content() {
        let (library, snippets) = sample();
        let text = export(&library, &snippets, "2026-10-30T10:00:00Z").unwrap();
        assert!(text.contains("format = \"pylentra-export\""));
        assert!(text.contains("version = 1"));
        let file = parse(&text).unwrap();
        assert_eq!(file.exported_at, "2026-10-30T10:00:00Z");
        assert_eq!(file.groups, library.groups);
        assert_eq!(file.connections, library.connections);
        assert_eq!(file.snippets, snippets);
    }

    #[test]
    fn extra_fields_and_secrets_are_not_exported() {
        let (mut library, mut snippets) = sample();
        library.groups[0].extra = future();
        library.connections[0].extra = future();
        snippets[0].extra = future();
        let text = export(&library, &snippets, "now").unwrap();
        assert!(!text.contains("futuro"));
        assert!(!text.contains("password ="));
    }

    #[test]
    fn rejects_wrong_or_missing_format() {
        for text in ["format = \"altro\"\nversion = 1", "version = 1"] {
            assert!(matches!(
                parse(text),
                Err(CoreError::InvalidInput { field, .. }) if field == "format"
            ));
        }
    }

    #[test]
    fn version_is_checked_before_the_rest() {
        let text = "format = \"pylentra-export\"\nversion = 2\n[[connections]]\nfuturo = 1\n";
        assert!(matches!(
            parse(text),
            Err(CoreError::UnsupportedVersion { found: 2 })
        ));
    }

    #[test]
    fn rejects_invalid_toml() {
        assert!(matches!(
            parse("{{non toml"),
            Err(CoreError::InvalidInput { field, .. }) if field == "file"
        ));
    }
}
