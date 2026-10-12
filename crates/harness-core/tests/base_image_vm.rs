//! Checks the base image inside a hardened instance on the real VM.
//!
//! Ignored by default; needs a running machine and a built image
//! (`harness image build`):
//! `cargo test -p harness-core --test base_image_vm -- --ignored --nocapture`
//!
//! Network access (pnpm download, proxy behaviour) is not covered here because
//! instances have no route out until the gateway exists.

use std::collections::BTreeMap;

use harness_core::image::{BASE_IMAGE, ImageStatus};
use harness_core::podman::{DEFAULT_MACHINE, PodmanProvider};
use harness_core::provider::{
    ContainerSpec, ExecSpec, Mount, MountSource, NetworkSpec, Provider, VolumeSpec,
};

struct Cleanup<'a> {
    provider: &'a PodmanProvider,
    container: String,
    volumes: Vec<String>,
    network: String,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let _ = self.provider.remove_container(&self.container);
        for volume in &self.volumes {
            let _ = self.provider.remove_volume(volume);
        }
        let _ = self.provider.remove_network(&self.network);
    }
}

#[test]
#[ignore = "needs the running Podman machine and the built base image"]
fn base_image_in_hardened_instance() {
    let machine = std::env::var("HARNESS_MACHINE").unwrap_or_else(|_| DEFAULT_MACHINE.into());
    let provider = PodmanProvider::for_machine(&machine).unwrap();
    assert_eq!(
        BASE_IMAGE.status(&provider).unwrap(),
        ImageStatus::Current,
        "base image missing or outdated, run `harness image build`"
    );

    let id = std::process::id();
    let cleanup = Cleanup {
        provider: &provider,
        container: format!("harness-imgtest-{id}"),
        volumes: vec![
            format!("harness-imgtest-ws-{id}"),
            format!("harness-imgtest-home-{id}"),
            format!("harness-imgtest-tools-{id}"),
        ],
        network: format!("harness-imgtest-net-{id}"),
    };
    provider
        .ensure_network(&NetworkSpec {
            name: cleanup.network.clone(),
            internal: true,
            ..NetworkSpec::default()
        })
        .unwrap();
    for volume in &cleanup.volumes {
        provider
            .ensure_volume(&VolumeSpec {
                name: volume.clone(),
                ..VolumeSpec::default()
            })
            .unwrap();
    }
    let volume_mount = |index: usize, target: &str| Mount {
        source: MountSource::Volume(cleanup.volumes[index].clone()),
        target: target.to_owned(),
        read_only: false,
    };
    provider
        .create_container(&ContainerSpec {
            name: cleanup.container.clone(),
            image: BASE_IMAGE.tag.to_owned(),
            network: cleanup.network.clone(),
            mounts: vec![
                volume_mount(0, "/workspace"),
                volume_mount(1, "/home/agent"),
                volume_mount(2, "/opt/harness-tools"),
            ],
            ..ContainerSpec::default()
        })
        .unwrap();
    provider.start_container(&cleanup.container).unwrap();

    let run = |command: &[&str]| {
        let output = provider
            .exec_output(&ExecSpec {
                container: cleanup.container.clone(),
                command: command.iter().map(|s| (*s).to_owned()).collect(),
                ..ExecSpec::default()
            })
            .unwrap();
        assert_eq!(
            output.code, 0,
            "{command:?}\n{}{}",
            output.stdout, output.stderr
        );
        output.stdout
    };
    // Login shell on purpose: it is the case where Debian's /etc/profile
    // resets PATH, and everything below must still work there.
    let sh = |script: &str| run(&["bash", "-lc", script]);

    // The image's USER and WORKDIR apply.
    assert_eq!(sh("id -un; pwd"), "agent\n/workspace\n");

    // Tools from the roadmap list are on PATH and runnable.
    let versions = sh(
        "git --version; node --version; npm --version; corepack --version; \
         python3 --version; uv --version; rg --version | head -1; fd --version; \
         jq --version; gcc --version | head -1; make --version | head -1; curl --version | head -1; \
         command -v pnpm",
    );
    println!("{versions}");
    assert!(versions.contains("git version"));
    assert!(versions.contains("v24."), "Node 24 expected:\n{versions}");
    assert!(versions.contains("/usr/local/bin/pnpm"));

    // Fresh volumes inherit agent ownership from the image's mount points.
    assert_eq!(
        sh("stat -c %U /workspace /home/agent /opt/harness-tools"),
        "agent\nagent\nagent\n"
    );
    sh("touch /workspace/a ~/b /opt/harness-tools/c");
    assert!(sh("ls -A ~").contains(".bashrc"), "skeleton not copied up");

    // Global npm installs land in the home volume, not the read-only root.
    assert_eq!(sh("npm config get prefix"), "/home/agent/.local\n");

    // System git config from images/base/gitconfig.
    let git = sh("git config --system --list");
    for entry in [
        "core.checkstat=minimal",
        "core.trustctime=false",
        "safe.directory=/workspace",
        "init.defaultbranch=main",
        "user.name=harness agent",
    ] {
        assert!(git.contains(entry), "missing {entry} in\n{git}");
    }
    // A repo in the workspace works and commits with the agent identity.
    sh(
        "cd /workspace && git init -q r && cd r && echo x > f && git add f && \
        git commit -qm init && test \"$(git log -1 --format=%an)\" = 'harness agent'",
    );

    // Login shells keep the tool directories (images/base/profile.sh).
    let login_path = sh("echo \"$PATH\"");
    for dir in [
        "/opt/harness-tools/bin",
        "/home/agent/.local/bin",
        "/usr/local/bin",
    ] {
        assert!(
            login_path.split(':').any(|p| p.trim() == dir),
            "{dir} missing in {login_path}"
        );
    }

    // Proxy settings point at the gateway for every tool family. Read without
    // a shell, i.e. what `podman exec … pi` sees.
    let env: BTreeMap<String, String> = run(&["env"])
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    for key in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy"] {
        assert_eq!(
            env.get(key).map(String::as_str),
            Some("http://gateway:8888"),
            "{key}"
        );
    }
    assert_eq!(env.get("NODE_USE_ENV_PROXY").map(String::as_str), Some("1"));
    assert_eq!(
        env.get("COREPACK_ENABLE_AUTO_PIN").map(String::as_str),
        Some("0")
    );
    assert!(env["PATH"].starts_with("/opt/harness-tools/bin:"));
}
