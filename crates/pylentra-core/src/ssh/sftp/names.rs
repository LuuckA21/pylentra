//! Name rules for entries a server sends us (spec §4.2).

pub(crate) const TEMP_SUFFIX: &str = "pylentra-tmp";
pub(crate) const LOCAL_WINDOWS: bool = cfg!(windows);
/// Bytes; real file systems stop at 255 bytes (Unix) or 255 UTF-16 units (NTFS, at most 765).
pub(crate) const MAX_NAME: usize = 1024;

/// True when `name` is one path component that is safe to create on a `windows` or Unix side.
pub(crate) fn valid_name(name: &str, windows: bool) -> bool {
    if name.is_empty()
        || name.len() > MAX_NAME
        || name == "."
        || name == ".."
        || name
            .chars()
            .any(|c| matches!(c, '/' | '\\') || c.is_control())
    {
        return false;
    }
    if !windows {
        return true;
    }
    if name.contains(['<', '>', ':', '"', '|', '?', '*']) || name.ends_with(['.', ' ']) {
        return false;
    }
    // Win32 drops trailing spaces and dots of the stem, so "CON .txt" is the CON device.
    let stem = name.split('.').next().unwrap_or_default();
    let stem = stem.trim_end_matches([' ', '.']).to_ascii_uppercase();
    let reserved = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|d| {
            let mut d = d.chars();
            matches!(
                (d.next(), d.next()),
                (Some('0'..='9' | '¹' | '²' | '³'), None)
            )
        });
    !reserved && !short_alias(name)
}

/// An 8.3 short name (`INSTAL~1.EXE`): Win32 would open the long-named item it stands for.
fn short_alias(name: &str) -> bool {
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    let digits = stem.rsplit_once('~').map_or("", |(_, d)| d);
    stem.chars().count() <= 8
        && ext.chars().count() <= 3
        && !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
}

/// `dir` and `name` as a remote path.
pub(crate) fn remote_join(dir: &str, name: &str) -> String {
    format!("{}/{name}", dir.trim_end_matches('/'))
}

/// A remote item path as (parent, name): "/a/b/" is ("/a", "b"), "/a" is ("", "a"), "a" is
/// (".", "a"); `remote_join` puts them back together.
pub(crate) fn remote_split(path: &str) -> (&str, &str) {
    let path = path.trim_end_matches('/');
    path.rsplit_once('/').unwrap_or((".", path))
}

pub(crate) fn temp_name(name: &str, n: u32) -> String {
    format!("{name}.{n}.{TEMP_SUFFIX}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        for ok in ["a", "a.txt", ".bashrc", "è", "a b"] {
            assert!(valid_name(ok, true), "{ok}");
        }
        for bad in ["", ".", "..", "a/b", "a\\b", "a\0b", "a\nb", "\u{7f}"] {
            assert!(!valid_name(bad, false), "{bad:?}");
        }
        for win in [
            "CON", "con.txt", "NUL", "COM1", "lpt9.log", "a:b", "a?", "a*", "a<", "a|", "a\"",
            "a.", "a ", "CON .txt", "nul .", "Aux  .x",
        ] {
            assert!(!valid_name(win, true), "{win}");
            assert!(valid_name(win, false) || win.contains(['/', '\\']), "{win}");
        }
        // Reserved by Win32 too, or an 8.3 short name.
        for win in [
            "COM\u{b9}",
            "lpt\u{b2}.log",
            "Com\u{b3}",
            "COM0",
            "lpt0.txt",
            "CONIN$",
            "conout$.log",
            "INSTAL~1.EXE",
            "docum~12",
            "A~1.B",
        ] {
            assert!(!valid_name(win, true), "{win}");
            assert!(valid_name(win, false), "{win}");
        }
        // Not in 8.3 alias shape: no digit after `~`, a long stem or a long extension.
        for ok in [
            "a~b.txt",
            "a~",
            "longername~1.txt",
            "a~1.long",
            "COM10",
            "LPT\u{b9}0",
        ] {
            assert!(valid_name(ok, true), "{ok}");
        }
        let long = "a".repeat(MAX_NAME);
        assert!(valid_name(&long, false));
        assert!(!valid_name(&format!("{long}b"), false));
    }

    #[test]
    fn join_and_temp() {
        assert_eq!(remote_join("/", "a"), "/a");
        assert_eq!(remote_join("/x", "a"), "/x/a");
        assert_eq!(temp_name("f.txt", 2), "f.txt.2.pylentra-tmp");
        assert_eq!(remote_split("/a/b/"), ("/a", "b"));
        assert_eq!(remote_split("/a"), ("", "a"));
        assert_eq!(remote_split("a"), (".", "a"));
        assert_eq!(remote_join(remote_split("/a/b").0, "b"), "/a/b");
    }
}
