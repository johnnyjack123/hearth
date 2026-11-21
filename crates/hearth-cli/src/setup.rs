//! `hearth setup`: the setup assistant (ROADMAP Phase 3). It checks one
//! step after the other and does only what is missing, asking before admin
//! rights (UAC) and large downloads:
//!
//! 1. Windows with Hyper-V (switched on if needed; then a restart),
//! 2. git and Podman (through winget),
//! 3. the VM service and the machine (created if there is none),
//! 4. the images in the VM,
//! 5. pairing with the hearth server and the background service at logon,
//! 6. `hearth` on the user's PATH.
//!
//! Run it again after a restart or a fix: finished steps only show ✓.
//!
//! On Linux and macOS it checks git and Podman, prepares the sandbox (macOS:
//! Podman's machine; Linux: the computer's own Podman) and its images,
//! pairs, sets up the background service and an entry for the desktop app.

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use hearth_core::config::ConfigDir;
use hearth_core::image::{BuildOptions, GATEWAY_PREBUILT, IMAGES, ImageDefinition, ImageStatus};
use hearth_core::lang::tr;
use hearth_core::podman::{MachineConfig, find_podman};
use hearth_core::sync::find_git;
use hearth_core::tr;
use hearth_core::vmd;

use crate::vmd::MachineSize;
use crate::{daemon, login, project};

/// What `hearth setup` was told on the command line.
pub struct Options {
    /// Server to pair with, instead of asking.
    pub server: Option<String>,
    /// Agree to every step without asking (downloads, admin rights).
    pub yes: bool,
}

/// Resources of a machine the assistant creates: what Phase 0 measured
/// as enough, the service starts it with 1 GB and lets it grow.
fn machine_size() -> MachineSize {
    let cpus = std::thread::available_parallelism()
        .map_or(2, |n| u32::try_from(n.get()).unwrap_or(2))
        .clamp(2, 4);
    MachineSize {
        cpus,
        memory: 4096,
        disk: 40,
    }
}

fn ok(text: impl AsRef<str>) {
    println!("✓ {}", text.as_ref());
}

fn warn(text: impl AsRef<str>) {
    println!("! {}", text.as_ref());
}

fn fail(text: impl AsRef<str>) {
    println!("✗ {}", text.as_ref());
}

struct Assistant {
    yes: bool,
    interactive: bool,
    /// Whether steps may change Windows itself (admin rights, winget);
    /// test runs with `HEARTH_HOME` only report them.
    system: bool,
}

impl Assistant {
    /// Whether the user agrees to `question`; without a terminal only with
    /// `--yes`.
    fn agrees(&self, question: &str) -> anyhow::Result<bool> {
        if self.yes {
            println!(
                "{}",
                tr!("{question} → ja (--yes)", "{question} → yes (--yes)")
            );
            return Ok(true);
        }
        if !self.interactive {
            println!(
                "{}",
                tr!(
                    "{question} → übersprungen (ohne Terminal: hearth setup --yes)",
                    "{question} → skipped (without a terminal: hearth setup --yes)"
                )
            );
            return Ok(false);
        }
        project::confirm(question, true)
    }

    /// [`Assistant::agrees`] for a step that changes Windows itself.
    fn agrees_system(&self, question: &str) -> anyhow::Result<bool> {
        if !self.system {
            println!(
                "{}",
                tr!(
                    "{question} → nicht im Testlauf (HEARTH_HOME)",
                    "{question} → not in a test run (HEARTH_HOME)"
                )
            );
            return Ok(false);
        }
        self.agrees(question)
    }

    fn ask(&self, question: &str) -> anyhow::Result<Option<String>> {
        if !self.interactive {
            return Ok(None);
        }
        print!("{question} ");
        std::io::Write::flush(&mut std::io::stdout())?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        let answer = answer.trim();
        Ok((!answer.is_empty()).then(|| answer.to_owned()))
    }
}

