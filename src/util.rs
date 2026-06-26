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
