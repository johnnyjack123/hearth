//! The VM behind every command that needs containers: started through the
//! VM service when it is installed, so the user never needs an admin
//! window for it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::bail;
use hearth_core::busy::{self, Marker};
use hearth_core::lang::tr;
use hearth_core::podman::PodmanProvider;
use hearth_core::tr;
use hearth_core::tunnel::VmStatus;
use hearth_core::vmd::{self, Request, VmState};

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
    if provider.answers(Duration::from_secs(3)) {
        return Ok(provider);
    }
    // macOS: Podman's own machine starts without administrator rights.
    if !cfg!(windows) {
        step(
            progress,
            "vm",
            &tr!(in progress.lang();
                "VM {machine} starten (bis zu einer Minute)",
                "Starting VM {machine} (up to a minute)"
            ),
            || {
                let output = std::process::Command::new(provider.podman_path())
                    .args(["machine", "start", machine])
                    .output()?;
                if !output.status.success() {
                    bail!(
                        "podman machine start {machine}: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    );
                }
                Ok(())
            },
        )?;
        return Ok(provider);
    }
    if !vmd::available() {
        bail!(tr!(
            "Die VM {machine} läuft nicht. Einmalig `hearth vmd install` einrichten, dann startet \
             hearth sie selbst. Sonst in einer Admin-PowerShell: podman machine start {machine}",
            "The VM {machine} is not running. Set up `hearth vmd install` once, then hearth \
             starts it itself. Otherwise, in an admin PowerShell: podman machine start {machine}"
        ));
    }
    // A stop of this process (the idle one may first shrink the disk)
    // holds the service until it is done.
    let label = if STOPPING.load(Ordering::SeqCst) {
        tr!(in progress.lang();
            "VM {machine} starten, sobald der laufende Stopp fertig ist (bis zu einigen Minuten)",
            "Starting VM {machine} once the running stop is done (up to a few minutes)"
        )
    } else {
        tr!(in progress.lang();
            "VM {machine} starten (bis zu einer Minute)",
            "Starting VM {machine} (up to a minute)"
        )
    };
    step(progress, "vm", &label, || {
        let _starting = Flag::raise(&STARTING);
        let answer = vmd::request(Request::Start)?;
        if !answer.ok {
            bail!(tr!(
                "VM-Start über den Dienst: {}",
                "VM start through the service: {}",
                answer.message
            ));
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
            tr(
                "Swap in der VM eingerichtet (zram, komprimiert im Arbeitsspeicher): Compiler und \
                 Agent werden bei knappem Speicher nicht mehr beendet, nur langsamer.",
                "Swap set up in the VM (zram, compressed in memory): compiler and agent are no \
                 longer killed when memory runs short, only slower.",
            )
            .to_owned(),
        ),
        Ok(false) => None,
        Err(error) => Some(tr!(
            "Swap in der VM nicht eingerichtet: {error}",
            "Swap not set up in the VM: {error}"
        )),
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
    if cfg!(target_os = "linux") {
        return Ok(tr(
            "Ohne VM gibt es nichts zu stoppen.",
            "Without a VM there is nothing to stop.",
        )
        .to_owned());
    }
    if !cfg!(windows) {
        let podman = hearth_core::podman::find_podman()?;
        let output = std::process::Command::new(podman)
            .args(["machine", "stop", hearth_core::podman::DEFAULT_MACHINE])
            .output()?;
        if !output.status.success() {
            bail!(
                "podman machine stop: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let _ = request;
        return Ok(tr("Die VM ist gestoppt.", "The VM is stopped.").to_owned());
    }
    if !vmd::available() {
        bail!(tr(
            "Ohne VM-Dienst (`hearth vmd install`) kann hearth die VM nicht stoppen.",
            "Without the VM service (`hearth vmd install`), hearth cannot stop the VM."
        ));
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
    // Linux: the computer's own Podman, always there once installed.
    if cfg!(target_os = "linux") {
        return if hearth_core::podman::find_podman().is_ok() {
            VmStatus::Running
        } else {
            VmStatus::Unknown
        };
    }
    // macOS: Podman's machine, asked directly (no VM service there).
    if !cfg!(windows) {
        return machine_state();
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

/// The state of Podman's machine on macOS (`podman machine inspect`).
fn machine_state() -> VmStatus {
    let Ok(podman) = hearth_core::podman::find_podman() else {
        return VmStatus::Unknown;
    };
    let output = std::process::Command::new(podman)
        .args([
            "machine",
            "inspect",
            "--format",
            "{{.State}}",
            hearth_core::podman::DEFAULT_MACHINE,
        ])
        .output();
    match output.map(|output| {
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_lowercase()
    }) {
        Ok(state) if state == "running" => VmStatus::Running,
        Ok(state) if state == "starting" => VmStatus::Starting,
        Ok(state) if state == "stopped" => VmStatus::Stopped,
        _ => VmStatus::Unknown,
    }
}

/// Marks the VM of `machine` as used by this command until the process
/// ends, so the background service does not stop it as idle. Without a
/// marker (unwritable folder) the command runs anyway.
pub fn mark_in_use(machine: &str) -> Option<Marker> {
    let command = std::env::args().nth(1).filter(|a| !a.starts_with('-'));
    let label = match command {
        Some(command) => format!("hearth {command}"),
        None => "hearth".to_owned(),
    };
    busy::Marker::acquire(machine, &label).ok()
}
