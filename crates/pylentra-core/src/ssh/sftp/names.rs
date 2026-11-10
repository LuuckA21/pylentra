//! Name rules for entries a server sends us (spec §4.2).

pub(crate) const TEMP_SUFFIX: &str = "pylentra-tmp";
pub(crate) const LOCAL_WINDOWS: bool = cfg!(windows);

/// True when `name` is one path component that is safe to create on a `windows` or Unix side.
pub(crate) fn valid_name(name: &str, windows: bool) -> bool {
    if name.is_empty()
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
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    !reserved
}

/// `dir` and `name` as a remote path.
pub(crate) fn remote_join(dir: &str, name: &str) -> String {
    format!("{}/{name}", dir.trim_end_matches('/'))
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
    }

    #[test]
    fn join_and_temp() {
        assert_eq!(remote_join("/", "a"), "/a");
        assert_eq!(remote_join("/x", "a"), "/x/a");
        assert_eq!(temp_name("f.txt", 2), "f.txt.2.pylentra-tmp");
    }
}
