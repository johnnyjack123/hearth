//! `hearth vmd`: the VM service. `install` asks once for admin rights
//! (UAC) and then runs this binary elevated to set the service up; after
//! that `hearth` starts the VM itself when needed.
//!
//! Setting it up as admin:
//! 1. stop the VM with the user's own podman,
//! 2. lock `C:\ProgramData\hearth` down to administrators (owner
//!    included, so a folder a user process created beforehand cannot be
//!    reclaimed), the user may only read,
//! 3. put `hearth-vmd.exe` below Program Files,
//! 4. take the machine over: its JSON moves into the service directory,
//!    without host folder shares; the disk and SSH key stay where they are,
//! 5. register and start the service, which then starts the VM.
//!
//! `uninstall` reverses 3 to 5; the machine is the user's again.
//!
//! For `hearth setup`, the elevated half can also create the machine
//! first (`podman machine init`, with the user's profile like a manual
//! init in an admin PowerShell) and switch Hyper-V on.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};
use clap::Subcommand;
use hearth_core::apps::{self, Inventory};
use hearth_core::config::ConfigDir;
use hearth_core::image::BUILD_ONLY_IMAGES;
use hearth_core::lang::{self, tr};
use hearth_core::podman::find_podman;
use hearth_core::project::ProjectName;
use hearth_core::provider::{ImageInfo, Provider as _};
use hearth_core::rescue::{self, Unsaved};
use hearth_core::tr;
use hearth_core::vmd::{self, Request, Response, SERVICE_NAME, VmState};

// Help texts follow the user's language (hearth_core::lang), so they are
// attributes rather than doc comments.
#[derive(Subcommand)]
pub enum VmdCommand {
    #[command(about = tr(
        "Zeigt, ob der Dienst läuft und in welchem Zustand die VM ist (Standard)",
        "Shows whether the service runs and what state the VM is in (default)"
    ))]
    Status,
    #[command(about = tr("Startet die VM über den Dienst", "Starts the VM through the service"))]
    Start,
    #[command(about = tr("Stoppt die VM über den Dienst", "Stops the VM through the service"))]
    Stop,
    #[command(about = tr(
        "Startet die VM neu, z. B. wenn SSH nicht mehr antwortet",
        "Restarts the VM, e.g. when SSH no longer answers"
    ))]
    Restart,
    #[command(about = tr(
        "Räumt die VM auf (alte Image-Stände, Bau-Images, Daten gelöschter Projekte) und verkleinert ihre Festplatte",
        "Cleans up the VM (old image versions, build images, data of removed projects) and shrinks its disk"
    ))]
    Clean {
        #[arg(long, help = tr("Ohne Nachfrage", "Without asking"))]
        yes: bool,
    },
    #[command(about = tr(
        "Richtet den Dienst ein (fragt einmal nach Administratorrechten)",
        "Sets the service up (asks once for administrator rights)"
    ))]
    Install,
    #[command(about = tr(
        "Entfernt den Dienst wieder (fragt einmal nach Administratorrechten)",
        "Removes the service again (asks once for administrator rights)"
    ))]
    Uninstall,
    // Called by `install` with administrator rights.
    #[command(hide = true)]
    InstallElevated {
        #[arg(long)]
        user_sid: String,
        #[arg(long)]
        podman: PathBuf,
        #[arg(long)]
        machine: String,
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        log: PathBuf,
        #[arg(long, num_args = 3, value_names = ["CPUS", "MIB", "GIB"], help = tr(
            "Legt die Machine erst an: CPUs, Arbeitsspeicher (MiB), Platte (GiB)",
            "Creates the machine first: CPUs, memory (MiB), disk (GiB)"
        ))]
        create: Option<Vec<u32>>,
    },
    // Called by `hearth setup` with administrator rights.
    #[command(hide = true)]
    HypervElevated {
        #[arg(long)]
        log: PathBuf,
    },
    // Called by `uninstall` with administrator rights.
    #[command(hide = true)]
    UninstallElevated {
        #[arg(long)]
        machine: String,
        #[arg(long)]
        profile: PathBuf,
        #[arg(long)]
        log: PathBuf,
    },
}

/// `hearth vmd …`.
pub fn run(config: &ConfigDir, machine: &str, command: Option<VmdCommand>) -> anyhow::Result<bool> {
    match command.unwrap_or(VmdCommand::Status) {
        VmdCommand::Clean { yes } => clean(config, machine, yes),
        VmdCommand::Status => status(),
        VmdCommand::Start => ask(Request::Start, tr("Starte die VM …", "Starting the VM …")),
        VmdCommand::Stop => ask(Request::Stop, tr("Stoppe die VM …", "Stopping the VM …")),
        VmdCommand::Restart => ask(
            Request::Restart,
            tr("Starte die VM neu …", "Restarting the VM …"),
        ),
        VmdCommand::Install => install(machine, None),
        VmdCommand::Uninstall => uninstall(machine),
        VmdCommand::InstallElevated {
            user_sid,
            podman,
            machine,
            profile,
            log,
            create,
        } => elevated(&log, |log| {
            let size = create.and_then(|values| match values[..] {
                [cpus, memory, disk] => Some(MachineSize { cpus, memory, disk }),
                _ => None,
            });
            install_elevated(log, &user_sid, &podman, &machine, &profile, size)
        }),
        VmdCommand::HypervElevated { log } => elevated(&log, enable_hyperv_elevated),
        VmdCommand::UninstallElevated {
            machine,
            profile,
            log,
        } => elevated(&log, |log| uninstall_elevated(log, &machine, &profile)),
    }
}

