//! Turns the text of `~/.ssh/config` into import candidates. Pure: reading the
//! file and saving the chosen connections happen elsewhere.

use crate::model::{AuthMethod, Connection, ForwardRule};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::PathBuf;
use ts_rs::TS;
use uuid::Uuid;

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct ScanResult {
    pub path: String,
    pub missing: bool,
    pub candidates: Vec<Candidate>,
    pub skipped: Vec<FileNote>,
}

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct Candidate {
    pub connection: Connection,
    pub notes: Vec<ImportNote>,
    /// Set when the candidate cannot be imported as it is.
    pub error: Option<ImportNote>,
    /// A saved connection already has this name (ignoring case).
    pub exists: bool,
    pub needs_user: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[ts(export)]
pub struct FileNote {
    pub line: usize,
    pub note: ImportNote,
}

#[derive(Serialize, Clone, Debug, PartialEq, TS)]
#[serde(tag = "type")]
#[ts(export)]
pub enum ImportNote {
    Include,
    Match,
    IdentityTokens,
    MultiHopJump,
    ChainedJump {
        target: String,
    },
    JumpNotAlias {
        value: String,
    },
    UnknownJump {
        alias: String,
    },
    RemoteBindIgnored,
    ProxyCommand,
    /// Too many wildcard blocks for the number of hosts: the hosts that fit were read.
    TooComplex,
    InvalidPort {
        value: String,
    },
    InvalidForward {
        value: String,
    },
}

/// Most bytes (of lines and of pattern-by-alias comparisons) `parse` spends on aliases.
const MAX_WORK: usize = 20_000_000;
/// A pattern byte compared with an alias byte costs about this fraction of a line byte visited.
const GLOB_WEIGHT: usize = 32;
/// Longest host name `%h` may expand to.
const MAX_EXPANDED_HOST: usize = 1024;
const MAX_JUMP_NOTES: usize = 16;

struct Block {
    /// Empty for a `Match` block, so it applies to no host.
    patterns: Vec<String>,
    lines: Vec<(String, Vec<String>)>,
}

/// Splits arguments on whitespace; double quotes group, and an unquoted `#`
/// starting a token ends the line, as in OpenSSH.
fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        if chars.peek().is_none_or(|&c| c == '#') {
            return out;
        }
        let mut token = String::new();
        let mut quoted = false;
        while let Some(c) = chars.next_if(|&c| quoted || !c.is_whitespace()) {
            if c == '"' {
                quoted = !quoted;
            } else {
                token.push(c);
            }
        }
        out.push(token);
    }
}

