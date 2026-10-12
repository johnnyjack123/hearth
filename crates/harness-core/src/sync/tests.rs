//! Host-side sync tests against temporary repositories. The container side is
//! played by `FakeContainer`, which runs the same git commands as
//! `PREPARE_SCRIPT`/`EXPORT_SCRIPT`; the real scripts run in
//! `tests/sync_vm.rs` against the VM.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::*;

const BRANCH: &str = "agent/demo";

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new(find_git().unwrap())
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "init.defaultBranch=main",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn write(dir: &Path, file: &str, text: &str) {
    fs::write(dir.join(file), text).unwrap();
}

fn commit(dir: &Path, file: &str, text: &str, message: &str) {
    write(dir, file, text);
    git(dir, &["add", file]);
    git(dir, &["commit", "--quiet", "-m", message]);
}

/// A host repository with one commit on `main`.
fn host_repo(dir: &Path) -> HostRepo {
    fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "--quiet"]);
    // Local identity and line endings, so the result does not depend on the
    // global git config of the machine running the tests.
    git(dir, &["config", "user.name", "Host User"]);
    git(dir, &["config", "user.email", "host@example.com"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit(dir, "f.txt", "one\ntwo\nthree\n", "initial");
    HostRepo::open(dir).unwrap()
}

/// Mirrors what the container scripts do, using git on the host.
struct FakeContainer {
    dir: PathBuf,
}

impl FakeContainer {
    fn clone_from(bundle: &Path, dir: &Path) -> Self {
        let parent = dir.parent().unwrap();
        fs::create_dir_all(parent).unwrap();
        git(
            parent,
            &[
                "clone",
                "--quiet",
                "--origin",
                "host",
                &bundle.to_string_lossy(),
                &dir.to_string_lossy(),
            ],
        );
        let remote = format!("refs/remotes/host/{BRANCH}");
        let has_remote = Command::new(find_git().unwrap())
            .arg("-C")
            .arg(dir)
            .args(["show-ref", "--verify", "--quiet", &remote])
            .status()
            .unwrap()
            .success();
        if has_remote {
            git(
                dir,
                &[
                    "switch",
                    "--quiet",
                    "--no-track",
                    "-c",
                    BRANCH,
                    &format!("host/{BRANCH}"),
                ],
            );
        } else {
            git(dir, &["switch", "--quiet", "-c", BRANCH]);
        }
        FakeContainer {
            dir: dir.to_owned(),
        }
    }

    fn fetch(&self) {
        git(&self.dir, &["fetch", "--quiet", "--prune", "host"]);
    }

    /// Returns whether a bundle was written.
    fn export(&self, out: &Path, full: bool) -> bool {
        let reference = format!("refs/heads/{BRANCH}");
        let out = out.to_string_lossy();
        if full {
            git(
                &self.dir,
                &["bundle", "create", "--quiet", &out, &reference],
            );
            return true;
        }
        let new = git(
            &self.dir,
            &["rev-list", "--count", &reference, "--not", "--remotes=host"],
        );
        if new == "0" {
            return false;
        }
        git(
            &self.dir,
            &[
                "bundle",
                "create",
                "--quiet",
                &out,
                &reference,
                "--not",
                "--remotes=host",
            ],
        );
        true
    }
}

struct Setup {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    host: HostRepo,
    container: FakeContainer,
}

impl Setup {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = crate::paths::canonical(tmp.path()).unwrap();
        let host = host_repo(&root.join("host"));
        let in_bundle = root.join(IN_BUNDLE);
        host.export_bundle(&in_bundle).unwrap();
        let container = FakeContainer::clone_from(&in_bundle, &root.join("container"));
        Setup {
            _tmp: tmp,
            root,
            host,
            container,
        }
    }

    fn out(&self) -> PathBuf {
        self.root.join(OUT_BUNDLE)
    }

    fn agent_commit(&self, file: &str, text: &str) {
        commit(&self.container.dir, file, text, &format!("agent: {file}"));
    }

    fn import(&self, force: bool) -> Result<ImportOutcome> {
        assert!(
            self.container.export(&self.out(), false),
            "nothing to export"
        );
        self.host.import_bundle(&self.out(), BRANCH, force)
    }

    fn host_dir(&self) -> &Path {
        self.host.path()
    }
}