/// `hearth setup`. Returns whether everything is ready.
pub fn run(config: &ConfigDir, machine: &str, options: Options) -> anyhow::Result<bool> {
    let assistant = Assistant {
        yes: options.yes,
        interactive: std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        system: daemon::automatic(),
    };
    if !cfg!(windows) {
        return run_unix(&assistant, config, machine, options);
    }
    println!(
        "{}",
        tr(
            "Einrichtung von hearth auf diesem Rechner\n",
            "Setting up hearth on this computer\n"
        )
    );

    if !hyperv(&assistant)? {
        return Ok(false);
    }
    let git = tool(
        &assistant,
        "git",
        find_git().ok(),
        "Git.Git",
        tr("etwa 70 MB", "about 70 MB"),
        &["Git", "cmd", "git.exe"],
    )?;
    let podman = tool(
        &assistant,
        "Podman",
        find_podman().ok(),
        "RedHat.Podman",
        tr("etwa 30 MB", "about 30 MB"),
        &["RedHat", "Podman", "podman.exe"],
    )?;
    if git.is_none() || podman.is_none() {
        return Ok(false);
    }
    if !vm_service(&assistant, machine)? {
        return Ok(false);
    }
    if !images(&assistant, machine)? {
        return Ok(false);
    }
    let paired = server(&assistant, config, options.server)?;
    if paired {
        background(&assistant, config)?;
    }
    path(&assistant)?;
    desktop_entry(&assistant)?;
    println!();
    if paired {
        println!(
            "{}",
            tr(
                "Fertig. Im Ordner eines Projekts gibt `hearth init` es für Web-Chats frei, \
                 `hearth` startet den Agent im Terminal.",
                "Done. In a project's folder, `hearth init` offers it to web chats, `hearth` \
                 starts the agent in the terminal."
            )
        );
    } else {
        println!(
            "{}",
            tr(
                "Fertig für das Terminal: Im Ordner eines Projekts startet `hearth` den Agent. \
                 Für Web-Chats später `hearth login <Server>`.",
                "Done for the terminal: in a project's folder, `hearth` starts the agent. For \
                 web chats, later `hearth login <server>`."
            )
        );
    }
    Ok(true)
}

/// `hearth setup` on Linux and macOS: no VM service and nothing that needs
/// administrator rights; packages come from the system's package manager.
fn run_unix(
    assistant: &Assistant,
    config: &ConfigDir,
    machine: &str,
    options: Options,
) -> anyhow::Result<bool> {
    println!(
        "{}",
        tr(
            "Einrichtung von hearth auf diesem Rechner\n",
            "Setting up hearth on this computer\n"
        )
    );
    let install = if cfg!(target_os = "macos") {
        "brew install"
    } else {
        tr(
            "dem Paketmanager (apt, dnf, pacman …):",
            "the package manager (apt, dnf, pacman …):",
        )
    };
    match find_git() {
        Ok(git) => ok(tr!("git: {}", "git: {}", git.display())),
        Err(_) => {
            fail(tr!(
                "git fehlt. Installieren mit {install} git",
                "git is missing. Install it with {install} git"
            ));
            return Ok(false);
        }
    }
    match find_podman() {
        Ok(podman) => {
            ok(tr!("Podman: {}", "Podman: {}", podman.display()));
            sandbox_unix(assistant, &podman, machine)?;
        }
        Err(_) => warn(tr!(
            "Podman fehlt; der Direktmodus braucht es nicht. Für die Sandbox: {install} podman, dann `hearth setup` noch einmal",
            "Podman is missing; the direct mode does not need it. For the sandbox: {install} podman, then `hearth setup` again"
        )),
    }
    let paired = server(assistant, config, options.server)?;
    if paired {
        background(assistant, config)?;
    }
    desktop_entry(assistant)?;
    let here = std::env::current_exe()?;
    if let Some(dir) = here.parent()
        && !std::env::var_os("PATH")
            .is_some_and(|path| std::env::split_paths(&path).any(|entry| entry == dir))
    {
        warn(tr!(
            "{} steht nicht im PATH. In der Shell-Konfiguration ergänzen, z. B. export PATH=\"{}:$PATH\"",
            "{} is not on the PATH. Add it in the shell's configuration, e.g. export PATH=\"{}:$PATH\"",
            dir.display(),
            dir.display()
        ));
    }
    println!();
    println!(
        "{}",
        tr(
            "Fertig. Im Ordner eines Projekts: `hearth init`, dann `hearth project host on` für den Direktmodus (Agent im Ordner, jeder Befehl mit Freigabe). Freigaben gibst du in der Hearth-App oder mit `hearth host`.",
            "Done. In a project's folder: `hearth init`, then `hearth project host on` for the direct mode (agent in the folder, every command with approval). You answer approvals in the Hearth app or with `hearth host`."
        )
    );
    Ok(true)
}

