//! The agent attached to the user's terminal (`harness run`, `harness shell`).
//!
//! On Windows the agent runs on a pseudo console that harness relays, so
//! the gateway's questions can appear in the same terminal: approvals are
//! decided on the host, never inside the container (CLAUDE.md). Elsewhere,
//! or with `HARNESS_DIRECT_TERMINAL=1`, podman gets the terminal directly
//! and questions go to `harness net watch`.

#[cfg(windows)]
mod console;
#[cfg(any(windows, test))]
mod screen;
#[cfg(windows)]
mod session;

use harness_core::Provider;
use harness_core::config::{ConfigDir, Project};
use harness_core::podman::PodmanProvider;
use harness_core::provider::ExecSpec;

/// Turns the relay off, e.g. if a terminal misbehaves with it.
const DIRECT_ENV: &str = "HARNESS_DIRECT_TERMINAL";

/// Where answers to the gateway's questions go.
pub struct Approvals<'a> {
    /// For answer "immer", which writes the project file.
    pub config: &'a ConfigDir,
    /// The project whose gateway asks.
    pub project: &'a Project,
}

/// Whether [`attach`] will show the gateway's questions itself.
pub fn shows_questions(spec: &ExecSpec) -> bool {
    cfg!(windows) && spec.tty && std::env::var_os(DIRECT_ENV).is_none()
}

/// Runs `spec` attached to this terminal and returns the exit code.
pub fn attach(
    provider: &PodmanProvider,
    spec: &ExecSpec,
    approvals: Option<Approvals<'_>>,
) -> anyhow::Result<i32> {
    #[cfg(windows)]
    if shows_questions(spec) {
        match session::run(provider, spec, approvals) {
            Ok(result) => return result,
            Err(session::NotStarted(reason)) => println!(
                "{}",
                harness_core::tr!(
                    "Terminal direkt verbunden ({reason}). Rückfragen beantwortest du mit \
                     `harness net watch`.",
                    "Terminal connected directly ({reason}). Answer questions with \
                     `harness net watch`."
                )
            ),
        }
    }
    #[cfg(not(windows))]
    let _ = approvals;
    Ok(provider.exec(spec)?)
}
