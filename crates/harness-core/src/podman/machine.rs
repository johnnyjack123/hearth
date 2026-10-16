//! Reading the Podman machine JSON that `podman machine init` writes.
//!
//! The SSH port changes with every `init`, so it is always read from this file
//! and never stored anywhere else.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::paths::home_dir;
use crate::provider::validate_resource_name;
use crate::{Error, Result};

/// Name of the machine the harness manages.
pub const DEFAULT_MACHINE: &str = "harness";

/// Connection facts of a Podman machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineConfig {
    /// Machine name.
    pub name: String,
    /// Where the JSON was read from.
    pub path: PathBuf,
    /// SSH port forwarded to `127.0.0.1` on the host.
    pub ssh_port: u16,
    /// SSH user inside the VM, usually `core`.
    pub ssh_user: String,
    /// Private key podman generated for the machine.
    pub identity_path: PathBuf,
    /// UID of the SSH user inside the VM; locates the rootless socket.
    pub uid: u32,
    /// Whether the machine runs containers as root.
    pub rootful: bool,
    /// Host folders shared into the VM. Harness needs none: containers get
    /// their data through `podman cp`, and by default podman shares all of
    /// `C:\` and the user profile.
    pub mounts: Vec<MachineMount>,
}

/// A host folder shared into the VM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineMount {
    /// Folder on the host.
    pub source: PathBuf,
    /// Mount point inside the VM.
    pub target: String,
    /// Whether the VM sees it read-only.
    pub read_only: bool,
}

impl MachineConfig {
    /// Location of the JSON for a Hyper-V machine: the copy of the VM service
    /// (`harness-vmd`) if it owns the machine, else the user's own
    /// `%USERPROFILE%\.config\containers\podman\machine\hyperv\<name>.json`.
    pub fn default_path(name: &str) -> Result<PathBuf> {
        validate_resource_name("Machine-Name", name)?;
        let service = crate::vmd::machine_json(name);
        if service.is_file() {
            return Ok(service);
        }
        Ok(home_dir()?
            .join(".config")
            .join("containers")
            .join("podman")
            .join("machine")
            .join("hyperv")
            .join(format!("{name}.json")))
    }

    /// Loads the JSON of the named Hyper-V machine.
    pub fn load(name: &str) -> Result<Self> {
        Self::from_path(&Self::default_path(name)?)
    }

    /// Loads a machine JSON from an explicit path.
    pub fn from_path(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                Error::MachineConfigMissing {
                    path: path.to_owned(),
                }
            } else {
                Error::io(format!("{} lesen", path.display()), e)
            }
        })?;
        Self::parse(&text, path)
    }

    fn parse(text: &str, path: &Path) -> Result<Self> {
        let raw: RawMachine =
            serde_json::from_str(text).map_err(|source| Error::MachineConfigInvalid {
                path: path.to_owned(),
                source,
            })?;
        Ok(MachineConfig {
            name: raw.name,
            path: path.to_owned(),
            ssh_port: raw.ssh.port,
            ssh_user: raw.ssh.remote_username,
            identity_path: raw.ssh.identity_path,
            uid: raw.host_user.uid,
            rootful: raw.host_user.rootful,
            mounts: raw
                .mounts
                .unwrap_or_default()
                .into_iter()
                .map(|m| MachineMount {
                    source: m.source,
                    target: m.target,
                    read_only: m.read_only,
                })
                .collect(),
        })
    }

    /// The podman API socket inside the VM.
    pub fn socket_path(&self) -> String {
        if self.rootful {
            "/run/podman/podman.sock".to_owned()
        } else {
            format!("/run/user/{}/podman/podman.sock", self.uid)
        }
    }

    /// `ssh://` URL for `podman --url`.
    pub fn url(&self) -> String {
        format!(
            "ssh://{}@127.0.0.1:{}{}",
            self.ssh_user,
            self.ssh_port,
            self.socket_path()
        )
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawMachine {
    name: String,
    #[serde(rename = "SSH")]
    ssh: RawSsh,
    #[serde(default)]
    host_user: RawHostUser,
    // `null` when the machine has no shares, so not just `#[serde(default)]`.
    #[serde(default)]
    mounts: Option<Vec<RawMount>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawSsh {
    identity_path: PathBuf,
    port: u16,
    remote_username: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawHostUser {
    #[serde(default)]
    rootful: bool,
    #[serde(rename = "UID", default = "default_uid")]
    uid: u32,
}

impl Default for RawHostUser {
    fn default() -> Self {
        RawHostUser {
            rootful: false,
            uid: default_uid(),
        }
    }
}

fn default_uid() -> u32 {
    1000
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawMount {
    source: PathBuf,
    target: String,
    #[serde(default)]
    read_only: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed copy of a real machine JSON written by podman 5.8.3 (Hyper-V).
    const SAMPLE: &str = r#"{
        "Created": "2026-10-12T12:41:10.151706+02:00",
        "HostUser": { "Rootful": false, "UID": 1000, "HostUserModified": false },
        "Mounts": [{
            "OriginalInput": "C:\\Users\\u\\AppData\\Local\\llm-harness\\exchange:/mnt/exchange",
            "ReadOnly": false,
            "Source": "C:\\Users\\u\\AppData\\Local\\llm-harness\\exchange",
            "Tag": "vol0", "Target": "/mnt/exchange", "Type": "9p", "VSockNumber": 64070
        }],
        "Name": "harness",
        "Resources": { "CPUs": 4, "DiskSize": 40, "Memory": 4096, "USBs": [] },
        "SSH": {
            "IdentityPath": "C:\\Users\\u\\.local\\share\\containers\\podman\\machine\\machine",
            "Port": 51747,
            "RemoteUsername": "core"
        }
    }"#;

    #[test]
    fn parses_real_machine_json() {
        let machine = MachineConfig::parse(SAMPLE, Path::new("harness.json")).unwrap();
        assert_eq!(machine.name, "harness");
        assert_eq!(machine.ssh_port, 51747);
        assert_eq!(
            machine.url(),
            "ssh://core@127.0.0.1:51747/run/user/1000/podman/podman.sock"
        );
        assert_eq!(machine.mounts.len(), 1);
        assert_eq!(machine.mounts[0].target, "/mnt/exchange");
    }

    #[test]
    fn null_mounts() {
        let no_mounts = SAMPLE.replacen(r#""Mounts": ["#, r#""Unused": ["#, 1);
        let no_mounts = no_mounts.replacen(r#""Name""#, r#""Mounts": null, "Name""#, 1);
        let machine = MachineConfig::parse(&no_mounts, Path::new("x.json")).unwrap();
        assert!(machine.mounts.is_empty());
    }

    #[test]
    fn rootful_socket() {
        let rootful = SAMPLE.replace(r#""Rootful": false"#, r#""Rootful": true"#);
        let machine = MachineConfig::parse(&rootful, Path::new("x.json")).unwrap();
        assert_eq!(machine.socket_path(), "/run/podman/podman.sock");
    }

    #[test]
    fn missing_file_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let err = MachineConfig::from_path(&dir.path().join("nope.json")).unwrap_err();
        assert!(matches!(err, Error::MachineConfigMissing { .. }));
    }
}
