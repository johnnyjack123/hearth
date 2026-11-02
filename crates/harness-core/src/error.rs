//! Error type shared by all of `harness-core`.
//!
//! Messages are user-facing (the CLI prints them as is), so they come in
//! German and English like the rest of the UI ([`crate::lang`]); `Display`
//! is written by hand because `#[error]` takes only fixed text.

use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::lang::tr;
use crate::tr;

/// Result alias for `harness-core`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Everything that can go wrong in `harness-core`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The Podman machine JSON does not exist, usually because the machine was
    /// never initialised or has a different name.
    MachineConfigMissing {
        /// Expected location of the machine JSON.
        path: PathBuf,
    },

    /// The Podman machine JSON exists but could not be parsed.
    MachineConfigInvalid {
        /// Location of the machine JSON.
        path: PathBuf,
        /// Parse error.
        #[source]
        source: serde_json::Error,
    },

    /// The setup is technically valid but not supported by the harness.
    Unsupported(String),

    /// Nothing listens on the machine's SSH port, i.e. the VM is not running.
    VmUnreachable {
        /// Address that was probed.
        addr: String,
        /// Connect error.
        #[source]
        source: io::Error,
    },

    /// An external program (podman, ssh-keyscan) could not be started.
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
    Command {
        /// Subcommand, e.g. `volume create`.
        action: String,
        /// Exit code, `None` if the process was killed.
        code: Option<i32>,
        /// Trimmed stderr of the command.
        stderr: String,
    },

    /// A podman command succeeded but printed something we cannot interpret.
    UnexpectedOutput {
        /// Subcommand, e.g. `info`.
        action: String,
        /// What was wrong.
        detail: String,
    },

    /// A name, path or value failed validation before it reached podman.
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
    NetworkNotInternal(String),

    /// The VM service `harness-vmd` is missing or refused a request.
    Vmd(String),

    /// A configuration file cannot be parsed or fails validation.
    ConfigInvalid {
        /// The offending file.
        path: PathBuf,
        /// Parser or validation message.
        message: String,
    },

    /// A git command on the host exited with a non-zero status.
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
    Sync(String),

    /// The project's gateway refused a request or cannot be reached.
    Gateway(String),

    /// The harness server or the local credential store failed.
    Server(String),

    /// No project file with this name exists.
    ProjectUnknown(String),

    /// No harness definition with this name exists (neither file nor built-in).
    HarnessUnknown(String),

    /// No app of this name is in the catalog or in `apps/`.
    AppUnknown(String),

    /// Filesystem error with context.
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

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Error::MachineConfigMissing { path } => tr!(
                "Podman-Machine-Konfiguration nicht gefunden: {}",
                "Podman machine configuration not found: {}",
                path.display()
            ),
            Error::MachineConfigInvalid { path, source } => tr!(
                "Podman-Machine-Konfiguration {} ist ungültig: {source}",
                "Podman machine configuration {} is invalid: {source}",
                path.display()
            ),
            Error::Unsupported(message) | Error::Sync(message) => return f.write_str(message),
            Error::VmUnreachable { addr, source } => tr!(
                "VM nicht erreichbar ({addr}): {source}. Läuft die Machine?",
                "VM not reachable ({addr}): {source}. Is the machine running?"
            ),
            Error::Spawn { program, source } => tr!(
                "{} konnte nicht gestartet werden: {source}",
                "{} could not be started: {source}",
                program.display()
            ),
            Error::Command {
                action,
                code,
                stderr,
            } => tr!(
                "`podman {action}` ist fehlgeschlagen ({}){}",
                "`podman {action}` failed ({}){}",
                exit_text(*code),
                stderr_text(stderr)
            ),
            Error::UnexpectedOutput { action, detail } => tr!(
                "Unerwartete Ausgabe von `podman {action}`: {detail}",
                "Unexpected output of `podman {action}`: {detail}"
            ),
            Error::Invalid {
                what,
                value,
                reason,
            } => tr!(
                "Ungültiger Wert {value:?} für {what}: {reason}",
                "Invalid value {value:?} for {what}: {reason}"
            ),
            Error::NetworkNotInternal(network) => tr!(
                "Netz {network} ist nicht intern. Instanzen dürfen nur in internen Netzen laufen",
                "Network {network} is not internal. Instances may only run in internal networks"
            ),
            Error::Vmd(message) => tr!("VM-Dienst: {message}", "VM service: {message}"),
            Error::ConfigInvalid { path, message } => tr!(
                "Konfiguration {} ist ungültig: {message}",
                "Configuration {} is invalid: {message}",
                path.display()
            ),
            Error::Git {
                action,
                code,
                stderr,
            } => tr!(
                "`git {action}` ist fehlgeschlagen ({}){}",
                "`git {action}` failed ({}){}",
                exit_text(*code),
                stderr_text(stderr)
            ),
            Error::Gateway(message) => return write!(f, "Gateway: {message}"),
            Error::Server(message) => return write!(f, "Server: {message}"),
            Error::ProjectUnknown(name) => tr!(
                "Projekt {name} ist nicht angelegt",
                "Project {name} does not exist"
            ),
            Error::HarnessUnknown(name) => tr!(
                "Harness {name} ist nicht definiert (harnesses/{name}.toml fehlt)",
                "Harness {name} is not defined (harnesses/{name}.toml is missing)"
            ),
            Error::AppUnknown(name) => tr!(
                "App {name} gibt es nicht (harness app list zeigt alle; ausschalten mit harness \
                 app off {name})",
                "There is no app {name} (harness app list shows all; switch it off with harness \
                 app off {name})"
            ),
            Error::Io { context, source } => return write!(f, "{context}: {source}"),
        };
        f.write_str(&text)
    }
}

fn exit_text(code: Option<i32>) -> String {
    match code {
        Some(code) => tr!("Exit-Code {code}", "exit code {code}"),
        None => tr("abgebrochen", "aborted").to_owned(),
    }
}

fn stderr_text(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!(": {stderr}")
    }
}
