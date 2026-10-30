//! `harness setup`: the setup assistant (ROADMAP Phase 3). It checks one
//! step after the other and does only what is missing, asking before admin
//! rights (UAC) and large downloads:
//!
//! 1. Windows with Hyper-V (switched on if needed; then a restart),
//! 2. git and Podman (through winget),
//! 3. the VM service and the machine (created if there is none),
//! 4. the images in the VM,
//! 5. pairing with the harness server and the background service at logon,
//! 6. `harness` on the user's PATH.
//!
//! Run it again after a restart or a fix: finished steps only show ✓.

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use harness_core::config::ConfigDir;
use harness_core::image::{BuildOptions, GATEWAY_PREBUILT, IMAGES, ImageDefinition, ImageStatus};
use harness_core::podman::{MachineConfig, find_podman};
use harness_core::sync::find_git;
use harness_core::vmd;

use crate::vmd::MachineSize;
use crate::{daemon, login, project};

/// What `harness setup` was told on the command line.
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
    /// test runs with `HARNESS_HOME` only report them.
    system: bool,
}

impl Assistant {
    /// Whether the user agrees to `question`; without a terminal only with
    /// `--yes`.
    fn agrees(&self, question: &str) -> anyhow::Result<bool> {
        if self.yes {
            println!("{question} → ja (--yes)");
            return Ok(true);
        }
        if !self.interactive {
            println!("{question} → übersprungen (ohne Terminal: harness setup --yes)");
            return Ok(false);
        }
        project::confirm(question, true)
    }

