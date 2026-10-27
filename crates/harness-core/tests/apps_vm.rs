//! Apps on the VM: installed into volumes of their own with a receipt, the
//! ones they require mounted while they install, mounted read-only into an
//! instance with their PATH and variables, a failed install leaving nothing
//! behind; and the work an agent never handed over saved as a bundle before
//! its volumes go (`rescue`).
//!
//! The recipes are made up and download nothing; the catalog's real ones
//! are tried with `harness app install` (CLAUDE.md, "App-Katalog").
//!
//! Ignored by default; needs a running machine and both images:
//! `cargo test -p harness-core --test apps_vm -- --ignored --nocapture`

use std::path::PathBuf;
use std::process::Command;

use harness_core::apps::{self, Inventory, ReadyApp};
use harness_core::config::{AppConfig, FileMode, NetworkPreset, Project};
use harness_core::gateway;
use harness_core::image::{BASE_IMAGE, GATEWAY_IMAGE, ImageStatus};
use harness_core::instance;
use harness_core::podman::{DEFAULT_MACHINE, PodmanProvider};
use harness_core::project::ProjectName;
use harness_core::provider::{ExecOutput, ExecSpec, Provider, Resources};
use harness_core::rescue;
use harness_core::sync::{self, HostRepo, find_git};

struct Cleanup<'a> {
    provider: &'a PodmanProvider,
    name: &'a ProjectName,
    apps: Vec<String>,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let p = self.provider;
        let name = self.name;
        for container in [
            name.instance(),
            name.gateway(),
            format!("harness-rescue-{name}"),
        ] {
            let _ = p.remove_container(&container);
        }
        for app in &self.apps {
            let _ = p.remove_container(&format!("harness-app-install-{app}"));
        }
        if let Ok(volumes) = p.volumes() {
            for volume in volumes {
                let ours = self
                    .apps
                    .iter()
                    .any(|app| volume.name.starts_with(&format!("harness-app-{app}-")));
                if ours {
                    let _ = p.remove_volume(&volume.name);
                }
            }
        }
        for volume in [name.workspace_volume(), name.home_volume()] {
            let _ = p.remove_volume(&volume);
        }
        let _ = p.remove_network(&name.network());
    }
}

fn app(text: &str) -> AppConfig {
    toml::from_str(text).unwrap()
}

