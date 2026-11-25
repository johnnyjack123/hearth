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
    /// A secret (`.env`, keys, tokens, browser profiles): never read, and
    /// written only in the project folder with approval.
    Secret,
    /// Every write in the project folder asks (`[host] writes = "ask"`).
    WritesAsk,
    /// hearth's own configuration, data or programs: never read or written.
    Hearth,
    /// A folder of the operating system (Windows, programs, `/usr`, `/etc`
    /// …): never read or written.
    System,
    /// A file that runs when the user logs in, opens a shell or uses git
    /// (shell profiles, autostart, git's settings): never written outside
    /// the project.
    Startup,
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

/// Names of likely secrets; reading them would send them to the server and
/// the model's provider. `id_rsa` and friends also match with an ending
/// (`id_rsa.old`), except the public half (`.pub`).
pub const SECRET_NAMES: &[&str] = &[
    ".npmrc",
    ".pypirc",
    ".netrc",
    "_netrc",
    ".pgpass",
    ".git-credentials",
    ".credentials.json",
    "credentials.json",
    "id_rsa",
    "id_ed25519",
    "id_ecdsa",
    "id_dsa",
];

/// Endings of keys, certificates with private parts, key stores and
/// password databases.
pub const SECRET_EXTENSIONS: &[&str] = &[
    "pem", "key", "p12", "pfx", "p8", "ppk", "jks", "keystore", "kdbx",
];

/// Folders whose files are all secrets, compared in lower case with `/`
/// as a sequence of names anywhere in a path: keys and tokens of tools,
/// Windows' credential stores, browser profiles (cookies, saved passwords)
/// and the SSH key of hearth's VM. Docker and Kubernetes only by their
/// files: projects keep a `.docker/` or `.kube/` folder of their own.
pub const SECRET_DIRS: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".aws",
    ".azure",
    ".kube/config",
    ".docker/config.json",
    ".password-store",
    ".config/gh",
    ".config/gcloud",
    ".local/share/containers/podman/machine",
    "appdata/roaming/microsoft/credentials",
    "appdata/local/microsoft/credentials",
    "appdata/roaming/microsoft/protect",
    "appdata/roaming/microsoft/usersecrets",
    "library/keychains",
    "google/chrome/user data",
    "microsoft/edge/user data",
    "vivaldi/user data",
    "bravesoftware/brave-browser/user data",
    "mozilla/firefox/profiles",
    ".mozilla/firefox",
    ".config/google-chrome",
    ".config/chromium",
    ".config/bravesoftware",
    ".config/vivaldi",
    "library/application support/google/chrome",
    "library/application support/firefox",
];

/// Files and folders below the home folder that run something when the
/// user logs in, opens a shell or uses git; lower case, with `/`.
pub const STARTUP_PATHS: &[&str] = &[
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".bash_logout",
    ".profile",
    ".zshrc",
    ".zprofile",
    ".zshenv",
    ".zlogin",
    ".config/fish",
    ".config/autostart",
    ".config/systemd/user",
    ".config/environment.d",
    ".gitconfig",
    ".config/git",
    "library/launchagents",
    "appdata/roaming/microsoft/windows/start menu/programs/startup",
    "documents/windowspowershell",
    "documents/powershell",
];

/// The operating system's own folders; nothing in them is the agent's
/// business, and writing there could change what other programs run.
pub fn system_folders() -> Vec<PathBuf> {
    if cfg!(windows) {
        [
            "SystemRoot",
            "windir",
            "ProgramFiles",
            "ProgramFiles(x86)",
            "ProgramW6432",
            "ProgramData",
        ]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect()
    } else {
        [
            "/bin",
            "/sbin",
            "/usr",
            "/lib",
            "/lib32",
            "/lib64",
            "/libx32",
            "/boot",
            "/etc",
            "/sys",
            "/proc",
            "/dev",
            "/System",
            "/Library",
            "/Applications",
            "/private/etc",
            "/private/var/db",
        ]
        .iter()
        .map(PathBuf::from)
        .collect()
    }
}

/// The rules for one project.
#[derive(Debug, Clone)]
pub struct Policy {
    root: PathBuf,
    hearth: Vec<PathBuf>,
    system: Vec<PathBuf>,
    home: Option<PathBuf>,
    writes_ask: bool,
}