/// The sandbox outside Windows: on macOS Podman's machine (created after a
/// question; it downloads about 1 GB), on Linux the computer's own rootless
/// Podman; then the images. A refusal leaves the direct mode.
fn sandbox_unix(assistant: &Assistant, podman: &Path, machine: &str) -> anyhow::Result<()> {
    if cfg!(target_os = "macos") && MachineConfig::load(machine).is_err() {
        if !assistant.agrees_system(&tr!(
            "Podman-Machine „{machine}“ für die Sandbox anlegen (lädt etwa 1 GB, 4 GB RAM, 40 GB Platte nach Bedarf)?",
            "Create the Podman machine “{machine}” for the sandbox (downloads about 1 GB, 4 GB RAM, 40 GB disk as needed)?"
        ))? {
            warn(tr(
                "Ohne Machine keine Sandbox; der Direktmodus geht trotzdem.",
                "Without a machine no sandbox; the direct mode works anyway.",
            ));
            return Ok(());
        }
        let created = Command::new(podman)
            .args([
                "machine",
                "init",
                machine,
                "--cpus",
                "2",
                "--memory",
                "4096",
                "--disk-size",
                "40",
            ])
            .status()?;
        if !created.success() {
            fail(tr!(
                "podman machine init {machine} gescheitert",
                "podman machine init {machine} failed"
            ));
            return Ok(());
        }
        ok(tr!(
            "Machine {machine} angelegt",
            "Machine {machine} created"
        ));
    }
    if cfg!(target_os = "linux") {
        ok(tr(
            "Sandbox mit dem rootless Podman dieses Rechners (ohne VM: die Container teilen den Kernel des Rechners)",
            "Sandbox with this computer's rootless Podman (no VM: the containers share the computer's kernel)",
        ));
    }
    if !images(assistant, machine)? {
        warn(tr(
            "Ohne Images keine Sandbox; später: `hearth image build`.",
            "Without images no sandbox; later: `hearth image build`.",
        ));
    }
    Ok(())
}

/// A menu entry for the desktop app (Linux: `~/.local/share/applications`,
/// Windows: a shortcut in the user's start menu; macOS gets an app bundle
/// from the package).
fn desktop_entry(assistant: &Assistant) -> anyhow::Result<()> {
    if cfg!(target_os = "macos") || !assistant.system {
        return Ok(());
    }
    let app = std::env::current_exe()?.with_file_name(if cfg!(windows) {
        "hearth-app.exe"
    } else {
        "hearth-app"
    });
    if !app.is_file() {
        return Ok(());
    }
    if cfg!(windows) {
        start_menu_shortcut(&app);
        return Ok(());
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map_or_else(
            || hearth_core::paths::home_dir().map(|home| home.join(".local/share")),
            Ok,
        )?;
    let file = data.join("applications").join("io.hearth.app.desktop");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Hearth\nComment={}\nExec=\"{}\"\nTerminal=false\nCategories=Development;Utility;\nStartupWMClass=io.hearth.app\n",
        tr(
            "Projekte, Freigaben und Verbindung von hearth",
            "Projects, approvals and connection of hearth"
        ),
        app.display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    );
    if std::fs::read_to_string(&file).is_ok_and(|text| text == entry) {
        ok(tr(
            "Hearth-App im Startmenü",
            "Hearth app in the start menu",
        ));
        return Ok(());
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&file, entry)?;
    ok(tr(
        "Hearth-App im Startmenü eingetragen",
        "Hearth app added to the start menu",
    ));
    Ok(())
}