fn describe(state: VmState) -> &'static str {
    match state {
        VmState::Running => tr("läuft", "running"),
        VmState::Unresponsive => tr(
            "läuft, antwortet aber nicht (hearth vmd restart)",
            "running, but not answering (hearth vmd restart)",
        ),
        VmState::Stopped => tr("gestoppt", "stopped"),
        VmState::Unknown => tr("unbekannt", "unknown"),
    }
}

fn status() -> anyhow::Result<bool> {
    match vmd::request(Request::Status) {
        Ok(answer) => {
            println!(
                "{}",
                tr!(
                    "Dienst {SERVICE_NAME}: läuft",
                    "Service {SERVICE_NAME}: running"
                )
            );
            println!("VM: {}", describe(answer.state));
            Ok(true)
        }
        Err(error) => {
            println!("{error}");
            Ok(false)
        }
    }
}

/// Whether a volume belongs to a project that no longer exists here.
fn orphaned(volume: &str, projects: &[String]) -> Option<String> {
    let name = volume
        .strip_prefix("hearth-ws-")
        .or_else(|| volume.strip_prefix("hearth-home-"))
        .or_else(|| {
            volume
                .strip_prefix("hearth-cfg-")
                .and_then(|rest| rest.split_once('_'))
                .map(|(project, _)| project)
        })?;
    (!projects.iter().any(|project| project == name)).then(|| name.to_owned())
}