#[test]
fn round_trip_fast_forward() {
    let s = Setup::new();
    assert!(!s.container.export(&s.out(), false), "nothing new yet");

    s.agent_commit("new.txt", "from agent\n");
    let outcome = s.import(false).unwrap();
    assert!(
        matches!(
            outcome,
            ImportOutcome::Updated {
                from: None,
                commits: 1,
                ..
            }
        ),
        "{outcome:?}"
    );
    // The incremental bundle only carries what the host lacks.
    assert_eq!(BundleHeader::read(&s.out()).unwrap().prerequisites.len(), 1);
    assert_eq!(
        s.host.import_bundle(&s.out(), BRANCH, false).unwrap(),
        ImportOutcome::Unchanged
    );

    assert_eq!(
        s.host.merge(BRANCH).unwrap(),
        MergeOutcome::FastForward { commits: 1 }
    );
    assert_eq!(
        fs::read_to_string(s.host_dir().join("new.txt")).unwrap(),
        "from agent\n"
    );
    assert_eq!(s.host.merge(BRANCH).unwrap(), MergeOutcome::UpToDate);

    // Next round: the container learns about the host's state first.
    s.host.export_bundle(&s.root.join(IN_BUNDLE)).unwrap();
    s.container.fetch();
    assert!(!s.container.export(&s.out(), false));
    s.agent_commit("second.txt", "2\n");
    assert!(matches!(
        s.import(false).unwrap(),
        ImportOutcome::Updated {
            from: Some(_),
            commits: 1,
            ..
        }
    ));
}

#[test]
fn diverged_branches_merge_without_conflict() {
    let s = Setup::new();
    commit(s.host_dir(), "host.txt", "user work\n", "user");
    s.agent_commit("agent.txt", "agent work\n");
    s.import(false).unwrap();

    assert_eq!(
        s.host.merge(BRANCH).unwrap(),
        MergeOutcome::Merged { commits: 1 }
    );
    let parents = git(s.host_dir(), &["rev-list", "--parents", "-n", "1", "HEAD"]);
    assert_eq!(parents.split(' ').count(), 3, "expected a merge commit");
    assert!(s.host_dir().join("agent.txt").exists());
    assert!(s.host_dir().join("host.txt").exists());
}

#[test]
fn conflicts_leave_the_repository_untouched() {
    let s = Setup::new();
    commit(s.host_dir(), "f.txt", "one\nUSER\nthree\n", "user edit");
    s.agent_commit("f.txt", "one\nAGENT\nthree\n");
    s.import(false).unwrap();
    let head = git(s.host_dir(), &["rev-parse", "HEAD"]);

    assert_eq!(
        s.host.merge(BRANCH).unwrap(),
        MergeOutcome::Conflict {
            files: vec!["f.txt".to_owned()]
        }
    );
    assert_eq!(git(s.host_dir(), &["rev-parse", "HEAD"]), head);
    assert_eq!(git(s.host_dir(), &["status", "--porcelain"]), "");
    assert_eq!(
        fs::read_to_string(s.host_dir().join("f.txt")).unwrap(),
        "one\nUSER\nthree\n"
    );
}

#[test]
fn merge_never_touches_uncommitted_work() {
    let s = Setup::new();
    s.agent_commit("agent.txt", "x\n");
    s.import(false).unwrap();

    write(s.host_dir(), "f.txt", "unsaved user work\n");
    assert!(matches!(s.host.merge(BRANCH), Err(Error::Sync(_))));
    assert_eq!(
        fs::read_to_string(s.host_dir().join("f.txt")).unwrap(),
        "unsaved user work\n"
    );
    git(s.host_dir(), &["checkout", "--", "f.txt"]);

    // Untracked files do not block.
    write(s.host_dir(), "scratch.txt", "mine\n");
    assert_eq!(
        s.host.merge(BRANCH).unwrap(),
        MergeOutcome::FastForward { commits: 1 }
    );
    assert!(s.host_dir().join("scratch.txt").exists());

    git(s.host_dir(), &["checkout", "--quiet", "--detach"]);
    assert!(matches!(s.host.merge(BRANCH), Err(Error::Sync(_))));
}

#[test]
fn rewritten_agent_branch_needs_force() {
    let s = Setup::new();
    s.agent_commit("a.txt", "1\n");
    s.import(false).unwrap();

    git(
        &s.container.dir,
        &["commit", "--quiet", "--amend", "-m", "rewritten"],
    );
    assert!(matches!(s.import(false), Err(Error::Sync(_))));
    assert!(matches!(
        s.import(true).unwrap(),
        ImportOutcome::Updated { from: Some(_), .. }
    ));
}

