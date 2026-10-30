use crate::error::CoreError;
use crate::model::{AuthMethod, Connection, Group, expand_home};
use crate::snippets::Snippet;
use crate::store::Library;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use ts_rs::TS;
use uuid::Uuid;

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

/// The jump host of a file connection; the UI words the note from the selection.
#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct JumpRef {
    pub name: String,
    pub in_file: Option<Uuid>,
    /// A saved connection already has this name.
    pub existing: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct ImportRow {
    pub id: Uuid,
    pub name: String,
    /// `user@host:port`.
    pub target: String,
    pub group: Option<String>,
    /// A saved connection already has this exact name.
    pub duplicate: bool,
    pub missing_key: Option<String>,
    pub jump: Option<JumpRef>,
}

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct ImportPreview {
    pub rows: Vec<ImportRow>,
    pub groups_new: u32,
    pub groups_reused: u32,
    pub snippets_global: u32,
    pub snippets_per_connection: u32,
}

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct ImportCounts {
    pub connections: u32,
    pub groups: u32,
    pub snippets: u32,
}

/// `snippets` is false if `snippets.toml` was unavailable and none were written.
#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct ExportDone {
    pub path: String,
    pub snippets: bool,
}

/// `path` is only shown: `library_import` reads the file the app remembered.
#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct ImportPick {
    pub path: String,
    pub preview: ImportPreview,
}

/// What to add to the library: fresh ids, references already pointing at them.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportPlan {
    pub groups: Vec<Group>,
    pub connections: Vec<Connection>,
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
    let file: ExportFile = table.try_into().map_err(invalid_file)?;
    // The preview keys its rows by id: a repeat would tie two rows together.
    let mut ids: Vec<Uuid> = file.connections.iter().map(|c| c.id).collect();
    ids.sort_unstable();
    if ids.windows(2).any(|w| w[0] == w[1]) {
        return Err(CoreError::InvalidInput {
            field: "id".into(),
            reason: "repeated in the file".into(),
        });
    }
    Ok(file)
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