/// `hearth vmd clean`: removes what the VM keeps without need, then has
/// the service shrink the VM's disk file, which never shrinks by itself.
///
/// Data of removed projects may still hold work the user never took over;
/// that is saved as a git bundle first, or the data stays.
fn clean(config: &ConfigDir, machine: &str, yes: bool) -> anyhow::Result<bool> {
    let provider = crate::vm::provider(machine)?;
    let images = provider.images()?;
    let unnamed = images
        .iter()
        .filter(|image| image.tags.is_empty() && image.containers == 0)
        .count();
    let builders: Vec<&ImageInfo> = images
        .iter()
        .filter(|image| {
            image.containers == 0
                && image.tags.iter().any(|tag| {
                    BUILD_ONLY_IMAGES
                        .iter()
                        .any(|prefix| tag.starts_with(prefix))
                })
        })
        .collect();
    let projects: Vec<String> = config
        .project_names()?
        .iter()
        .map(ToString::to_string)
        .collect();
    // Another configuration (tests, smoke scripts) knows only its own
    // projects; the user's would all look removed.
    let mut gone: Vec<String> = if std::env::var_os("HEARTH_HOME").is_some() {
        println!(
            "{}",
            tr(
                "Mit HEARTH_HOME kennt hearth nur die Projekte dieser Konfiguration; Daten von \
                 Projekten fasst clean deshalb nicht an.",
                "With HEARTH_HOME, hearth only knows the projects of this configuration, so \
                 clean leaves project data alone."
            )
        );
        Vec::new()
    } else {
        provider
            .volumes()?
            .iter()
            .filter_map(|volume| orphaned(&volume.name, &projects))
            .collect()
    };
    gone.sort();
    gone.dedup();
    let inventory = Inventory::load(&provider)?;
    let old_apps = inventory.superseded();

    // Work the agents of removed projects left behind; data that cannot be
    // checked stays.
    let mut unsaved: Vec<(ProjectName, Unsaved)> = Vec::new();
    let mut unchecked: Vec<String> = Vec::new();
    for name in &gone {
        let Ok(project) = ProjectName::new(name) else {
            continue;
        };
        match rescue::unsaved(&provider, &project) {
            Ok(Some(work)) if work.any() => unsaved.push((project, work)),
            Ok(_) => {}
            Err(error) => {
                println!(
                    "{}",
                    tr!(
                        "{name}: nicht prüfbar, ob noch Arbeit des Agents fehlt ({error}).",
                        "{name}: cannot check whether work of the agent is still missing ({error})."
                    )
                );
                unchecked.push(name.clone());
            }
        }
    }
    gone.retain(|name| !unchecked.contains(name));

    println!("{}", tr("Aufräumen in der VM:", "Cleaning up in the VM:"));
    println!(
        "{}",
        tr!(
            "- {unnamed} alte Image-Stände und Zwischenstufen von Builds",
            "- {unnamed} old image versions and intermediate build stages"
        )
    );
    if !builders.is_empty() {
        let size: u64 = builders.iter().map(|image| image.size).sum();
        let gigabytes = format!("{:.1}", size as f64 / 1e9);
        let gigabytes = if lang::german() {
            gigabytes.replace('.', ",")
        } else {
            gigabytes
        };
        let tags = builders
            .iter()
            .flat_map(|image| image.tags.first())
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "{}",
            tr!(
                "- Images, die nur zum Bauen dienen ({gigabytes} GB; ein späterer Bau lädt sie neu): {tags}",
                "- Images only used for building ({gigabytes} GB; a later build downloads them again): {tags}"
            )
        );
    }
    if !gone.is_empty() {
        println!(
            "{}",
            tr!(
                "- Container und Daten gelöschter Projekte: {}",
                "- Containers and data of removed projects: {}",
                gone.join(", ")
            )
        );
    }
    if !old_apps.is_empty() || !inventory.leftovers.is_empty() {
        let mut names: Vec<String> = old_apps
            .iter()
            .map(|install| format!("{} {}", install.app, install.version))
            .collect();
        if !inventory.leftovers.is_empty() {
            names.push(tr!(
                "{} abgebrochene Installation(en)",
                "{} aborted installation(s)",
                inventory.leftovers.len()
            ));
        }
        println!(
            "{}",
            tr!(
                "- Ältere Stände von Apps, soweit keine Instanz sie noch nutzt: {}",
                "- Older versions of apps, as far as no instance still uses them: {}",
                names.join(", ")
            )
        );
    }
    let rescued_dir = config.root().join("gesichert");
    for (project, work) in &unsaved {
        println!(
            "{}",
            tr!(
                "  Achtung, {project} enthält Arbeit des Agents, die nie auf deinem Rechner ankam: \
                 {} Commit(s), {} nicht committete Datei(en). Sie wird vorher als Git-Bundle nach \
                 {} gesichert.",
                "  Attention, {project} holds work of the agent that never reached your computer: \
                 {} commit(s), {} uncommitted file(s). It is saved as a Git bundle to {} first.",
                work.commits,
                work.changes,
                rescued_dir.display()
            )
        );
    }
    println!(
        "{}",
        tr(
            "Danach gibt der VM-Dienst den Platz an Windows zurück; die VM ist dabei kurz aus.",
            "Then the VM service gives the space back to Windows; the VM is off for a moment."
        )
    );
    if !yes {
        if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
            bail!(tr(
                "Ohne Terminal nur mit --yes",
                "Without a terminal only with --yes"
            ));
        }
        if !crate::project::confirm(tr("Aufräumen?", "Clean up?"), true)? {
            println!("{}", tr("Abgebrochen.", "Cancelled."));
            return Ok(false);
        }
        if !unsaved.is_empty()
            && !crate::project::confirm(
                tr(
                    "Die Arbeit oben sichern? Nein lässt diese Projekte in der VM, wie sie sind.",
                    "Save the work above? No leaves these projects in the VM as they are.",
                ),
                true,
            )?
        {
            let kept: Vec<String> = unsaved.iter().map(|(p, _)| p.to_string()).collect();
            gone.retain(|name| !kept.contains(name));
            unsaved.clear();
        }
    }
    for (project, _) in &unsaved {
        let stamp = crate::connect::utc_stamp()
            .replace(' ', "_")
            .replace([':', 'Z'], "");
        let target = rescued_dir.join(format!("{project}-{stamp}.bundle"));
        let saved = fs::create_dir_all(&rescued_dir)
            .map_err(anyhow::Error::from)
            .and_then(|()| Ok(rescue::save(&provider, project, &target)?));
        match saved {
            Ok(()) => println!(
                "{}",
                tr!(
                    "{project}: gesichert in {0}. Holen in einem Repository: git fetch \"{0}\" \
                     \"refs/heads/*:refs/remotes/gesichert/*\"",
                    "{project}: saved in {0}. To fetch it into a repository: git fetch \"{0}\" \
                     \"refs/heads/*:refs/remotes/gesichert/*\"",
                    target.display()
                )
            ),
            Err(error) => {
                println!(
                    "{}",
                    tr!(
                        "{project}: Sichern gescheitert ({error:#}); die Daten bleiben in der VM.",
                        "{project}: saving failed ({error:#}); the data stays in the VM."
                    )
                );
                gone.retain(|name| name != project.as_str());
            }
        }
    }
    provider.prune_images()?;
    for image in builders {
        if let Err(error) = provider.remove_image(&image.id) {
            println!(
                "{}",
                tr!(
                    "{} bleibt: {error}",
                    "{} stays: {error}",
                    image.tags.join(", ")
                )
            );
        }
    }
    for install in old_apps {
        if let Err(error) = apps::remove(&provider, install) {
            println!(
                "{}",
                tr!(
                    "{} {} bleibt: {error}",
                    "{} {} stays: {error}",
                    install.app,
                    install.version
                )
            );
        }
    }
    for volume in &inventory.leftovers {
        // A running install still uses its volume; podman refuses then.
        let _ = provider.remove_volume(volume);
    }
    for name in &gone {
        let Ok(project) = ProjectName::new(name) else {
            continue;
        };
        let removed = [project.instance(), project.installer(), project.gateway()]
            .iter()
            .try_for_each(|container| provider.remove_container(container))
            .and_then(|()| provider.remove_network(&project.network()))
            .and_then(|()| provider.remove_volume(&project.workspace_volume()))
            .and_then(|()| provider.remove_volume(&project.home_volume()))
            .and_then(|()| {
                provider.volumes()?.iter().try_for_each(|volume| {
                    if volume.name.starts_with(&project.config_prefix()) {
                        provider.remove_volume(&volume.name)
                    } else {
                        Ok(())
                    }
                })
            });
        if let Err(error) = removed {
            println!("{name}: {error}");
        }
    }
    println!(
        "{}",
        tr(
            "Gebe den Platz an Windows zurück (kann einige Minuten dauern) …",
            "Giving the space back to Windows (can take a few minutes) …"
        )
    );
    match provider.trim() {
        Ok(trimmed) => println!("{trimmed}"),
        Err(error) => println!(
            "{}",
            tr!(
                "fstrim in der VM gescheitert ({error}); verkleinert wird nur, was schon frei war.",
                "fstrim in the VM failed ({error}); only what was already free gets shrunk."
            )
        ),
    }
    match vmd::request(Request::Compact) {
        Ok(answer) => {
            println!("{}", answer.message);
            Ok(answer.ok)
        }
        Err(error) => {
            println!(
                "{}",
                tr!(
                    "Der VM-Dienst kann die Festplatte nicht verkleinern ({error}). Ältere Dienste \
                     kennen das noch nicht: `hearth vmd install` aktualisiert ihn \
                     (Administratorrechte).",
                    "The VM service cannot shrink the disk ({error}). Older services do not know \
                     this yet: `hearth vmd install` updates it (administrator rights)."
                )
            );
            Ok(false)
        }
    }
}

