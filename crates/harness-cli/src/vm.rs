//! The VM behind every command that needs containers: started through the
//! VM service when it is installed, so the user never needs an admin
//! window for it.

use std::time::Duration;

use anyhow::bail;
use harness_core::podman::PodmanProvider;
use harness_core::tunnel::VmStatus;
use harness_core::vmd::{self, Request, VmState};

use crate::progress::{Progress, Terminal, step};

/// A provider for `machine`, starting the VM first if SSH does not answer.
pub fn provider(machine: &str) -> anyhow::Result<PodmanProvider> {
    provider_with(machine, &Terminal::default())
}

/// [`provider`], reporting a VM start to `progress`.
pub fn provider_with(machine: &str, progress: &dyn Progress) -> anyhow::Result<PodmanProvider> {
    let provider = PodmanProvider::for_machine(machine)?;
    let port = provider.machine().ssh_port;
    if vmd::ssh_answers(port, Duration::from_secs(3)) {
        return Ok(provider);
    }
    if !vmd::available() {
        bail!(
            "Die VM {machine} läuft nicht. Einmalig `harness vmd install` einrichten, dann startet \
             harness sie selbst. Sonst in einer Admin-PowerShell: podman machine start {machine}"
        );
    }
    step(
        progress,
        "vm",
        &format!("VM {machine} starten (bis zu einer Minute)"),
        || {
            let answer = vmd::request(Request::Start)?;
            if !answer.ok {
                bail!("VM-Start über den Dienst: {}", answer.message);
            }
            Ok(())
        },
    )?;
    Ok(provider)
}

/// The VM's state for the web app; asks the VM service, which is fast and
/// starts nothing.
pub fn status() -> VmStatus {
    if !vmd::available() {
        return VmStatus::Unknown;
    }
    match vmd::request(Request::Status) {
        Ok(answer) => match answer.state {
            VmState::Running => VmStatus::Running,
            VmState::Stopped => VmStatus::Stopped,
            VmState::Unresponsive | VmState::Unknown => VmStatus::Unknown,
        },
        Err(_) => VmStatus::Unknown,
    }
}
