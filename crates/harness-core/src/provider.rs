//! The sandbox provider abstraction.
//!
//! A provider owns "where containers run": today only [`crate::podman::PodmanProvider`]
//! (Podman machine on Hyper-V), later possibly Docker on Linux or others
//! (ARCHITECTURE.md §3). Everything above this trait (instances, sync, CLI)
//! must stay provider-agnostic.
//!
//! Security invariants that every provider enforces itself, so callers cannot
//! forget them:
//! - containers are always created hardened (no capabilities, read-only root,
//!   no new privileges, fixed unprivileged user, pid limit);
//! - containers only join internal networks. The single exception is the
//!   gateway ([`Provider::create_gateway`]): it joins one internal network
//!   and one normal one, gets no mounts, and only runs our gateway image;
//! - containers see no host folders; data moves only through
//!   [`Provider::copy_into`] and [`Provider::copy_out`]. A host folder shared
//!   into the VM would need a file server on the host, which in the
//!   privileged VM service (`harness-vmd`) would serve whatever the user's
//!   processes point it at;
//! - every created resource carries [`crate::MANAGED_LABEL`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Child;

use crate::{Error, Result};

/// Key/value labels attached to images, containers, volumes and networks.
pub type Labels = BTreeMap<String, String>;

/// A place to run sandboxed containers.
pub trait Provider {
    /// Short identifier for logs and `doctor`, e.g. `podman`.
    fn name(&self) -> &'static str;

    /// Checks that the VM (or engine) is reachable and returns facts about it.
    fn ping(&self) -> Result<HostInfo>;

    /// Labels of a local image, or `None` if the image does not exist.
    fn image_labels(&self, image: &str) -> Result<Option<Labels>>;

    /// Builds an image from a context directory on the host. Output is shown
    /// to the user as it happens.
    fn build_image(&self, build: &ImageBuild) -> Result<()>;

    /// Creates the volume unless it exists.
    fn ensure_volume(&self, volume: &VolumeSpec) -> Result<()>;

    /// Removes a volume if it exists. Fails while a container still uses it.
    fn remove_volume(&self, name: &str) -> Result<()>;

    /// Creates the network unless it exists. An existing network whose
    /// internal flag differs from the spec is an error, never silently reused.
    fn ensure_network(&self, network: &NetworkSpec) -> Result<()>;

    /// Removes a network if it exists. Fails while a container still uses it.
    fn remove_network(&self, name: &str) -> Result<()>;

    /// Looks up a container by exact name.
    fn container(&self, name: &str) -> Result<Option<ContainerInfo>>;

    /// All running containers, also those harness did not create.
    fn running_containers(&self) -> Result<Vec<ContainerInfo>>;

    /// Creates (but does not start) a hardened container.
    fn create_container(&self, spec: &ContainerSpec) -> Result<()>;

    /// Creates (but does not start) the egress gateway of a project: in the
    /// internal network under an alias, plus one normal network with a route
    /// out. Fails unless the first network is internal and the second is not.
    fn create_gateway(&self, spec: &GatewaySpec) -> Result<()>;

    /// Starts a created or stopped container. Starting a running one is a no-op.
    fn start_container(&self, name: &str) -> Result<()>;

    /// Stops a container if it exists and is running.
    fn stop_container(&self, name: &str) -> Result<()>;

    /// Removes a container if it exists, stopping it first.
    fn remove_container(&self, name: &str) -> Result<()>;

    /// Runs a command in a running container attached to this process's
    /// stdio (with a TTY if requested) and returns its exit code.
    fn exec(&self, exec: &ExecSpec) -> Result<i32>;

    /// The process [`Provider::exec`] would start, for callers that attach
    /// it to a console of their own (`pty`). Its stdio settings are unset.
    fn exec_command(&self, exec: &ExecSpec) -> Result<std::process::Command>;

    /// Runs a command in a running container and captures its output.
    fn exec_output(&self, exec: &ExecSpec) -> Result<ExecOutput>;