fn ask(request: Request, doing: &str) -> anyhow::Result<bool> {
    println!("{doing}");
    let answer: Response = vmd::request(request)?;
    if !answer.message.is_empty() {
        println!("{}", answer.message);
    }
    println!("VM: {}", describe(answer.state));
    Ok(answer.ok)
}

// ---------------------------------------------------------------------------
// Unelevated half: collect what the admin half needs, then ask Windows.

fn service_exe_next_to_harness() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe().context(tr("Pfad von hearth.exe", "Path of hearth.exe"))?;
    let service = exe.with_file_name("hearth-vmd.exe");
    if !service.is_file() {
        bail!(tr!(
            "{} fehlt; sie wird mit hearth.exe gebaut (cargo build)",
            "{} is missing; it is built with hearth.exe (cargo build)",
            service.display()
        ));
    }
    Ok(service)
}

/// Whether the installed service is another build than the one next to
/// `hearth.exe` (after an update); `None` if either is missing.
pub fn service_outdated() -> Option<bool> {
    let installed = fs::read(program_dir().join("hearth-vmd.exe")).ok()?;
    let here = fs::read(service_exe_next_to_harness().ok()?).ok()?;
    Some(installed != here)
}

fn log_file(action: &str) -> PathBuf {
    std::env::temp_dir().join(format!("hearth-vmd-{action}-{}.log", std::process::id()))
}

/// Resources of a machine `hearth setup` creates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineSize {
    /// Virtual CPUs.
    pub cpus: u32,
    /// Most memory in MiB; the service starts the VM with less.
    pub memory: u32,
    /// Disk in GiB; it grows as it is used.
    pub disk: u32,
}

/// Sets the service up; with `create` it first creates the machine.
pub fn install(machine: &str, create: Option<MachineSize>) -> anyhow::Result<bool> {
    let sid = vmd::current_user_sid()?;
    let podman = find_podman()?;
    service_exe_next_to_harness()?;
    let profile = hearth_core::paths::home_dir()?;
    let log = log_file("install");
    println!(
        "{}",
        tr(
            "Richte den VM-Dienst ein. Windows fragt dafür einmal nach Administratorrechten.",
            "Setting up the VM service. Windows asks once for administrator rights."
        )
    );
    if create.is_some() {
        println!(
            "{}",
            tr!(
                "Zuerst entsteht die VM {machine}; podman lädt dafür ihr Betriebssystem (etwa 1 GB).",
                "First the VM {machine} is created; podman downloads its operating system for it \
                 (about 1 GB)."
            )
        );
    } else {
        println!(
            "{}",
            tr!(
                "Die VM {machine} wird dabei gestoppt und vom Dienst übernommen (Images und Volumes bleiben).",
                "The VM {machine} is stopped and taken over by the service (images and volumes stay)."
            )
        );
    }
    let mut args: Vec<String> = vec![
        "vmd".into(),
        "install-elevated".into(),
        "--user-sid".into(),
        sid,
        "--podman".into(),
        podman.display().to_string(),
        "--machine".into(),
        machine.to_owned(),
        "--profile".into(),
        profile.display().to_string(),
        "--log".into(),
        log.display().to_string(),
    ];
    if let Some(size) = create {
        args.extend([
            "--create".into(),
            size.cpus.to_string(),
            size.memory.to_string(),
            size.disk.to_string(),
        ]);
    }
    let code = run_elevated(&args, &log)?;
    if code != 0 {
        println!(
            "{}",
            tr!(
                "Einrichtung fehlgeschlagen (Exit-Code {code}).",
                "Setup failed (exit code {code})."
            )
        );
        return Ok(false);
    }
    println!();
    status()
}

/// Switches Hyper-V on (UAC); Windows needs a restart afterwards.
pub fn enable_hyperv() -> anyhow::Result<bool> {
    let log = log_file("hyperv");
    println!(
        "{}",
        tr(
            "Schalte Hyper-V ein. Windows fragt dafür einmal nach Administratorrechten.",
            "Switching Hyper-V on. Windows asks once for administrator rights."
        )
    );
    let args: Vec<String> = vec![
        "vmd".into(),
        "hyperv-elevated".into(),
        "--log".into(),
        log.display().to_string(),
    ];
    Ok(run_elevated(&args, &log)? == 0)
}

