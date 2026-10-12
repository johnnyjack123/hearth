//! Provider for a Podman machine (Hyper-V on Windows), driven through the
//! `podman` CLI over SSH.
//!
//! Connection handling follows `spikes/phase-0/pm.sh`: the user's
//! `~/.ssh/known_hosts` contains lines podman's Go SSH client cannot parse
//! ("missing host pattern"), so podman runs with an isolated `HOME` whose
//! `known_hosts` only holds the machine's host key. URL and key are passed
//! explicitly instead of relying on podman's connection list.

mod args;
pub mod machine;

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};
use std::time::Duration;

use serde::Deserialize;

pub use machine::{DEFAULT_MACHINE, EXCHANGE_GUEST_DIR, MachineConfig, MachineMount};

use crate::paths::local_data_dir;
use crate::provider::{
    ContainerInfo, ContainerSpec, ContainerStatus, ExchangeDir, ExecOutput, ExecSpec, HostInfo,
    ImageBuild, Labels, NetworkSpec, Provider, VolumeSpec, validate_image_ref,
    validate_resource_name,
};
use crate::{Error, Result};

const SSH_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// [`Provider`] backed by a Podman machine.
#[derive(Debug, Clone)]
pub struct PodmanProvider {
    podman: PathBuf,
    machine: MachineConfig,
    exchange: Option<ExchangeDir>,
    home: PathBuf,
}

impl PodmanProvider {
    /// Provider for the named Hyper-V machine, using the installed podman and
    /// `%LOCALAPPDATA%\llm-harness\podman` for connection state.
    pub fn for_machine(name: &str) -> Result<Self> {
        let machine = MachineConfig::load(name)?;
        Self::new(machine, find_podman()?, &local_data_dir()?.join("podman"))
    }

    /// Provider for an already loaded machine config.
    ///
    /// `state_dir` holds one isolated podman home per machine and SSH port; a
    /// re-initialised machine gets a new port and therefore a fresh
    /// `known_hosts` instead of a host key mismatch.
    pub fn new(machine: MachineConfig, podman: PathBuf, state_dir: &Path) -> Result<Self> {
        validate_resource_name("Machine-Name", &machine.name)?;
        if machine.rootful {
            return Err(Error::Unsupported(format!(
                "Machine {} läuft rootful. Die Harness braucht rootless Podman",
                machine.name
            )));
        }
        let home = state_dir.join(format!("{}-{}", machine.name, machine.ssh_port));
        Ok(PodmanProvider {
            podman,
            exchange: machine.exchange(),
            machine,
            home,
        })
    }

    /// The machine this provider talks to.
    pub fn machine(&self) -> &MachineConfig {
        &self.machine
    }

    /// The podman executable in use.
    pub fn podman_path(&self) -> &Path {
        &self.podman
    }