/// Writes the shortcut unless one with this target exists; prints `same`
/// or `saved`. Paths come in through the environment, never as script text.
const SHORTCUT_PS: &str = r#"$ErrorActionPreference = 'Stop'
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($env:HEARTH_LINK)
if ((Test-Path -LiteralPath $env:HEARTH_LINK) -and $link.TargetPath -eq $env:HEARTH_TARGET) { 'same'; exit 0 }
$link.TargetPath = $env:HEARTH_TARGET
$link.WorkingDirectory = Split-Path -Parent $env:HEARTH_TARGET
$link.Description = $env:HEARTH_DESCRIPTION
$link.Save()
'saved'"#;

/// `Hearth.lnk` in the user's start menu. A `.lnk` needs COM, which
/// PowerShell has at hand; a failure is only a warning, the app still runs.
fn start_menu_shortcut(app: &Path) {
    let Some(appdata) = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
    else {
        return;
    };
    let link = [
        "Microsoft",
        "Windows",
        "Start Menu",
        "Programs",
        "Hearth.lnk",
    ]
    .iter()
    .fold(appdata, |path, part| path.join(part));
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", SHORTCUT_PS])
        .env("HEARTH_LINK", &link)
        .env("HEARTH_TARGET", app)
        .env(
            "HEARTH_DESCRIPTION",
            tr(
                "Projekte, Freigaben und Verbindung von hearth",
                "Projects, approvals and connection of hearth",
            ),
        )
        .output();
    match output {
        Ok(output) if String::from_utf8_lossy(&output.stdout).trim() == "same" => {
            ok(tr(
                "Hearth-App im Startmenü",
                "Hearth app in the start menu",
            ));
        }
        Ok(output) if String::from_utf8_lossy(&output.stdout).trim() == "saved" => ok(tr(
            "Hearth-App im Startmenü eingetragen",
            "Hearth app added to the start menu",
        )),
        Ok(output) => warn(tr!(
            "Kein Startmenü-Eintrag für die Hearth-App: {}",
            "No start menu entry for the Hearth app: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => warn(tr!(
            "Kein Startmenü-Eintrag für die Hearth-App: {error}",
            "No start menu entry for the Hearth app: {error}"
        )),
    }
}

/// Hyper-V switched on; if not, why not, and an offer to switch it on.
fn hyperv(assistant: &Assistant) -> anyhow::Result<bool> {
    if sys::service_exists("vmms") {
        ok(tr("Hyper-V ist eingeschaltet", "Hyper-V is switched on"));
        return Ok(true);
    }
    let edition = sys::edition().unwrap_or_default();
    if edition.starts_with("Core") {
        fail(tr!(
            "Windows Home ({edition}) hat kein Hyper-V. hearth braucht vorerst Windows Pro, \
             Enterprise oder Education; ein Weg über WSL ist geplant.",
            "Windows Home ({edition}) has no Hyper-V. For now hearth needs Windows Pro, \
             Enterprise or Education; a way through WSL is planned."
        ));
        return Ok(false);
    }
    if sys::firmware_virtualization() == Some(false) {
        fail(tr(
            "Die Virtualisierung ist im BIOS/UEFI ausgeschaltet. Dort einschalten (Intel VT-x, \
             AMD-V oder SVM), dann `hearth setup` erneut.",
            "Virtualization is switched off in the BIOS/UEFI. Switch it on there (Intel VT-x, \
             AMD-V or SVM), then run `hearth setup` again.",
        ));
        return Ok(false);
    }
    warn(tr(
        "Hyper-V ist ausgeschaltet; die VM von hearth braucht es.",
        "Hyper-V is switched off; the VM of hearth needs it.",
    ));
    if assistant.agrees_system(tr(
        "Hyper-V jetzt einschalten (Administratorrechte, danach Neustart)?",
        "Switch Hyper-V on now (administrator rights, then a restart)?",
    ))? && crate::vmd::enable_hyperv()?
    {
        println!(
            "{}",
            tr(
                "Bitte Windows neu starten und danach `hearth setup` erneut ausführen.",
                "Please restart Windows and then run `hearth setup` again."
            )
        );
    }
    Ok(false)
}

