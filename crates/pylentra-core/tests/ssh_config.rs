//! `~/.ssh/config` text → import candidates (spec §7.1).
use std::path::PathBuf;

use pylentra_core::model::{AuthMethod, Connection, ForwardRule};
use pylentra_core::ssh_config::{Candidate, FileNote, ImportNote, ScanResult, parse};
use uuid::Uuid;

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/ssh_config/{name}.conf",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path).unwrap()
}

fn scan(name: &str) -> ScanResult {
    parse(&fixture(name), &[])
}

fn get<'a>(r: &'a ScanResult, alias: &str) -> &'a Candidate {
    r.candidates
        .iter()
        .find(|c| c.connection.name == alias)
        .unwrap_or_else(|| panic!("no candidate {alias}"))
}

fn names(r: &ScanResult) -> Vec<&str> {
    r.candidates
        .iter()
        .map(|c| c.connection.name.as_str())
        .collect()
}

fn saved(name: &str) -> Connection {
    Connection {
        id: Uuid::new_v4(),
        name: name.into(),
        group_id: None,
        host: "saved.example.com".into(),
        port: 22,
        username: "luca".into(),
        auth: AuthMethod::Agent,
        jump_via: None,
        forwards: vec![],
        color: None,
        icon: None,
        notes: None,
    }
}

#[test]
fn host_star_supplies_defaults_first_value_wins() {
    let r = scan("host_star_defaults");
    assert_eq!(names(&r), ["web"]);
    let c = &get(&r, "web").connection;
    assert_eq!(
        (c.host.as_str(), c.username.as_str(), c.port),
        ("w1", "luca", 2222)
    );
    assert_eq!((c.color, c.icon, c.group_id), (None, None, None));
    assert_eq!((r.path.as_str(), r.missing), ("", false));
}

#[test]
fn several_aliases_on_one_line() {
    let r = scan("several_aliases");
    assert_eq!(names(&r), ["a", "b"]);
    assert_eq!(get(&r, "b").connection.host, "shared");
    assert_ne!(get(&r, "a").connection.id, get(&r, "b").connection.id);
}

#[test]
fn duplicate_alias_gives_one_candidate() {
    let r = scan("duplicate_alias");
    assert_eq!(names(&r), ["a"]);
    let c = &get(&r, "a").connection;
    // Later matching blocks still fill keys the first block left unset.
    assert_eq!((c.host.as_str(), c.username.as_str()), ("first", "late"));
}

#[test]
fn negated_pattern_excludes_the_block() {
    let r = scan("negated_pattern");
    assert_eq!(names(&r), ["bastion", "app"]);
    assert_eq!(get(&r, "bastion").connection.username, "");
    assert_eq!(get(&r, "app").connection.username, "x");
}

#[test]
fn equals_sign_quotes_and_case() {
    let r = scan("equals_quotes_case");
    let c = &get(&r, "a").connection;
    assert_eq!(
        (c.host.as_str(), c.port, c.username.as_str()),
        ("my host", 22, "luca")
    );
    assert_eq!(get(&r, "a").error, None);
}

#[test]
fn comments_blank_lines_crlf_and_bom() {
    // Inline rather than a fixture: .gitattributes would turn CRLF into LF.
    let r = parse(
        "\u{feff}Host a\r\n\r\n  # c\r\n  HostName h # trailing\r\n",
        &[],
    );
    assert_eq!(names(&r), ["a"]);
    assert_eq!(get(&r, "a").connection.host, "h");
    assert!(r.skipped.is_empty());
}

#[test]
fn include_and_match_are_skipped_with_line() {
    let r = scan("include_match");
    assert_eq!(
        r.skipped,
        [
            FileNote {
                line: 1,
                note: ImportNote::Include
            },
            FileNote {
                line: 3,
                note: ImportNote::Match
            },
        ]
    );
    assert_eq!(names(&r), ["a", "c"]);
    // Lines under Match belong to no Host block.
    assert_eq!(get(&r, "a").connection.username, "");
}

#[test]
fn proxyjump_to_alias_links_the_candidate() {
    let r = scan("proxyjump_alias");
    let app = get(&r, "app");
    assert_eq!(
        app.connection.jump_via,
        Some(get(&r, "bastion").connection.id)
    );
    assert!(app.notes.is_empty());
}

#[test]
fn proxyjump_to_saved_connection() {
    let bastion = saved("Bastion");
    let r = parse(&fixture("proxyjump_saved"), std::slice::from_ref(&bastion));
    let app = get(&r, "app");
    assert_eq!(app.connection.jump_via, Some(bastion.id));
    assert!(app.notes.is_empty());
}

#[test]
fn proxyjump_multi_hop_user_host_and_unknown() {
    let r = scan("proxyjump_kinds");
    let check = |alias: &str, notes: Vec<ImportNote>| {
        let c = get(&r, alias);
        assert_eq!((c.connection.jump_via, &c.notes), (None, &notes), "{alias}");
    };
    check("multi", vec![ImportNote::MultiHopJump]);
    check(
        "userhost",
        vec![ImportNote::JumpNotAlias {
            value: "u@h".into(),
        }],
    );
    check(
        "unknown",
        vec![ImportNote::UnknownJump {
            alias: "zzz".into(),
        }],
    );
    check("off", vec![]);
}

