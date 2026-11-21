//! Where the direct mode's tools may go: every path is resolved (links
//! included) and sorted into free, ask or deny (`docs/security/concept.md`
//! §4).

use std::path::{Component, Path, PathBuf};

use super::ToolError;

/// What a tool wants to do with a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Read a file, or list or search a folder.
    Read,
    /// Create, replace or change a file.
    Write,
}

/// Why a path needs the user's approval or is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// Outside the project folder.
    Outside,
    /// In the project folder, but writing there can run code later or
    /// grant rights (git hooks, editor and agent settings).
    Protected,
    /// In the project folder, but likely a secret (`.env`, keys).
    Secret,
    /// Every write in the project folder asks (`[host] writes = "ask"`).
    WritesAsk,
    /// hearth's own configuration, data or programs: never written.
    Hearth,
}

/// What the policy says about one access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Go ahead without asking.
    Free,
    /// Only with the user's approval.
    Ask(Reason),
    /// Never, not even with approval.
    Deny(Reason),
}

/// A path after resolving it against the project folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Absolute, with links resolved as far as the path exists.
    pub path: PathBuf,
    /// Relative to the project folder with `/`, if inside it.
    pub relative: Option<String>,
}

impl Resolved {
    /// How the path is shown: relative inside the project, else absolute.
    pub fn display(&self) -> String {
        match &self.relative {
            Some(relative) if relative.is_empty() => ".".to_owned(),
            Some(relative) => relative.clone(),
            None => self.path.display().to_string(),
        }
    }
}

/// Folders and files in a project that run code or grant rights once
/// written, compared in lower case with `/`. The same ideas as the merge
/// review (`review.rs`): what is risky to merge is risky to write directly.
const PROTECTED_DIRS: &[&str] = &[
    ".git/",
    ".husky/",
    ".githooks/",
    ".git-hooks/",
    ".hooks/",
    ".vscode/",
    ".idea/",
    ".claude/",
    ".cursor/",
    ".pi/",
    ".hearth/",
];

/// Single files with the same risk, by name anywhere in the project.
const PROTECTED_NAMES: &[&str] = &[
    ".gitattributes",
    ".gitmodules",
    ".lfsconfig",
    ".mcp.json",
    ".envrc",
    "desktop.ini",
    "autorun.inf",
];

/// Endings Windows acts on when a folder is merely opened in Explorer.
const PROTECTED_EXTENSIONS: &[&str] = &["lnk", "url", "scf", "library-ms"];

/// Names of likely secrets; reading them sends them to the server and the
/// model's provider.
const SECRET_NAMES: &[&str] = &[
    ".npmrc",
    ".pypirc",
    ".netrc",
    "id_rsa",
    "id_ed25519",
    "id_ecdsa",
    "id_dsa",
];

/// Endings of keys and certificates with private parts.
const SECRET_EXTENSIONS: &[&str] = &["pem", "key", "p12", "pfx"];

/// The rules for one project.
#[derive(Debug, Clone)]
pub struct Policy {
    root: PathBuf,
    no_writes: Vec<PathBuf>,
    writes_ask: bool,
}

impl Policy {
    /// Rules for the project folder `root`, which must exist. `no_writes`
    /// are folders nothing may write into (hearth's configuration, data and
    /// programs); `writes_ask` makes every write in the project ask.
    ///
    /// # Errors
    ///
    /// When `root` cannot be resolved.
    pub fn new(root: &Path, no_writes: &[PathBuf], writes_ask: bool) -> Result<Self, ToolError> {
        let root =
            crate::paths::canonical(root).map_err(|error| ToolError::new(error.to_string()))?;
        // A folder that holds the project itself (the program next to it,
        // a configuration above it) cannot close the project: its rules win.
        let no_writes = no_writes
            .iter()
            .map(|path| resolve_existing(path))
            .filter(|dir| !is_below(&root, dir))
            .collect();
        Ok(Policy {
            root,
            no_writes,
            writes_ask,
        })
    }

    /// The project folder, resolved.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolves a path the model gave: relative to the project folder or
    /// absolute, `..` lexically, then links through the part that exists.
    ///
    /// # Errors
    ///
    /// Device paths, network paths, alternate data streams and empty paths.
    pub fn resolve(&self, input: &str) -> Result<Resolved, ToolError> {
        let input = input.trim();
        if input.is_empty() || input.contains('\0') {
            return Err(ToolError::new("The path is empty."));
        }
        refuse_special(input)?;
        let expanded = expand_home(input);
        let joined = if Path::new(&expanded).is_absolute() {
            PathBuf::from(&expanded)
        } else {
            self.root.join(&expanded)
        };
        let path = resolve_existing(&lexical(&joined));
        let relative = relative_to(&path, &self.root);
        Ok(Resolved { path, relative })
    }

