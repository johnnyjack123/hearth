//! Runs the Podman provider against the real VM.
//!
//! Ignored by default because it needs a running machine:
//! `cargo test -p harness-core --test podman_vm -- --ignored --nocapture`
//! (`HARNESS_MACHINE` selects another machine than `harness`).
//!
//! Uses `alpine:3.22`, so it does not depend on the base image.

use std::collections::BTreeMap;
use std::fs;
use std::time::Instant;

use harness_core::podman::{DEFAULT_MACHINE, PodmanProvider};
use harness_core::provider::{
    ContainerSpec, ContainerStatus, ExecSpec, Mount, MountSource, NetworkSpec, Provider, VolumeSpec,
};
use harness_core::{Error, MANAGED_LABEL};

const IMAGE: &str = "docker.io/library/alpine:3.22";

struct Names {
    container: String,
    network: String,
    external_network: String,
    volume: String,
    exchange_dir: String,
}

/// Removes everything the test created, also when an assertion fails midway.
struct Cleanup<'a> {
    provider: &'a PodmanProvider,
    names: &'a Names,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let p = self.provider;
        let n = self.names;
        let _ = p.remove_container(&n.container);
        let _ = p.remove_volume(&n.volume);
        let _ = p.remove_network(&n.network);
        let _ = p.remove_network(&n.external_network);
        if let Ok(exchange) = p.exchange()
            && let Ok(dir) = exchange.host_path(&n.exchange_dir)
        {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

fn sh(provider: &PodmanProvider, container: &str, script: &str) -> (i32, String) {
    let output = provider
        .exec_output(&ExecSpec {
            container: container.to_owned(),
            command: vec!["sh".into(), "-c".into(), script.into()],
            ..ExecSpec::default()
        })
        .unwrap();
    (output.code, format!("{}{}", output.stdout, output.stderr))
}

fn timed<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let value = f();
    println!("{label}: {} ms", start.elapsed().as_millis());
    value
}

