//! The direct mode's proxy: the model, through the server, asks for tool
//! calls; hearth checks each against the project's rules, asks the user
//! where they say so, and runs it on this computer
//! (`docs/security/concept.md`).
//!
//! The split: [`Session::plan`] decides (free, ask, refuse) without side
//! effects, the caller gets the user's answer through a surface it trusts
//! (never the model, and the browser only where the project allows it),
//! then [`Session::execute`] runs the call.

pub mod audit;
pub mod inbox;
pub mod policy;
pub mod shell;
pub mod tools;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub use policy::{Access, Policy, Reason, Resolved, Verdict};
pub use shell::Shell;
pub use tools::Replacement;

use crate::lang::tr;
use crate::tr;

/// Default time limit of a command.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
/// Longest time limit a command may ask for.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// A failed tool call; the text goes to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError(pub String);

impl ToolError {
    /// An error with this text for the model.
    pub fn new(text: impl Into<String>) -> Self {
        ToolError(text.into())
    }

    /// A file system error about `shown`.
    pub fn io(shown: &str, error: &std::io::Error) -> Self {
        let what = match error.kind() {
            std::io::ErrorKind::NotFound => "does not exist".to_owned(),
            std::io::ErrorKind::PermissionDenied => {
                "is not accessible (permission denied)".to_owned()
            }
            _ => format!("failed: {error}"),
        };
        ToolError(format!("{shown} {what}."))
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One tool call of the model, as the server sends it (`tool` plus
/// `input`, field names as in JavaScript).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "tool", content = "input", rename_all = "lowercase")]
pub enum ToolCall {
    /// Read a text file, from line `offset` (1-based), at most `limit` lines.
    Read {
        /// The file.
        path: String,
        /// First line, 1-based.
        #[serde(default)]
        offset: Option<usize>,
        /// Lines at most.
        #[serde(default)]
        limit: Option<usize>,
    },
    /// Create or replace a file.
    Write {
        /// The file.
        path: String,
        /// Its new content.
        content: String,
    },
    /// Replace exact passages of a file.
    Edit {
        /// The file.
        path: String,
        /// The replacements.
        edits: Vec<Replacement>,
    },
    /// List a folder.
    List {
        /// The folder; the project folder without.
        #[serde(default)]
        path: Option<String>,
    },
    /// Find files by glob.
    Find {
        /// Glob, e.g. `**/*.rs`.
        pattern: String,
        /// Folder to search; the project folder without.
        #[serde(default)]
        path: Option<String>,
    },
    /// Search file contents.
    Grep {
        /// Regular expression (or text with `literal`).
        pattern: String,
        /// Folder or file; the project folder without.
        #[serde(default)]
        path: Option<String>,
        /// Only files matching this glob.
        #[serde(default)]
        glob: Option<String>,
        /// Ignore case.
        #[serde(default, rename = "ignoreCase")]
        ignore_case: bool,
        /// Plain text instead of a regular expression.
        #[serde(default)]
        literal: bool,
    },
    /// Run a command; always asks.
    Shell {
        /// The command text for the shell.
        command: String,
        /// Time limit in seconds.
        #[serde(default)]
        timeout: Option<u64>,
    },
}

impl ToolCall {
    /// The tool's name.
    pub fn name(&self) -> &'static str {
        match self {
            ToolCall::Read { .. } => "read",
            ToolCall::Write { .. } => "write",
            ToolCall::Edit { .. } => "edit",
            ToolCall::List { .. } => "list",
            ToolCall::Find { .. } => "find",
            ToolCall::Grep { .. } => "grep",
            ToolCall::Shell { .. } => "shell",
        }
    }

    /// The path the call works on and how; `None` for commands.
    fn target(&self) -> Option<(Option<&str>, Access)> {
        match self {
            ToolCall::Read { path, .. } => Some((Some(path), Access::Read)),
            ToolCall::Write { path, .. } | ToolCall::Edit { path, .. } => {
                Some((Some(path), Access::Write))
            }
            ToolCall::List { path } | ToolCall::Find { path, .. } | ToolCall::Grep { path, .. } => {
                Some((path.as_deref(), Access::Read))
            }
            ToolCall::Shell { .. } => None,
        }
    }
}

/// What a tool call needs before it may run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Run it.
    Run,
    /// Ask the user first.
    Ask(Request),
    /// Refuse it with this text for the model.
    Refuse(String),
}

/// What the user is asked to allow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Request {
    /// Run a command.
    Command {
        /// The command text.
        command: String,
        /// The shell that runs it.
        shell: String,
        /// Where it runs.
        cwd: String,
    },
    /// Read a file or a folder (list, find, grep).
    Read {
        /// The resolved path.
        path: String,
        /// Why it asks: `outside` or `secret`.
        reason: String,
        /// Whether it is a folder.
        folder: bool,
    },
    /// Write a file.
    Write {
        /// The resolved path.
        path: String,
        /// Why it asks: `outside`, `protected` or `writes`.
        reason: String,
    },
}