    /// What the rules say about `access` to `path`.
    pub fn check(&self, path: &Resolved, access: Access) -> Verdict {
        if access == Access::Write && self.no_writes.iter().any(|dir| is_below(&path.path, dir)) {
            return Verdict::Deny(Reason::Hearth);
        }
        let Some(relative) = &path.relative else {
            return Verdict::Ask(Reason::Outside);
        };
        match access {
            Access::Read if is_secret(relative) => Verdict::Ask(Reason::Secret),
            Access::Read => Verdict::Free,
            Access::Write if is_protected(relative) => Verdict::Ask(Reason::Protected),
            Access::Write if self.writes_ask => Verdict::Ask(Reason::WritesAsk),
            Access::Write => Verdict::Free,
        }
    }
}

/// Refuses what Windows treats specially, on every system: a model's
/// paths should not depend on where hearth runs.
fn refuse_special(input: &str) -> Result<(), ToolError> {
    let normalized = input.replace('\\', "/");
    if normalized.starts_with("//") {
        return Err(ToolError::new(
            "Network and device paths (\\\\server\\share, \\\\?\\, \\\\.\\) are not allowed.",
        ));
    }
    // A colon only after a drive letter; anything else is an alternate
    // data stream on Windows (`file.txt:hidden`).
    let colons = normalized.match_indices(':').map(|(at, _)| at);
    for at in colons {
        let drive = at == 1 && normalized.as_bytes()[0].is_ascii_alphabetic();
        if !drive {
            return Err(ToolError::new(
                "Paths with ':' (other than after a drive letter) are not allowed.",
            ));
        }
    }
    for part in normalized.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
        if reserved && cfg!(windows) {
            return Err(ToolError::new(format!(
                "'{part}' is a device name on Windows, not a file."
            )));
        }
    }
    Ok(())
}

/// `~` and `~/…` as the user's home folder; models write them often.
fn expand_home(input: &str) -> String {
    let rest = if input == "~" {
        Some("")
    } else {
        input
            .strip_prefix("~/")
            .or_else(|| input.strip_prefix("~\\"))
    };
    match (rest, crate::paths::home_dir()) {
        (Some(rest), Ok(home)) => home.join(rest).display().to_string(),
        _ => input.to_owned(),
    }
}

/// Removes `.` and applies `..` without touching the disk.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // Never above the root or prefix.
                if !matches!(
                    out.components().next_back(),
                    Some(Component::RootDir | Component::Prefix(_)) | None
                ) {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Resolves links through the longest existing part of `path`; the rest is
/// appended as written (it has no `..` left).
fn resolve_existing(path: &Path) -> PathBuf {
    let mut existing = path.to_owned();
    let mut rest = Vec::new();
    loop {
        if let Ok(resolved) = crate::paths::canonical(&existing) {
            let mut out = resolved;
            for part in rest.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (
            existing.file_name().map(ToOwned::to_owned),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                rest.push(name);
                existing = parent.to_owned();
            }
            _ => return path.to_owned(),
        }
    }
}

/// Components compared as the file systems do: without case on Windows and
/// macOS (both are case-insensitive by default).
fn comparable(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| {
            let text = component.as_os_str().to_string_lossy();
            if cfg!(any(windows, target_os = "macos")) {
                text.to_lowercase()
            } else {
                text.into_owned()
            }
        })
        .collect()
}

fn is_below(path: &Path, dir: &Path) -> bool {
    let (path, dir) = (comparable(path), comparable(dir));
    path.len() >= dir.len() && path.iter().zip(&dir).all(|(a, b)| a == b)
}

/// `path` relative to `root` with `/`, if inside; the parts keep their
/// case as written.
fn relative_to(path: &Path, root: &Path) -> Option<String> {
    if !is_below(path, root) {
        return None;
    }
    let parts: Vec<String> = path
        .components()
        .skip(root.components().count())
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    Some(parts.join("/"))
}

fn extension(name: &str) -> Option<String> {
    name.rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map(|(_, ext)| ext.to_ascii_lowercase())
}

/// Whether writing `relative` (inside the project) can run code later.
pub fn is_protected(relative: &str) -> bool {
    let lower = relative.to_lowercase();
    let with_slash = format!("{lower}/");
    if PROTECTED_DIRS
        .iter()
        .any(|dir| with_slash.starts_with(dir) || with_slash.contains(&format!("/{dir}")))
    {
        return true;
    }
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    PROTECTED_NAMES.contains(&name)
        || extension(name).is_some_and(|ext| PROTECTED_EXTENSIONS.contains(&ext.as_str()))
}

/// Whether `relative` (inside the project) looks like a secret.
pub fn is_secret(relative: &str) -> bool {
    let lower = relative.to_lowercase();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    if name == ".env"
        || (name.starts_with(".env.")
            && !matches!(name, ".env.example" | ".env.sample" | ".env.template"))
    {
        return true;
    }
    if SECRET_NAMES
        .iter()
        .any(|secret| name == *secret || name.starts_with(&format!("{secret}.")))
    {
        // id_rsa.pub is the public half.
        return !name.ends_with(".pub");
    }
    extension(name).is_some_and(|ext| SECRET_EXTENSIONS.contains(&ext.as_str()))
}
