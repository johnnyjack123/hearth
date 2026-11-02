//! Pure builders for podman argument lists, kept apart from process handling so
//! the security-relevant flags are unit-testable.
//!
//! Every value that ends up in an argument is validated first: a name starting
//! with `-` would otherwise be parsed as a flag, and a comma in a path would
//! inject options into `--mount`.

use std::ffi::OsString;
use std::path::Path;

use crate::MANAGED_LABEL;
use crate::lang::tr;
use crate::provider::{
    ContainerSpec, ExecSpec, GatewaySpec, ImageBuild, Labels, MountSource, NetworkSpec, VolumeSpec,
    validate_container_path, validate_env_key, validate_image_ref, validate_label, validate_memory,
    validate_resource_name,
};
use crate::{Error, Result};

/// UID/GID of the `agent` user in the base image. With
/// `--userns=keep-id:uid=…` the VM user maps exactly onto it, so files in
/// volumes belong to the VM user and nobody runs as (namespaced) root.
pub(crate) const AGENT_ID: &str = "1000";

/// Seconds podman waits after SIGTERM before killing a container.
pub(crate) const STOP_TIMEOUT: &str = "10";

pub(crate) fn volume_create(spec: &VolumeSpec) -> Result<Vec<String>> {
    validate_resource_name(tr("Volume", "volume"), &spec.name)?;
    let mut args = vec!["volume".to_owned(), "create".to_owned()];
    push_labels(&mut args, &spec.labels)?;
    args.push(spec.name.clone());
    Ok(args)
}

pub(crate) fn network_create(spec: &NetworkSpec) -> Result<Vec<String>> {
    validate_resource_name(tr("Netz", "network"), &spec.name)?;
    let mut args = vec!["network".to_owned(), "create".to_owned()];
    if spec.internal {
        args.push("--internal".to_owned());
    }
    push_labels(&mut args, &spec.labels)?;
    args.push(spec.name.clone());
    Ok(args)
}

/// `podman create` for an instance. The hardening block is unconditional.
pub(crate) fn container_create(spec: &ContainerSpec) -> Result<Vec<String>> {
    validate_resource_name(tr("Containername", "container name"), &spec.name)?;
    validate_resource_name(tr("Netz", "network"), &spec.network)?;
    validate_image_ref(&spec.image)?;

    let mut args: Vec<String> = ["create", "--name", &spec.name, "--hostname", &spec.name]
        .map(str::to_owned)
        .to_vec();
    push_labels(&mut args, &spec.labels)?;
    args.extend(["--network".to_owned(), spec.network.clone()]);

    // Hardening (ARCHITECTURE.md §3.3). `--init` puts catatonit in front of
    // the image's CMD so processes orphaned by `exec` sessions get reaped.
    args.extend(
        [
            "--init",
            &format!("--userns=keep-id:uid={AGENT_ID},gid={AGENT_ID}"),
            "--user",
            &format!("{AGENT_ID}:{AGENT_ID}"),
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--read-only",
            // podman's default /tmp tmpfs is noexec, which breaks build tools
            // that run helpers from /tmp.
            "--tmpfs",
            "/tmp:rw,nosuid,nodev,exec,size=1g,mode=1777",
            "--stop-timeout",
            STOP_TIMEOUT,
            "--pids-limit",
            &spec.resources.pids_limit.to_string(),
        ]
        .map(str::to_owned),
    );
    if let Some(memory) = &spec.resources.memory {
        validate_memory(memory)?;
        args.extend(["--memory".to_owned(), memory.clone()]);
    }
    if let Some(cpus) = spec.resources.cpus {
        if !(cpus.is_finite() && cpus > 0.0) {
            return Err(Error::invalid(
                tr("CPU-Limit", "CPU limit"),
                &cpus.to_string(),
                tr("muss größer als 0 sein", "must be greater than 0"),
            ));
        }
        args.extend(["--cpus".to_owned(), cpus.to_string()]);
    }
    if let Some(workdir) = &spec.workdir {
        validate_container_path(tr("Arbeitsverzeichnis", "working directory"), workdir)?;
        args.extend(["--workdir".to_owned(), workdir.clone()]);
    }
    for mount in &spec.mounts {
        validate_container_path(tr("Mount-Ziel", "mount target"), &mount.target)?;
        let source = match &mount.source {
            MountSource::Volume(name) => {
                validate_resource_name(tr("Volume", "volume"), name)?;
                format!("type=volume,source={name}")
            }
        };
        let read_only = if mount.read_only { ",readonly" } else { "" };
        args.extend([
            "--mount".to_owned(),
            format!("{source},target={}{read_only}", mount.target),
        ]);
    }
    push_env(&mut args, &spec.env)?;
    args.push(spec.image.clone());
    args.extend(spec.command.iter().cloned());
    Ok(args)
}