pub fn preview(
    file: &ExportFile,
    library: &Library,
    key_exists: impl Fn(&Path) -> bool,
) -> ImportPreview {
    let in_file = |id: Uuid| file.connections.iter().find(|c| c.id == id);
    let saved = |name: &str| library.connections.iter().any(|c| c.name == name);
    let rows = file
        .connections
        .iter()
        .map(|c| ImportRow {
            id: c.id,
            name: c.name.clone(),
            target: format!("{}@{}:{}", c.username, c.host, c.port),
            group: c
                .group_id
                .and_then(|id| file.groups.iter().find(|g| g.id == id))
                .map(|g| g.name.clone()),
            duplicate: saved(&c.name),
            missing_key: match &c.auth {
                AuthMethod::Key { path, .. } if !key_exists(&expand_home(path)) => {
                    Some(path.display().to_string())
                }
                _ => None,
            },
            jump: c.jump_via.and_then(in_file).map(|j| JumpRef {
                name: j.name.clone(),
                in_file: Some(j.id),
                existing: saved(&j.name),
            }),
        })
        .collect();
    let mut names: Vec<&str> = file.groups.iter().map(|g| g.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    let reused = names
        .iter()
        .filter(|n| library.groups.iter().any(|g| g.name == **n))
        .count();
    let global = file.snippets.iter().filter(|s| s.connection_id.is_none());
    // Snippets of a connection missing from the file are never imported.
    let per_connection = file
        .snippets
        .iter()
        .filter(|s| s.connection_id.and_then(in_file).is_some());
    ImportPreview {
        rows,
        groups_new: count(names.len() - reused),
        groups_reused: count(reused),
        snippets_global: count(global.count()),
        snippets_per_connection: count(per_connection.count()),
    }
}

/// Drops the global snippets a saved global one already has (same name and
/// text), so importing a newer export does not add them twice.
pub fn drop_saved_globals(snippets: &mut Vec<Snippet>, saved: &[Snippet]) {
    snippets.retain(|s| {
        s.connection_id.is_some()
            || !saved
                .iter()
                .any(|o| o.connection_id.is_none() && o.name == s.name && o.text == s.text)
    });
}

/// Does not validate the connections: the store does when it adds them.
pub fn plan(
    file: &ExportFile,
    selected: &[Uuid],
    library: &Library,
) -> Result<ImportPlan, CoreError> {
    if let Some(id) = selected
        .iter()
        .find(|id| !file.connections.iter().any(|c| c.id == **id))
    {
        return Err(CoreError::InvalidInput {
            field: "selected".into(),
            reason: format!("{id} is not in the file"),
        });
    }
    let mut new_ids = HashMap::new();
    // Each connection gets its own id even if a hand-edited file repeats one.
    let picked: Vec<(&Connection, Uuid)> = file
        .connections
        .iter()
        .filter(|c| selected.contains(&c.id))
        .map(|c| {
            let id = Uuid::new_v4();
            new_ids.entry(c.id).or_insert(id);
            (c, id)
        })
        .collect();
    let mut groups = Vec::new();
    let mut new_groups: HashMap<&str, Uuid> = HashMap::new();
    let mut connections = Vec::new();
    for (c, id) in picked {
        let group_id = c
            .group_id
            .and_then(|g| file.groups.iter().find(|f| f.id == g))
            .map(|g| match library.groups.iter().find(|l| l.name == g.name) {
                Some(l) => l.id,
                None => *new_groups.entry(&g.name).or_insert_with(|| {
                    let id = Uuid::new_v4();
                    groups.push(Group { id, ..g.clone() });
                    id
                }),
            });
        let jump_via = c.jump_via.and_then(|j| {
            new_ids.get(&j).copied().or_else(|| {
                let name = &file.connections.iter().find(|f| f.id == j)?.name;
                let saved = library.connections.iter().find(|l| &l.name == name)?;
                Some(saved.id)
            })
        });
        connections.push(Connection {
            id,
            group_id,
            jump_via,
            ..c.clone()
        });
    }
    let snippets = file
        .snippets
        .iter()
        .filter_map(|s| {
            let connection_id = match s.connection_id {
                Some(c) => Some(*new_ids.get(&c)?),
                None => None,
            };
            Some(Snippet {
                id: Uuid::new_v4(),
                connection_id,
                ..s.clone()
            })
        })
        .collect();
    Ok(ImportPlan {
        groups,
        connections,
        snippets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AuthMethod, ConnIcon, ForwardRule, TagColor, expand_home};
    use std::cell::RefCell;
    use std::path::PathBuf;

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
    fn rejects_repeated_connection_ids() {
        let a = conn("a", AuthMethod::Agent);
        let b = Connection {
            name: "b".into(),
            ..a.clone()
        };
        let text = export(&lib(vec![], vec![a, b]), &[], "now").unwrap();
        assert!(matches!(
            parse(&text),
            Err(CoreError::InvalidInput { field, reason })
                if field == "id" && reason == "repeated in the file"
        ));
    }

    #[test]
    fn saved_global_snippets_are_not_added_again() {
        let c = Uuid::new_v4();
        let mut same_name_other_text = snippet("ls", None);
        same_name_other_text.text = "ls -la".into();
        let mut snippets = vec![
            snippet("ls", None),
            same_name_other_text.clone(),
            snippet("ls", Some(c)),
            snippet("new", None),
        ];
        let saved = vec![snippet("ls", None), snippet("new", Some(c))];
        drop_saved_globals(&mut snippets, &saved);
        let kept: Vec<_> = snippets
            .iter()
            .map(|s| (s.text.as_str(), s.connection_id))
            .collect();
        assert_eq!(kept, [("ls -la", None), ("ls", Some(c)), ("ls", None)]);
        assert_eq!(snippets[2].name, "new");
    }

    #[test]
    fn rejects_invalid_toml() {
        assert!(matches!(
            parse("{{non toml"),
            Err(CoreError::InvalidInput { field, .. }) if field == "file"
        ));
    }

    fn group(name: &str) -> Group {
        Group {
            id: Uuid::new_v4(),
            name: name.into(),
            color: None,
            extra: toml::Table::new(),
        }
    }

    fn snippet(name: &str, connection_id: Option<Uuid>) -> Snippet {
        Snippet {
            id: Uuid::new_v4(),
            name: name.into(),
            text: "ls".into(),
            run: false,
            connection_id,
            extra: toml::Table::new(),
        }
    }

    fn file(
        groups: Vec<Group>,
        connections: Vec<Connection>,
        snippets: Vec<Snippet>,
    ) -> ExportFile {
        ExportFile {
            format: EXPORT_FORMAT.into(),
            version: EXPORT_VERSION,
            exported_at: "now".into(),
            groups,
            connections,
            snippets,
        }
    }

    fn lib(groups: Vec<Group>, connections: Vec<Connection>) -> Library {
        Library {
            groups,
            connections,
            ..Library::default()
        }
    }

    fn all(file: &ExportFile) -> Vec<Uuid> {
        file.connections.iter().map(|c| c.id).collect()
    }

    #[test]
    fn preview_rows() {
        let g = group("Prod");
        let mut web = conn(
            "web",
            AuthMethod::Key {
                path: "~/k".into(),
                save_passphrase: false,
            },
        );
        web.host = "10.0.0.5".into();
        web.username = "deploy".into();
        web.group_id = Some(g.id);
        let other = conn("other", AuthMethod::Agent);
        let f = file(vec![g], vec![web.clone(), other], vec![]);
        let library = lib(vec![], vec![conn("web", AuthMethod::Agent)]);
        let seen = RefCell::new(Vec::<PathBuf>::new());
        let p = preview(&f, &library, |path| {
            seen.borrow_mut().push(path.into());
            false
        });
        assert_eq!(
            p.rows[0],
            ImportRow {
                id: web.id,
                name: "web".into(),
                target: "deploy@10.0.0.5:22".into(),
                group: Some("Prod".into()),
                duplicate: true,
                missing_key: Some("~/k".into()),
                jump: None,
            }
        );
        assert_eq!(*seen.borrow(), vec![expand_home(Path::new("~/k"))]);
        let row = &p.rows[1];
        assert_eq!(
            (row.duplicate, &row.missing_key, &row.group),
            (false, &None, &None)
        );
    }

    #[test]
    fn preview_found_key_is_not_missing() {
        let key = conn(
            "k",
            AuthMethod::Key {
                path: "/k".into(),
                save_passphrase: false,
            },
        );
        let f = file(vec![], vec![key], vec![]);
        let p = preview(&f, &lib(vec![], vec![]), |_| true);
        assert_eq!(p.rows[0].missing_key, None);
    }

    #[test]
    fn preview_counts_groups_and_snippets() {
        let (a, b) = (group("A"), group("B"));
        let c = conn("c", AuthMethod::Agent);
        let snippets = vec![
            snippet("g1", None),
            snippet("g2", None),
            snippet("on c", Some(c.id)),
            snippet("orphan", Some(Uuid::new_v4())),
        ];
        let f = file(vec![a, b], vec![c], snippets);
        let p = preview(&f, &lib(vec![group("A")], vec![]), |_| true);
        assert_eq!((p.groups_new, p.groups_reused), (1, 1));
        assert_eq!((p.snippets_global, p.snippets_per_connection), (2, 1));
    }

    #[test]
    fn preview_jump() {
        let bastion = conn("bastion", AuthMethod::Agent);
        let mut web = conn("web", AuthMethod::Agent);
        web.jump_via = Some(bastion.id);
        let mut lone = conn("lone", AuthMethod::Agent);
        lone.jump_via = Some(Uuid::new_v4());
        let f = file(vec![], vec![bastion.clone(), web, lone], vec![]);
        for existing in [false, true] {
            let saved = if existing {
                vec![conn("bastion", AuthMethod::Agent)]
            } else {
                vec![]
            };
            let p = preview(&f, &lib(vec![], saved), |_| true);
            assert_eq!(
                p.rows[1].jump,
                Some(JumpRef {
                    name: "bastion".into(),
                    in_file: Some(bastion.id),
                    existing,
                })
            );
            assert_eq!(p.rows[2].jump, None);
        }
    }

    #[test]
    fn plan_remaps_every_reference() {
        let (library, mut snippets) = sample();
        snippets.push(snippet("global", None));
        let text = export(&library, &snippets, "now").unwrap();
        let f = parse(&text).unwrap();
        let p = plan(&f, &all(&f), &Library::default()).unwrap();
        let old: Vec<Uuid> = f
            .groups
            .iter()
            .map(|g| g.id)
            .chain(all(&f))
            .chain(f.snippets.iter().map(|s| s.id))
            .collect();
        let new: Vec<Uuid> = p
            .groups
            .iter()
            .map(|g| g.id)
            .chain(p.connections.iter().map(|c| c.id))
            .chain(p.snippets.iter().map(|s| s.id))
            .collect();
        assert_eq!(new.len(), 5);
        let mut unique = new.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 5);
        assert!(new.iter().all(|id| !old.contains(id)));
        let (a, b) = (&p.connections[0], &p.connections[1]);
        assert_eq!(a.group_id, Some(p.groups[0].id));
        assert_eq!(b.jump_via, Some(a.id));
        assert_eq!(p.snippets[0].connection_id, Some(b.id));
        assert_eq!(p.snippets[1].connection_id, None);
    }

    #[test]
    fn plan_merges_groups_by_name() {
        let (a1, a2, b, unused) = (group("A"), group("A"), group("B"), group("C"));
        let mut x = conn("x", AuthMethod::Agent);
        x.group_id = Some(a1.id);
        let mut y = conn("y", AuthMethod::Agent);
        y.group_id = Some(a2.id);
        let mut z = conn("z", AuthMethod::Agent);
        z.group_id = Some(b.id);
        let f = file(vec![a1, a2, b, unused], vec![x, y, z], vec![]);
        let saved_b = group("B");
        let p = plan(&f, &all(&f), &lib(vec![saved_b.clone()], vec![])).unwrap();
        assert_eq!(p.groups.len(), 1);
        assert_eq!(p.groups[0].name, "A");
        let gid: Vec<_> = p.connections.iter().map(|c| c.group_id).collect();
        assert_eq!(
            gid,
            [Some(p.groups[0].id), Some(p.groups[0].id), Some(saved_b.id)]
        );

        let saved_a = group("A");
        let p = plan(&f, &all(&f), &lib(vec![saved_a.clone()], vec![])).unwrap();
        assert_eq!(p.groups.len(), 1);
        assert_eq!(p.groups[0].name, "B");
        assert_eq!(p.connections[0].group_id, Some(saved_a.id));
        assert_eq!(p.connections[1].group_id, Some(saved_a.id));
    }

    #[test]
    fn plan_jump_hosts() {
        let bastion = conn("bastion", AuthMethod::Agent);
        let old = conn("old", AuthMethod::Agent);
        let gone = conn("gone", AuthMethod::Agent);
        let webs: Vec<Connection> = [
            Some(bastion.id),
            Some(old.id),
            Some(gone.id),
            Some(Uuid::new_v4()),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, jump)| {
            let mut c = conn(&format!("web{i}"), AuthMethod::Agent);
            c.jump_via = jump;
            c
        })
        .collect();
        let mut selected: Vec<Uuid> = webs.iter().map(|c| c.id).collect();
        selected.push(bastion.id);
        let mut connections = vec![bastion, old, gone];
        connections.extend(webs);
        let f = file(vec![], connections, vec![]);
        let saved = vec![
            conn("old", AuthMethod::Agent),
            conn("old", AuthMethod::Agent),
            conn("bastion", AuthMethod::Agent),
        ];
        let p = plan(&f, &selected, &lib(vec![], saved.clone())).unwrap();
        let new_bastion = p.connections[0].id;
        assert_eq!(p.connections[0].name, "bastion");
        let jumps: Vec<_> = p.connections[1..].iter().map(|c| c.jump_via).collect();
        assert_eq!(jumps, [Some(new_bastion), Some(saved[0].id), None, None]);
    }

    #[test]
    fn plan_snippets() {
        let a = conn("a", AuthMethod::Agent);
        let b = conn("b", AuthMethod::Agent);
        let global = snippet("global", None);
        let f = file(
            vec![],
            vec![a.clone(), b.clone()],
            vec![
                global.clone(),
                snippet("on a", Some(a.id)),
                snippet("on b", Some(b.id)),
                snippet("orphan", Some(Uuid::new_v4())),
            ],
        );
        let p = plan(&f, &[a.id], &Library::default()).unwrap();
        let names: Vec<_> = p.snippets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["global", "on a"]);
        assert_ne!(p.snippets[0].id, global.id);
        assert_eq!(p.snippets[1].connection_id, Some(p.connections[0].id));
    }

    #[test]
    fn plan_rejects_unknown_selection() {
        let f = file(vec![], vec![conn("a", AuthMethod::Agent)], vec![]);
        assert!(matches!(
            plan(&f, &[Uuid::new_v4()], &Library::default()),
            Err(CoreError::InvalidInput { field, .. }) if field == "selected"
        ));
    }

    #[test]
    fn export_then_import_keeps_content() {
        let (mut library, mut snippets) = sample();
        library.groups[0].color = Some(TagColor::Red);
        let a = &mut library.connections[0];
        a.forwards = vec![ForwardRule::Dynamic {
            bind_host: "127.0.0.1".into(),
            bind_port: 1080,
            auto_start: true,
        }];
        a.notes = Some("note".into());
        a.color = Some(TagColor::Teal);
        a.icon = Some(ConnIcon::Web);
        snippets.push(snippet("global", None));
        let text = export(&library, &snippets, "now").unwrap();
        let f = parse(&text).unwrap();
        let p = plan(&f, &all(&f), &Library::default()).unwrap();
        assert_eq!(
            p.groups,
            vec![Group {
                id: p.groups[0].id,
                ..library.groups[0].clone()
            }]
        );
        for (new, old) in p.connections.iter().zip(&library.connections) {
            let expected = Connection {
                id: new.id,
                group_id: new.group_id,
                jump_via: new.jump_via,
                ..old.clone()
            };
            assert_eq!(*new, expected);
        }
        for (new, old) in p.snippets.iter().zip(&snippets) {
            let expected = Snippet {
                id: new.id,
                connection_id: new.connection_id,
                ..old.clone()
            };
            assert_eq!(*new, expected);
        }
        assert_eq!(p.connections.len(), 2);
        assert_eq!(p.snippets.len(), 2);
    }
}