#[test]
#[ignore = "needs the running Podman machine"]
fn provider_against_vm() {
    let machine = std::env::var("HARNESS_MACHINE").unwrap_or_else(|_| DEFAULT_MACHINE.into());
    let provider = PodmanProvider::for_machine(&machine).unwrap();

    let info = timed("ping", || provider.ping().unwrap());
    assert!(info.rootless, "machine must be rootless");
    println!("VM: {info:?}");

    let id = std::process::id();
    let names = Names {
        container: format!("harness-test-{id}"),
        network: format!("harness-test-net-{id}"),
        external_network: format!("harness-test-ext-{id}"),
        volume: format!("harness-test-ws-{id}"),
        exchange_dir: format!("harness-test-{id}"),
    };
    let _cleanup = Cleanup {
        provider: &provider,
        names: &names,
    };

    // Networks: idempotent, and the internal flag is never silently ignored.
    let internal = NetworkSpec {
        name: names.network.clone(),
        internal: true,
        ..NetworkSpec::default()
    };
    timed("ensure_network (create)", || {
        provider.ensure_network(&internal).unwrap()
    });
    timed("ensure_network (exists)", || {
        provider.ensure_network(&internal).unwrap()
    });
    let same_name_external = NetworkSpec {
        internal: false,
        ..internal.clone()
    };
    assert!(provider.ensure_network(&same_name_external).is_err());

    let volume = VolumeSpec {
        name: names.volume.clone(),
        ..VolumeSpec::default()
    };
    provider.ensure_volume(&volume).unwrap();
    provider.ensure_volume(&volume).unwrap();

    // Exchange folder: a file placed on the host must be visible in the
    // container, and a file written in the container must reach the host.
    let exchange = provider.exchange().unwrap().clone();
    exchange.create_dir(&names.exchange_dir).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let source = scratch.path().join("hello.txt");
    fs::write(&source, "from host\n").unwrap();
    exchange
        .put_file(&source, &format!("{}/hello.txt", names.exchange_dir))
        .unwrap();

    let mut spec = ContainerSpec {
        name: names.container.clone(),
        image: IMAGE.to_owned(),
        network: names.network.clone(),
        mounts: vec![
            Mount {
                source: MountSource::Volume(names.volume.clone()),
                target: "/workspace".into(),
                read_only: false,
            },
            Mount {
                source: MountSource::Exchange(names.exchange_dir.clone()),
                target: "/mnt/exchange".into(),
                read_only: false,
            },
            Mount {
                source: MountSource::Exchange(names.exchange_dir.clone()),
                target: "/mnt/exchange-ro".into(),
                read_only: true,
            },
        ],
        env: BTreeMap::from([("FROM_CREATE".to_owned(), "x y".to_owned())]),
        labels: BTreeMap::from([("io.llm-harness.test".to_owned(), "1".to_owned())]),
        workdir: Some("/workspace".into()),
        command: vec!["sleep".into(), "infinity".into()],
        ..ContainerSpec::default()
    };

    // Instances must never join a network with a route out.
    provider
        .ensure_network(&NetworkSpec {
            name: names.external_network.clone(),
            internal: false,
            ..NetworkSpec::default()
        })
        .unwrap();
    let external = ContainerSpec {
        network: names.external_network.clone(),
        ..spec.clone()
    };
    assert!(matches!(
        provider.create_container(&external),
        Err(Error::NetworkNotInternal(_))
    ));

    assert!(provider.container(&names.container).unwrap().is_none());
    timed("create_container", || {
        provider.create_container(&spec).unwrap()
    });
    let created = provider.container(&names.container).unwrap().unwrap();
    assert_eq!(created.status, ContainerStatus::Created);
    assert_eq!(
        created.labels.get(MANAGED_LABEL).map(String::as_str),
        Some("true")
    );
    assert_eq!(
        created
            .labels
            .get("io.llm-harness.test")
            .map(String::as_str),
        Some("1")
    );

    timed("start_container", || {
        provider.start_container(&names.container).unwrap()
    });
    assert_eq!(
        provider
            .container(&names.container)
            .unwrap()
            .unwrap()
            .status,
        ContainerStatus::Running
    );
    let c = &names.container;

    // Hardening.
    assert_eq!(sh(&provider, c, "id -u; id -g"), (0, "1000\n1000\n".into()));
    let (_, status) = sh(
        &provider,
        c,
        "grep -E '^(CapEff|NoNewPrivs)' /proc/self/status",
    );
    assert!(status.contains("CapEff:\t0000000000000000"), "{status}");
    assert!(status.contains("NoNewPrivs:\t1"), "{status}");
    let (code, _) = sh(&provider, c, "touch /etc/harness-test");
    assert_ne!(code, 0, "root filesystem must be read-only");
    assert_eq!(sh(&provider, c, "cat /sys/fs/cgroup/pids.max").1, "1024\n");
    // `--init` mounts catatonit as /run/podman-init.
    assert_eq!(sh(&provider, c, "cat /proc/1/comm").1, "podman-init\n");
    let (code, _) = sh(&provider, c, "touch /tmp/x && chmod +x /tmp/x");
    assert_eq!(code, 0, "/tmp must be writable");

    // No way out without the gateway.
    let (code, out) = sh(&provider, c, "wget -T 3 -q -O- http://1.1.1.1/ 2>&1");
    assert_ne!(code, 0, "direct IP reachable: {out}");
    let (code, out) = sh(&provider, c, "nslookup example.com 2>&1");
    assert_ne!(code, 0, "external DNS resolvable: {out}");

    // Volume, workdir and env.
    assert_eq!(sh(&provider, c, "pwd").1, "/workspace\n");
    assert_eq!(
        sh(&provider, c, "echo hi > /workspace/f && cat /workspace/f").1,
        "hi\n"
    );
    assert_eq!(sh(&provider, c, "echo \"$FROM_CREATE\"").1, "x y\n");
    let with_env = provider
        .exec_output(&ExecSpec {
            container: c.clone(),
            command: vec!["sh".into(), "-c".into(), "echo \"$FOO\"; pwd".into()],
            workdir: Some("/tmp".into()),
            env: BTreeMap::from([("FOO".to_owned(), "a \"b\" $c".to_owned())]),
            ..ExecSpec::default()
        })
        .unwrap();
    assert_eq!(with_env.stdout, "a \"b\" $c\n/tmp\n");

    // Exchange folder in both directions.
    assert_eq!(
        sh(&provider, c, "cat /mnt/exchange/hello.txt").1,
        "from host\n"
    );
    let (code, out) = sh(&provider, c, "echo back > /mnt/exchange/back.txt");
    assert_eq!(code, 0, "exchange not writable: {out}");
    let back = exchange
        .host_path(&format!("{}/back.txt", names.exchange_dir))
        .unwrap();
    assert_eq!(fs::read_to_string(back).unwrap(), "back\n");
    let (code, _) = sh(&provider, c, "echo no > /mnt/exchange-ro/no.txt");
    assert_ne!(code, 0, "read-only exchange mount is writable");

    // Attached exec (no TTY here) passes the exit code through.
    let attached = |script: &str| {
        provider
            .exec(&ExecSpec {
                container: c.clone(),
                command: vec!["sh".into(), "-c".into(), script.into()],
                ..ExecSpec::default()
            })
            .unwrap()
    };
    assert_eq!(timed("exec (attached)", || attached("true")), 0);
    assert_eq!(attached("exit 3"), 3);

    // Lifecycle: stop, start again, data survives.
    timed("stop_container", || provider.stop_container(c).unwrap());
    assert_eq!(
        provider.container(c).unwrap().unwrap().status,
        ContainerStatus::Exited
    );
    provider.stop_container(c).unwrap();
    provider.start_container(c).unwrap();
    assert_eq!(sh(&provider, c, "cat /workspace/f").1, "hi\n");

    spec.name = "--privileged".into();
    assert!(provider.create_container(&spec).is_err());
}
