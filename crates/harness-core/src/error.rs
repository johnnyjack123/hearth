//! Error type shared by all of `harness-core`.
//!
//! Messages are user-facing (the CLI prints them as is), so they are German
//! like the rest of the UI.

use std::io;
use std::path::PathBuf;

/// Result alias for `harness-core`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Everything that can go wrong in `harness-core`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The Podman machine JSON does not exist, usually because the machine was
    /// never initialised or has a different name.
    #[error("Podman-Machine-Konfiguration nicht gefunden: {}", path.display())]
    MachineConfigMissing {
        /// Expected location of the machine JSON.
        path: PathBuf,
    },

    /// The Podman machine JSON exists but could not be parsed.
    #[error("Podman-Machine-Konfiguration {} ist ungültig: {source}", path.display())]
    MachineConfigInvalid {
        /// Location of the machine JSON.
        path: PathBuf,
        /// Parse error.
        #[source]
        source: serde_json::Error,
    },

    /// The setup is technically valid but not supported by the harness.
    #[error("{0}")]
    Unsupported(String),

    /// Nothing listens on the machine's SSH port, i.e. the VM is not running.
    #[error("VM nicht erreichbar ({addr}): {source}. Läuft die Machine?")]
    VmUnreachable {
        /// Address that was probed.
        addr: String,
        /// Connect error.
        #[source]
        source: io::Error,
    },

    /// An external program (podman, ssh-keyscan) could not be started.
    #[error("{} konnte nicht gestartet werden: {source}", program.display())]
    Spawn {
        /// Program that failed to start.
        program: PathBuf,
        /// Spawn error.
        #[source]
        source: io::Error,
    },

    /// A podman command exited with a non-zero status.
    ///
    /// Only the subcommand is recorded, never the full argument list, because
    /// arguments may carry environment values.
    #[error("`podman {action}` ist fehlgeschlagen ({}){}", exit_text(*code), stderr_text(stderr))]
    Command {
        /// Subcommand, e.g. `volume create`.
        action: String,
        /// Exit code, `None` if the process was killed.
        code: Option<i32>,
        /// Trimmed stderr of the command.
        stderr: String,
    },

    /// A podman command succeeded but printed something we cannot interpret.
    #[error("Unerwartete Ausgabe von `podman {action}`: {detail}")]
    UnexpectedOutput {
        /// Subcommand, e.g. `info`.
        action: String,
        /// What was wrong.
        detail: String,
    },

    /// A name, path or value failed validation before it reached podman.
    #[error("Ungültiger Wert {value:?} für {what}: {reason}")]
    Invalid {
        /// What the value is used for, e.g. `Containername`.
        what: &'static str,
        /// The rejected value.
        value: String,
        /// Why it was rejected.
        reason: &'static str,
    },

    /// A container was about to be attached to a network with a route to the
    /// outside. Instances must only live in internal networks (gvproxy
    /// forwards host localhost into every normal network, phase 0 §12.6).
    #[error("Netz {0} ist nicht intern. Instanzen dürfen nur in internen Netzen laufen")]
    NetworkNotInternal(String),

    /// The VM service `harness-vmd` is missing or refused a request.
    #[error("VM-Dienst: {0}")]
    Vmd(String),

    /// A configuration file cannot be parsed or fails validation.
    #[error("Konfiguration {} ist ungültig: {message}", path.display())]
    ConfigInvalid {
        /// The offending file.
        path: PathBuf,
        /// Parser or validation message.
        message: String,
    },

    /// A git command on the host exited with a non-zero status.
    #[error("`git {action}` ist fehlgeschlagen ({}){}", exit_text(*code), stderr_text(stderr))]
    Git {
        /// Subcommand, e.g. `bundle create`.
        action: String,
        /// Exit code, `None` if the process was killed.
        code: Option<i32>,
        /// Trimmed stderr of the command.
        stderr: String,
    },

    /// Sync or merge was refused for a reason the user can fix (dirty working
    /// tree, rewritten agent branch, …). The message says what to do.
    #[error("{0}")]
    Sync(String),

    /// The project's gateway refused a request or cannot be reached.
    #[error("Gateway: {0}")]
    Gateway(String),

    /// The harness server or the local credential store failed.
    #[error("Server: {0}")]
    Server(String),

    /// No project file with this name exists.
    #[error("Projekt {0} ist nicht angelegt")]
    ProjectUnknown(String),

    /// No harness definition with this name exists (neither file nor built-in).
    #[error("Harness {0} ist nicht definiert (harnesses/{0}.toml fehlt)")]
    HarnessUnknown(String),

    /// No app of this name is in the catalog or in `apps/`.
    #[error(
        "App {0} gibt es nicht (harness app list zeigt alle; ausschalten mit harness app off {0})"
    )]
    AppUnknown(String),

    /// Filesystem error with context.
    #[error("{context}: {source}")]
    Io {
        /// What we were doing.
        context: String,
        /// Underlying error.
        #[source]
        source: io::Error,
    },
}

impl Error {
    /// Wraps an I/O error with a short description of what was being done.
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Error::Io {
            context: context.into(),
            source,
        }
    }

    /// Builds an [`Error::Invalid`].
    pub(crate) fn invalid(what: &'static str, value: &str, reason: &'static str) -> Self {
        Error::Invalid {
            what,
            value: value.to_owned(),
            reason,
        }
    }
}

fn exit_text(code: Option<i32>) -> String {
    match code {
        Some(code) => format!("Exit-Code {code}"),
        None => "abgebrochen".to_owned(),
    }
}

fn stderr_text(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    }
}