impl Request {
    /// One line for the user, in the UI language.
    pub fn summary(&self) -> String {
        match self {
            Request::Command { command, .. } => {
                let first = command.lines().next().unwrap_or("");
                let more = if command.lines().count() > 1 {
                    " …"
                } else {
                    ""
                };
                tr!(
                    "Befehl ausführen: {first}{more}",
                    "Run command: {first}{more}"
                )
            }
            Request::Read {
                path, folder: true, ..
            } => tr!("Ordner lesen: {path}", "Read folder: {path}"),
            Request::Read { path, .. } => tr!("Datei lesen: {path}", "Read file: {path}"),
            Request::Write { path, .. } => tr!("Datei schreiben: {path}", "Write file: {path}"),
        }
    }

    /// Why it asks, for the user.
    pub fn why(&self) -> String {
        let reason = match self {
            Request::Command { .. } => {
                return tr(
                    "Befehle laufen mit allen deinen Rechten auf diesem Rechner. Build- und Testbefehle führen auch Code aus, den der Agent geschrieben haben kann.",
                    "Commands run with all your rights on this computer. Build and test commands also run code the agent may have written.",
                )
                .to_owned();
            }
            Request::Read { reason, .. } | Request::Write { reason, .. } => reason.as_str(),
        };
        match reason {
            "outside" => tr("liegt außerhalb des Projektordners", "is outside the project folder"),
            "secret" => tr(
                "würde ein Geheimnis schreiben oder überschreiben (.env, Schlüssel); lesen kann der Agent es nicht",
                "would write or replace a secret (.env, keys); the agent cannot read it",
            ),
            "protected" => tr(
                "kann später ohne dein Zutun Code ausführen oder Rechte vergeben (Git-Hooks, Editor- und Agent-Einstellungen)",
                "can later run code or grant rights without you doing anything (git hooks, editor and agent settings)",
            ),
            _ => tr(
                "in diesem Projekt fragt jedes Schreiben",
                "every write asks in this project",
            ),
        }
        .to_owned()
    }

    /// Whether "for this chat" is offered: commands (that exact text) and
    /// reads; every write outside the free area asks on its own.
    pub fn chat_wide(&self) -> bool {
        !matches!(self, Request::Write { .. })
    }
}

/// Danger from which a command counts as high risk: "for this chat" is not
/// offered then.
pub const HIGH_DANGER_PERCENT: u8 = 75;

/// Jev's rating of a command, as the server sent it with the call. Advice
/// only: it can take options away (see [`HIGH_DANGER_PERCENT`]), never
/// allow anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assessment {
    /// Potential for harm, 0 to 100.
    pub percent: u8,
    /// Jev's model.
    pub model: String,
}

impl Assessment {
    /// From the server's numbers: danger from 0 to 1 and the model; `None`
    /// for anything out of range.
    pub fn from_wire(danger: f64, model: &str) -> Option<Self> {
        if !danger.is_finite() || !(0.0..=1.0).contains(&danger) {
            return None;
        }
        let model: String = model
            .chars()
            .filter(|c| !c.is_control())
            .take(100)
            .collect();
        Some(Assessment {
            percent: (danger * 100.0).round() as u8,
            model,
        })
    }

    /// Whether the command counts as high risk.
    pub fn high(&self) -> bool {
        self.percent >= HIGH_DANGER_PERCENT
    }

    /// The level in words, in the UI language.
    pub fn level(&self) -> &'static str {
        match self.percent {
            75.. => tr("hoch", "high"),
            50.. => tr("mittel", "medium"),
            25.. => tr("gering", "low"),
            _ => tr("sehr gering", "very low"),
        }
    }

    /// One line for the user.
    pub fn text(&self) -> String {
        tr!(
            "Jev: Schadenspotenzial {} ({} %)",
            "Jev: potential for harm {} ({} %)",
            self.level(),
            self.percent
        )
    }
}

