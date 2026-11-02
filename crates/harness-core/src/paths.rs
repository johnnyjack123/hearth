//! Well-known locations on the host and path comparison helpers.

use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::lang::tr;
use crate::{Error, Result, tr};

/// Directory for machine-local state that is not configuration: the isolated
/// podman home and build contexts.
///
/// Windows: `%LOCALAPPDATA%\llm-harness`. Elsewhere: `$XDG_DATA_HOME/llm-harness`
/// or `~/.local/share/llm-harness`.
pub fn local_data_dir() -> Result<PathBuf> {
    if let Some(dir) = env::var_os("LOCALAPPDATA") {
        return Ok(PathBuf::from(dir).join("llm-harness"));
    }
    if let Some(dir) = env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(dir).join("llm-harness"));
    }
    Ok(home_dir()?.join(".local").join("share").join("llm-harness"))
}

/// The user's home directory (`%USERPROFILE%` on Windows, `$HOME` elsewhere).
pub fn home_dir() -> Result<PathBuf> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| {
            Error::Unsupported(
                tr(
                    "Weder USERPROFILE noch HOME ist gesetzt",
                    "Neither USERPROFILE nor HOME is set",
                )
                .to_owned(),
            )
        })
}

/// First `exe` found in a `PATH` directory.
pub fn find_on_path(exe: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(exe))
        .find(|p| p.is_file())
}

/// Absolute, symlink-free form of an existing path, without the `\\?\`
/// prefix Windows adds, so it can be shown to users and stored in config.
pub fn canonical(path: &Path) -> Result<PathBuf> {
    let resolved = fs::canonicalize(path)
        .map_err(|e| Error::io(tr!("{} auflösen", "Resolving {}", path.display()), e))?;
    Ok(strip_verbatim(resolved))
}

/// Removes the verbatim prefix (`\\?\C:\…` → `C:\…`, `\\?\UNC\s\x` → `\\s\x`).
pub fn strip_verbatim(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\")
        && rest.as_bytes().get(1) == Some(&b':')
    {
        return PathBuf::from(rest);
    }
    path
}

/// Whether two paths name the same location. Existing paths are resolved
/// first; on Windows the comparison ignores case like the filesystem does.
pub fn same_path(a: &Path, b: &Path) -> bool {
    let a = canonical(a).unwrap_or_else(|_| a.to_owned());
    let b = canonical(b).unwrap_or_else(|_| b.to_owned());
    let a = comparable_components(&a);
    a.len() == comparable_components(&b).len() && is_prefix(&a, &b)
}

/// Whether `inner` is `outer` or lies below it.
pub fn is_within(inner: &Path, outer: &Path) -> bool {
    let inner = canonical(inner).unwrap_or_else(|_| inner.to_owned());
    let outer = canonical(outer).unwrap_or_else(|_| outer.to_owned());
    is_prefix(&comparable_components(&outer), &inner)
}

fn is_prefix(prefix: &[String], path: &Path) -> bool {
    let path = comparable_components(path);
    path.len() >= prefix.len() && prefix.iter().zip(&path).all(|(a, b)| a == b)
}

fn comparable_components(path: &Path) -> Vec<String> {
    path.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .map(|c| {
            let text = c.as_os_str().to_string_lossy();
            if cfg!(windows) {
                text.to_lowercase()
            } else {
                text.into_owned()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbatim_prefix_is_removed() {
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\C:\Users\x")),
            PathBuf::from(r"C:\Users\x")
        );
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\UNC\server\share")),
            PathBuf::from(r"\\server\share")
        );
        assert_eq!(
            strip_verbatim(PathBuf::from("/home/x")),
            PathBuf::from("/home/x")
        );
    }

    #[test]
    fn containment_and_equality() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("a").join("b");
        fs::create_dir_all(&inner).unwrap();
        assert!(is_within(&inner, dir.path()));
        assert!(is_within(dir.path(), dir.path()));
        assert!(!is_within(dir.path(), &inner));
        // A sibling sharing a name prefix is not inside.
        let sibling = dir.path().join("ab");
        fs::create_dir_all(&sibling).unwrap();
        assert!(!is_within(&sibling, &dir.path().join("a")));
        assert!(same_path(&inner, &dir.path().join("a").join(".").join("b")));
        assert!(!same_path(&inner, dir.path()));
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_ignore_case() {
        let dir = tempfile::tempdir().unwrap();
        let upper = PathBuf::from(dir.path().to_string_lossy().to_uppercase());
        assert!(same_path(dir.path(), &upper));
    }
}