    /// Runs a command in a running container with stdin and stdout piped to
    /// the returned child (stderr goes to this process's stderr). For
    /// long-lived line protocols such as the gateway's `ctl`.
    fn exec_piped(&self, exec: &ExecSpec) -> Result<Child>;

    /// Copies a host file into a running container; the directory must
    /// exist there. With [`Provider::copy_out`] the only way data moves
    /// between host and containers.
    fn copy_into(&self, container: &str, host_file: &Path, path: &str) -> Result<()>;

    /// Writes a regular file of a running container to `host_file`. Fails if
    /// it is missing, no regular file, or larger than `max_bytes`. Whatever
    /// arrives is untrusted.
    fn copy_out(&self, container: &str, path: &str, host_file: &Path, max_bytes: u64)
    -> Result<()>;

    /// Address under which containers in normal networks (the gateway)
    /// reach services on the host's loopback interface, if any.
    fn host_address(&self) -> Option<&str>;
}

/// Facts about the VM, reported by [`Provider::ping`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostInfo {
    /// CPU architecture, e.g. `arm64`.
    pub arch: String,
    /// Distribution name, e.g. `fedora`.
    pub distribution: String,
    /// Distribution version, e.g. `44`.
    pub distribution_version: String,
    /// Kernel release.
    pub kernel: String,
    /// Version of the container engine inside the VM.
    pub engine_version: String,
    /// Whether containers run rootless.
    pub rootless: bool,
    /// Total memory of the VM in bytes.
    pub mem_total: u64,
    /// Free memory of the VM in bytes.
    pub mem_free: u64,
}

/// Parameters for [`Provider::build_image`].
#[derive(Debug, Clone, Default)]
pub struct ImageBuild {
    /// Fully qualified tag, e.g. `localhost/harness/base:latest`.
    pub tag: String,
    /// Build context on the host; must contain a `Containerfile`.
    pub context: PathBuf,
    /// Labels added to the image.
    pub labels: Labels,
    /// `--build-arg` values.
    pub build_args: BTreeMap<String, String>,
    /// Always pull newer base images instead of using cached ones.
    pub pull: bool,
    /// Ignore the layer cache.
    pub no_cache: bool,
}

/// Parameters for [`Provider::ensure_volume`].
#[derive(Debug, Clone, Default)]
pub struct VolumeSpec {
    /// Volume name.
    pub name: String,
    /// Extra labels; the managed label is added automatically.
    pub labels: Labels,
}

/// Parameters for [`Provider::ensure_network`].
#[derive(Debug, Clone, Default)]
pub struct NetworkSpec {
    /// Network name.
    pub name: String,
    /// Internal networks have no route to the outside and no external DNS.
    pub internal: bool,
    /// Extra labels; the managed label is added automatically.
    pub labels: Labels,
}

/// Parameters for [`Provider::create_container`].
///
/// Hardening is not configurable here on purpose, see the module docs.
#[derive(Debug, Clone, Default)]
pub struct ContainerSpec {
    /// Container name, also used as hostname.
    pub name: String,
    /// Image reference.
    pub image: String,
    /// The single network the container joins; must be internal.
    pub network: String,
    /// Volumes.
    pub mounts: Vec<Mount>,
    /// Environment variables.
    pub env: BTreeMap<String, String>,
    /// Extra labels; the managed label is added automatically.
    pub labels: Labels,
    /// Working directory, absolute path inside the container.
    pub workdir: Option<String>,
    /// CPU, memory and process limits.
    pub resources: Resources,
    /// Command override; empty keeps the image's `CMD`.
    pub command: Vec<String>,
}

/// Parameters for [`Provider::create_gateway`]. Hardening and limits are
/// fixed by the provider.
#[derive(Debug, Clone, Default)]
pub struct GatewaySpec {
    /// Container name.
    pub name: String,
    /// Image reference.
    pub image: String,
    /// Internal network of the project's instances.
    pub internal_network: String,
    /// Name the instances reach the gateway under (`HTTP_PROXY`).
    pub alias: String,
    /// Normal network with a route to the outside.
    pub egress_network: String,
    /// Extra labels; the managed label is added automatically.
    pub labels: Labels,
    /// Command override; empty keeps the image's `CMD`.
    pub command: Vec<String>,
}