fn uninstall(machine: &str) -> anyhow::Result<bool> {
    let profile = hearth_core::paths::home_dir()?;
    let log = log_file("uninstall");
    println!(
        "{}",
        tr(
            "Entferne den VM-Dienst. Windows fragt dafür einmal nach Administratorrechten.",
            "Removing the VM service. Windows asks once for administrator rights."
        )
    );
    let args: Vec<String> = vec![
        "vmd".into(),
        "uninstall-elevated".into(),
        "--machine".into(),
        machine.to_owned(),
        "--profile".into(),
        profile.display().to_string(),
        "--log".into(),
        log.display().to_string(),
    ];
    let code = run_elevated(&args, &log)?;
    Ok(code == 0)
}

/// Prints what the elevated half wrote to `log` since `offset`, line by
/// line, and returns the new offset. A line still being written waits.
fn print_new(log: &Path, offset: usize) -> usize {
    let Ok(text) = fs::read(log) else {
        return offset;
    };
    let Some(end) = text.iter().rposition(|&b| b == b'\n').map(|i| i + 1) else {
        return offset;
    };
    if end <= offset {
        return offset;
    }
    for line in String::from_utf8_lossy(&text[offset..end]).lines() {
        println!("  {}", hearth_core::sync::sanitize(line));
    }
    end
}

/// Runs this binary with `args` elevated (UAC) in a hidden window, shows
/// its `log` while it runs and returns its exit code.
#[cfg(windows)]
fn run_elevated(args: &[String], log: &Path) -> anyhow::Result<u32> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

    use windows_sys::Win32::Foundation::{ERROR_CANCELLED, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };

    let exe = std::env::current_exe()?;
    let wide = |text: &OsStr| -> Vec<u16> { text.encode_wide().chain([0]).collect() };
    let file = wide(exe.as_os_str());
    let verb = wide(OsStr::new("runas"));
    // The same quoting rules as for any command line; the program name
    // is not part of lpParameters.
    let mut command = Command::new("x");
    command.args(args);
    let line = hearth_core::pty::command_line(&command);
    // Drop the program ("x ") in front of the arguments.
    let parameters: Vec<u16> = line[2..].to_vec();
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = parameters.as_ptr();
    // SW_HIDE: the elevated half reports through its log file.
    info.nShow = 0;
    // SAFETY: all strings are NUL-terminated and outlive the call.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_CANCELLED as i32) {
            bail!(tr(
                "Abgebrochen: Die Administratorrechte wurden nicht erteilt",
                "Cancelled: the administrator rights were not granted"
            ));
        }
        bail!(tr!(
            "Start mit Administratorrechten fehlgeschlagen: {error}",
            "Start with administrator rights failed: {error}"
        ));
    }
    if info.hProcess.is_null() {
        bail!(tr(
            "Kein Prozess für die Einrichtung erhalten",
            "Got no process for the setup"
        ));
    }
    // SAFETY: SEE_MASK_NOCLOSEPROCESS hands us the process handle.
    let process = unsafe { OwnedHandle::from_raw_handle(info.hProcess) };
    let mut code = 1u32;
    let mut shown = 0;
    // SAFETY: valid process handle.
    unsafe {
        // Creating a machine downloads for minutes; the log shows progress.
        while WaitForSingleObject(process.as_raw_handle(), 500) == WAIT_TIMEOUT {
            shown = print_new(log, shown);
        }
        GetExitCodeProcess(process.as_raw_handle(), &mut code);
    }
    print_new(log, shown);
    let _ = fs::remove_file(log);
    Ok(code)
}

#[cfg(not(windows))]
fn run_elevated(_args: &[String], _log: &Path) -> anyhow::Result<u32> {
    bail!(tr(
        "Den VM-Dienst gibt es nur unter Windows",
        "The VM service exists only on Windows"
    ))
}

// ---------------------------------------------------------------------------
// Elevated half.

/// Step-by-step report, written to the log file the unelevated half shows.
struct Log {
    file: fs::File,
}

impl Log {
    fn line(&mut self, text: &str) {
        let _ = writeln!(self.file, "{text}");
        let _ = self.file.flush();
    }
}

fn elevated(log: &Path, body: impl FnOnce(&mut Log) -> anyhow::Result<()>) -> anyhow::Result<bool> {
    let mut log = Log {
        file: fs::File::create(log)
            .with_context(|| tr!("{} anlegen", "Creating {}", log.display()))?,
    };
    if !is_elevated() {
        log.line(tr(
            "✗ läuft ohne Administratorrechte",
            "✗ runs without administrator rights",
        ));
        return Ok(false);
    }
    match body(&mut log) {
        Ok(()) => Ok(true),
        Err(error) => {
            log.line(&format!("✗ {error:#}"));
            Ok(false)
        }
    }
}

#[cfg(windows)]
fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: pseudo handle of this process and valid out pointers.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut size = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&raw mut elevation).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

#[cfg(not(windows))]
fn is_elevated() -> bool {
    false
}

