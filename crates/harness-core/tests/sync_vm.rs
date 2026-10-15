//! Git sync round trip with the real container scripts on the VM.
//!
//! Ignored by default; needs a running machine and a current base image:
//! `cargo test -p harness-core --test sync_vm -- --ignored --nocapture`

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use harness_core::image::{BASE_IMAGE, ImageStatus};
use harness_core::podman::{DEFAULT_MACHINE, PodmanProvider};
use harness_core::project::ProjectName;
use harness_core::provider::{
    ContainerSpec, ExecSpec, Mount, MountSource, NetworkSpec, Provider, VolumeSpec,
};
use harness_core::sync::{
    CONTAINER_EXCHANGE, HostRepo, ImportOutcome, MAX_CHANGES, MergeOutcome, PullOutcome, WORKSPACE,
    WorkspaceState, WorkspaceStatus, find_git, pull_from_workspace, push_to_workspace,
    workspace_status,
};

struct Cleanup<'a> {
    provider: &'a PodmanProvider,
    name: &'a ProjectName,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let p = self.provider;
        let _ = p.remove_container(&self.name.instance());
        let _ = p.remove_volume(&self.name.workspace_volume());
        let _ = p.remove_volume(&self.name.home_volume());
        let _ = p.remove_network(&self.name.network());
        if let Ok(exchange) = p.exchange()
            && let Ok(dir) = exchange.host_path(self.name.exchange_dir())
        {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new(find_git().unwrap())
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn timed<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let value = f();
    println!("{label}: {} ms", start.elapsed().as_millis());
    value
}