/// The user's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// Allow this call.
    Once,
    /// Allow this call and the same again in this chat (see [`Request::chat_wide`]).
    Chat,
    /// Refuse.
    Deny,
}

/// The outcome of a tool call, for the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Output {
    /// What the model reads.
    pub text: String,
    /// Whether the call failed.
    #[serde(rename = "isError")]
    pub is_error: bool,
    /// Exit code of a command, if it ended by itself.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "exitCode")]
    pub exit_code: Option<i32>,
}

impl Output {
    fn ok(text: String) -> Self {
        Output {
            text,
            is_error: false,
            exit_code: None,
        }
    }

    /// A failed call with this text for the model.
    pub fn error(text: impl Into<String>) -> Self {
        Output {
            text: text.into(),
            is_error: true,
            exit_code: None,
        }
    }
}

/// What a "for this chat" answer allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GrantKind {
    /// Running exactly this command text.
    Command,
    /// Reading below this folder.
    Folder,
    /// Reading this file.
    File,
}

/// One "for this chat" answer; the user can take it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    /// Id to take it back with (like an approval id).
    pub id: String,
    /// What it allows.
    pub kind: GrantKind,
    /// The command text or the resolved path.
    pub target: String,
}

/// One chat's work in one project: the rules, the shell and what the user
/// allowed for the chat.
#[derive(Debug, Clone)]
pub struct Session {
    policy: Policy,
    shell: Shell,
    grants: Vec<Grant>,
}

impl Session {
    /// A session with these rules and this shell.
    pub fn new(policy: Policy, shell: Shell) -> Self {
        Session {
            policy,
            shell,
            grants: Vec::new(),
        }
    }

    /// What the user allowed for the chat so far, oldest first.
    pub fn grants(&self) -> &[Grant] {
        &self.grants
    }

    /// Takes a "for this chat" answer back; whether there was one.
    pub fn revoke(&mut self, id: &str) -> bool {
        let before = self.grants.len();
        self.grants.retain(|grant| grant.id != id);
        self.grants.len() != before
    }

    /// The rules.
    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    /// The shell commands run with.
    pub fn shell(&self) -> &Shell {
        &self.shell
    }

    fn granted_read(&self, path: &Path) -> bool {
        self.grants.iter().any(|grant| match grant.kind {
            GrantKind::File => path == Path::new(&grant.target),
            GrantKind::Folder => path.starts_with(&grant.target),
            GrantKind::Command => false,
        })
    }

    fn granted_command(&self, command: &str) -> bool {
        self.grants
            .iter()
            .any(|grant| grant.kind == GrantKind::Command && grant.target == command)
    }

    /// What `call` needs before it may run. Changes nothing.
    pub fn plan(&self, call: &ToolCall) -> Plan {
        if let ToolCall::Shell { command, .. } = call {
            if command.trim().is_empty() {
                return Plan::Refuse("The command is empty.".to_owned());
            }
            let secrets = policy::secret_mentions(command);
            if !secrets.is_empty() {
                return Plan::Refuse(format!(
                    "The command names secret files ({}); hearth never lets the model read secrets, so it refuses such commands without asking. If the task needs them, ask the user to run the command themselves.",
                    secrets.join(", ")
                ));
            }
            if self.granted_command(command) {
                return Plan::Run;
            }
            return Plan::Ask(Request::Command {
                command: command.clone(),
                shell: self.shell.name.clone(),
                cwd: self.policy.root().display().to_string(),
            });
        }
        let Some((path, access)) = call.target() else {
            return Plan::Run;
        };
        let resolved = match path {
            Some(path) => match self.policy.resolve(path) {
                Ok(resolved) => resolved,
                Err(error) => return Plan::Refuse(error.0),
            },
            None => Resolved {
                path: self.policy.root().to_owned(),
                relative: Some(String::new()),
            },
        };
        match self.policy.check(&resolved, access) {
            Verdict::Free => Plan::Run,
            Verdict::Deny(reason) => Plan::Refuse(refusal(&resolved.display(), &reason)),
            Verdict::Ask(reason) => {
                let reason = match reason {
                    Reason::Outside => "outside",
                    Reason::Secret => "secret",
                    Reason::Protected => "protected",
                    Reason::WritesAsk | Reason::Hearth | Reason::System | Reason::Startup => {
                        "writes"
                    }
                };
                match access {
                    Access::Read if self.granted_read(&resolved.path) => Plan::Run,
                    Access::Read => Plan::Ask(Request::Read {
                        path: resolved.path.display().to_string(),
                        reason: reason.to_owned(),
                        folder: !matches!(call, ToolCall::Read { .. }) && !resolved.path.is_file(),
                    }),
                    Access::Write => Plan::Ask(Request::Write {
                        path: resolved.path.display().to_string(),
                        reason: reason.to_owned(),
                    }),
                }
            }
        }
    }

