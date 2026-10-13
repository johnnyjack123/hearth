//! Projects: a git repository on the host that agents work on, plus the names
//! of everything the harness creates for it.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// Validated project name: `[a-z0-9]` followed by `[a-z0-9-]`, at most
/// [`ProjectName::MAX_LEN`] characters.
///
/// Lowercase only because project files live on a case-insensitive
/// filesystem (`Foo.toml` and `foo.toml` are the same file on Windows), and
/// short enough that every derived resource name stays a valid hostname.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectName(String);

impl ProjectName {
    /// Longest allowed name; `harness-home-<name>` must fit in 63 characters.
    pub const MAX_LEN: usize = 40;

    /// Validates a name.
    pub fn new(name: &str) -> Result<Self> {
        let invalid = |reason| Error::invalid("Projektname", name, reason);
        if name.is_empty() || name.len() > Self::MAX_LEN {
            return Err(invalid("muss 1 bis 40 Zeichen lang sein"));
        }
        if !name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit()) {
            return Err(invalid(
                "muss mit einem Kleinbuchstaben oder einer Ziffer beginnen",
            ));
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(invalid(
                "darf nur Kleinbuchstaben, Ziffern und '-' enthalten",
            ));
        }
        Ok(ProjectName(name.to_owned()))
    }

    /// Derives a name from a folder name: lowercase, German umlauts
    /// transliterated, everything else that is not allowed becomes `-`.
    pub fn derive(folder: &Path) -> Self {
        let raw = folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut name = String::with_capacity(raw.len());
        for c in raw.chars().flat_map(char::to_lowercase) {
            match c {
                'a'..='z' | '0'..='9' => name.push(c),
                'ä' => name.push_str("ae"),
                'ö' => name.push_str("oe"),
                'ü' => name.push_str("ue"),
                'ß' => name.push_str("ss"),
                _ if !name.ends_with('-') => name.push('-'),
                _ => {}
            }
        }
        Self::shorten(&name, Self::MAX_LEN)
    }

    /// The name with `-<n>` appended, shortened so the result stays valid.
    /// Used when another project already has the derived name.
    pub fn with_suffix(&self, n: u32) -> Self {
        let suffix = format!("-{n}");
        let base = Self::shorten(&self.0, Self::MAX_LEN - suffix.len());
        ProjectName(format!("{}{suffix}", base.0))
    }

    fn shorten(name: &str, max: usize) -> Self {
        let trimmed = name.trim_matches('-');
        let cut = &trimmed[..trimmed.len().min(max)];
        let cut = cut.trim_end_matches('-');
        if cut.is_empty() {
            ProjectName("project".to_owned())
        } else {
            ProjectName(cut.to_owned())
        }
    }

    /// The name as a string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Container running the agent.
    pub fn instance(&self) -> String {
        format!("harness-{}", self.0)
    }

    /// Internal network of the instance.
    pub fn network(&self) -> String {
        format!("harness-net-{}", self.0)
    }

    /// Egress gateway of the project; one per project because each has its
    /// own rules.
    pub fn gateway(&self) -> String {
        format!("harness-gw-{}", self.0)
    }

    /// Volume holding the agent's clone of the repository (`/workspace`).
    pub fn workspace_volume(&self) -> String {
        format!("harness-ws-{}", self.0)
    }

    /// Volume holding the agent's home directory (caches, shell history).
    pub fn home_volume(&self) -> String {
        format!("harness-home-{}", self.0)
    }

    /// Subdirectory of the exchange folder for this project's bundles, so a
    /// container only ever sees its own project's data.
    pub fn exchange_dir(&self) -> &str {
        &self.0
    }

    /// Branch the agent commits to and that `harness sync` brings home.
    pub fn agent_branch(&self) -> String {
        format!("agent/{}", self.0)
    }
}

impl fmt::Display for ProjectName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `pad` instead of `write_str`, so `{:<24}` in listings works.
        f.pad(&self.0)
    }
}

/// The root of the git repository containing `start`, i.e. the nearest
/// ancestor with a `.git` entry (a directory, or a file for worktrees).
pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_names() {
        assert!(ProjectName::new("llm-harness").is_ok());
        assert!(ProjectName::new("a1").is_ok());
        for bad in ["", "-x", "Foo", "a_b", "a.b", "ä", &"a".repeat(41)] {
            assert!(ProjectName::new(bad).is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn derives_names_from_folders() {
        let derive = |s: &str| ProjectName::derive(Path::new(s)).0;
        assert_eq!(derive(r"C:\Projekte\llm_harness"), "llm-harness");
        assert_eq!(derive("/x/My Cool.App"), "my-cool-app");
        assert_eq!(derive("/x/Übungen Größe"), "uebungen-groesse");
        assert_eq!(derive("/x/__weird__"), "weird");
        assert_eq!(derive("/x/日本"), "project");
        assert_eq!(derive("/"), "project");
        let long = derive(&format!("/x/{}-b", "a".repeat(39)));
        assert_eq!(long, "a".repeat(39));
        for name in ["/x/a-b", "/x/My Cool.App", "/x/Übungen"] {
            assert!(ProjectName::new(&derive(name)).is_ok());
        }
    }

    #[test]
    fn suffixes_stay_within_limits() {
        let long = ProjectName::new(&"a".repeat(40)).unwrap();
        let suffixed = long.with_suffix(12);
        assert_eq!(suffixed.0.len(), 40);
        assert!(suffixed.0.ends_with("-12"));
        assert_eq!(ProjectName::new("foo").unwrap().with_suffix(2).0, "foo-2");
    }

    #[test]
    fn derived_resource_names_are_valid() {
        let name = ProjectName::new(&"a".repeat(40)).unwrap();
        for resource in [
            name.instance(),
            name.network(),
            name.gateway(),
            name.workspace_volume(),
            name.home_volume(),
        ] {
            crate::provider::validate_resource_name("x", &resource).unwrap();
        }
        assert_eq!(name.agent_branch(), format!("agent/{}", "a".repeat(40)));
    }

    #[test]
    fn finds_repo_root() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("src").join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_repo_root(&nested), None);
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        assert_eq!(find_repo_root(&nested).as_deref(), Some(dir.path()));
    }
}