/// Runs a program, logs its output and fails on a non-zero exit.
fn step(log: &mut Log, what: &str, command: &mut Command) -> anyhow::Result<String> {
    let output = command.output().with_context(|| {
        tr!(
            "{what}: Programm nicht gestartet",
            "{what}: program not started"
        )
    })?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        log.line(&format!("✓ {what}"));
        Ok(text)
    } else {
        bail!("{what}: {}", text.trim())
    }
}

fn system32(program: &str) -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").unwrap_or("C:\\Windows".into()))
        .join("System32")
        .join(program)
}

/// `C:\Program Files\hearth`.
fn program_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("ProgramFiles").unwrap_or("C:\\Program Files".into()))
        .join("hearth")
}

fn user_machine_json(profile: &Path, machine: &str) -> PathBuf {
    profile
        .join(".config")
        .join("containers")
        .join("podman")
        .join("machine")
        .join("hyperv")
        .join(format!("{machine}.json"))
}

/// Suffix of the user's machine JSON while the service owns the machine.
const MOVED: &str = "moved-to-hearth-vmd";

fn service_state() -> Option<String> {
    let output = Command::new(system32("sc.exe"))
        .args(["query", SERVICE_NAME])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    output.status.success().then_some(text)
}

fn stop_service(log: &mut Log) {
    if let Some(state) = service_state()
        && state.contains("RUNNING")
    {
        let _ = Command::new(system32("sc.exe"))
            .args(["stop", SERVICE_NAME])
            .output();
        let deadline = Instant::now() + Duration::from_secs(20);
        while service_state().is_some_and(|s| !s.contains("STOPPED")) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(300));
        }
        log.line(tr(
            "✓ laufenden Dienst gestoppt",
            "✓ running service stopped",
        ));
    }
}