impl Policy {
    /// Rules for the project folder `root`, which must exist. `hearth` are
    /// hearth's own folders (configuration, data, programs), which no tool
    /// reads or writes; `writes_ask` makes every write in the project ask.
    ///
    /// # Errors
    ///
    /// When `root` cannot be resolved.
    pub fn new(root: &Path, hearth: &[PathBuf], writes_ask: bool) -> Result<Self, ToolError> {
        let root =
            crate::paths::canonical(root).map_err(|error| ToolError::new(error.to_string()))?;
        // A folder that holds the project itself (the program next to it,
        // a configuration above it) cannot close the project: its rules win.
        let closed = |dirs: Vec<PathBuf>| -> Vec<PathBuf> {
            dirs.iter()
                .map(|path| resolve_existing(path))
                .filter(|dir| !is_below(&root, dir))
                .collect()
        };
        let hearth = closed(hearth.to_vec());
        let system = closed(system_folders());
        let home = crate::paths::home_dir()
            .ok()
            .map(|home| resolve_existing(&home));
        Ok(Policy {
            root,
            hearth,
            system,
            home,
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
        if self.hearth.iter().any(|dir| is_below(&path.path, dir)) {
            return Verdict::Deny(Reason::Hearth);
        }
        if self.system.iter().any(|dir| is_below(&path.path, dir)) {
            return Verdict::Deny(Reason::System);
        }
        // Inside the project only the part below it counts, so a project
        // that happens to live in such a folder keeps working.
        let secret = match &path.relative {
            Some(relative) => is_secret(relative),
            None => is_secret(&path.path.to_string_lossy()),
        };
        let Some(relative) = &path.relative else {
            return match access {
                _ if secret => Verdict::Deny(Reason::Secret),
                Access::Write if self.is_startup(&path.path) => Verdict::Deny(Reason::Startup),
                _ => Verdict::Ask(Reason::Outside),
            };
        };
        match access {
            Access::Read if secret => Verdict::Deny(Reason::Secret),
            Access::Read => Verdict::Free,
            // Creating `.env` from the example is ordinary work; overwriting
            // the user's own is not done without asking.
            Access::Write if secret => Verdict::Ask(Reason::Secret),
            Access::Write if is_protected(relative) => Verdict::Ask(Reason::Protected),
            Access::Write if self.writes_ask => Verdict::Ask(Reason::WritesAsk),
            Access::Write => Verdict::Free,
        }
    }

    /// Whether `path` (outside the project) is a startup file of the user.
    fn is_startup(&self, path: &Path) -> bool {
        let Some(relative) = self
            .home
            .as_deref()
            .and_then(|home| relative_to(path, home))
        else {
            return false;
        };
        let lower = relative.to_lowercase();
        STARTUP_PATHS
            .iter()
            .any(|entry| lower == *entry || lower.starts_with(&format!("{entry}/")))
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

/// Whether a file name (lower case) is a secret. `in_command` leaves out
/// what also reads as code in a command (`process.env`, `obj.key`): there
/// only names that are unmistakably files count.
fn is_secret_name(name: &str, in_command: bool) -> bool {
    if name == ".env" {
        return true;
    }
    if let Some(rest) = name.strip_prefix(".env.") {
        // Templates without values, also their copies (`.env.example.bak`).
        let first = rest.split('.').next().unwrap_or(rest);
        return !matches!(first, "example" | "sample" | "template" | "dist");
    }
    if SECRET_NAMES
        .iter()
        .any(|secret| name == *secret || name.starts_with(&format!("{secret}.")))
    {
        // id_rsa.pub is the public half.
        return !name.ends_with(".pub");
    }
    let Some((stem, ext)) = name.rsplit_once('.').filter(|(stem, _)| !stem.is_empty()) else {
        return false;
    };
    if in_command && stem.contains('.') {
        // `.data.key` in jq, `a.b.key` in code.
        return false;
    }
    if SECRET_EXTENSIONS.contains(&ext) {
        return true;
    }
    // `prod.env`, as docker compose reads them.
    !in_command
        && ext == "env"
        && !["example", "sample", "template"]
            .iter()
            .any(|word| stem.contains(word))
}

/// Whether `path` (with `/` or `\`) names a secret: by its file name or a
/// folder of [`SECRET_DIRS`] in it.
pub fn is_secret(path: &str) -> bool {
    secret_text(path, false)
}

fn secret_text(path: &str, in_command: bool) -> bool {
    let lower = path.replace('\\', "/").to_lowercase();
    let lower = lower.trim_end_matches('/');
    let name = lower.rsplit('/').next().unwrap_or(lower);
    if is_secret_name(name, in_command) {
        return true;
    }
    let with_slash = format!("/{lower}/");
    SECRET_DIRS
        .iter()
        .any(|dir| with_slash.contains(&format!("/{dir}/")))
        // The environment of a process, with whatever keys it holds.
        || (lower.starts_with("/proc/") && name == "environ")
}

/// Words of a command that name a secret (`.env`, keys, `~/.ssh` …). Such a
/// command is refused before anyone is asked, so it cannot slip through in
/// a long command. A command can still read secrets without naming them
/// (`grep -r` over the folder, a glob); Jev's rating and the user's eyes
/// are left for those.
pub fn secret_mentions(command: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let words = command.split(|c: char| c.is_whitespace() || ";|&<>()'\"`=,{}".contains(c));
    for word in words {
        let word = word.trim_matches(|c: char| matches!(c, ':' | '@'));
        if word.is_empty() || found.iter().any(|known| known == word) {
            continue;
        }
        if secret_text(word, true) {
            found.push(word.to_owned());
        }
    }
    found
}
