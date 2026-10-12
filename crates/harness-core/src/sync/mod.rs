//! Git sync in isolated mode (docs/ROADMAP.md, Phase 2, package 5).
//!
//! The agent works on its own clone in a VM volume. Data moves only as git
//! bundles through the project's subdirectory of the exchange folder, which
//! the container sees at [`CONTAINER_EXCHANGE`]:
//!
//! - host → container: the host bundles its branches and tags into
//!   `in.bundle`; the container clones it once (remote `host`) and fetches it
//!   on every later run;
//! - container → host: the container bundles `agent/<project>` into
//!   `out.bundle`, excluding what the host already has; the host imports that
//!   one branch and nothing else;
//! - [`HostRepo::merge`] brings the agent branch into the user's branch when
//!   that is conflict-free.
//!
//! 9p cannot hold a working tree (no chmod, broken recursive delete), but
//! single files and git's lockfile-and-rename work (verified 12.10.2026).

mod bundle;
mod host;

use std::fs;

pub use bundle::BundleHeader;
pub use host::{HostRepo, ImportOutcome, MergeOutcome, find_git};

use crate::project::ProjectName;
use crate::provider::{ExecSpec, Provider};
use crate::{Error, Result};

/// Mount point of the project's exchange subdirectory inside the container.
pub const CONTAINER_EXCHANGE: &str = "/mnt/exchange";

/// The agent's clone inside the container.
pub const WORKSPACE: &str = "/workspace";

/// Bundle from the host to the container.
pub const IN_BUNDLE: &str = "in.bundle";

/// Bundle from the container to the host.
pub const OUT_BUNDLE: &str = "out.bundle";

/// Name of the remote in the agent's clone that stands for the host repository.
pub const HOST_REMOTE: &str = "host";

/// Clones or refreshes `/workspace` from `in.bundle`. `$1` is the agent
/// branch. On the first run the agent branch continues the host's agent
/// branch if there is one, else it starts at the host's HEAD.
///
/// Parameters are passed as positional arguments, never spliced into the
/// script text. PATH is pinned because `~/.local/bin` is writable by the agent.
const PREPARE_SCRIPT: &str = r#"set -eu
export PATH=/usr/bin:/bin
branch="$1"
cd /workspace
if [ -e .git ]; then
  git fetch --quiet --prune host
  echo "result=fetched"
else
  if [ -n "$(ls -A)" ]; then
    echo "/workspace ist nicht leer, enthält aber kein Git-Repository" >&2
    exit 3
  fi
  git clone --quiet --origin host /mnt/exchange/in.bundle .
  if git show-ref --verify --quiet "refs/remotes/host/$branch"; then
    git switch --quiet --no-track -c "$branch" "host/$branch"
  else
    git switch --quiet -c "$branch"
  fi
  echo "result=cloned"
fi
echo "branch=$(git branch --show-current)"
"#;

/// Writes `out.bundle` with the agent branch. `$1` is the branch, `$2` is
/// `incremental` (only commits the host lacks) or `full`.
const EXPORT_SCRIPT: &str = r#"set -eu
export PATH=/usr/bin:/bin
branch="$1"
mode="$2"
out=/mnt/exchange/out.bundle
cd /workspace
if [ ! -e .git ]; then echo "result=no-workspace"; exit 0; fi
if ! git show-ref --verify --quiet "refs/heads/$branch"; then echo "result=no-branch"; exit 0; fi
rm -f "$out"
if [ "$mode" = full ]; then
  git bundle create --quiet "$out" "refs/heads/$branch"
else
  if [ "$(git rev-list --count "refs/heads/$branch" --not --remotes=host)" = 0 ]; then
    echo "result=nothing-new"
    exit 0
  fi
  git bundle create --quiet "$out" "refs/heads/$branch" --not --remotes=host
fi
echo "result=written"
"#;

/// State of the agent's clone after [`push_to_workspace`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceState {
    /// First run: the clone was created.
    Cloned {
        /// Branch checked out in the container.
        branch: String,
    },
    /// The clone existed and fetched the host's current branches.
    Fetched {
        /// Branch checked out in the container.
        branch: String,
    },
}