    /// Remembers a "for this chat" answer to `request`; whether that added
    /// a grant.
    pub fn grant(&mut self, request: &Request, decision: Decision) -> bool {
        if decision != Decision::Chat {
            return false;
        }
        let (kind, target) = match request {
            Request::Command { command, .. } => (GrantKind::Command, command.clone()),
            Request::Read {
                path, folder: true, ..
            } => (GrantKind::Folder, path.clone()),
            Request::Read { path, .. } => (GrantKind::File, path.clone()),
            Request::Write { .. } => return false,
        };
        if self
            .grants
            .iter()
            .any(|grant| grant.kind == kind && grant.target == target)
        {
            return false;
        }
        let id = loop {
            let id = inbox::new_id();
            if !self.grants.iter().any(|grant| grant.id == id) {
                break id;
            }
        };
        self.grants.push(Grant { id, kind, target });
        true
    }

    /// Runs `call`, which [`Session::plan`] allowed or the user approved.
    /// `cancel` ends a running command.
    pub fn execute(&self, call: &ToolCall, cancel: &AtomicBool) -> Output {
        let result = match call {
            ToolCall::Shell { command, timeout } => {
                return self.run_command(command, *timeout, cancel);
            }
            ToolCall::Read {
                path,
                offset,
                limit,
            } => self
                .resolved(path)
                .and_then(|r| tools::read(&r.path, &r.display(), *offset, *limit)),
            ToolCall::Write { path, content } => self
                .resolved(path)
                .and_then(|r| tools::write(&r.path, &r.display(), content)),
            ToolCall::Edit { path, edits } => self
                .resolved(path)
                .and_then(|r| tools::edit(&r.path, &r.display(), edits)),
            ToolCall::List { path } => self
                .folder(path.as_deref())
                .and_then(|r| tools::list(&r.path, &r.display())),
            ToolCall::Find { pattern, path } => self
                .folder(path.as_deref())
                .and_then(|r| tools::find(&self.policy, &r.path, pattern)),
            ToolCall::Grep {
                pattern,
                path,
                glob,
                ignore_case,
                literal,
            } => self.folder(path.as_deref()).and_then(|r| {
                let options = tools::GrepOptions {
                    glob: glob.clone(),
                    ignore_case: *ignore_case,
                    literal: *literal,
                };
                tools::grep(&self.policy, &r.path, pattern, &options)
            }),
        };
        match result {
            Ok(text) => Output::ok(text),
            Err(error) => Output::error(error.0),
        }
    }

    fn resolved(&self, path: &str) -> Result<Resolved, ToolError> {
        self.policy.resolve(path)
    }

    fn folder(&self, path: Option<&str>) -> Result<Resolved, ToolError> {
        match path {
            Some(path) => self.policy.resolve(path),
            None => Ok(Resolved {
                path: self.policy.root().to_owned(),
                relative: Some(String::new()),
            }),
        }
    }

    fn run_command(&self, command: &str, timeout: Option<u64>, cancel: &AtomicBool) -> Output {
        let timeout = timeout
            .map_or(DEFAULT_TIMEOUT, Duration::from_secs)
            .clamp(Duration::from_secs(1), MAX_TIMEOUT);
        match shell::run(&self.shell, command, self.policy.root(), timeout, cancel) {
            Err(error) => Output::error(format!(
                "The shell ({}) could not start: {error}",
                self.shell.name
            )),
            Ok(outcome) => {
                let mut text = outcome.output;
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                let ending = if outcome.cancelled {
                    "[Cancelled by the user.]".to_owned()
                } else if outcome.timed_out {
                    format!(
                        "[Ended after the time limit of {} s; pass a larger timeout (seconds) if it needs longer.]",
                        timeout.as_secs()
                    )
                } else {
                    match outcome.code {
                        Some(0) if text.is_empty() => "[Exit code 0, no output.]".to_owned(),
                        Some(0) => String::new(),
                        Some(code) => format!("[Exit code {code}]"),
                        None => "[Ended by a signal.]".to_owned(),
                    }
                };
                text.push_str(&ending);
                Output {
                    text,
                    is_error: outcome.cancelled || outcome.timed_out || outcome.code != Some(0),
                    exit_code: outcome.code,
                }
            }
        }
    }
}