fn install_elevated(
    log: &mut Log,
    sid: &str,
    podman: &Path,
    machine: &str,
    profile: &Path,
    create: Option<MachineSize>,
) -> anyhow::Result<()> {
    // The same checks the service applies, before anything changes.
    let config = format!(
        "# Geschrieben von `hearth vmd install`; nur Administratoren dürfen sie ändern.\n\
         machine = \"{machine}\"\n\
         podman = '{}'\n\
         user_sid = \"{sid}\"\n",
        podman.display()
    );
    hearth_core::provider::validate_resource_name(tr("Machine-Name", "machine name"), machine)?;
    if !vmd::is_user_sid(sid) {
        bail!(tr!("{sid} ist keine Nutzer-SID", "{sid} is not a user SID"));
    }
    if !vmd::below_program_files(podman) || podman.display().to_string().contains('\'') {
        bail!(tr!(
            "{} liegt nicht unter Program Files; der Dienst führt als SYSTEM nur aus, was nur \
             Administratoren ändern können",
            "{} is not below Program Files; as SYSTEM, the service only runs what only \
             administrators can change",
            podman.display()
        ));
    }
    let source = service_exe_next_to_harness()?;
    let user_json = user_machine_json(profile, machine);
    let service_json = vmd::machine_json(machine);
    if !service_json.is_file() && !user_json.is_file() {
        let Some(size) = create else {
            bail!(tr!(
                "Machine {machine} nicht gefunden: {}",
                "Machine {machine} not found: {}",
                user_json.display()
            ));
        };
        create_machine(log, podman, machine, size)?;
        if !user_json.is_file() {
            bail!(tr!(
                "podman hat die Machine angelegt, aber {} fehlt",
                "podman created the machine, but {} is missing",
                user_json.display()
            ));
        }
    }

    // 1. The VM stops with the user's podman, which still owns it.
    if user_json.is_file() {
        let output = Command::new(podman)
            .args(["machine", "stop", machine])
            .env("CONTAINERS_MACHINE_PROVIDER", "hyperv")
            .output()
            .context("podman machine stop")?;
        log.line(&tr!(
            "✓ VM gestoppt ({})",
            "✓ VM stopped ({})",
            String::from_utf8_lossy(&output.stdout).trim()
        ));
    }

    // 2. Lock the data directory down: owner and ACL.
    let root = vmd::service_dir().parent().map(Path::to_owned).context(tr(
        "Dienstordner ohne Elternordner",
        "Service folder without a parent folder",
    ))?;
    // Another account may have prepared the folder (security review M8):
    // nothing is created, changed or written while it holds links or
    // belongs to someone else.
    vmd::check_service_root(&root, Some(sid), true)?;
    fs::create_dir_all(vmd::service_dir())?;
    let icacls = system32("icacls.exe");
    // The folder itself first, so nobody adds anything while the rest runs.
    step(
        log,
        tr("Datenordner gesperrt", "Data folder locked"),
        Command::new(&icacls)
            .arg(&root)
            .args(["/setowner", "*S-1-5-32-544", "/C", "/Q"]),
    )?;
    step(
        log,
        tr(
            "Nur Administratoren schreiben in den Datenordner",
            "Only administrators write into the data folder",
        ),
        Command::new(&icacls).arg(&root).args([
            "/inheritance:r",
            "/grant:r",
            "*S-1-5-18:(OI)(CI)F",
            "*S-1-5-32-544:(OI)(CI)F",
            &format!("*{sid}:(OI)(CI)RX"),
            "/C",
            "/Q",
        ]),
    )?;
    // What arrived before the lock is checked before anything goes deeper.
    vmd::check_service_root(&root, None, true)?;
    step(
        log,
        tr(
            "Besitzer des Datenordners: Administratoren",
            "Owner of the data folder: administrators",
        ),
        Command::new(&icacls)
            .arg(&root)
            .args(["/setowner", "*S-1-5-32-544", "/T", "/C", "/Q"]),
    )?;
    step(
        log,
        tr(
            "Rechte im Datenordner zurückgesetzt",
            "Permissions in the data folder reset",
        ),
        Command::new(&icacls)
            .arg(&root)
            .args(["/reset", "/T", "/C", "/Q"]),
    )?;
    step(
        log,
        tr(
            "Datenordner nur für Administratoren beschreibbar",
            "Data folder writable for administrators only",
        ),
        Command::new(&icacls).arg(&root).args([
            "/inheritance:r",
            "/grant:r",
            "*S-1-5-18:(OI)(CI)F",
            "*S-1-5-32-544:(OI)(CI)F",
            &format!("*{sid}:(OI)(CI)RX"),
            "/C",
            "/Q",
        ]),
    )?;
    for dir in ["home", "appdata", "localappdata", "programdata", "tmp"] {
        fs::create_dir_all(vmd::service_dir().join(dir))?;
    }
    fs::write(vmd::service_dir().join("vmd.toml"), config)?;
    fs::write(
        vmd::service_dir().join("containers.conf"),
        "# podman des VM-Dienstes; nur Administratoren dürfen sie ändern.\n[machine]\nprovider = \"hyperv\"\n",
    )?;
    log.line(tr(
        "✓ vmd.toml und containers.conf geschrieben",
        "✓ vmd.toml and containers.conf written",
    ));

    // 3. The service binary below Program Files.
    stop_service(log);
    fs::create_dir_all(program_dir())?;
    let target = program_dir().join("hearth-vmd.exe");
    fs::copy(&source, &target).with_context(|| {
        tr!(
            "{} nach {} kopieren",
            "Copying {} to {}",
            source.display(),
            target.display()
        )
    })?;
    log.line(&format!("✓ {}", target.display()));

    // 4. Take the machine over, without host folder shares.
    if !service_json.is_file() {
        let text = fs::read_to_string(&user_json)?;
        let mut json: serde_json::Value = serde_json::from_str(&text)
            .with_context(|| tr!("{} lesen", "Reading {}", user_json.display()))?;
        json["Mounts"] = serde_json::Value::Array(Vec::new());
        let dir = service_json
            .parent()
            .context(tr("Machine-Ordner", "Machine folder"))?;
        fs::create_dir_all(dir)?;
        fs::write(&service_json, serde_json::to_string_pretty(&json)?)?;
        let ignition = user_json.with_extension("ign");
        if ignition.is_file() {
            fs::copy(&ignition, service_json.with_extension("ign"))?;
        }
        fs::rename(
            &user_json,
            user_json.with_extension(format!("json.{MOVED}")),
        )?;
        log.line(&tr!(
            "✓ Machine {machine} übernommen, ohne Freigaben ({})",
            "✓ Machine {machine} taken over, without folder shares ({})",
            service_json.display()
        ));
    } else {
        log.line(tr(
            "✓ Machine gehört dem Dienst schon",
            "✓ Machine already belongs to the service",
        ));
    }

    // 5. Register (or update) and start the service.
    let sc = system32("sc.exe");
    let bin_path = format!("\"{}\"", target.display());
    if service_state().is_some() {
        step(
            log,
            tr("Dienst aktualisiert", "Service updated"),
            Command::new(&sc).args([
                "config",
                SERVICE_NAME,
                "binPath=",
                &bin_path,
                "start=",
                "auto",
            ]),
        )?;
    } else {
        step(
            log,
            tr("Dienst angelegt", "Service created"),
            Command::new(&sc).args([
                "create",
                SERVICE_NAME,
                "binPath=",
                &bin_path,
                "start=",
                "auto",
                "DisplayName=",
                "hearth VM",
            ]),
        )?;
    }
    let _ = Command::new(&sc)
        .args([
            "description",
            SERVICE_NAME,
            tr(
                "Startet und stoppt die VM von hearth für einen Nutzer.",
                "Starts and stops the VM of hearth for one user.",
            ),
        ])
        .output();
    let _ = Command::new(&sc)
        .args([
            "failure",
            SERVICE_NAME,
            "reset=",
            "86400",
            "actions=",
            "restart/5000",
        ])
        .output();
    step(
        log,
        tr("Dienst gestartet", "Service started"),
        Command::new(&sc).args(["start", SERVICE_NAME]),
    )?;

    // The pipe appears once the service read its configuration.
    let deadline = Instant::now() + Duration::from_secs(15);
    while !vmd::available() {
        if Instant::now() > deadline {
            bail!(tr!(
                "Der Dienst antwortet nicht; Details in {}",
                "The service does not answer; details in {}",
                vmd::service_dir().join("vmd.log").display()
            ));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    log.line(tr("… VM wird gestartet", "… starting the VM"));
    let answer = vmd::request(Request::Start)?;
    if !answer.ok {
        bail!(tr!(
            "VM-Start über den Dienst: {}",
            "VM start through the service: {}",
            answer.message
        ));
    }
    log.line(&format!("✓ {}", answer.message));
    Ok(())
}

/// `podman machine init` as the elevated user, so the machine lands in the
/// user's profile as after a manual init; the service takes it over next.
/// Its output goes to the log as it comes, so the user sees the download.
fn create_machine(
    log: &mut Log,
    podman: &Path,
    machine: &str,
    size: MachineSize,
) -> anyhow::Result<()> {
    log.line(&tr!(
        "… lege die VM {machine} an ({} CPUs, bis {} MiB RAM, {} GiB Platte); das dauert ein paar Minuten",
        "… creating the VM {machine} ({} CPUs, up to {} MiB RAM, {} GiB disk); this takes a few minutes",
        size.cpus,
        size.memory,
        size.disk
    ));
    let mut child = Command::new(podman)
        .args([
            "machine",
            "init",
            "--cpus",
            &size.cpus.to_string(),
            "--memory",
            &size.memory.to_string(),
            "--disk-size",
            &size.disk.to_string(),
            machine,
        ])
        .env("CONTAINERS_MACHINE_PROVIDER", "hyperv")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context(tr(
            "podman machine init: Programm nicht gestartet",
            "podman machine init: program not started",
        ))?;
    let stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut stderr) = stderr {
            let _ = std::io::Read::read_to_string(&mut stderr, &mut text);
        }
        text
    });
    if let Some(stdout) = child.stdout.take() {
        for line in std::io::BufRead::lines(std::io::BufReader::new(stdout)).map_while(Result::ok) {
            let line = line.trim();
            if !line.is_empty() {
                log.line(&format!("  {line}"));
            }
        }
    }
    let status = child.wait().context("podman machine init")?;
    let errors = errors.join().unwrap_or_default();
    if !status.success() {
        bail!("podman machine init: {}", errors.trim());
    }
    log.line(&tr!("✓ VM {machine} angelegt", "✓ VM {machine} created"));
    Ok(())
}