fn git(dir: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new(find_git().unwrap())
        .current_dir(dir)
        .args(["-c", "core.autocrlf=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
#[ignore = "needs the running Podman machine and both images"]
fn apps_install_once_mount_read_only_and_work_is_rescued() {
    let machine = std::env::var("HARNESS_MACHINE").unwrap_or_else(|_| DEFAULT_MACHINE.into());
    let provider = PodmanProvider::for_machine(&machine).unwrap();
    for image in [BASE_IMAGE, GATEWAY_IMAGE] {
        assert_eq!(
            image.status(&provider).unwrap(),
            ImageStatus::Current,
            "run `harness image build` first"
        );
    }
    let pid = std::process::id();
    let name = ProjectName::new(&format!("apptest-{pid}")).unwrap();
    let project = Project {
        name: name.clone(),
        file: PathBuf::from("unused.toml"),
        path: PathBuf::from("/unused"),
        harness: "pi".into(),
        mode: FileMode::Isolated,
        network: NetworkPreset::Off,
        ask: false,
        allow: vec![],
        deny: vec![],
        llm_apis: vec![],
        resources: Resources::default(),
        apps: vec![],
    };
    let (lib, user, broken) = (
        format!("tlib-{pid}"),
        format!("tuse-{pid}"),
        format!("tbad-{pid}"),
    );
    let _cleanup = Cleanup {
        provider: &provider,
        name: &name,
        apps: vec![lib.clone(), user.clone(), broken.clone()],
    };
    gateway::ensure_running(&provider, &project, None).unwrap();

    // An app with a program, and one that uses it while it installs.
    let lib_app = app(&format!(
        "name = '{lib}'\ntitle = 'Lib'\npath = ['bin']\ninstall = '''\n\
         mkdir -p \"$PREFIX/bin\"\n\
         printf '#!/bin/sh\\necho from-lib\\n' > \"$PREFIX/bin/{lib}\"\n\
         chmod +x \"$PREFIX/bin/{lib}\"\n\
         harness_version \"1.$MACHINE\"\n'''\n"
    ));
    let user_app = app(&format!(
        "name = '{user}'\ntitle = 'User'\nrequires = ['{lib}']\n\
         install = '''\nmkdir -p \"$PREFIX/data\"\n{lib} > \"$PREFIX/data/out\"\n\
         harness_version 2.0\n'''\n\
         [env]\nTUSE_HOME = '{{prefix}}/data'\nTUSE_CACHE = '~/.tuse'\n"
    ));
    let first = apps::install(&provider, &project, &lib_app, &[]).unwrap();
    assert!(first.version.starts_with("1."), "{first:?}");
    let lib_ready = ReadyApp {
        app: lib_app.clone(),
        install: first.clone(),
    };
    let used = apps::install(
        &provider,
        &project,
        &user_app,
        std::slice::from_ref(&lib_ready),
    )
    .unwrap();
    assert_eq!(used.version, "2.0");
    let inventory = Inventory::load(&provider).unwrap();
    assert_eq!(inventory.latest(&lib), Some(&first));
    assert!(!inventory.latest(&lib).unwrap().outdated(&lib_app));

    // A recipe that fails leaves neither volume nor receipt.
    let broken_app = app(&format!(
        "name = '{broken}'\ntitle = 'Broken'\ninstall = 'exit 3'\n"
    ));
    assert!(apps::install(&provider, &project, &broken_app, &[]).is_err());
    let no_version = app(&format!(
        "name = '{broken}'\ntitle = 'Broken'\ninstall = 'harness_version \"$(false)\"'\n"
    ));
    assert!(apps::install(&provider, &project, &no_version, &[]).is_err());
    let leftovers = provider.volumes().unwrap();
    assert!(
        !leftovers
            .iter()
            .any(|v| v.name.starts_with(&format!("harness-app-{broken}-"))),
        "{leftovers:?}"
    );

    // The instance gets both, read-only, on PATH also for login shells.
    let ready = [
        lib_ready,
        ReadyApp {
            app: user_app.clone(),
            install: used,
        },
    ];
    assert!(instance::ensure_running(&provider, &project, &[], &ready).unwrap());
    assert!(!instance::ensure_running(&provider, &project, &[], &ready).unwrap());
    let run = |shell: &str, script: &str| -> ExecOutput {
        provider
            .exec_output(&ExecSpec {
                container: name.instance(),
                command: vec![shell.into(), "-c".into(), script.into()],
                ..ExecSpec::default()
            })
            .unwrap()
    };
    for shell in ["sh", "bash -l"] {
        let (program, login) = shell.split_once(' ').unwrap_or((shell, ""));
        let script = format!("{lib} && cat \"$TUSE_HOME/out\" && echo \"$TUSE_CACHE\"");
        let output = provider
            .exec_output(&ExecSpec {
                container: name.instance(),
                command: [program, login, "-c", &script]
                    .into_iter()
                    .filter(|a| !a.is_empty())
                    .map(str::to_owned)
                    .collect(),
                ..ExecSpec::default()
            })
            .unwrap();
        assert_eq!(
            output.stdout, "from-lib\nfrom-lib\n/home/agent/.tuse\n",
            "{shell}: {}",
            output.stderr
        );
    }
    let write = run("sh", &format!("touch /opt/harness-apps/{lib}/x"));
    assert_ne!(write.code, 0, "app writable from the instance");

    // A new install of the library: the old one stays while the instance
    // mounts it, and goes once the instance moved on.
    let second = apps::install(&provider, &project, &lib_app, &[]).unwrap();
    assert_ne!(second.volume, first.volume);
    let inventory = Inventory::load(&provider).unwrap();
    assert_eq!(inventory.latest(&lib), Some(&second));
    assert!(inventory.superseded().contains(&&first));
    let ready = [
        ReadyApp {
            app: lib_app,
            install: second,
        },
        ready[1].clone(),
    ];
    assert!(instance::ensure_running(&provider, &project, &[], &ready).unwrap());
    apps::remove(&provider, &first).unwrap();
    assert!(Inventory::load(&provider).unwrap().superseded().is_empty());

    // Rescue: a commit and an uncommitted file the host never got.
    let host = tempfile::tempdir().unwrap();
    git(host.path(), &["init", "-q", "-b", "main"]);
    git(host.path(), &["config", "user.email", "t@t"]);
    git(host.path(), &["config", "user.name", "t"]);
    std::fs::write(host.path().join("README.md"), "demo\n").unwrap();
    git(host.path(), &["add", "."]);
    git(host.path(), &["commit", "-qm", "init"]);
    let repo = HostRepo::open(host.path()).unwrap();
    sync::push_to_workspace(&provider, &name.instance(), &repo, &name).unwrap();
    assert_eq!(
        rescue::unsaved(&provider, &name).unwrap(),
        Some(rescue::Unsaved {
            commits: 0,
            changes: 0
        })
    );
    let work = run(
        "sh",
        "cd /workspace && echo a > a.txt && git add a.txt && git commit -qm a && echo b > b.txt",
    );
    assert_eq!(work.code, 0, "{}", work.stderr);
    let unsaved = rescue::unsaved(&provider, &name).unwrap().unwrap();
    assert_eq!((unsaved.commits, unsaved.changes), (1, 1));
    let bundle = host.path().join("rescued.bundle");
    rescue::save(&provider, &name, &bundle).unwrap();
    let clone = tempfile::tempdir().unwrap();
    git(
        clone.path(),
        &[
            "clone",
            "-q",
            "-b",
            &name.agent_branch(),
            bundle.to_str().unwrap(),
            "c",
        ],
    );
    let files = git(&clone.path().join("c"), &["ls-files"]);
    assert_eq!(files, "README.md\na.txt\nb.txt\n");
}