/// Result of [`pull_from_workspace`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullOutcome {
    /// The container has no clone yet.
    NoWorkspace,
    /// The clone has no agent branch.
    NoBranch,
    /// The agent branch has no commits the host lacks.
    NothingNew,
    /// The agent branch was imported (or already up to date).
    Imported(ImportOutcome),
}

/// Host → container: bundles the host repository into the exchange folder and
/// clones or fetches it in the container.
pub fn push_to_workspace(
    provider: &dyn Provider,
    container: &str,
    repo: &HostRepo,
    name: &ProjectName,
) -> Result<WorkspaceState> {
    let dir = provider.exchange()?.create_dir(name.exchange_dir())?;
    repo.export_bundle(&dir.join(IN_BUNDLE))?;
    let output = run_script(provider, container, PREPARE_SCRIPT, &[&name.agent_branch()])?;
    let branch = value(&output, "branch").unwrap_or_default().to_owned();
    match value(&output, "result") {
        Some("cloned") => Ok(WorkspaceState::Cloned { branch }),
        Some("fetched") => Ok(WorkspaceState::Fetched { branch }),
        _ => Err(unexpected_output(&output)),
    }
}

/// Container → host: bundles the agent branch in the container and imports
/// it as `agent/<project>` on the host. Falls back to a full bundle when the
/// host lacks the commits an incremental one builds on.
pub fn pull_from_workspace(
    provider: &dyn Provider,
    container: &str,
    repo: &HostRepo,
    name: &ProjectName,
    force: bool,
) -> Result<PullOutcome> {
    let dir = provider.exchange()?.create_dir(name.exchange_dir())?;
    let branch = name.agent_branch();
    for mode in ["incremental", "full"] {
        let output = run_script(provider, container, EXPORT_SCRIPT, &[&branch, mode])?;
        match value(&output, "result") {
            Some("no-workspace") => return Ok(PullOutcome::NoWorkspace),
            Some("no-branch") => return Ok(PullOutcome::NoBranch),
            Some("nothing-new") => return Ok(PullOutcome::NothingNew),
            Some("written") => {}
            _ => return Err(unexpected_output(&output)),
        }
        let bundle = dir.join(OUT_BUNDLE);
        let imported = repo.import_bundle(&bundle, &branch, force);
        // The bundle has served its purpose; a stale one must not be
        // imported by a later run whose export failed.
        let _ = fs::remove_file(&bundle);
        match imported? {
            ImportOutcome::NeedsFullBundle => continue,
            outcome => return Ok(PullOutcome::Imported(outcome)),
        }
    }
    Err(Error::Sync(
        "Auch das vollständige Bundle des Containers passt nicht zum Host-Repository".to_owned(),
    ))
}

fn run_script(
    provider: &dyn Provider,
    container: &str,
    script: &str,
    args: &[&str],
) -> Result<String> {
    let mut command = vec![
        "sh".to_owned(),
        "-c".to_owned(),
        script.to_owned(),
        "harness-sync".to_owned(),
    ];
    command.extend(args.iter().map(|a| (*a).to_owned()));
    let output = provider.exec_output(&ExecSpec {
        container: container.to_owned(),
        command,
        workdir: Some(WORKSPACE.to_owned()),
        // The agent's ~/.gitconfig must not steer the sync.
        env: [
            ("GIT_CONFIG_GLOBAL".to_owned(), "/dev/null".to_owned()),
            ("GIT_TERMINAL_PROMPT".to_owned(), "0".to_owned()),
        ]
        .into(),
        ..ExecSpec::default()
    })?;
    if output.code != 0 {
        return Err(Error::Sync(format!(
            "Git im Container ist fehlgeschlagen (Exit-Code {}): {}",
            output.code,
            sanitize(output.stderr.trim())
        )));
    }
    Ok(output.stdout)
}

/// Value of the last `key=value` line. Output of the container is untrusted,
/// so only these lines are interpreted.
fn value<'a>(output: &'a str, key: &str) -> Option<&'a str> {
    output
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
}

fn unexpected_output(output: &str) -> Error {
    Error::Sync(format!(
        "Unerwartete Ausgabe aus dem Container: {}",
        sanitize(output.trim())
    ))
}

/// Removes control characters (except newline and tab) from text that came
/// out of the container, so an agent cannot smuggle terminal escape sequences
/// into the user's terminal through error messages.
pub fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                '�'
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