/// `*` and `?` wildcards, ignoring ASCII case like OpenSSH. Iterative, so a
/// hostile file cannot make it backtrack exponentially or overflow the stack.
fn glob(pattern: &[u8], s: &[u8]) -> bool {
    let (mut p, mut i) = (0, 0);
    // Pattern index just after the last `*`, and the text index it currently stands for.
    let mut star = None;
    while let Some(&c) = s.get(i) {
        match pattern.get(p) {
            Some(b'*') => {
                p += 1;
                star = Some((p, i));
            }
            Some(&q) if q == b'?' || q.eq_ignore_ascii_case(&c) => {
                p += 1;
                i += 1;
            }
            _ => match star {
                Some((sp, si)) => {
                    p = sp;
                    i = si + 1;
                    star = Some((sp, i));
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == b'*')
}

fn block_matches(patterns: &[String], alias: &str) -> bool {
    let mut matched = false;
    for p in patterns {
        match p.strip_prefix('!') {
            Some(neg) if glob(neg.as_bytes(), alias.as_bytes()) => return false,
            Some(_) => {}
            None => matched |= glob(p.as_bytes(), alias.as_bytes()),
        }
    }
    matched
}

/// `port`, `host:port` or `[host]:port`, with the port in 1..=65535.
fn host_port(s: &str) -> Option<(Option<&str>, u16)> {
    let (host, port) = match s.rsplit_once(':') {
        None => (None, s),
        Some((h, p)) => {
            let unbracketed = h.strip_prefix('[').and_then(|h| h.strip_suffix(']'));
            (Some(unbracketed.unwrap_or(h)), p)
        }
    };
    let port = port.parse::<u16>().ok().filter(|p| *p != 0)?;
    Some((host, port))
}

/// Defaults and spellings from ssh(1): no address is loopback, empty or `*` is every interface.
fn bind_address(bind: Option<&str>) -> Option<&str> {
    match bind {
        None | Some("localhost") => Some("127.0.0.1"),
        Some("" | "*") => Some("0.0.0.0"),
        Some(h) => h.parse::<IpAddr>().is_ok().then_some(h),
    }
}

fn dynamic(args: &[String], notes: &mut Vec<ImportNote>) -> Option<ForwardRule> {
    let rule = match args {
        [listen] => host_port(listen).and_then(|(bind, bind_port)| {
            bind_address(bind).map(|bind_host| ForwardRule::Dynamic {
                bind_host: bind_host.into(),
                bind_port,
                auto_start: false,
            })
        }),
        _ => None,
    };
    if rule.is_none() {
        notes.push(ImportNote::InvalidForward {
            value: args.join(" "),
        });
    }
    rule
}

fn forward(key: &str, args: &[String], notes: &mut Vec<ImportNote>) -> Option<ForwardRule> {
    let parsed = match args {
        [listen, dest] => host_port(listen).zip(host_port(dest)),
        _ => None,
    };
    let rule = match parsed {
        Some(((bind, bind_port), (Some(dest_host), dest_port))) if !dest_host.is_empty() => {
            let dest_host = dest_host.to_string();
            if key == "remoteforward" {
                if bind.is_some() {
                    notes.push(ImportNote::RemoteBindIgnored);
                }
                Some(ForwardRule::Remote {
                    bind_port,
                    dest_host,
                    dest_port,
                    auto_start: false,
                })
            } else {
                bind_address(bind).map(|bind_host| ForwardRule::Local {
                    bind_host: bind_host.into(),
                    bind_port,
                    dest_host,
                    dest_port,
                    auto_start: false,
                })
            }
        }
        _ => None,
    };
    if rule.is_none() {
        notes.push(ImportNote::InvalidForward {
            value: args.join(" "),
        });
    }
    rule
}

/// Characters that make a `ProxyJump` hop a `user@host:port` or URI, not a host alias.
const NOT_ALIAS: [char; 3] = ['@', ':', '/'];

/// One `ProxyJump` hop. A saved connection wins: a candidate of the same name is
/// marked `exists` and not imported.
fn resolve_jump(
    hop: &str,
    saved: &HashMap<String, Uuid>,
    aliases: &HashMap<String, Uuid>,
    notes: &mut Vec<ImportNote>,
) -> Option<Uuid> {
    // A candidate lists a few unresolved hops at most, so a huge hop list cannot flood the notes.
    let room = notes.len() < MAX_JUMP_NOTES;
    if hop.contains(NOT_ALIAS) {
        if room {
            notes.push(ImportNote::JumpNotAlias { value: hop.into() });
        }
        return None;
    }
    let key = hop.to_ascii_lowercase();
    let id = saved.get(&key).or_else(|| aliases.get(&key)).copied();
    if id.is_none() && room {
        notes.push(ImportNote::UnknownJump { alias: hop.into() });
    }
    id
}

/// Turns each resolved `ProxyJump h1,…,hn` into jumps: target → hn, hk → hk-1.
/// A chain applies only if no hop has a jump of its own that differs (saved ones are
/// never changed) and no host would get two different jumps from all the chains
/// together, so the outcome does not depend on the order of the file.
fn link_chains(
    candidates: &mut [Candidate],
    chains: &[(usize, Vec<Uuid>)],
    free: &[bool],
    saved: &[Connection],
) {
    let index: HashMap<Uuid, usize> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| (c.connection.id, i))
        .collect();
    let links: Vec<Vec<(Uuid, Uuid)>> = chains
        .iter()
        .map(|(t, hops)| {
            let target = candidates[*t].connection.id;
            let last = hops.last().map(|&h| (target, h));
            last.into_iter()
                .chain(hops.windows(2).map(|w| (w[1], w[0])))
                .collect()
        })
        .collect();
    let mut wanted: HashMap<Uuid, HashSet<Uuid>> = HashMap::new();
    for &(host, via) in links.iter().flatten() {
        wanted.entry(host).or_default().insert(via);
    }
    let ok: Vec<bool> = chains
        .iter()
        .zip(&links)
        .map(|((t, hops), links)| {
            let mut hosts: HashSet<Uuid> = hops.iter().copied().collect();
            // A repeated host would make the chain loop back on itself.
            let distinct = hosts.len() == hops.len() && hosts.insert(candidates[*t].connection.id);
            let settable = |&(host, via): &(Uuid, Uuid)| match index.get(&host) {
                Some(&i) => i == *t || free[i] || candidates[i].connection.jump_via == Some(via),
                None => saved
                    .iter()
                    .any(|c| c.id == host && c.jump_via == Some(via)),
            };
            distinct
                && links
                    .iter()
                    .all(|l| wanted.get(&l.0).is_some_and(|v| v.len() == 1) && settable(l))
        })
        .collect();
    // By target name, so a shared hop's notes do not depend on block order.
    let mut order: Vec<_> = chains.iter().zip(&links).zip(ok).collect();
    order.sort_by_key(|((chain, _), _)| candidates[chain.0].connection.name.to_lowercase());
    for (((t, _), links), ok) in order {
        if !ok {
            candidates[*t].notes.push(ImportNote::MultiHopJump);
            continue;
        }
        let target = candidates[*t].connection.name.clone();
        for &(host, via) in links {
            if let Some(&i) = index.get(&host) {
                candidates[i].connection.jump_via = Some(via);
                if i != *t && free[i] {
                    candidates[i].notes.push(ImportNote::ChainedJump {
                        target: target.clone(),
                    });
                }
            }
        }
    }
}

pub fn parse(text: &str, saved: &[Connection]) -> ScanResult {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut skipped = Vec::new();
    // Lines before the first `Host` apply to every host, as in OpenSSH.
    let mut blocks = vec![Block {
        patterns: vec!["*".into()],
        lines: vec![],
    }];
    for (i, line) in text.lines().enumerate() {
        let line = line.trim_start();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let end = line
            .find(|c: char| c.is_whitespace() || c == '=')
            .unwrap_or(line.len());
        let (key, rest) = line.split_at(end);
        let rest = rest.trim_start();
        let key = key.to_ascii_lowercase();
        let args = tokens(rest.strip_prefix('=').unwrap_or(rest));
        let note = |note| FileNote { line: i + 1, note };
        match key.as_str() {
            "host" => blocks.push(Block {
                patterns: args,
                lines: vec![],
            }),
            "match" => {
                skipped.push(note(ImportNote::Match));
                blocks.push(Block {
                    patterns: vec![],
                    lines: vec![],
                });
            }
            "include" => skipped.push(note(ImportNote::Include)),
            _ => {
                if let Some(block) = blocks.last_mut() {
                    block.lines.push((key, args));
                }
            }
        }
    }

    // Concrete aliases in order of first appearance, ignoring case as OpenSSH does.
    let is_literal = |p: &String| !p.contains(['*', '?']) && !p.starts_with('!');
    let mut aliases: Vec<(&str, Uuid)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    // Blocks of literal names only are found by lookup; the others are tested one by one.
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    let mut general: Vec<usize> = Vec::new();
    for (bi, b) in blocks.iter().enumerate() {
        if b.patterns.iter().all(is_literal) {
            for p in &b.patterns {
                by_name.entry(p.to_ascii_lowercase()).or_default().push(bi);
            }
        } else {
            general.push(bi);
        }
        for p in &b.patterns {
            if !p.trim().is_empty() && is_literal(p) && seen.insert(p.to_ascii_lowercase()) {
                aliases.push((p, Uuid::new_v4()));
            }
        }
    }

    // Every alias replays the wildcard blocks (each pattern against its name) and the lines of
    // its own blocks; a line costs its bytes, which also bounds a `ProxyJump` hop list.
    let lines_cost = |bi: &usize| -> usize {
        (blocks[*bi].lines.iter())
            .map(|(_, args)| 1 + args.iter().map(String::len).sum::<usize>())
            .sum()
    };
    let general_lines: usize = general.iter().map(lines_cost).sum();
    let general_pats: usize = (general.iter())
        .flat_map(|&bi| &blocks[bi].patterns)
        .map(|p| p.len() + 1)
        .sum();
    let mut spent = 0usize;
    let kept = aliases
        .iter()
        .take_while(|(a, _)| {
            let own: usize = (by_name.get(&a.to_ascii_lowercase()))
                .map_or(0, |v| v.iter().map(lines_cost).sum());
            spent = spent.saturating_add(
                general_lines + general_pats.saturating_mul(a.len() + 1) / GLOB_WEIGHT + own,
            );
            spent <= MAX_WORK
        })
        .count();
    if kept < aliases.len() {
        aliases.truncate(kept);
        skipped.push(FileNote {
            line: text.lines().count(),
            note: ImportNote::TooComplex,
        });
    }
    let alias_ids: HashMap<String, Uuid> = (aliases.iter())
        .map(|(a, id)| (a.to_ascii_lowercase(), *id))
        .collect();
    let mut saved_ids: HashMap<String, Uuid> = HashMap::new();
    for c in saved {
        saved_ids.entry(c.name.to_ascii_lowercase()).or_insert(c.id);
    }

    // Multi-hop `ProxyJump`s that resolve, and which candidates have no jump of their own.
    let mut chains = Vec::new();
    let mut free = Vec::new();
    let mut candidates: Vec<Candidate> = aliases
        .iter()
        .enumerate()
        .map(|(i, &(alias, id))| {
            let mut first: HashMap<&str, &str> = HashMap::new();
            let mut forwards = Vec::new();
            let mut jump: Option<&[String]> = None;
            let mut hits = by_name
                .get(&alias.to_ascii_lowercase())
                .cloned()
                .unwrap_or_default();
            hits.extend(
                general
                    .iter()
                    .copied()
                    .filter(|&bi| block_matches(&blocks[bi].patterns, alias)),
            );
            hits.sort_unstable();
            hits.dedup();
            for block in hits.into_iter().map(|bi| &blocks[bi]) {
                for (key, args) in &block.lines {
                    // Forwards add up over every matching block; everything else keeps its first
                    // non-blank value (`HostName ""` would otherwise give an unimportable empty host).
                    if matches!(
                        key.as_str(),
                        "localforward" | "remoteforward" | "dynamicforward"
                    ) {
                        forwards.push((key.as_str(), args.as_slice()));
                    } else if let Some(value) = args.first().filter(|v| !v.trim().is_empty()) {
                        first.entry(key.as_str()).or_insert(value.as_str());
                        // Unquoted `ProxyJump a, b` splits into several tokens; every one is a hop list.
                        if key == "proxyjump" {
                            jump.get_or_insert(args);
                        }
                    }
                }
            }

            let mut notes = Vec::new();
            let mut error = None;
            let port = match first.get("port") {
                None => 22,
                Some(v) => v
                    .parse::<u16>()
                    .ok()
                    .filter(|p| *p != 0)
                    .unwrap_or_else(|| {
                        error = Some(ImportNote::InvalidPort {
                            value: v.to_string(),
                        });
                        0
                    }),
            };
            let auth = match first.get("identityfile") {
                Some(path) if path.contains('%') => {
                    notes.push(ImportNote::IdentityTokens);
                    AuthMethod::Auto
                }
                Some(path) => AuthMethod::Key {
                    path: PathBuf::from(path),
                    save_passphrase: false,
                },
                None => AuthMethod::Auto,
            };
            let hops: Vec<&str> = jump.map_or_else(Vec::new, |args| {
                args.iter()
                    .flat_map(|a| a.split(','))
                    .map(str::trim)
                    .filter(|h| !h.is_empty())
                    .collect()
            });
            let no_jump = match hops[..] {
                [] => true,
                // `Host *` + `ProxyJump bastion` reaches bastion too; it simply connects directly.
                [v] => {
                    v.eq_ignore_ascii_case("none")
                        || (v.eq_ignore_ascii_case(alias) && !v.contains(NOT_ALIAS))
                }
                _ => false,
            };
            free.push(no_jump);
            let jump_via = match hops[..] {
                _ if no_jump => None,
                [v] => resolve_jump(v, &saved_ids, &alias_ids, &mut notes),
                _ => {
                    let ids: Vec<Uuid> = hops
                        .iter()
                        .filter_map(|h| resolve_jump(h, &saved_ids, &alias_ids, &mut notes))
                        .collect();
                    if ids.len() == hops.len() {
                        chains.push((i, ids));
                    }
                    None
                }
            };
            let forwards = forwards
                .into_iter()
                .filter_map(|(key, args)| match key {
                    "dynamicforward" => dynamic(args, &mut notes),
                    _ => forward(key, args, &mut notes),
                })
                .collect();
            if first
                .get("proxycommand")
                .is_some_and(|v| !v.eq_ignore_ascii_case("none"))
            {
                notes.push(ImportNote::ProxyCommand);
            }

            // `%h` is the alias, as in `HostName %h.example.com`.
            let host = match first.get("hostname") {
                None => alias.into(),
                Some(h) if h.len() + h.matches("%h").count() * alias.len() > MAX_EXPANDED_HOST => {
                    error.get_or_insert(ImportNote::TooComplex);
                    alias.into()
                }
                Some(h) => h.replace("%h", alias),
            };
            let username = first.get("user").copied().unwrap_or_default().to_string();
            Candidate {
                needs_user: username.is_empty(),
                exists: saved_ids.contains_key(&alias.to_ascii_lowercase()),
                error,
                notes,
                connection: Connection {
                    id,
                    name: alias.into(),
                    group_id: None,
                    host,
                    port,
                    username,
                    auth,
                    jump_via,
                    forwards,
                    color: None,
                    icon: None,
                    notes: None,
                    extra: Default::default(),
                },
            }
        })
        .collect();
    link_chains(&mut candidates, &chains, &free, saved);

    ScanResult {
        path: String::new(),
        missing: false,
        candidates,
        skipped,
    }
}