/// The gateway runs as `nobody`: it needs no files of the VM user, so
/// without `keep-id` it maps to an unused subordinate UID in the VM.
pub(crate) const GATEWAY_USER: &str = "65534:65534";

/// Every connection costs two threads, and threads count against the pid
/// limit; the gateway caps itself at 200 connections.
const GATEWAY_PIDS_LIMIT: &str = "512";

const GATEWAY_MEMORY: &str = "128m";

/// `podman create` for a project's gateway. Like [`container_create`], the
/// hardening is unconditional; there are no mounts at all.
pub(crate) fn gateway_create(spec: &GatewaySpec) -> Result<Vec<String>> {
    validate_resource_name(tr("Containername", "container name"), &spec.name)?;
    validate_resource_name(tr("Netz", "network"), &spec.internal_network)?;
    validate_resource_name(tr("Netz", "network"), &spec.egress_network)?;
    validate_resource_name(tr("Netzwerk-Alias", "network alias"), &spec.alias)?;
    validate_image_ref(&spec.image)?;
    if spec.internal_network == spec.egress_network {
        return Err(Error::invalid(
            tr("Netz", "network"),
            &spec.egress_network,
            tr(
                "internes und äußeres Netz müssen verschieden sein",
                "internal and outer network must differ",
            ),
        ));
    }

    let mut args: Vec<String> = ["create", "--name", &spec.name, "--hostname", &spec.name]
        .map(str::to_owned)
        .to_vec();
    push_labels(&mut args, &spec.labels)?;
    args.extend([
        "--network".to_owned(),
        format!("{}:alias={}", spec.internal_network, spec.alias),
        "--network".to_owned(),
        spec.egress_network.clone(),
    ]);
    args.extend(
        [
            "--init",
            "--user",
            GATEWAY_USER,
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--read-only",
            // Only for the control socket.
            "--tmpfs",
            "/tmp:rw,nosuid,nodev,noexec,size=16m,mode=1777",
            "--stop-timeout",
            STOP_TIMEOUT,
            "--pids-limit",
            GATEWAY_PIDS_LIMIT,
            "--memory",
            GATEWAY_MEMORY,
        ]
        .map(str::to_owned),
    );
    args.push(spec.image.clone());
    args.extend(spec.command.iter().cloned());
    Ok(args)
}

pub(crate) fn exec(spec: &ExecSpec) -> Result<Vec<String>> {
    validate_resource_name(tr("Containername", "container name"), &spec.container)?;
    if spec.command.is_empty() {
        return Err(Error::invalid(
            tr("Befehl", "command"),
            "",
            tr("darf nicht leer sein", "must not be empty"),
        ));
    }
    let mut args = vec!["exec".to_owned()];
    if spec.interactive {
        args.push("--interactive".to_owned());
    }
    if spec.tty {
        args.push("--tty".to_owned());
    }
    if let Some(workdir) = &spec.workdir {
        validate_container_path(tr("Arbeitsverzeichnis", "working directory"), workdir)?;
        args.extend(["--workdir".to_owned(), workdir.clone()]);
    }
    push_env(&mut args, &spec.env)?;
    // podman stops parsing its own flags at the container name, so the
    // command's arguments are passed through untouched.
    args.push(spec.container.clone());
    args.extend(spec.command.iter().cloned());
    Ok(args)
}

