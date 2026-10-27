//! Work of an agent that deleting a project's volumes would lose: commits the
//! host never got and changes the agent did not commit. `harness vmd clean`
//! (data of removed projects) and `harness project remove` check for it
//! first and save it, with the user's consent, before anything goes.
//!
//! The check runs in a short-lived container of its own that mounts the
//! project's clone and home, so it works without the project's file, its
//! network or its instance.

use std::path::Path;

use crate::image::BASE_IMAGE;
use crate::project::ProjectName;
use crate::provider::{ContainerSpec, ExecSpec, Mount, MountSource, NetworkSpec, Provider};
use crate::sync::{MAX_OUT_BUNDLE_BYTES, WORKSPACE, sanitize};
use crate::{Error, PROJECT_LABEL, Result};

/// Internal network of the check containers; they need no network at all.
const NETWORK: &str = "harness-rescue";

/// Where the bundle is written: the home volume, `/tmp` is a small tmpfs.
const BUNDLE: &str = "/home/agent/.harness-rescue.bundle";

/// Counts commits on branches (and a detached HEAD) that no `host/*`
/// branch contains, and uncommitted changes. PATH is pinned because
/// `~/.local/bin` belongs to the agent.
const CHECK_SCRIPT: &str = r#"set -eu
export PATH=/usr/bin:/bin
cd /workspace
if [ ! -e .git ]; then echo "result=no-workspace"; exit 0; fi
echo "commits=$(git rev-list --count --branches HEAD --not --remotes=host)"
echo "changes=$(git status --porcelain=v1 --untracked-files=normal | wc -l)"
echo "result=ok"
"#;

/// Commits uncommitted changes (on a new branch if HEAD is detached) and
/// bundles every branch to `$1`, complete, so the bundle can be cloned
/// anywhere. Hooks stay off: they belong to the agent.
const SAVE_SCRIPT: &str = r#"set -eu
export PATH=/usr/bin:/bin
out="$1"
cd /workspace
if [ -n "$(git status --porcelain=v1 --untracked-files=normal)" ]; then
  if [ -z "$(git branch --show-current)" ]; then
    git switch --quiet -c harness-gesichert
  fi
  git add -A
  git -c core.hooksPath=/dev/null -c commit.gpgSign=false commit --quiet --no-verify \
    -m "Nicht committete Änderungen, gesichert von harness vor dem Löschen"
fi
rm -f "$out"
git bundle create --quiet "$out" --branches
echo "result=written"
"#;

/// What a project's clone holds that the host does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsaved {
    /// Commits no branch of the host contains.
    pub commits: u32,
    /// Files changed or added but not committed.
    pub changes: u32,
}

impl Unsaved {
    /// Whether anything would be lost.
    pub fn any(&self) -> bool {
        self.commits > 0 || self.changes > 0
    }
}

/// What the project's clone holds that the host lacks; `None` if it has no
/// clone. Needs the base image.
pub fn unsaved(provider: &dyn Provider, name: &ProjectName) -> Result<Option<Unsaved>> {
    with_container(provider, name, |container| {
        let output = run(provider, container, CHECK_SCRIPT, &[])?;
        match value(&output, "result") {
            Some("no-workspace") => Ok(None),
            Some("ok") => {
                let count = |key| {
                    value(&output, key)
                        .and_then(|n| n.trim().parse().ok())
                        .ok_or_else(|| unexpected(&output))
                };
                Ok(Some(Unsaved {
                    commits: count("commits")?,
                    changes: count("changes")?,
                }))
            }
            _ => Err(unexpected(&output)),
        }
    })
}

/// Commits what is uncommitted in the project's clone and writes all its
/// branches as a git bundle to `target` on the host.
pub fn save(provider: &dyn Provider, name: &ProjectName, target: &Path) -> Result<()> {
    with_container(provider, name, |container| {
        let output = run(provider, container, SAVE_SCRIPT, &[BUNDLE])?;
        if value(&output, "result") != Some("written") {
            return Err(unexpected(&output));
        }
        provider.copy_out(container, BUNDLE, target, MAX_OUT_BUNDLE_BYTES)
    })
}

/// Runs `work` with a short-lived container that mounts the project's clone
/// and home, and removes it afterwards. Hardened like an instance, in an
/// internal network with nothing else in it.
fn with_container<T>(
    provider: &dyn Provider,
    name: &ProjectName,
    work: impl FnOnce(&str) -> Result<T>,
) -> Result<T> {
    provider.ensure_network(&NetworkSpec {
        name: NETWORK.to_owned(),
        internal: true,
        ..NetworkSpec::default()
    })?;
    let container = format!("harness-rescue-{name}");
    provider.remove_container(&container)?;
    let volume = |source: String, target: &str| Mount {
        source: MountSource::Volume(source),
        target: target.to_owned(),
        read_only: false,
    };
    provider.create_container(&ContainerSpec {
        name: container.clone(),
        image: BASE_IMAGE.tag.to_owned(),
        network: NETWORK.to_owned(),
        mounts: vec![
            volume(name.workspace_volume(), WORKSPACE),
            volume(name.home_volume(), crate::config::AGENT_HOME),
        ],
        labels: [(PROJECT_LABEL.to_owned(), name.to_string())].into(),
        workdir: Some(WORKSPACE.to_owned()),
        ..ContainerSpec::default()
    })?;
    let result = provider
        .start_container(&container)
        .and_then(|()| work(&container));
    let removed = provider.remove_container(&container);
    let value = result?;
    removed.map(|()| value)
}

fn run(provider: &dyn Provider, container: &str, script: &str, args: &[&str]) -> Result<String> {
    let mut command = vec![
        "sh".to_owned(),
        "-c".to_owned(),
        script.to_owned(),
        "harness-rescue".to_owned(),
    ];
    command.extend(args.iter().map(|a| (*a).to_owned()));
    let output = provider.exec_output(&ExecSpec {
        container: container.to_owned(),
        command,
        workdir: Some(WORKSPACE.to_owned()),
        // The agent's ~/.gitconfig must not steer git here.
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

/// Value of the last `key=value` line; the container's output is untrusted.
fn value<'a>(output: &'a str, key: &str) -> Option<&'a str> {
    output
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
}

fn unexpected(output: &str) -> Error {
    Error::Sync(format!(
        "Unerwartete Ausgabe aus dem Container: {}",
        sanitize(output.trim())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_its_own_lines() {
        let output = "commits=3\nnoise=1\nchanges=0\nresult=ok\n";
        assert_eq!(value(output, "commits"), Some("3"));
        assert_eq!(value(output, "result"), Some("ok"));
        assert_eq!(value(output, "missing"), None);
        assert!(
            Unsaved {
                commits: 0,
                changes: 1
            }
            .any()
        );
        assert!(
            !Unsaved {
                commits: 0,
                changes: 0
            }
            .any()
        );
    }
}
