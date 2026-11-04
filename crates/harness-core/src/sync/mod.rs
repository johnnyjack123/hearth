//! Git sync in isolated mode (docs/ROADMAP.md, Phase 2, package 5).
//!
//! The agent works on its own clone in a VM volume. Data moves only as git
//! bundles, copied into and out of the container ([`Provider::copy_into`],
//! [`Provider::copy_out`]); the VM sees no host folder:
//!
//! - host → container: the host bundles its branches and tags into
//!   [`IN_BUNDLE`]; the container clones it once (remote `host`) and fetches
//!   it on every later run;
//! - container → host: the container bundles `agent/<project>` into
//!   [`OUT_BUNDLE`], excluding what the host already has; the host imports
//!   that one branch and nothing else;
//! - [`HostRepo::merge`] brings the agent branch into the user's branch when
//!   that is conflict-free.

mod bundle;
mod host;

pub use bundle::BundleHeader;
pub use host::{
    HostRepo, ImportOutcome, Incoming, IncomingFile, MergeOutcome, file_list, find_git,
};

use crate::lang::tr;
use crate::project::ProjectName;
use crate::provider::{ExecSpec, Provider};
use crate::{Error, Result, tr};

/// The agent's clone inside the container.
pub const WORKSPACE: &str = "/workspace";

/// Bundle from the host, in the home volume: `/tmp` is a small tmpfs.
pub const IN_BUNDLE: &str = "/home/agent/.harness-sync-in.bundle";

/// Bundle for the host, next to [`IN_BUNDLE`].
pub const OUT_BUNDLE: &str = "/home/agent/.harness-sync-out.bundle";

/// Larger bundles from the container are refused; they come from the agent
/// and could otherwise fill the host's disk.
pub const MAX_OUT_BUNDLE_BYTES: u64 = 4 << 30;

/// Name of the remote in the agent's clone that stands for the host repository.
pub const HOST_REMOTE: &str = "host";

/// Clones or refreshes `/workspace` from the bundle at `$2`. `$1` is the
/// agent branch. On the first run the agent branch continues the host's agent
/// branch if there is one, else it starts at the host's HEAD. The bundle's
/// path is the remote's URL; clones from before the copies used
/// `/mnt/exchange`, hence `set-url` on every fetch.
///
/// Parameters are passed as positional arguments, never spliced into the
/// script text. PATH is pinned because `~/.local/bin` is writable by the agent.
const PREPARE_SCRIPT: &str = r#"set -eu
export PATH=/usr/bin:/bin
branch="$1"
bundle="$2"
cd /workspace
if [ -e .git ]; then
  git remote set-url host "$bundle"
  git fetch --quiet --prune host
  rm -f "$bundle"
  echo "result=fetched"
else
  if [ -n "$(ls -A)" ]; then
    echo "/workspace ist nicht leer, enthält aber kein Git-Repository" >&2
    exit 3
  fi
  git clone --quiet --origin host "$bundle" .
  if git show-ref --verify --quiet "refs/remotes/host/$branch"; then
    git switch --quiet --no-track -c "$branch" "host/$branch"
  else
    git switch --quiet -c "$branch"
  fi
  rm -f "$bundle"
  echo "result=cloned"
fi
echo "branch=$(git branch --show-current)"
"#;

/// Writes the agent branch as a bundle to `$3`. `$1` is the branch, `$2` is
/// `incremental` (only commits the host lacks) or `full`.
const EXPORT_SCRIPT: &str = r#"set -eu
export PATH=/usr/bin:/bin
branch="$1"
mode="$2"
out="$3"
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

/// Prints the checked-out branch and the uncommitted changes of the clone,
/// at most [`MAX_CHANGES`] + 1 of them.
const STATUS_SCRIPT: &str = r#"set -eu
export PATH=/usr/bin:/bin
cd /workspace
[ -e .git ] || exit 0
echo "branch=$(git branch --show-current)"
git status --porcelain=v1 --untracked-files=normal | head -n "$1" | sed 's/^/change=/'
"#;

/// Changes [`workspace_status`] lists; more are only counted as "more".
pub const MAX_CHANGES: usize = 20;

