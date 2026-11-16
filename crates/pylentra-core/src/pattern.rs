/// `*` and `?` wildcards, ignoring ASCII case like OpenSSH. Iterative, so a
/// hostile file cannot make it backtrack exponentially or overflow the stack.
pub(crate) fn glob(pattern: &[u8], s: &[u8]) -> bool {
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

/// OpenSSH pattern list: a matching `!negated` entry rejects, otherwise any
/// positive entry accepts, so a list of only negations matches nothing.
pub(crate) fn list_matches<'a>(patterns: impl IntoIterator<Item = &'a str>, s: &str) -> bool {
    let mut matched = false;
    for p in patterns {
        match p.strip_prefix('!') {
            Some(neg) if glob(neg.as_bytes(), s.as_bytes()) => return false,
            Some(_) => {}
            None => matched |= glob(p.as_bytes(), s.as_bytes()),
        }
    }
    matched
}