#[test]
fn identity_file_and_tokens() {
    let r = scan("identity_file");
    assert_eq!(
        get(&r, "key").connection.auth,
        AuthMethod::Key {
            path: PathBuf::from("~/.ssh/k"),
            save_passphrase: false
        }
    );
    let tokens = get(&r, "tokens");
    assert_eq!(tokens.connection.auth, AuthMethod::Auto);
    assert_eq!(tokens.notes, [ImportNote::IdentityTokens]);
    let none = get(&r, "none");
    assert_eq!(none.connection.auth, AuthMethod::Auto);
    assert!(none.notes.is_empty());
}

#[test]
fn forwards() {
    let r = scan("forwards");
    let c = get(&r, "a");
    assert_eq!(
        c.connection.forwards,
        [
            ForwardRule::Local {
                bind_host: "127.0.0.1".into(),
                bind_port: 8080,
                dest_host: "db".into(),
                dest_port: 5432,
                auto_start: false,
            },
            ForwardRule::Local {
                bind_host: "0.0.0.0".into(),
                bind_port: 9000,
                dest_host: "h".into(),
                dest_port: 1,
                auto_start: false,
            },
            ForwardRule::Remote {
                bind_port: 9000,
                dest_host: "h".into(),
                dest_port: 80,
                auto_start: false,
            },
        ]
    );
    assert_eq!(c.notes, [ImportNote::RemoteBindIgnored]);
}

#[test]
fn ipv6_bind_and_invalid_forward() {
    let r = parse(
        "Host a\n LocalForward [::1]:8080 [fe80::1]:22\n LocalForward 8080\n LocalForward x:1 h:2\n",
        &[],
    );
    let c = get(&r, "a");
    assert_eq!(
        c.connection.forwards,
        [ForwardRule::Local {
            bind_host: "::1".into(),
            bind_port: 8080,
            dest_host: "fe80::1".into(),
            dest_port: 22,
            auto_start: false,
        }]
    );
    assert_eq!(
        c.notes,
        [
            ImportNote::InvalidForward {
                value: "8080".into()
            },
            ImportNote::InvalidForward {
                value: "x:1 h:2".into()
            },
        ]
    );
}

#[test]
fn unsupported_directives_noted() {
    let r = scan("unsupported");
    assert_eq!(
        get(&r, "a").notes,
        [ImportNote::DynamicForward, ImportNote::ProxyCommand]
    );
}

#[test]
fn bad_port_marks_the_candidate() {
    let r = scan("bad_port");
    assert_eq!(
        get(&r, "a").error,
        Some(ImportNote::InvalidPort {
            value: "99999".into()
        })
    );
}

#[test]
fn missing_user_sets_needs_user() {
    let r = scan("missing_user");
    let c = get(&r, "a");
    assert_eq!((c.connection.username.as_str(), c.needs_user), ("", true));
    assert_eq!((c.connection.port, c.error.clone()), (22, None));
    assert!(!scan("host_star_defaults").candidates[0].needs_user);
}

#[test]
fn wildcard_only_hosts_are_not_candidates() {
    assert!(scan("wildcard_only").candidates.is_empty());
}

#[test]
fn exists_matches_saved_names_case_insensitively() {
    let r = parse(&fixture("exists"), &[saved("WEB")]);
    assert!(get(&r, "web").exists);
    assert!(!get(&r, "new").exists);
}

#[test]
fn empty_file() {
    let r = parse("", &[]);
    assert!(r.candidates.is_empty() && r.skipped.is_empty());
}

#[test]
fn proxyjump_to_itself_is_dropped() {
    let r = parse(
        "Host app\nHost bastion\nHost *\n    ProxyJump bastion\n",
        &[],
    );
    let bastion = get(&r, "bastion");
    assert_eq!(
        (bastion.connection.jump_via, bastion.notes.len()),
        (None, 0)
    );
    assert_eq!(
        get(&r, "app").connection.jump_via,
        Some(bastion.connection.id)
    );
}

#[test]
fn long_alias_and_pathological_pattern() {
    let long = "a".repeat(100_000);
    let r = parse(&format!("Host {long}\n"), &[]);
    assert_eq!(r.candidates[0].connection.host, long);

    let pattern = format!("{}b", "*a".repeat(12));
    let text = format!(
        "Host {pattern}\n    User x\nHost {} {}b\n",
        "a".repeat(40),
        "a".repeat(40)
    );
    let r = parse(&text, &[]);
    assert_eq!(r.candidates[0].connection.username, "");
    assert_eq!(r.candidates[1].connection.username, "x");
}

#[test]
fn empty_host_name_falls_back_to_the_alias() {
    let r = parse(
        "Host a\n    HostName \"\"\nHost b\n    HostName \"  \"\n",
        &[],
    );
    assert_eq!(get(&r, "a").connection.host, "a");
    assert_eq!(get(&r, "b").connection.host, "b");
}

#[test]
fn garbage_input_does_not_break_the_import() {
    let text = "\"\nPort=\nHost\nHost \"\"\nHost \" \"\n=x\nHöst ünïcode\n\t\n\
                Host ok \"\nHost g\n    Port=\n    LocalForward 8080\n    Ünknown wert\n    \
                HostName héllo\n    User \"\n    RemoteForward\n    ProxyJump\n";
    let r = parse(text, &[]);
    assert_eq!(names(&r), ["ok", "g"]);
    for c in r.candidates.iter().filter(|c| c.error.is_none()) {
        let mut conn = c.connection.clone();
        if c.needs_user {
            conn.username = "u".into();
        }
        assert!(conn.validate().is_ok(), "{conn:?}");
    }
}
