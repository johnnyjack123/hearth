//! The VM behind every command that needs containers: started through the
//! VM service when it is installed, so the user never needs an admin
//! window for it.

use std::io::Write as _;
use std::time::{Duration, Instant};

use anyhow::bail;
use harness_core::podman::PodmanProvider;
use harness_core::vmd::{self, Request};

/// A provider for `machine`, starting the VM first if SSH does not answer.
pub fn provider(machine: &str) -> anyhow::Result<PodmanProvider> {
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
    print!("VM {machine} startet … ");
    let _ = std::io::stdout().flush();
    let started = Instant::now();
    let answer = vmd::request(Request::Start)?;
    if !answer.ok {
        println!("✗");
        bail!("VM-Start über den Dienst: {}", answer.message);
    }
    println!("✓ ({:.0} s)", started.elapsed().as_secs_f32());
    Ok(provider)
}