/// A program hearth needs: found, or installed with winget. `known` is
/// where winget puts it below Program Files; this process's PATH does not
/// know about it yet.
fn tool(
    assistant: &Assistant,
    name: &str,
    found: Option<PathBuf>,
    package: &str,
    size: &str,
    known: &[&str],
) -> anyhow::Result<Option<PathBuf>> {
    if let Some(path) = found {
        ok(format!("{name}: {}", path.display()));
        return Ok(Some(path));
    }
    warn(tr!("{name} fehlt.", "{name} is missing."));
    if !winget_available() {
        fail(tr!(
            "winget ist nicht da. {name} bitte von Hand installieren (winget-Paket {package}), \
             dann `hearth setup` erneut.",
            "winget is not available. Please install {name} by hand (winget package {package}), \
             then run `hearth setup` again."
        ));
        return Ok(None);
    }
    if !assistant.agrees_system(&tr!(
        "{name} mit winget installieren (lädt {size})?",
        "Install {name} with winget (downloads {size})?"
    ))? {
        return Ok(None);
    }
    let status = Command::new("winget")
        .args([
            "install",
            "--id",
            package,
            "--exact",
            "--source",
            "winget",
            "--accept-source-agreements",
            "--accept-package-agreements",
        ])
        .status();
    if !status.is_ok_and(|s| s.success()) {
        fail(tr!(
            "Die Installation von {name} ist gescheitert.",
            "The installation of {name} failed."
        ));
        return Ok(None);
    }
    let installed = std::env::var_os("ProgramFiles")
        .map(|dir| {
            known
                .iter()
                .fold(PathBuf::from(dir), |path, part| path.join(part))
        })
        .filter(|path| path.is_file());
    match installed {
        Some(path) => {
            ok(tr!(
                "{name} installiert: {}",
                "{name} installed: {}",
                path.display()
            ));
            // Later steps of this run look it up on PATH.
            if let (Some(dir), Some(current)) = (path.parent(), std::env::var_os("PATH")) {
                let mut paths: Vec<PathBuf> = std::env::split_paths(&current).collect();
                paths.push(dir.to_owned());
                if let Ok(joined) = std::env::join_paths(paths) {
                    // SAFETY: single-threaded at this point of the assistant.
                    unsafe { std::env::set_var("PATH", joined) };
                }
            }
            Ok(Some(path))
        }
        None => {
            warn(tr!(
                "{name} ist installiert, aber nicht am erwarteten Ort. Bitte ein neues Terminal \
                 öffnen und `hearth setup` erneut ausführen.",
                "{name} is installed, but not in the expected place. Please open a new terminal \
                 and run `hearth setup` again."
            ));
            Ok(None)
        }
    }
}