/// Commits what the agent left uncommitted on the agent branch (`$1`) with
/// message `$2`, unless more than `$3` files changed (build output that is
/// missing from `.gitignore`, say). Hooks stay off: they belong to the
/// agent and could block or rewrite the commit.
const COMMIT_SCRIPT: &str = r#"set -eu
export PATH=/usr/bin:/bin
branch="$1"
message="$2"
limit="$3"
cd /workspace
if [ ! -e .git ]; then echo "result=no-workspace"; exit 0; fi
current="$(git branch --show-current)"
echo "branch=$current"
if [ "$current" != "$branch" ]; then echo "result=other-branch"; exit 0; fi
git add -A
if git diff --cached --quiet; then echo "result=clean"; exit 0; fi
files="$(git diff --cached --name-only | wc -l)"
echo "files=$files"
if [ "$files" -gt "$limit" ]; then git reset --quiet; echo "result=too-many"; exit 0; fi
git -c core.hooksPath=/dev/null -c commit.gpgSign=false commit --quiet --no-verify -m "$message"
echo "result=committed"
"#;

/// Moves the agent branch (`$1`) to the host's merged state `$3`, but only
/// if it still is at `$2`, the commit the host checked and merged. `--keep`
/// refuses instead of overwriting changes made since.
const REFRESH_SCRIPT: &str = r#"set -eu
export PATH=/usr/bin:/bin
branch="$1"
expected="$2"
target="$3"
cd /workspace
if [ ! -e .git ]; then echo "result=no-workspace"; exit 0; fi
if [ "$(git branch --show-current)" != "$branch" ]; then echo "result=other-branch"; exit 0; fi
if [ "$(git rev-parse HEAD)" != "$expected" ]; then echo "result=moved"; exit 0; fi
git -c core.hooksPath=/dev/null reset --quiet --keep "$target"
echo "result=refreshed"
"#;

/// Result of [`commit_workspace`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitOutcome {
    /// The container has no clone yet.
    NoWorkspace,
    /// Another branch is checked out; nothing was committed.
    OtherBranch {
        /// The checked-out branch, sanitized.
        branch: String,
    },
    /// Nothing uncommitted.
    Clean,
    /// The changes were committed on the agent branch.
    Committed {
        /// Files in the commit.
        files: u32,
    },
    /// Too many changed files; nothing was committed.
    TooMany {
        /// Files that changed.
        files: u32,
    },
}

/// Commits uncommitted changes of the agent's clone on its branch, so
/// [`pull_from_workspace`] carries them home. At most `max_files` files.
pub fn commit_workspace(
    provider: &dyn Provider,
    container: &str,
    name: &ProjectName,
    message: &str,
    max_files: u32,
) -> Result<CommitOutcome> {
    let limit = max_files.to_string();
    let output = run_script(
        provider,
        container,
        COMMIT_SCRIPT,
        &[&name.agent_branch(), message, &limit],
    )?;
    let files = value(&output, "files")
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0);
    match value(&output, "result") {
        Some("no-workspace") => Ok(CommitOutcome::NoWorkspace),
        Some("other-branch") => Ok(CommitOutcome::OtherBranch {
            branch: value(&output, "branch").map(sanitize).unwrap_or_default(),
        }),
        Some("clean") => Ok(CommitOutcome::Clean),
        Some("committed") => Ok(CommitOutcome::Committed { files }),
        Some("too-many") => Ok(CommitOutcome::TooMany { files }),
        _ => Err(unexpected_output(&output)),
    }
}

/// Result of [`refresh_workspace`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// The agent branch in the container is at the merged state now.
    Refreshed,
    /// The agent committed after the check; the branch stayed.
    Moved,
    /// Another branch is checked out in the container.
    OtherBranch,
    /// The container has no clone.
    NoWorkspace,
}