/// `podman cp` of a host file into a container. The container path is
/// resolved inside the container's root, so it cannot point at the VM.
pub(crate) fn copy_into(container: &str, host_file: &Path, path: &str) -> Result<Vec<OsString>> {
    validate_resource_name(tr("Containername", "container name"), container)?;
    validate_container_path(tr("Zielpfad", "target path"), path)?;
    if path.contains(':') {
        return Err(Error::invalid(
            tr("Zielpfad", "target path"),
            path,
            tr(
                "darf keinen Doppelpunkt enthalten",
                "must not contain a colon",
            ),
        ));
    }
    Ok(vec![
        "cp".into(),
        host_file.as_os_str().to_owned(),
        format!("{container}:{path}").into(),
    ])
}

/// Prints a regular file of a container to stdout. The script refuses
/// symlinks and other file types, and pins PATH because the agent owns
/// `~/.local/bin`.
pub(crate) fn copy_out(container: &str, path: &str) -> Result<Vec<String>> {
    validate_container_path(tr("Quellpfad", "source path"), path)?;
    let script = r#"export PATH=/usr/bin:/bin
[ -f "$1" ] && [ ! -L "$1" ] || { echo "keine reguläre Datei: $1" >&2; exit 3; }
exec cat -- "$1""#;
    exec(&ExecSpec {
        container: container.to_owned(),
        command: vec![
            "sh".into(),
            "-c".into(),
            script.into(),
            "sh".into(),
            path.to_owned(),
        ],
        ..ExecSpec::default()
    })
}

pub(crate) fn build(spec: &ImageBuild) -> Result<Vec<OsString>> {
    validate_image_ref(&spec.tag)?;
    let mut args: Vec<OsString> = vec!["build".into(), "--tag".into(), spec.tag.clone().into()];
    let mut labels = Vec::new();
    push_labels(&mut labels, &spec.labels)?;
    args.extend(labels.into_iter().map(OsString::from));
    for (key, value) in &spec.build_args {
        validate_env_key(key)?;
        if value.chars().any(char::is_control) {
            return Err(Error::invalid(
                tr("Build-Argument", "build argument"),
                value,
                tr("enthält Steuerzeichen", "contains control characters"),
            ));
        }
        args.push("--build-arg".into());
        args.push(format!("{key}={value}").into());
    }
    if spec.pull {
        args.push("--pull=always".into());
    }
    if spec.no_cache {
        args.push("--no-cache".into());
    }
    args.push(spec.context.clone().into_os_string());
    Ok(args)
}

/// Adds `--label` pairs, always including the managed label.
fn push_labels(args: &mut Vec<String>, labels: &Labels) -> Result<()> {
    args.extend(["--label".to_owned(), format!("{MANAGED_LABEL}=true")]);
    for (key, value) in labels {
        validate_label(key, value)?;
        if key == MANAGED_LABEL {
            continue;
        }
        args.extend(["--label".to_owned(), format!("{key}={value}")]);
    }
    Ok(())
}