fn winget_available() -> bool {
    // winget is an app execution alias; only starting it tells whether it works.
    Command::new("winget")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// The VM service, taking over the machine or creating it first.
fn vm_service(assistant: &Assistant, machine: &str) -> anyhow::Result<bool> {
    if vmd::available() {
        if crate::vmd::service_outdated() == Some(true) {
            warn(tr(
                "Der VM-Dienst ist ein anderer Stand als dieses hearth.",
                "The VM service is a different version than this hearth.",
            ));
            if assistant.agrees_system(tr(
                "VM-Dienst aktualisieren (Administratorrechte; die VM startet dabei neu)?",
                "Update the VM service (administrator rights; the VM restarts)?",
            ))? {
                return crate::vmd::install(machine, None);
            }
        }
        ok(tr("VM-Dienst läuft", "VM service running"));
        return Ok(true);
    }
    let exists = MachineConfig::default_path(machine)?.is_file();
    let agreed = if exists {
        assistant.agrees_system(&tr!(
            "VM-Dienst einrichten und die VM {machine} übernehmen (Administratorrechte)?",
            "Set up the VM service and let it take over the VM {machine} (administrator rights)?"
        ))?
    } else {
        let size = machine_size();
        println!(
            "{}",
            tr!(
                "Noch keine VM {machine}. hearth legt sie mit {} CPUs, bis {} MiB RAM und {} GiB \
                 Platte an (belegt nur, was sie braucht).",
                "No VM {machine} yet. hearth creates it with {} CPUs, up to {} MiB RAM and {} GiB \
                 disk (it only takes up what it needs).",
                size.cpus,
                size.memory,
                size.disk
            )
        );
        assistant.agrees_system(tr(
            "VM anlegen und VM-Dienst einrichten (Administratorrechte, lädt etwa 1 GB)?",
            "Create the VM and set up the VM service (administrator rights, downloads about 1 GB)?",
        ))?
    };
    if !agreed {
        return Ok(false);
    }
    let size = (!exists).then(machine_size);
    if !crate::vmd::install(machine, size)? || !vmd::available() {
        fail(tr(
            "Der VM-Dienst läuft nicht; Details oben und in C:\\ProgramData\\hearth\\vmd\\vmd.log.",
            "The VM service is not running; details above and in C:\\ProgramData\\hearth\\vmd\\vmd.log.",
        ));
        return Ok(false);
    }
    Ok(true)
}

/// The base and gateway images in the VM, built if missing or outdated.
fn images(assistant: &Assistant, machine: &str) -> anyhow::Result<bool> {
    let provider = crate::vm::provider(machine)?;
    let mut missing: Vec<ImageDefinition> = Vec::new();
    for image in IMAGES {
        match image.status(&provider)? {
            ImageStatus::Current => ok(tr!("Image {} aktuell", "Image {} up to date", image.tag)),
            ImageStatus::Outdated => {
                warn(tr!("Image {} veraltet", "Image {} outdated", image.tag));
                missing.push(*image);
            }
            ImageStatus::Missing => {
                warn(tr!("Image {} fehlt", "Image {} missing", image.tag));
                missing.push(*image);
            }
        }
    }
    if missing.is_empty() {
        return Ok(true);
    }
    // Without the gateway program in this hearth the VM compiles it and
    // first downloads the Rust image.
    let question = if GATEWAY_PREBUILT {
        tr(
            "Images in der VM bauen (lädt Basis-Images, beim ersten Mal etwa 250 MB; dauert \
             einige Minuten)?",
            "Build the images in the VM (downloads base images, about 250 MB the first time; \
             takes a few minutes)?",
        )
    } else {
        tr(
            "Images in der VM bauen (lädt Basis-Images und Rust zum Kompilieren des Gateways, \
             beim ersten Mal etwa 600 MB; dauert einige Minuten)?",
            "Build the images in the VM (downloads base images and Rust to compile the gateway, \
             about 600 MB the first time; takes a few minutes)?",
        )
    };
    if !assistant.agrees(question)? {
        return Ok(false);
    }
    crate::build_images(machine, &missing, BuildOptions::default())?;
    Ok(true)
}

/// Pairing with a hearth server; asks for its address if there is none.
fn server(assistant: &Assistant, config: &ConfigDir, url: Option<String>) -> anyhow::Result<bool> {
    if let Some((client, _)) = login::paired(config)? {
        ok(tr!("Gekoppelt mit {}", "Paired with {}", client.url()));
        return Ok(true);
    }
    let url = match url {
        Some(url) => Some(url),
        None => assistant.ask(tr(
            "Adresse deines hearth-Servers für Web-Chats (z. B. https://hearth.example.org oder im Heimnetz http://192.168.1.10:8080; leer: später)?",
            "Address of your hearth server for web chats (e.g. https://hearth.example.org, or at home http://192.168.1.10:8080; empty: later)?",
        ))?,
    };
    let Some(url) = url else {
        warn(tr(
            "Nicht mit einem Server gekoppelt; Web-Chats gehen erst nach `hearth login <Server>`.",
            "Not paired with a server; web chats only work after `hearth login <server>`.",
        ));
        return Ok(false);
    };
    // Without a terminal (scripts, tests) the link is printed, not opened.
    login::login(config, Some(url), assistant.interactive)
}

/// The background service at logon, for web chats.
fn background(assistant: &Assistant, config: &ConfigDir) -> anyhow::Result<()> {
    if !daemon::automatic() {
        warn(tr(
            "Mit HEARTH_HOME richtet setup keinen Autostart ein (`hearth daemon start`).",
            "With HEARTH_HOME, setup does not set up an autostart (`hearth daemon start`).",
        ));
        return Ok(());
    }
    let running = tr(
        "Hintergrunddienst läuft und startet bei der Anmeldung",
        "Background service running and starting at sign-in",
    );
    if daemon::autostart().is_some() && daemon::running(config) {
        ok(running);
        return Ok(());
    }
    if assistant.agrees(tr(
        "Hintergrunddienst für Web-Chats bei jeder Anmeldung starten?",
        "Start the background service for web chats at every sign-in?",
    ))? {
        if daemon::install(config)? {
            ok(running);
        } else {
            fail(tr(
                "Der Hintergrunddienst startet nicht, siehe `hearth daemon`.",
                "The background service does not start, see `hearth daemon`.",
            ));
        }
    }
    Ok(())
}

/// Whether `dir` is one of the entries of a PATH value.
/// A PATH value without the entries naming `dir`.
fn without_entry(value: &str, dir: &Path) -> String {
    let wanted = dir
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase();
    value
        .split(';')
        .filter(|entry| entry.trim().trim_end_matches(['\\', '/']).to_lowercase() != wanted)
        .collect::<Vec<_>>()
        .join(";")
}

fn in_path(value: &str, dir: &Path) -> bool {
    let wanted = dir
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase();
    value
        .split(';')
        .any(|entry| entry.trim().trim_end_matches(['\\', '/']).to_lowercase() == wanted)
}

/// `hearth` callable from every new terminal.
fn path(assistant: &Assistant) -> anyhow::Result<()> {
    // Test runs must not change the user's PATH.
    if !daemon::automatic() {
        return Ok(());
    }
    let Some(dir) = std::env::current_exe()?.parent().map(Path::to_owned) else {
        return Ok(());
    };
    let current = std::env::var("PATH").unwrap_or_default();
    let mut user = sys::user_path().unwrap_or_default();
    // The folder of llm-harness (before 0.2.0) leaves the PATH, so `hearth`
    // no longer starts the old version.
    if let Some(old) = dir.parent().map(|parent| parent.join("llm-harness")) {
        let kept = without_entry(&user, &old);
        if kept != user {
            sys::set_user_path(&kept)?;
            ok(tr!(
                "{} aus dem PATH entfernt",
                "{} removed from the PATH",
                old.display()
            ));
            user = kept;
        }
    }
    if in_path(&current, &dir) || in_path(&user, &dir) {
        ok(tr!("{} ist im PATH", "{} is on the PATH", dir.display()));
        return Ok(());
    }
    if assistant.agrees(&tr!(
        "{} zum PATH hinzufügen, damit `hearth` in jedem neuen Terminal geht?",
        "Add {} to the PATH, so `hearth` works in every new terminal?",
        dir.display()
    ))? {
        let value = if user.trim().is_empty() {
            dir.display().to_string()
        } else {
            format!("{};{}", user.trim_end_matches(';'), dir.display())
        };
        sys::set_user_path(&value)?;
        ok(tr(
            "PATH ergänzt; gilt in neuen Terminals",
            "PATH extended; applies to new terminals",
        ));
    }
    Ok(())
}

#[cfg(windows)]
mod sys {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt as _;
    use std::process::Command;

    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_EXPAND_SZ, RRF_NOEXPAND,
        RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegGetValueW, RegSetKeyValueW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };

    fn wide(text: &str) -> Vec<u16> {
        OsStr::new(text).encode_wide().chain([0]).collect()
    }

    /// A string value from the registry, unexpanded.
    fn registry_string(root: HKEY, key: &str, value: &str) -> Option<String> {
        let key = wide(key);
        let name = wide(value);
        let mut buffer = vec![0u16; 32 * 1024];
        let mut size = u32::try_from(buffer.len() * 2).unwrap_or(u32::MAX);
        // SAFETY: the buffer and its size match; strings are terminated.
        let status = unsafe {
            RegGetValueW(
                root,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != 0 {
            return None;
        }
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..len]))
    }

    /// `EditionID` of Windows, e.g. `Professional` or `Core` (Home).
    pub fn edition() -> Option<String> {
        registry_string(
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            "EditionID",
        )
    }

    /// Whether a Windows service exists (Hyper-V's `vmms` only with Hyper-V).
    pub fn service_exists(name: &str) -> bool {
        Command::new("sc.exe")
            .args(["query", name])
            .output()
            .is_ok_and(|o| o.status.success())
    }

    /// Whether the firmware has virtualization switched on; `None` if unknown
    /// or a hypervisor already runs (then the flag reads false anyway).
    pub fn firmware_virtualization() -> Option<bool> {
        let output = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$s = Get-CimInstance Win32_ComputerSystem; $p = Get-CimInstance Win32_Processor | \
                 Select-Object -First 1; \"$($s.HypervisorPresent) $($p.VirtualizationFirmwareEnabled)\"",
            ])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&output.stdout);
        let mut parts = text.split_whitespace();
        match (parts.next(), parts.next()) {
            (Some("True"), _) => None,
            (_, Some("True")) => Some(true),
            (_, Some("False")) => Some(false),
            _ => None,
        }
    }

    /// The user's own PATH, unexpanded.
    pub fn user_path() -> Option<String> {
        registry_string(HKEY_CURRENT_USER, "Environment", "Path")
    }

    /// Writes the user's PATH (keeping `%…%` references) and tells running
    /// programs, so new terminals see it.
    pub fn set_user_path(value: &str) -> anyhow::Result<()> {
        let key = wide("Environment");
        let name = wide("Path");
        let data = wide(value);
        // SAFETY: terminated strings; the size counts the terminator.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_EXPAND_SZ,
                data.as_ptr().cast(),
                u32::try_from(data.len() * 2).unwrap_or(u32::MAX),
            )
        };
        if status != 0 {
            anyhow::bail!(hearth_core::tr!(
                "PATH nicht geschrieben: {}",
                "PATH not written: {}",
                std::io::Error::from_raw_os_error(status as i32)
            ));
        }
        let environment = wide("Environment");
        let mut result = 0usize;
        // SAFETY: a broadcast with a terminated string and a time limit.
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST as HWND,
                WM_SETTINGCHANGE,
                0,
                environment.as_ptr() as isize,
                SMTO_ABORTIFHUNG,
                2_000,
                &mut result,
            );
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod sys {
    pub fn edition() -> Option<String> {
        None
    }
    pub fn service_exists(_name: &str) -> bool {
        false
    }
    pub fn firmware_virtualization() -> Option<bool> {
        None
    }
    pub fn user_path() -> Option<String> {
        None
    }
    pub fn set_user_path(_value: &str) -> anyhow::Result<()> {
        anyhow::bail!(hearth_core::lang::tr(
            "nur unter Windows",
            "only on Windows"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_entries_match_without_case_and_trailing_slash() {
        let dir = Path::new(r"C:\Tools\Hearth");
        assert!(in_path(
            r"C:\Windows;c:\tools\hearth\;%USERPROFILE%\bin",
            dir
        ));
        assert!(!in_path(r"C:\Windows;C:\Tools\Hearth2", dir));
        assert!(!in_path("", dir));
    }

    #[test]
    fn the_old_folder_leaves_the_path() {
        let old = Path::new(r"C:\Users\u\AppData\Local\Programs\llm-harness");
        assert_eq!(
            without_entry(
                r"C:\Windows;c:\users\u\appdata\local\programs\llm-harness\;%USERPROFILE%\bin",
                old
            ),
            r"C:\Windows;%USERPROFILE%\bin"
        );
        assert_eq!(without_entry(r"C:\Windows", old), r"C:\Windows");
    }

    #[test]
    fn machines_stay_modest() {
        let size = machine_size();
        assert!((2..=4).contains(&size.cpus));
        assert_eq!((size.memory, size.disk), (4096, 40));
    }
}