/// Resource limits of a container.
#[derive(Debug, Clone, PartialEq)]
pub struct Resources {
    /// CPU limit, e.g. `2.0`. `None` means no limit.
    pub cpus: Option<f64>,
    /// Memory limit in podman notation, e.g. `2g`. `None` means no limit.
    pub memory: Option<String>,
    /// Maximum number of processes; guards the VM against fork bombs.
    pub pids_limit: u32,
}

impl Default for Resources {
    fn default() -> Self {
        Resources {
            cpus: None,
            memory: None,
            pids_limit: 1024,
        }
    }
}

/// A mount into a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    /// What is mounted.
    pub source: MountSource,
    /// Absolute path inside the container.
    pub target: String,
    /// Mount read-only.
    pub read_only: bool,
}

/// Source of a [`Mount`]. Arbitrary host or VM paths are deliberately not
/// representable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountSource {
    /// A named volume inside the VM.
    Volume(String),
}

/// A container as reported by [`Provider::container`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerInfo {
    /// Container name.
    pub name: String,
    /// Lifecycle state.
    pub status: ContainerStatus,
    /// Image the container was created from.
    pub image: String,
    /// Container labels.
    pub labels: Labels,
}

/// Lifecycle state of a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerStatus {
    /// Created, never started.
    Created,
    /// Running.
    Running,
    /// Paused.
    Paused,
    /// Stopped or exited.
    Exited,
    /// Any state this crate does not model (e.g. `removing`).
    Other(String),
}

impl ContainerStatus {
    /// Maps an engine state string to a status.
    pub fn parse(state: &str) -> Self {
        match state.to_ascii_lowercase().as_str() {
            "created" | "configured" | "initialized" => ContainerStatus::Created,
            "running" => ContainerStatus::Running,
            "paused" => ContainerStatus::Paused,
            "exited" | "stopped" => ContainerStatus::Exited,
            other => ContainerStatus::Other(other.to_owned()),
        }
    }
}

/// Parameters for [`Provider::exec`] and [`Provider::exec_output`].
#[derive(Debug, Clone, Default)]
pub struct ExecSpec {
    /// Container to run in.
    pub container: String,
    /// Program and arguments; passed as argv, never through a shell.
    pub command: Vec<String>,
    /// Working directory, absolute path inside the container.
    pub workdir: Option<String>,
    /// Environment variables for this command only.
    pub env: BTreeMap<String, String>,
    /// Keep stdin open (`-i`).
    pub interactive: bool,
    /// Allocate a TTY (`-t`). Only valid with [`Provider::exec`].
    pub tty: bool,
}

/// Captured result of [`Provider::exec_output`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    /// Exit code of the command (125-127 are engine errors).
    pub code: i32,
    /// Standard output, lossily decoded as UTF-8.
    pub stdout: String,
    /// Standard error, lossily decoded as UTF-8.
    pub stderr: String,
}

/// Checks a container, volume or network name.
///
/// Podman allows `[a-zA-Z0-9][a-zA-Z0-9_.-]*`; the first character rule also
/// keeps a name from being parsed as a command-line flag. The length cap keeps
/// names usable as hostnames.
pub fn validate_resource_name(what: &'static str, name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 63 {
        return Err(Error::invalid(
            what,
            name,
            "muss 1 bis 63 Zeichen lang sein",
        ));
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap_or_default();
    if !first.is_ascii_alphanumeric() {
        return Err(Error::invalid(
            what,
            name,
            "muss mit einem Buchstaben oder einer Ziffer beginnen",
        ));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')) {
        return Err(Error::invalid(
            what,
            name,
            "darf nur Buchstaben, Ziffern, '_', '.' und '-' enthalten",
        ));
    }
    Ok(())
}