/// Switches the Hyper-V feature on; DISM exits with 3010 when Windows
/// needs a restart for it, which is the usual case.
fn enable_hyperv_elevated(log: &mut Log) -> anyhow::Result<()> {
    log.line(tr(
        "… schalte Hyper-V ein (DISM), das dauert eine Weile",
        "… switching Hyper-V on (DISM), this takes a while",
    ));
    let output = Command::new(system32("dism.exe"))
        .args([
            "/Online",
            "/Enable-Feature",
            "/FeatureName:Microsoft-Hyper-V",
            "/All",
            "/NoRestart",
        ])
        .output()
        .context(tr(
            "dism.exe: Programm nicht gestartet",
            "dism.exe: program not started",
        ))?;
    match output.status.code() {
        Some(0) => log.line(tr("✓ Hyper-V eingeschaltet", "✓ Hyper-V switched on")),
        Some(3010) => log.line(tr(
            "✓ Hyper-V eingeschaltet; Windows muss neu starten",
            "✓ Hyper-V switched on; Windows has to restart",
        )),
        code => bail!(tr!(
            "DISM endete mit {:?}: {}",
            "DISM ended with {:?}: {}",
            code,
            String::from_utf8_lossy(&output.stdout).trim()
        )),
    }
    Ok(())
}

fn uninstall_elevated(log: &mut Log, machine: &str, profile: &Path) -> anyhow::Result<()> {
    hearth_core::provider::validate_resource_name(tr("Machine-Name", "machine name"), machine)?;
    stop_service(log);
    if service_state().is_some() {
        step(
            log,
            tr("Dienst entfernt", "Service removed"),
            Command::new(system32("sc.exe")).args(["delete", SERVICE_NAME]),
        )?;
    }
    let user_json = user_machine_json(profile, machine);
    let moved = user_json.with_extension(format!("json.{MOVED}"));
    if moved.is_file() {
        fs::rename(&moved, &user_json)?;
        log.line(&tr!(
            "✓ Machine {machine} gehört wieder dir",
            "✓ Machine {machine} is yours again"
        ));
    }
    let service_json = vmd::machine_json(machine);
    let _ = fs::remove_file(service_json.with_extension("ign"));
    if service_json.is_file() {
        fs::remove_file(&service_json)?;
    }
    let _ = fs::remove_file(program_dir().join("hearth-vmd.exe"));
    log.line(tr(
        "✓ Die VM startest du jetzt wieder selbst (Admin-PowerShell: podman machine start)",
        "✓ From now on you start the VM yourself again (admin PowerShell: podman machine start)",
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_data_of_removed_projects() {
        let projects = vec!["shop".to_owned()];
        assert_eq!(orphaned("hearth-ws-old", &projects).as_deref(), Some("old"));
        assert_eq!(
            orphaned("hearth-home-old", &projects).as_deref(),
            Some("old")
        );
        assert_eq!(orphaned("hearth-ws-shop", &projects), None);
        assert_eq!(
            orphaned("hearth-cfg-old_pi", &projects).as_deref(),
            Some("old")
        );
        assert_eq!(orphaned("hearth-cfg-shop_pi", &projects), None);
        // Shared by all projects (the config volume before 0.1.17 too).
        assert_eq!(orphaned("hearth-tools", &projects), None);
        assert_eq!(orphaned("hearth-cfg-pi", &projects), None);
    }
}