fn push_env(
    args: &mut Vec<String>,
    env: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    for (key, value) in env {
        validate_env_key(key)?;
        if value.contains('\0') {
            return Err(Error::invalid(
                tr("Umgebungsvariable", "environment variable"),
                key,
                tr(
                    "Wert enthält ein NUL-Zeichen",
                    "value contains a NUL character",
                ),
            ));
        }
        args.extend(["--env".to_owned(), format!("{key}={value}")]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::provider::{Mount, Resources};

    fn instance() -> ContainerSpec {
        ContainerSpec {
            name: "harness-demo".into(),
            image: "localhost/harness/base:latest".into(),
            network: "harness-net-demo".into(),
            mounts: vec![
                Mount {
                    source: MountSource::Volume("harness-ws-demo".into()),
                    target: "/workspace".into(),
                    read_only: false,
                },
                Mount {
                    source: MountSource::Volume("harness-tools".into()),
                    target: "/opt/harness-tools".into(),
                    read_only: true,
                },
            ],
            env: [("FOO".to_owned(), "a b=c".to_owned())].into(),
            labels: [("io.llm-harness.project".to_owned(), "demo".to_owned())].into(),
            workdir: Some("/workspace".into()),
            resources: Resources {
                cpus: Some(2.0),
                memory: Some("2g".into()),
                pids_limit: 512,
            },
            command: vec![],
        }
    }

    fn has_pair(args: &[String], flag: &str, value: &str) -> bool {
        args.windows(2).any(|w| w[0] == flag && w[1] == value)
    }

    #[test]
    fn create_always_hardens() {
        let args = container_create(&ContainerSpec::default_with_name()).unwrap();
        for flag in [
            "--init",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--read-only",
            "--userns=keep-id:uid=1000,gid=1000",
        ] {
            assert!(args.iter().any(|a| a == flag), "missing {flag}");
        }
        assert!(has_pair(&args, "--user", "1000:1000"));
        assert!(has_pair(&args, "--pids-limit", "1024"));
        assert!(has_pair(&args, "--label", "io.llm-harness.managed=true"));
        assert!(!args.iter().any(|a| a.contains("privileged")));
    }

    #[test]
    fn create_maps_spec() {
        let args = container_create(&instance()).unwrap();
        assert!(has_pair(&args, "--network", "harness-net-demo"));
        assert!(has_pair(
            &args,
            "--mount",
            "type=volume,source=harness-ws-demo,target=/workspace"
        ));
        assert!(has_pair(
            &args,
            "--mount",
            "type=volume,source=harness-tools,target=/opt/harness-tools,readonly"
        ));
        assert!(has_pair(&args, "--env", "FOO=a b=c"));
        assert!(has_pair(&args, "--memory", "2g"));
        assert!(has_pair(&args, "--cpus", "2"));
        assert!(has_pair(&args, "--pids-limit", "512"));
        assert!(has_pair(&args, "--label", "io.llm-harness.project=demo"));
        assert_eq!(args.last().unwrap(), "localhost/harness/base:latest");
    }

    #[test]
    fn create_rejects_injection() {
        let mut spec = instance();
        spec.name = "--privileged".into();
        assert!(container_create(&spec).is_err());

        let mut spec = instance();
        spec.mounts[0].target = "/workspace,readonly=false".into();
        assert!(container_create(&spec).is_err());

        let mut spec = instance();
        spec.mounts[1].source = MountSource::Volume("../..".into());
        assert!(container_create(&spec).is_err());

        let mut spec = instance();
        spec.env.insert("A=B".into(), "x".into());
        assert!(container_create(&spec).is_err());
    }

    #[test]
    fn copies_stay_inside_the_container() {
        let args = copy_into(
            "harness-demo",
            &PathBuf::from("C:\\t\\in.bundle"),
            "/home/agent/x",
        )
        .unwrap();
        assert_eq!(
            args,
            ["cp", "C:\\t\\in.bundle", "harness-demo:/home/agent/x"].map(OsString::from)
        );
        assert!(copy_into("harness-demo", Path::new("x"), "relative").is_err());
        assert!(copy_into("harness-demo", Path::new("x"), "/a:b").is_err());
        assert!(copy_into("-x", Path::new("x"), "/a").is_err());

        let args = copy_out("harness-demo", "/home/agent/out.bundle").unwrap();
        assert_eq!(args[..2], ["exec", "harness-demo"]);
        assert_eq!(args.last().unwrap(), "/home/agent/out.bundle");
        assert!(copy_out("harness-demo", "/a/../b").is_err());
    }

    #[test]
    fn gateway_is_hardened_and_joins_both_networks() {
        let spec = GatewaySpec {
            name: "harness-gw-demo".into(),
            image: "localhost/harness/gateway:latest".into(),
            internal_network: "harness-net-demo".into(),
            alias: "gateway".into(),
            egress_network: "harness-egress".into(),
            labels: [("io.llm-harness.project".to_owned(), "demo".to_owned())].into(),
            command: vec![],
        };
        let args = gateway_create(&spec).unwrap();
        assert!(has_pair(
            &args,
            "--network",
            "harness-net-demo:alias=gateway"
        ));
        assert!(has_pair(&args, "--network", "harness-egress"));
        assert!(has_pair(&args, "--user", "65534:65534"));
        assert!(has_pair(&args, "--memory", "128m"));
        assert!(has_pair(&args, "--label", "io.llm-harness.managed=true"));
        for flag in [
            "--init",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--read-only",
        ] {
            assert!(args.iter().any(|a| a == flag), "missing {flag}");
        }
        assert!(
            !args
                .iter()
                .any(|a| a == "--mount" || a.contains("privileged"))
        );
        assert_eq!(args.last().unwrap(), "localhost/harness/gateway:latest");

        let mut same = spec.clone();
        same.egress_network = same.internal_network.clone();
        assert!(gateway_create(&same).is_err());
        let mut bad = spec;
        bad.alias = "gateway,x".into();
        assert!(gateway_create(&bad).is_err());
    }

    #[test]
    fn exec_args() {
        let spec = ExecSpec {
            container: "harness-demo".into(),
            command: vec!["pi".into(), "--help".into()],
            workdir: Some("/workspace".into()),
            env: [("TERM".to_owned(), "xterm".to_owned())].into(),
            interactive: true,
            tty: true,
        };
        assert_eq!(
            exec(&spec).unwrap(),
            [
                "exec",
                "--interactive",
                "--tty",
                "--workdir",
                "/workspace",
                "--env",
                "TERM=xterm",
                "harness-demo",
                "pi",
                "--help"
            ]
        );
        let empty = ExecSpec {
            container: "harness-demo".into(),
            ..ExecSpec::default()
        };
        assert!(exec(&empty).is_err());
    }

    #[test]
    fn network_and_volume_args() {
        let net = NetworkSpec {
            name: "harness-net-demo".into(),
            internal: true,
            labels: Labels::new(),
        };
        assert_eq!(
            network_create(&net).unwrap(),
            [
                "network",
                "create",
                "--internal",
                "--label",
                "io.llm-harness.managed=true",
                "harness-net-demo"
            ]
        );
        let vol = VolumeSpec {
            name: "harness-ws-demo".into(),
            labels: Labels::new(),
        };
        assert_eq!(
            volume_create(&vol).unwrap(),
            [
                "volume",
                "create",
                "--label",
                "io.llm-harness.managed=true",
                "harness-ws-demo"
            ]
        );
    }

    #[test]
    fn build_args() {
        let spec = ImageBuild {
            tag: "localhost/harness/base:latest".into(),
            context: PathBuf::from("ctx"),
            labels: [("io.llm-harness.context-hash".to_owned(), "abc".to_owned())].into(),
            build_args: [("NODE_MAJOR".to_owned(), "24".to_owned())].into(),
            pull: true,
            no_cache: false,
        };
        let args: Vec<String> = build(&spec)
            .unwrap()
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect();
        assert!(has_pair(&args, "--tag", "localhost/harness/base:latest"));
        assert!(has_pair(
            &args,
            "--label",
            "io.llm-harness.context-hash=abc"
        ));
        assert!(has_pair(&args, "--build-arg", "NODE_MAJOR=24"));
        assert!(args.iter().any(|a| a == "--pull=always"));
        assert_eq!(args.last().unwrap(), "ctx");
    }

    impl ContainerSpec {
        fn default_with_name() -> Self {
            ContainerSpec {
                name: "harness-demo".into(),
                image: "localhost/harness/base:latest".into(),
                network: "harness-net-demo".into(),
                ..ContainerSpec::default()
            }
        }
    }
}
