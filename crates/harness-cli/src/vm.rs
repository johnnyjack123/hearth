//! The VM behind every command that needs containers: started through the
//! VM service when it is installed, so the user never needs an admin
//! window for it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::bail;
use harness_core::busy::{self, Marker};
use harness_core::podman::PodmanProvider;
use harness_core::tunnel::VmStatus;
use harness_core::vmd::{self, Request, VmState};

use crate::progress::{Progress, Terminal, step};

/// Set while this process asks the VM service for a start; the service's
/// own status cannot tell a booting VM from a hung one.
static STARTING: AtomicBool = AtomicBool::new(false);

/// Set while this process asks the VM service for a stop.
static STOPPING: AtomicBool = AtomicBool::new(false);

/// Resets a flag when the request ends, also on errors.
struct Flag(&'static AtomicBool);

impl Flag {
    fn raise(flag: &'static AtomicBool) -> Flag {
        flag.store(true, Ordering::SeqCst);
        Flag(flag)
    }
}

impl Drop for Flag {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

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
    // A stop of this process (the idle one may first shrink the disk)
    // holds the service until it is done.
    let label = if STOPPING.load(Ordering::SeqCst) {
        format!(
            "VM {machine} starten, sobald der laufende Stopp fertig ist (bis zu einigen Minuten)"
        )
    } else {
        format!("VM {machine} starten (bis zu einer Minute)")
    };
    step(progress, "vm", &label, || {
        let _starting = Flag::raise(&STARTING);
        let answer = vmd::request(Request::Start)?;
        if !answer.ok {
            bail!("VM-Start über den Dienst: {}", answer.message);
        }
        Ok(())
    })?;
    if let Some(note) = ensure_swap(&provider) {
        println!("{note}");
    }
    Ok(provider)
}

/// Gives the running VM its swap if it has none yet
/// (`PodmanProvider::ensure_swap`); returns what to tell the user, if
/// anything. Failing only warns: the VM works without, just tighter.
pub fn ensure_swap(provider: &PodmanProvider) -> Option<String> {
    match provider.ensure_swap() {
        Ok(true) => Some(
            "Swap in der VM eingerichtet (zram, komprimiert im Arbeitsspeicher): Compiler und \
             Agent werden bei knappem Speicher nicht mehr beendet, nur langsamer."
                .to_owned(),
        ),
        Ok(false) => None,
        Err(error) => Some(format!("Swap in der VM nicht eingerichtet: {error}")),
    }
}

/// Stops the VM through the VM service; returns the service's message.
pub fn stop() -> anyhow::Result<String> {
    stop_with(Request::Stop)
}

/// Stops the idle VM; the service first gives free space back to Windows
/// when that is due, which can take minutes. Older services only stop.
pub fn stop_idle() -> anyhow::Result<String> {
    stop_with(Request::StopIdle)
}

fn stop_with(request: Request) -> anyhow::Result<String> {
    if !vmd::available() {
        bail!("Ohne VM-Dienst (`harness vmd install`) kann harness die VM nicht stoppen.");
    }
    let _stopping = Flag::raise(&STOPPING);
    let mut answer = vmd::request(request)?;
    if !answer.ok && answer.message == vmd::UNKNOWN_REQUEST && request != Request::Stop {
        answer = vmd::request(Request::Stop)?;
    }
    if !answer.ok {
        bail!("{}", answer.message);
    }
    Ok(answer.message)
}

/// The VM's state for the web app; asks the VM service, which is fast and
/// starts nothing.
pub fn status() -> VmStatus {
    if STOPPING.load(Ordering::SeqCst) {
        return VmStatus::Stopping;
    }
    if !vmd::available() {
        return VmStatus::Unknown;
    }
    let state = match vmd::request(Request::Status) {
        Ok(answer) => answer.state,
        Err(_) => return VmStatus::Unknown,
    };
    match state {
        VmState::Running => VmStatus::Running,
        _ if STARTING.load(Ordering::SeqCst) => VmStatus::Starting,
        VmState::Stopped => VmStatus::Stopped,
        VmState::Unresponsive | VmState::Unknown => VmStatus::Unknown,
    }
}

/// Marks the VM of `machine` as used by this command until the process
/// ends, so the background service does not stop it as idle. Without a
/// marker (unwritable folder) the command runs anyway.
pub fn mark_in_use(machine: &str) -> Option<Marker> {
    let command = std::env::args().nth(1).filter(|a| !a.starts_with('-'));
    let label = match command {
        Some(command) => format!("harness {command}"),
        None => "harness".to_owned(),
    };
    busy::Marker::acquire(machine, &label).ok()
}