/// After a merge on the host: brings the host's state into the container
/// and moves the agent branch there from `expected` (the merged commit) to
/// `target` (the host's new HEAD), so the agent continues from what the
/// user has.
pub fn refresh_workspace(
    provider: &dyn Provider,
    container: &str,
    repo: &HostRepo,
    name: &ProjectName,
    expected: &str,
    target: &str,
) -> Result<RefreshOutcome> {
    push_to_workspace(provider, container, repo, name)?;
    let output = run_script(
        provider,
        container,
        REFRESH_SCRIPT,
        &[&name.agent_branch(), expected, target],
    )?;
    match value(&output, "result") {
        Some("refreshed") => Ok(RefreshOutcome::Refreshed),
        Some("moved") => Ok(RefreshOutcome::Moved),
        Some("other-branch") => Ok(RefreshOutcome::OtherBranch),
        Some("no-workspace") => Ok(RefreshOutcome::NoWorkspace),
        _ => Err(unexpected_output(&output)),
    }
}

/// What [`pull_from_workspace`] leaves behind: it carries commits on the
/// agent branch only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceStatus {
    /// Branch checked out in the clone; empty without a clone or detached.
    pub branch: String,
    /// Uncommitted changes as `git status --porcelain` lines, sanitized.
    pub changes: Vec<String>,
    /// Whether there are more than [`MAX_CHANGES`] changes.
    pub more: bool,
}

/// Uncommitted changes and the checked-out branch of the agent's clone.
pub fn workspace_status(provider: &dyn Provider, container: &str) -> Result<WorkspaceStatus> {
    let limit = (MAX_CHANGES + 1).to_string();
    let output = run_script(provider, container, STATUS_SCRIPT, &[&limit])?;
    let mut changes: Vec<String> = output
        .lines()
        .filter_map(|line| line.strip_prefix("change="))
        .map(sanitize)
        .collect();
    let more = changes.len() > MAX_CHANGES;
    changes.truncate(MAX_CHANGES);
    Ok(WorkspaceStatus {
        branch: value(&output, "branch").map(sanitize).unwrap_or_default(),
        changes,
        more,
    })
}

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

/// Host → container: bundles the host repository, copies it into the
/// container and clones or fetches it there.
pub fn push_to_workspace(
    provider: &dyn Provider,
    container: &str,
    repo: &HostRepo,
    name: &ProjectName,
) -> Result<WorkspaceState> {
    let dir = temp_dir()?;
    let bundle = dir.path().join("in.bundle");
    repo.export_bundle(&bundle)?;
    provider.copy_into(container, &bundle, IN_BUNDLE)?;
    let output = run_script(
        provider,
        container,
        PREPARE_SCRIPT,
        &[&name.agent_branch(), IN_BUNDLE],
    )?;
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
    let dir = temp_dir()?;
    let branch = name.agent_branch();
    for mode in ["incremental", "full"] {
        let output = run_script(
            provider,
            container,
            EXPORT_SCRIPT,
            &[&branch, mode, OUT_BUNDLE],
        )?;
        match value(&output, "result") {
            Some("no-workspace") => return Ok(PullOutcome::NoWorkspace),
            Some("no-branch") => return Ok(PullOutcome::NoBranch),
            Some("nothing-new") => return Ok(PullOutcome::NothingNew),
            Some("written") => {}
            _ => return Err(unexpected_output(&output)),
        }
        let bundle = dir.path().join(format!("out-{mode}.bundle"));
        provider.copy_out(container, OUT_BUNDLE, &bundle, MAX_OUT_BUNDLE_BYTES)?;
        let imported = repo.import_bundle(&bundle, &branch, force);
        match imported? {
            ImportOutcome::NeedsFullBundle => continue,
            outcome => return Ok(PullOutcome::Imported(outcome)),
        }
    }
    Err(Error::Sync(
        tr(
            "Auch das vollständige Bundle des Containers passt nicht zum Host-Repository",
            "Even the container's full bundle does not fit the host repository",
        )
        .to_owned(),
    ))
}

fn temp_dir() -> Result<tempfile::TempDir> {
    tempfile::Builder::new()
        .prefix("harness-sync-")
        .tempdir()
        .map_err(|e| {
            Error::io(
                tr("Temporären Ordner anlegen", "creating a temporary folder"),
                e,
            )
        })
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
        return Err(Error::Sync(tr!(
            "Git im Container ist fehlgeschlagen (Exit-Code {}): {}",
            "Git in the container failed (exit code {}): {}",
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
    Error::Sync(tr!(
        "Unerwartete Ausgabe aus dem Container: {}",
        "Unexpected output from the container: {}",
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