pub(crate) fn validate_image_ref(image: &str) -> Result<()> {
    const WHAT: &str = "Image";
    if image.is_empty() || image.starts_with('-') {
        return Err(Error::invalid(WHAT, image, "ist leer oder beginnt mit '-'"));
    }
    if !image
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | ':' | '@'))
    {
        return Err(Error::invalid(WHAT, image, "enthält unzulässige Zeichen"));
    }
    Ok(())
}

/// Absolute path inside a container. Commas are rejected because they would
/// inject options into `--mount` arguments.
pub(crate) fn validate_container_path(what: &'static str, path: &str) -> Result<()> {
    if !path.starts_with('/') {
        return Err(Error::invalid(what, path, "muss absolut sein"));
    }
    if path.chars().any(|c| c == ',' || c.is_control()) {
        return Err(Error::invalid(
            what,
            path,
            "darf keine Kommas oder Steuerzeichen enthalten",
        ));
    }
    if path.split('/').any(|part| part == "..") {
        return Err(Error::invalid(what, path, "darf kein '..' enthalten"));
    }
    Ok(())
}

pub(crate) fn validate_env_key(key: &str) -> Result<()> {
    let mut chars = key.chars();
    let valid = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if valid {
        Ok(())
    } else {
        Err(Error::invalid(
            "Umgebungsvariable",
            key,
            "erlaubt sind Buchstaben, Ziffern und '_', nicht am Anfang eine Ziffer",
        ))
    }
}

pub(crate) fn validate_label(key: &str, value: &str) -> Result<()> {
    let key_valid = !key.is_empty()
        && !key.starts_with('-')
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'));
    if !key_valid {
        return Err(Error::invalid("Label", key, "ungültiger Schlüssel"));
    }
    if value.chars().any(char::is_control) {
        return Err(Error::invalid("Label", value, "Wert enthält Steuerzeichen"));
    }
    Ok(())
}

pub(crate) fn validate_memory(memory: &str) -> Result<()> {
    let digits = memory.trim_end_matches(['b', 'k', 'm', 'g']);
    let suffix_len = memory.len() - digits.len();
    if digits.is_empty() || suffix_len > 1 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(Error::invalid(
            "Speicherlimit",
            memory,
            "erwartet z. B. 512m oder 2g",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_names() {
        assert!(validate_resource_name("x", "harness-foo_1.2").is_ok());
        for bad in ["", "-rm", ".hidden", "a b", "a/b", "ä", &"a".repeat(64)] {
            assert!(
                validate_resource_name("x", bad).is_err(),
                "{bad:?} accepted"
            );
        }
    }

    #[test]
    fn image_refs() {
        assert!(validate_image_ref("localhost/harness/base:latest").is_ok());
        assert!(validate_image_ref("docker.io/library/alpine@sha256:abc").is_ok());
        assert!(validate_image_ref("--privileged").is_err());
        assert!(validate_image_ref("alpine latest").is_err());
    }

    #[test]
    fn container_paths() {
        assert!(validate_container_path("x", "/workspace").is_ok());
        assert!(validate_container_path("x", "workspace").is_err());
        assert!(validate_container_path("x", "/a,readonly=false").is_err());
        assert!(validate_container_path("x", "/a/../etc").is_err());
    }

    #[test]
    fn env_keys_and_memory() {
        assert!(validate_env_key("HTTP_PROXY").is_ok());
        assert!(validate_env_key("_x1").is_ok());
        assert!(validate_env_key("1X").is_err());
        assert!(validate_env_key("A=B").is_err());
        assert!(validate_memory("2g").is_ok());
        assert!(validate_memory("512m").is_ok());
        assert!(validate_memory("1024").is_ok());
        assert!(validate_memory("g").is_err());
        assert!(validate_memory("2gb").is_err());
        assert!(validate_memory("2 g").is_err());
    }

    #[test]
    fn status_parsing() {
        assert_eq!(ContainerStatus::parse("Running"), ContainerStatus::Running);
        assert_eq!(ContainerStatus::parse("exited"), ContainerStatus::Exited);
        assert_eq!(
            ContainerStatus::parse("removing"),
            ContainerStatus::Other("removing".into())
        );
    }
}