    fn probe_ssh_port(&self) -> Result<()> {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, self.machine.ssh_port));
        TcpStream::connect_timeout(&addr, SSH_PROBE_TIMEOUT)
            .map(drop)
            .map_err(|source| Error::VmUnreachable {
                addr: addr.to_string(),
                source,
            })
    }

    fn known_hosts(&self) -> PathBuf {
        self.home.join(".ssh").join("known_hosts")
    }

    /// Records the machine's host key on first use (trust on first use).
    ///
    /// The port is only bound on `127.0.0.1` by gvproxy, so the first answer
    /// comes from a local process; after that the key is pinned.
    fn ensure_known_hosts(&self) -> Result<()> {
        let file = self.known_hosts();
        if fs::metadata(&file).is_ok_and(|m| m.len() > 0) {
            return Ok(());
        }
        self.probe_ssh_port()?;
        let keyscan = find_keyscan();
        let output = Command::new(&keyscan)
            .args(["-T", "5", "-p"])
            .arg(self.machine.ssh_port.to_string())
            .arg(Ipv4Addr::LOCALHOST.to_string())
            .stdin(Stdio::null())
            .output()
            .map_err(|source| Error::Spawn {
                program: keyscan.clone(),
                source,
            })?;
        let keys: String = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
            .map(|line| format!("{line}\n"))
            .collect();
        if keys.is_empty() {
            return Err(Error::UnexpectedOutput {
                action: format!("{} (ssh-keyscan)", keyscan.display()),
                detail: "kein Host-Key erhalten".to_owned(),
            });
        }
        let dir = file.parent().unwrap_or(&self.home);
        fs::create_dir_all(dir).map_err(|e| Error::io(format!("{} anlegen", dir.display()), e))?;
        // Write-then-rename so a concurrent harness process never sees a
        // half-written file.
        let partial = file.with_extension("partial");
        fs::write(&partial, keys)
            .map_err(|e| Error::io(format!("{} schreiben", partial.display()), e))?;
        fs::rename(&partial, &file)
            .map_err(|e| Error::io(format!("{} schreiben", file.display()), e))
    }

    fn command(&self) -> Result<Command> {
        self.ensure_known_hosts()?;
        let mut cmd = Command::new(&self.podman);
        cmd.arg("--url")
            .arg(self.machine.url())
            .arg("--identity")
            .arg(&self.machine.identity_path)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            // Explicit flags win anyway; removing these keeps a user's own
            // podman setup from leaking into error messages and behaviour.
            .env_remove("CONTAINER_HOST")
            .env_remove("CONTAINER_CONNECTION")
            .env_remove("CONTAINER_SSHKEY");
        Ok(cmd)
    }

    fn output<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<Output> {
        let mut cmd = self.command()?;
        cmd.args(args).stdin(Stdio::null());
        cmd.output().map_err(|source| Error::Spawn {
            program: self.podman.clone(),
            source,
        })
    }

    fn status_inherited<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<ExitStatus> {
        let mut cmd = self.command()?;
        cmd.args(args);
        cmd.status().map_err(|source| Error::Spawn {
            program: self.podman.clone(),
            source,
        })
    }

    /// Runs podman and returns stdout; a non-zero exit becomes an error that
    /// names only `action`, never the arguments.
    fn run<S: AsRef<OsStr>>(&self, action: &str, args: &[S]) -> Result<String> {
        let output = self.output(args)?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(command_error(action, &output))
        }
    }

    /// For `podman … exists`: exit code 0 means yes, 1 means no.
    fn exists(&self, action: &str, args: &[&str]) -> Result<bool> {
        let output = self.output(args)?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(command_error(action, &output)),
        }
    }

    fn network_internal(&self, name: &str) -> Result<Option<bool>> {
        if !self.exists("network exists", &["network", "exists", name])? {
            return Ok(None);
        }
        let json = self.run("network inspect", &["network", "inspect", name])?;
        let networks: Vec<RawNetwork> =
            serde_json::from_str(&json).map_err(|e| unexpected("network inspect", e))?;
        networks
            .first()
            .map(|n| Some(n.internal))
            .ok_or_else(|| Error::UnexpectedOutput {
                action: "network inspect".to_owned(),
                detail: "leere Antwort".to_owned(),
            })
    }
}