#[test]
#[ignore = "needs the running Podman machine and the built base image"]
fn sync_round_trip_through_the_vm() {
    let machine = std::env::var("HARNESS_MACHINE").unwrap_or_else(|_| DEFAULT_MACHINE.into());
    let provider = PodmanProvider::for_machine(&machine).unwrap();
    assert_eq!(
        BASE_IMAGE.status(&provider).unwrap(),
        ImageStatus::Current,
        "run `harness image build` first"
    );

    let name = ProjectName::new(&format!("synctest-{}", std::process::id())).unwrap();
    let _cleanup = Cleanup {
        provider: &provider,
        name: &name,
    };
    let branch = name.agent_branch();

    // Host repository with one commit.
    let tmp = tempfile::tempdir().unwrap();
    let host_dir = tmp.path().join("repo");
    fs::create_dir_all(&host_dir).unwrap();
    git(&host_dir, &["init", "--quiet", "--initial-branch=main"]);
    git(&host_dir, &["config", "user.name", "Host User"]);
    git(&host_dir, &["config", "user.email", "host@example.com"]);
    git(&host_dir, &["config", "core.autocrlf", "false"]);
    fs::write(host_dir.join("readme.txt"), "hello\n").unwrap();
    git(&host_dir, &["add", "readme.txt"]);
    git(&host_dir, &["commit", "--quiet", "-m", "initial"]);
    let host = HostRepo::open(&host_dir).unwrap();

    // Instance as `harness run` will set it up (package 8).
    provider
        .ensure_network(&NetworkSpec {
            name: name.network(),
            internal: true,
            ..NetworkSpec::default()
        })
        .unwrap();
    for volume in [name.workspace_volume(), name.home_volume()] {
        provider
            .ensure_volume(&VolumeSpec {
                name: volume,
                ..VolumeSpec::default()
            })
            .unwrap();
    }
    provider
        .exchange()
        .unwrap()
        .create_dir(name.exchange_dir())
        .unwrap();
    let mount = |source, target: &str| Mount {
        source,
        target: target.to_owned(),
        read_only: false,
    };
    provider
        .create_container(&ContainerSpec {
            name: name.instance(),
            image: BASE_IMAGE.tag.to_owned(),
            network: name.network(),
            mounts: vec![
                mount(MountSource::Volume(name.workspace_volume()), WORKSPACE),
                mount(MountSource::Volume(name.home_volume()), "/home/agent"),
                mount(
                    MountSource::Exchange(name.exchange_dir().to_owned()),
                    CONTAINER_EXCHANGE,
                ),
            ],
            ..ContainerSpec::default()
        })
        .unwrap();
    provider.start_container(&name.instance()).unwrap();

    let agent = |script: &str| {
        let output = provider
            .exec_output(&ExecSpec {
                container: name.instance(),
                command: vec!["sh".into(), "-c".into(), script.into()],
                workdir: Some(WORKSPACE.into()),
                ..ExecSpec::default()
            })
            .unwrap();
        assert_eq!(output.code, 0, "{script}: {}", output.stderr);
        output.stdout.trim().to_owned()
    };

    // Round 1: clone, agent commits, back to the host, fast-forward.
    let state = timed("push (clone)", || {
        push_to_workspace(&provider, &name.instance(), &host, &name).unwrap()
    });
    assert_eq!(
        state,
        WorkspaceState::Cloned {
            branch: branch.clone()
        }
    );
    assert_eq!(agent("cat readme.txt"), "hello");
    assert_eq!(
        pull_from_workspace(&provider, &name.instance(), &host, &name, false).unwrap(),
        PullOutcome::NothingNew
    );

    agent("echo 'from agent' > agent.txt && git add agent.txt && git commit -qm 'agent work'");
    let pulled = timed("pull", || {
        pull_from_workspace(&provider, &name.instance(), &host, &name, false).unwrap()
    });
    assert!(
        matches!(
            pulled,
            PullOutcome::Imported(ImportOutcome::Updated {
                from: None,
                commits: 1,
                ..
            })
        ),
        "{pulled:?}"
    );
    assert_eq!(
        git(&host_dir, &["log", "-1", "--format=%an", &branch]),
        "harness agent"
    );
    assert_eq!(
        host.merge(&branch).unwrap(),
        MergeOutcome::FastForward { commits: 1 }
    );
    assert_eq!(
        fs::read_to_string(host_dir.join("agent.txt")).unwrap(),
        "from agent\n"
    );
    // The host removed the out bundle; the container only sees its own folder.
    assert_eq!(agent("ls /mnt/exchange"), "in.bundle");

    // Round 2: both sides move on, the merge needs a merge commit.
    fs::write(host_dir.join("host.txt"), "user\n").unwrap();
    git(&host_dir, &["add", "host.txt"]);
    git(&host_dir, &["commit", "--quiet", "-m", "user work"]);
    let state = timed("push (fetch)", || {
        push_to_workspace(&provider, &name.instance(), &host, &name).unwrap()
    });
    assert_eq!(
        state,
        WorkspaceState::Fetched {
            branch: branch.clone()
        }
    );
    assert_eq!(
        agent("git rev-parse host/main"),
        git(&host_dir, &["rev-parse", "HEAD"])
    );

    agent("echo more >> agent.txt && git commit -qam 'agent again'");
    let pulled = pull_from_workspace(&provider, &name.instance(), &host, &name, false).unwrap();
    assert!(
        matches!(
            pulled,
            PullOutcome::Imported(ImportOutcome::Updated { from: Some(_), .. })
        ),
        "{pulled:?}"
    );
    assert_eq!(
        host.merge(&branch).unwrap(),
        MergeOutcome::Merged { commits: 1 }
    );
    assert_eq!(
        fs::read_to_string(host_dir.join("agent.txt")).unwrap(),
        "from agent\nmore\n"
    );
    assert!(host_dir.join("host.txt").exists());

    // What sync does not carry, so `harness sync` can point it out.
    let status = workspace_status(&provider, &name.instance()).unwrap();
    assert_eq!(
        status,
        WorkspaceStatus {
            branch: branch.clone(),
            changes: vec![],
            more: false
        }
    );
    agent("echo draft > draft.txt && echo edit >> readme.txt && git switch -q -c side");
    let status = workspace_status(&provider, &name.instance()).unwrap();
    assert_eq!(status.branch, "side");
    assert_eq!(status.changes, [" M readme.txt", "?? draft.txt"]);
    assert!(!status.more);
    agent("for i in $(seq 25); do touch f$i; done");
    let status = workspace_status(&provider, &name.instance()).unwrap();
    assert_eq!(status.changes.len(), MAX_CHANGES);
    assert!(status.more);
}