    /// [`Assistant::agrees`] for a step that changes Windows itself.
    fn agrees_system(&self, question: &str) -> anyhow::Result<bool> {
        if !self.system {
            println!("{question} → nicht im Testlauf (HARNESS_HOME)");
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

/// `harness setup`. Returns whether everything is ready.
pub fn run(config: &ConfigDir, machine: &str, options: Options) -> anyhow::Result<bool> {
    if !cfg!(windows) {
        anyhow::bail!(
            "`harness setup` richtet Windows ein; auf anderen Systemen gibt es das noch nicht."
        );
    }
    let assistant = Assistant {
        yes: options.yes,
        interactive: std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
        system: daemon::automatic(),
    };
    println!("Einrichtung von harness auf diesem Rechner\n");

    if !hyperv(&assistant)? {
        return Ok(false);
    }
    let git = tool(
        &assistant,
        "git",
        find_git().ok(),
        "Git.Git",
        "etwa 70 MB",
        &["Git", "cmd", "git.exe"],
    )?;
    let podman = tool(
        &assistant,
        "Podman",
        find_podman().ok(),
        "RedHat.Podman",
        "etwa 30 MB",
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
    println!();
    if paired {
        println!(
            "Fertig. Im Ordner eines Projekts gibt `harness init` es für Web-Chats frei, \
             `harness` startet den Agent im Terminal."
        );
    } else {
        println!(
            "Fertig für das Terminal: Im Ordner eines Projekts startet `harness` den Agent. Für \
             Web-Chats später `harness login <Server>`."
        );
    }
    Ok(true)
}

/// Hyper-V switched on; if not, why not, and an offer to switch it on.
fn hyperv(assistant: &Assistant) -> anyhow::Result<bool> {
    if sys::service_exists("vmms") {
        ok("Hyper-V ist eingeschaltet");
        return Ok(true);
    }
    let edition = sys::edition().unwrap_or_default();
    if edition.starts_with("Core") {
        fail(format!(
            "Windows Home ({edition}) hat kein Hyper-V. harness braucht vorerst Windows Pro, \
             Enterprise oder Education; ein Weg über WSL ist geplant."
        ));
        return Ok(false);
    }
    if sys::firmware_virtualization() == Some(false) {
        fail(
            "Die Virtualisierung ist im BIOS/UEFI ausgeschaltet. Dort einschalten (Intel VT-x, \
             AMD-V oder SVM), dann `harness setup` erneut.",
        );
        return Ok(false);
    }
    warn("Hyper-V ist ausgeschaltet; die VM von harness braucht es.");
    if assistant
        .agrees_system("Hyper-V jetzt einschalten (Administratorrechte, danach Neustart)?")?
        && crate::vmd::enable_hyperv()?
    {
        println!("Bitte Windows neu starten und danach `harness setup` erneut ausführen.");
    }
    Ok(false)
}

/// A program harness needs: found, or installed with winget. `known` is
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
    warn(format!("{name} fehlt."));
    if !winget_available() {
        fail(format!(
            "winget ist nicht da. {name} bitte von Hand installieren (winget-Paket {package}), \
             dann `harness setup` erneut."
        ));
        return Ok(None);
    }
    if !assistant.agrees_system(&format!("{name} mit winget installieren (lädt {size})?"))? {
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
        fail(format!("Die Installation von {name} ist gescheitert."));
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
            ok(format!("{name} installiert: {}", path.display()));
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
            warn(format!(
                "{name} ist installiert, aber nicht am erwarteten Ort. Bitte ein neues Terminal \
                 öffnen und `harness setup` erneut ausführen."
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
            warn("Der VM-Dienst ist ein anderer Stand als dieses harness.");
            if assistant.agrees_system(
                "VM-Dienst aktualisieren (Administratorrechte; die VM startet dabei neu)?",
            )? {
                return crate::vmd::install(machine, None);
            }
        }
        ok("VM-Dienst läuft");
        return Ok(true);
    }
    let exists = MachineConfig::default_path(machine)?.is_file();
    let agreed = if exists {
        assistant.agrees_system(&format!(
            "VM-Dienst einrichten und die VM {machine} übernehmen (Administratorrechte)?"
        ))?
    } else {
        let size = machine_size();
        println!(
            "Noch keine VM {machine}. harness legt sie mit {} CPUs, bis {} MiB RAM und {} GiB \
             Platte an (belegt nur, was sie braucht).",
            size.cpus, size.memory, size.disk
        );
        assistant.agrees_system(
            "VM anlegen und VM-Dienst einrichten (Administratorrechte, lädt etwa 1 GB)?",
        )?
    };
    if !agreed {
        return Ok(false);
    }
    let size = (!exists).then(machine_size);
    if !crate::vmd::install(machine, size)? || !vmd::available() {
        fail(
            "Der VM-Dienst läuft nicht; Details oben und in C:\\ProgramData\\llm-harness\\vmd\\vmd.log.",
        );
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
            ImageStatus::Current => ok(format!("Image {} aktuell", image.tag)),
            ImageStatus::Outdated => {
                warn(format!("Image {} veraltet", image.tag));
                missing.push(*image);
            }
            ImageStatus::Missing => {
                warn(format!("Image {} fehlt", image.tag));
                missing.push(*image);
            }
        }
    }
    if missing.is_empty() {
        return Ok(true);
    }
    // Without the gateway program in this harness the VM compiles it and
    // first downloads the Rust image.
    let question = if GATEWAY_PREBUILT {
        "Images in der VM bauen (lädt Basis-Images, beim ersten Mal etwa 250 MB; dauert einige \
         Minuten)?"
    } else {
        "Images in der VM bauen (lädt Basis-Images und Rust zum Kompilieren des Gateways, beim \
         ersten Mal etwa 600 MB; dauert einige Minuten)?"
    };
    if !assistant.agrees(question)? {
        return Ok(false);
    }
    crate::build_images(machine, &missing, BuildOptions::default())?;
    Ok(true)
}

/// Pairing with a harness server; asks for its address if there is none.
fn server(assistant: &Assistant, config: &ConfigDir, url: Option<String>) -> anyhow::Result<bool> {
    if let Some((client, _)) = login::paired(config)? {
        ok(format!("Gekoppelt mit {}", client.url()));
        return Ok(true);
    }
    let url = match url {
        Some(url) => Some(url),
        None => assistant.ask(
            "Adresse deines harness-Servers für Web-Chats (z. B. http://192.168.1.20:8085; leer: später)?",
        )?,
    };
    let Some(url) = url else {
        warn(
            "Nicht mit einem Server gekoppelt; Web-Chats gehen erst nach `harness login <Server>`.",
        );
        return Ok(false);
    };
    // Without a terminal (scripts, tests) the link is printed, not opened.
    login::login(config, Some(url), assistant.interactive)
}

/// The background service at logon, for web chats.
fn background(assistant: &Assistant, config: &ConfigDir) -> anyhow::Result<()> {
    if !daemon::automatic() {
        warn("Mit HARNESS_HOME richtet setup keinen Autostart ein (`harness daemon start`).");
        return Ok(());
    }
    if daemon::autostart().is_some() && daemon::running(config) {
        ok("Hintergrunddienst läuft und startet bei der Anmeldung");
        return Ok(());
    }
    if assistant.agrees("Hintergrunddienst für Web-Chats bei jeder Anmeldung starten?")? {
        if daemon::install(config)? {
            ok("Hintergrunddienst läuft und startet bei der Anmeldung");
        } else {
            fail("Der Hintergrunddienst startet nicht, siehe `harness daemon`.");
        }
    }
    Ok(())
}

/// Whether `dir` is one of the entries of a PATH value.
fn in_path(value: &str, dir: &Path) -> bool {
    let wanted = dir
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_lowercase();
    value
        .split(';')
        .any(|entry| entry.trim().trim_end_matches(['\\', '/']).to_lowercase() == wanted)
}

/// `harness` callable from every new terminal.
fn path(assistant: &Assistant) -> anyhow::Result<()> {
    // Test runs must not change the user's PATH.
    if !daemon::automatic() {
        return Ok(());
    }
    let Some(dir) = std::env::current_exe()?.parent().map(Path::to_owned) else {
        return Ok(());
    };
    let current = std::env::var("PATH").unwrap_or_default();
    let user = sys::user_path().unwrap_or_default();
    if in_path(&current, &dir) || in_path(&user, &dir) {
        ok(format!("{} ist im PATH", dir.display()));
        return Ok(());
    }
    if assistant.agrees(&format!(
        "{} zum PATH hinzufügen, damit `harness` in jedem neuen Terminal geht?",
        dir.display()
    ))? {
        let value = if user.trim().is_empty() {
            dir.display().to_string()
        } else {
            format!("{};{}", user.trim_end_matches(';'), dir.display())
        };
        sys::set_user_path(&value)?;
        ok("PATH ergänzt; gilt in neuen Terminals");
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
            anyhow::bail!(
                "PATH nicht geschrieben: {}",
                std::io::Error::from_raw_os_error(status as i32)
            );
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
        anyhow::bail!("nur unter Windows")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_entries_match_without_case_and_trailing_slash() {
        let dir = Path::new(r"C:\Tools\Harness");
        assert!(in_path(
            r"C:\Windows;c:\tools\harness\;%USERPROFILE%\bin",
            dir
        ));
        assert!(!in_path(r"C:\Windows;C:\Tools\Harness2", dir));
        assert!(!in_path("", dir));
    }

    #[test]
    fn machines_stay_modest() {
        let size = machine_size();
        assert!((2..=4).contains(&size.cpus));
        assert_eq!((size.memory, size.disk), (4096, 40));
    }
}