impl Provider for PodmanProvider {
    fn name(&self) -> &'static str {
        "podman"
    }

    fn ping(&self) -> Result<HostInfo> {
        self.probe_ssh_port()?;
        let json = self.run("info", &["info", "--format", "json"])?;
        parse_info(&json)
    }

    fn image_labels(&self, image: &str) -> Result<Option<Labels>> {
        validate_image_ref(image)?;
        if !self.exists("image exists", &["image", "exists", image])? {
            return Ok(None);
        }
        let json = self.run("image inspect", &["image", "inspect", image])?;
        let images: Vec<RawImage> =
            serde_json::from_str(&json).map_err(|e| unexpected("image inspect", e))?;
        Ok(Some(
            images
                .into_iter()
                .next()
                .and_then(|i| i.labels)
                .unwrap_or_default(),
        ))
    }

    fn build_image(&self, build: &ImageBuild) -> Result<()> {
        if !build.context.join("Containerfile").is_file() {
            return Err(Error::Unsupported(format!(
                "Kein Containerfile in {}",
                build.context.display()
            )));
        }
        let args = args::build(build)?;
        let status = self.status_inherited(&args)?;
        if status.success() {
            Ok(())
        } else {
            Err(Error::Command {
                action: "build".to_owned(),
                code: status.code(),
                stderr: String::new(),
            })
        }
    }

    fn ensure_volume(&self, volume: &VolumeSpec) -> Result<()> {
        let args = args::volume_create(volume)?;
        if !self.exists("volume exists", &["volume", "exists", &volume.name])? {
            self.run("volume create", &args)?;
        }
        Ok(())
    }

    fn remove_volume(&self, name: &str) -> Result<()> {
        validate_resource_name("Volume", name)?;
        if self.exists("volume exists", &["volume", "exists", name])? {
            self.run("volume rm", &["volume", "rm", name])?;
        }
        Ok(())
    }

    fn ensure_network(&self, network: &NetworkSpec) -> Result<()> {
        let args = args::network_create(network)?;
        match self.network_internal(&network.name)? {
            None => {
                self.run("network create", &args)?;
                Ok(())
            }
            Some(internal) if internal == network.internal => Ok(()),
            Some(_) if network.internal => Err(Error::NetworkNotInternal(network.name.clone())),
            Some(_) => Err(Error::Unsupported(format!(
                "Netz {} existiert bereits als internes Netz",
                network.name
            ))),
        }
    }

    fn remove_network(&self, name: &str) -> Result<()> {
        validate_resource_name("Netz", name)?;
        if self.exists("network exists", &["network", "exists", name])? {
            self.run("network rm", &["network", "rm", name])?;
        }
        Ok(())
    }

    fn container(&self, name: &str) -> Result<Option<ContainerInfo>> {
        validate_resource_name("Containername", name)?;
        // The name filter is a regex; the exact comparison below makes the
        // `.` in names harmless.
        let filter = format!("name=^{name}$");
        let json = self.run(
            "ps",
            &["ps", "--all", "--filter", &filter, "--format", "json"],
        )?;
        let containers: Vec<RawContainer> =
            serde_json::from_str(&json).map_err(|e| unexpected("ps", e))?;
        Ok(containers
            .into_iter()
            .find(|c| c.names.iter().any(|n| n == name))
            .map(|c| ContainerInfo {
                name: name.to_owned(),
                status: ContainerStatus::parse(&c.state),
                image: c.image,
                labels: c.labels.unwrap_or_default(),
            }))
    }

    fn create_container(&self, spec: &ContainerSpec) -> Result<()> {
        let args = args::container_create(spec, self.exchange.as_ref())?;
        match self.network_internal(&spec.network)? {
            Some(true) => {}
            Some(false) => return Err(Error::NetworkNotInternal(spec.network.clone())),
            None => {
                return Err(Error::Unsupported(format!(
                    "Netz {} existiert nicht",
                    spec.network
                )));
            }
        }
        self.run("create", &args)?;
        Ok(())
    }

    fn start_container(&self, name: &str) -> Result<()> {
        validate_resource_name("Containername", name)?;
        self.run("start", &["start", name])?;
        Ok(())
    }

    fn stop_container(&self, name: &str) -> Result<()> {
        validate_resource_name("Containername", name)?;
        self.run(
            "stop",
            &["stop", "--ignore", "--time", args::STOP_TIMEOUT, name],
        )?;
        Ok(())
    }

    fn remove_container(&self, name: &str) -> Result<()> {
        validate_resource_name("Containername", name)?;
        self.run(
            "rm",
            &[
                "rm",
                "--force",
                "--ignore",
                "--time",
                args::STOP_TIMEOUT,
                name,
            ],
        )?;
        Ok(())
    }

    fn exec(&self, exec: &ExecSpec) -> Result<i32> {
        let args = args::exec(exec)?;
        let status = self.status_inherited(&args)?;
        // A missing code means the process was killed; report it like a shell.
        Ok(status.code().unwrap_or(137))
    }

    fn exec_output(&self, exec: &ExecSpec) -> Result<ExecOutput> {
        if exec.tty || exec.interactive {
            return Err(Error::Unsupported(
                "exec_output unterstützt weder TTY noch stdin".to_owned(),
            ));
        }
        let output = self.output(&args::exec(exec)?)?;
        Ok(ExecOutput {
            code: output.status.code().unwrap_or(137),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn exchange(&self) -> Result<&ExchangeDir> {
        self.exchange
            .as_ref()
            .ok_or_else(|| Error::ExchangeMissing(EXCHANGE_GUEST_DIR.to_owned()))
    }
}

/// Locates `podman`: `HARNESS_PODMAN`, then `PATH`, then the default install
/// location of the Windows installer.
pub fn find_podman() -> Result<PathBuf> {
    if let Some(path) = env::var_os("HARNESS_PODMAN") {
        return Ok(PathBuf::from(path));
    }
    let exe = if cfg!(windows) {
        "podman.exe"
    } else {
        "podman"
    };
    if let Some(path) = find_on_path(exe) {
        return Ok(path);
    }
    let program_files = env::var_os("ProgramFiles").map(PathBuf::from);
    if let Some(path) = program_files
        .map(|dir| dir.join("RedHat").join("Podman").join(exe))
        .filter(|p| p.is_file())
    {
        return Ok(path);
    }
    Err(Error::Unsupported(
        "podman nicht gefunden (PATH, %ProgramFiles%\\RedHat\\Podman). Pfad über HARNESS_PODMAN setzen"
            .to_owned(),
    ))
}

/// Prefers the Windows OpenSSH client over whatever `PATH` offers (e.g. Git
/// Bash's), so behaviour does not depend on the shell the CLI runs in.
fn find_keyscan() -> PathBuf {
    if let Some(dir) = env::var_os("WINDIR") {
        let path = PathBuf::from(dir)
            .join("System32")
            .join("OpenSSH")
            .join("ssh-keyscan.exe");
        if path.is_file() {
            return path;
        }
    }
    let exe = if cfg!(windows) {
        "ssh-keyscan.exe"
    } else {
        "ssh-keyscan"
    };
    find_on_path(exe).unwrap_or_else(|| PathBuf::from(exe))
}

fn find_on_path(exe: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|dir| dir.join(exe))
        .find(|p| p.is_file())
}

fn command_error(action: &str, output: &Output) -> Error {
    Error::Command {
        action: action.to_owned(),
        code: output.status.code(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    }
}

fn unexpected(action: &str, error: serde_json::Error) -> Error {
    Error::UnexpectedOutput {
        action: action.to_owned(),
        detail: error.to_string(),
    }
}

fn parse_info(json: &str) -> Result<HostInfo> {
    let raw: RawInfo = serde_json::from_str(json).map_err(|e| unexpected("info", e))?;
    Ok(HostInfo {
        arch: raw.host.arch,
        distribution: raw.host.distribution.distribution,
        distribution_version: raw.host.distribution.version,
        kernel: raw.host.kernel,
        engine_version: raw.version.version,
        rootless: raw.host.security.rootless,
        mem_total: raw.host.mem_total,
        mem_free: raw.host.mem_free,
    })
}

#[derive(Deserialize)]
struct RawInfo {
    host: RawInfoHost,
    version: RawInfoVersion,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawInfoHost {
    arch: String,
    distribution: RawDistribution,
    #[serde(default)]
    kernel: String,
    mem_total: u64,
    mem_free: u64,
    security: RawSecurity,
}

#[derive(Deserialize)]
struct RawDistribution {
    distribution: String,
    #[serde(default)]
    version: String,
}

#[derive(Deserialize)]
struct RawSecurity {
    rootless: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawInfoVersion {
    version: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawImage {
    labels: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize)]
struct RawNetwork {
    #[serde(default)]
    internal: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawContainer {
    names: Vec<String>,
    state: String,
    image: String,
    labels: Option<BTreeMap<String, String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_info() {
        // Shape of `podman info --format json` (5.8, trimmed).
        let json = r#"{
            "host": {
                "arch": "arm64",
                "distribution": {"distribution": "fedora", "variant": "coreos", "version": "44"},
                "kernel": "7.1.0",
                "memFree": 235978752,
                "memTotal": 981479424,
                "security": {"rootless": true, "seccompEnabled": true}
            },
            "store": {},
            "version": {"APIVersion": "5.8.8", "Version": "5.8.8"}
        }"#;
        let info = parse_info(json).unwrap();
        assert_eq!(info.arch, "arm64");
        assert_eq!(info.distribution, "fedora");
        assert_eq!(info.engine_version, "5.8.8");
        assert!(info.rootless);
        assert_eq!(info.mem_total, 981_479_424);
    }

    #[test]
    fn parses_ps_and_network() {
        let ps = r#"[{"Id":"abc","Names":["harness-demo"],"State":"running",
            "Image":"localhost/harness/base:latest","Labels":{"io.llm-harness.managed":"true"}}]"#;
        let containers: Vec<RawContainer> = serde_json::from_str(ps).unwrap();
        assert_eq!(containers[0].names, ["harness-demo"]);
        assert_eq!(containers[0].state, "running");

        let net =
            r#"[{"name":"harness-net-demo","driver":"bridge","internal":true,"dns_enabled":true}]"#;
        let networks: Vec<RawNetwork> = serde_json::from_str(net).unwrap();
        assert!(networks[0].internal);
    }

    #[test]
    fn rootful_machines_are_refused() {
        let machine = MachineConfig {
            name: "harness".into(),
            path: PathBuf::from("harness.json"),
            ssh_port: 1,
            ssh_user: "root".into(),
            identity_path: PathBuf::from("key"),
            uid: 0,
            rootful: true,
            mounts: vec![],
        };
        let err =
            PodmanProvider::new(machine, PathBuf::from("podman"), Path::new("state")).unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
    }
}
