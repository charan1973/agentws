use std::path::{Path, PathBuf};

/// Expand a leading `~` to the user's home directory.
pub fn expand_tilde<P: AsRef<Path>>(p: P) -> PathBuf {
    let p = p.as_ref();
    let s = match p.to_str() {
        Some(s) => s,
        None => return p.to_path_buf(),
    };
    if let Some(home) = crate::config::home_dir() {
        if s == "~" {
            return home;
        }
        if let Some(rest) = s.strip_prefix("~/") {
            return home.join(rest);
        }
    }
    p.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_bare_home() {
        let Some(home) = crate::config::home_dir() else {
            return; // no resolvable home on this host -> skip
        };
        assert_eq!(expand_tilde("~"), home);
    }

    #[test]
    fn expand_tilde_home_subpath() {
        let Some(home) = crate::config::home_dir() else { return; };
        assert_eq!(expand_tilde("~/work"), home.join("work"));
    }

    #[test]
    fn expand_tilde_absolute_unchanged() {
        assert_eq!(expand_tilde("/usr/local/bin"), std::path::PathBuf::from("/usr/local/bin"));
    }

    #[test]
    fn expand_tilde_relative_unchanged() {
        assert_eq!(expand_tilde("relative/path"), std::path::PathBuf::from("relative/path"));
    }

    #[test]
    fn expand_tilde_non_leading_unchanged() {
        // a tilde not at the very start is left alone
        assert_eq!(expand_tilde("a~/b"), std::path::PathBuf::from("a~/b"));
    }
}