/// What the agent of a project in the direct mode may do, for people, in
/// the UI language (the web app has the same lists in its own texts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rights {
    /// Without asking.
    pub free: Vec<String>,
    /// Only with the user's approval.
    pub ask: Vec<String>,
    /// Never, not even with approval.
    pub never: Vec<String>,
}

/// The rights of a project's agent; `writes_ask` as `[host] writes = "ask"`.
pub fn rights(writes_ask: bool) -> Rights {
    let mut free = vec![
        tr(
            "Dateien im Projektordner lesen, auflisten und durchsuchen",
            "Read, list and search files in the project folder",
        )
        .to_owned(),
    ];
    let mut ask = vec![
        tr(
            "Jeden Befehl (läuft mit deinen Rechten)",
            "Every command (runs with your rights)",
        )
        .to_owned(),
        tr(
            "Lesen und Schreiben außerhalb des Projektordners",
            "Reading and writing outside the project folder",
        )
        .to_owned(),
        tr(
            "Schreiben von Git-Hooks, Editor- und Agent-Einstellungen (.git, .vscode, .claude …) und von .env-Dateien im Ordner",
            "Writing git hooks, editor and agent settings (.git, .vscode, .claude …) and .env files in the folder",
        )
        .to_owned(),
    ];
    if writes_ask {
        ask.insert(
            1,
            tr(
                "Jedes Schreiben im Projektordner",
                "Every write in the project folder",
            )
            .to_owned(),
        );
    } else {
        free.push(
            tr(
                "Dateien im Projektordner anlegen und ändern",
                "Create and change files in the project folder",
            )
            .to_owned(),
        );
    }
    let never = vec![
        tr(
            "Geheimnisse lesen: .env, Schlüssel und Zertifikate, Zugangsdaten (~/.ssh, ~/.aws, .npmrc …), Browser-Profile; auch kein Befehl, der sie beim Namen nennt",
            "Reading secrets: .env, keys and certificates, credentials (~/.ssh, ~/.aws, .npmrc …), browser profiles; nor any command that names them",
        )
        .to_owned(),
        tr(
            "Systemordner (Windows, Programme, ProgramData; /usr, /etc …)",
            "System folders (Windows, Program Files, ProgramData; /usr, /etc …)",
        )
        .to_owned(),
        tr("hearths eigene Ordner", "hearth's own folders").to_owned(),
        tr(
            "Startdateien schreiben: Shell-Profile, Autostart, Git-Einstellungen",
            "Writing startup files: shell profiles, autostart, git settings",
        )
        .to_owned(),
    ];
    Rights { free, ask, never }
}

/// What the model reads when the rules refuse a path.
fn refusal(shown: &str, reason: &Reason) -> String {
    match reason {
        Reason::Hearth => {
            format!("{shown} belongs to hearth itself; no tool may read or write there.")
        }
        Reason::System => format!(
            "{shown} is in a folder of the operating system; no tool may read or write there."
        ),
        Reason::Secret => format!(
            "{shown} looks like a secret (keys, tokens, .env, credential stores, browser profiles); hearth never lets the model read it, nor write it outside the project. If you need a value from it, ask the user."
        ),
        Reason::Startup => format!(
            "{shown} runs when the user logs in, opens a shell or uses git; no tool may write it."
        ),
        Reason::Outside | Reason::Protected | Reason::WritesAsk => {
            format!("{shown} is not allowed.")
        }
    }
}

/// hearth's own folders, which no tool reads or writes: its configuration
/// and data, and the folder of the running program.
pub fn hearth_folders(config_root: &Path) -> Vec<PathBuf> {
    let mut folders = vec![config_root.to_owned()];
    if let Ok(data) = crate::paths::local_data_dir() {
        folders.push(data);
    }
    if cfg!(windows)
        && let Some(program_data) = std::env::var_os("ProgramData")
    {
        folders.push(PathBuf::from(program_data).join("hearth"));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_owned))
    {
        folders.push(dir);
    }
    folders
}