#[test]
fn checked_out_agent_branch_is_not_moved() {
    let s = Setup::new();
    s.agent_commit("a.txt", "1\n");
    s.import(false).unwrap();
    git(s.host_dir(), &["switch", "--quiet", BRANCH]);
    s.agent_commit("b.txt", "2\n");
    assert!(matches!(s.import(false), Err(Error::Sync(_))));
    assert!(matches!(s.host.merge(BRANCH), Err(Error::Sync(_))));
}

#[test]
fn missing_prerequisites_ask_for_a_full_bundle() {
    let s = Setup::new();
    s.agent_commit("a.txt", "1\n");
    assert!(s.container.export(&s.out(), false));

    // A repository that never had the container's base commit.
    let other = host_repo(&s.root.join("other"));
    assert_eq!(
        other.import_bundle(&s.out(), BRANCH, false).unwrap(),
        ImportOutcome::NeedsFullBundle
    );
    assert!(s.container.export(&s.out(), true));
    assert!(matches!(
        other.import_bundle(&s.out(), BRANCH, false).unwrap(),
        ImportOutcome::Updated { from: None, .. }
    ));
}

#[test]
fn bundle_must_contain_the_branch() {
    let s = Setup::new();
    let in_bundle = s.root.join(IN_BUNDLE);
    assert!(matches!(
        s.host.import_bundle(&in_bundle, BRANCH, false),
        Err(Error::Sync(_))
    ));
    fs::write(s.out(), "not a bundle\n").unwrap();
    assert!(matches!(
        s.host.import_bundle(&s.out(), BRANCH, false),
        Err(Error::Sync(_))
    ));
}

#[test]
fn export_needs_a_commit() {
    let tmp = tempfile::tempdir().unwrap();
    git(tmp.path(), &["init", "--quiet"]);
    let repo = HostRepo::open(tmp.path()).unwrap();
    assert!(matches!(
        repo.export_bundle(&tmp.path().join("x.bundle")),
        Err(Error::Sync(_))
    ));
    let plain = tempfile::tempdir().unwrap();
    // A folder outside any repository (the temp dir is not inside one).
    assert!(HostRepo::open(plain.path()).is_err());
}

/// Agent-controlled files must never run on the host: neither hooks in
/// `.git/hooks` nor a tracked hooks directory set via `core.hooksPath`.
#[test]
fn hooks_never_run_on_the_host() {
    let s = Setup::new();
    let marker_dir = s.root.join("markers");
    fs::create_dir_all(&marker_dir).unwrap();
    let marker = |name: &str| marker_dir.join(name);
    let hook = |name: &str| {
        format!(
            "#!/bin/sh\necho ran > '{}'\n",
            marker(name).to_string_lossy().replace('\\', "/")
        )
    };

    // Control: hooks do run in this environment for plain git.
    let hooks = s.host_dir().join(".git").join("hooks");
    fs::write(hooks.join("post-commit"), hook("control")).unwrap();
    commit(s.host_dir(), "c.txt", "c\n", "control");
    assert!(marker("control").exists(), "hooks do not run here at all");

    // Round 1: hooks in .git/hooks.
    fs::write(hooks.join("post-merge"), hook("git-dir-merge")).unwrap();
    fs::write(hooks.join("reference-transaction"), hook("git-dir-ref")).unwrap();
    s.agent_commit("a.txt", "1\n");
    s.import(false).unwrap();
    s.host.merge(BRANCH).unwrap();

    // Round 2: a hooks directory in the working tree (husky style), which
    // the agent could fill through its commits.
    let tracked = s.host_dir().join(".husky");
    fs::create_dir_all(&tracked).unwrap();
    fs::write(tracked.join("post-merge"), hook("tracked-merge")).unwrap();
    fs::write(tracked.join("reference-transaction"), hook("tracked-ref")).unwrap();
    git(s.host_dir(), &["config", "core.hooksPath", ".husky"]);
    s.agent_commit("b.txt", "2\n");
    s.import(false).unwrap();
    s.host.merge(BRANCH).unwrap();

    for name in [
        "git-dir-merge",
        "git-dir-ref",
        "tracked-merge",
        "tracked-ref",
    ] {
        assert!(!marker(name).exists(), "hook {name} ran");
    }
}

#[test]
fn script_output_parsing() {
    let output = "noise\nresult=cloned\nbranch=agent/x\n";
    assert_eq!(value(output, "result"), Some("cloned"));
    assert_eq!(value(output, "branch"), Some("agent/x"));
    assert_eq!(value(output, "missing"), None);
    assert_eq!(value("resultat=x\n", "result"), None);
    assert_eq!(value("result=a\nresult=b\n", "result"), Some("b"));
    assert_eq!(sanitize("ok\n\u{1b}[31mred\u{7}"), "ok\n�[31mred�");
}
