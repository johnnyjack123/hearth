//! `harness vmd`: the VM service. `install` asks once for admin rights
//! (UAC) and then runs this binary elevated to set the service up; after
//! that `harness` starts the VM itself when needed.
//!
//! Setting it up as admin:
//! 1. stop the VM with the user's own podman,
//! 2. lock `C:\ProgramData\llm-harness` down to administrators (owner
//!    included, so a folder a user process created beforehand cannot be
//!    reclaimed), the user may only read,
//! 3. put `harness-vmd.exe` below Program Files,
//! 4. take the machine over: its JSON moves into the service directory,
//!    without host folder shares; the disk and SSH key stay where they are,
//! 5. register and start the service, which then starts the VM.
//!
//! `uninstall` reverses 3 to 5; the machine is the user's again.
//!
//! For `harness setup`, the elevated half can also create the machine
//! first (`podman machine init`, with the user's profile like a manual
//! init in an admin PowerShell) and switch Hyper-V on.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};
use clap::Subcommand;
use harness_core::podman::find_podman;
use harness_core::vmd::{self, Request, Response, SERVICE_NAME, VmState};

#[derive(Subcommand)]
pub enum VmdCommand {
    /// Zeigt, ob der Dienst läuft und in welchem Zustand die VM ist (Standard)
    Status,
    /// Startet die VM über den Dienst
    Start,
    /// Stoppt die VM über den Dienst
    Stop,
    /// Startet die VM neu, z. B. wenn SSH nicht mehr antwortet
    Restart,
    /// Richtet den Dienst ein (fragt einmal nach Administratorrechten)
    Install,
    /// Entfernt den Dienst wieder (fragt einmal nach Administratorrechten)
    Uninstall,
    /// Wird von `install` mit Administratorrechten aufgerufen
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
        /// Legt die Machine erst an: CPUs, Arbeitsspeicher (MiB), Platte (GiB)
        #[arg(long, num_args = 3, value_names = ["CPUS", "MIB", "GIB"])]
        create: Option<Vec<u32>>,
    },
    /// Wird von `harness setup` mit Administratorrechten aufgerufen
    #[command(hide = true)]
    HypervElevated {
        #[arg(long)]
        log: PathBuf,
    },
    /// Wird von `uninstall` mit Administratorrechten aufgerufen
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

/// `harness vmd …`.
pub fn run(machine: &str, command: Option<VmdCommand>) -> anyhow::Result<bool> {
    match command.unwrap_or(VmdCommand::Status) {
        VmdCommand::Status => status(),
        VmdCommand::Start => ask(Request::Start, "Starte die VM …"),
        VmdCommand::Stop => ask(Request::Stop, "Stoppe die VM …"),
        VmdCommand::Restart => ask(Request::Restart, "Starte die VM neu …"),
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
        VmState::Running => "läuft",
        VmState::Unresponsive => "läuft, antwortet aber nicht (harness vmd restart)",
        VmState::Stopped => "gestoppt",
        VmState::Unknown => "unbekannt",
    }
}

fn status() -> anyhow::Result<bool> {
    match vmd::request(Request::Status) {
        Ok(answer) => {
            println!("Dienst {SERVICE_NAME}: läuft");
            println!("VM: {}", describe(answer.state));
            Ok(true)
        }
        Err(error) => {
            println!("{error}");
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
    let exe = std::env::current_exe().context("Pfad von harness.exe")?;
    let service = exe.with_file_name("harness-vmd.exe");
    if !service.is_file() {
        bail!(
            "{} fehlt; sie wird mit harness.exe gebaut (cargo build)",
            service.display()
        );
    }
    Ok(service)
}

/// Whether the installed service is another build than the one next to
/// `harness.exe` (after an update); `None` if either is missing.
pub fn service_outdated() -> Option<bool> {
    let installed = fs::read(program_dir().join("harness-vmd.exe")).ok()?;
    let here = fs::read(service_exe_next_to_harness().ok()?).ok()?;
    Some(installed != here)
}

fn log_file(action: &str) -> PathBuf {
    std::env::temp_dir().join(format!("harness-vmd-{action}-{}.log", std::process::id()))
}

/// Resources of a machine `harness setup` creates.
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
    let profile = harness_core::paths::home_dir()?;
    let log = log_file("install");
    println!("Richte den VM-Dienst ein. Windows fragt dafür einmal nach Administratorrechten.");
    if create.is_some() {
        println!(
            "Zuerst entsteht die VM {machine}; podman lädt dafür ihr Betriebssystem (etwa 1 GB)."
        );
    } else {
        println!(
            "Die VM {machine} wird dabei gestoppt und vom Dienst übernommen (Images und Volumes bleiben)."
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
        println!("Einrichtung fehlgeschlagen (Exit-Code {code}).");
        return Ok(false);
    }
    println!();
    status()
}

/// Switches Hyper-V on (UAC); Windows needs a restart afterwards.
pub fn enable_hyperv() -> anyhow::Result<bool> {
    let log = log_file("hyperv");
    println!("Schalte Hyper-V ein. Windows fragt dafür einmal nach Administratorrechten.");
    let args: Vec<String> = vec![
        "vmd".into(),
        "hyperv-elevated".into(),
        "--log".into(),
        log.display().to_string(),
    ];
    Ok(run_elevated(&args, &log)? == 0)
}

fn uninstall(machine: &str) -> anyhow::Result<bool> {
    let profile = harness_core::paths::home_dir()?;
    let log = log_file("uninstall");
    println!("Entferne den VM-Dienst. Windows fragt dafür einmal nach Administratorrechten.");
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
        println!("  {}", harness_core::sync::sanitize(line));
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
    let line = harness_core::pty::command_line(&command);
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
            bail!("Abgebrochen: Die Administratorrechte wurden nicht erteilt");
        }
        bail!("Start mit Administratorrechten fehlgeschlagen: {error}");
    }
    if info.hProcess.is_null() {
        bail!("Kein Prozess für die Einrichtung erhalten");
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
    bail!("Den VM-Dienst gibt es nur unter Windows")
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
        file: fs::File::create(log).with_context(|| format!("{} anlegen", log.display()))?,
    };
    if !is_elevated() {
        log.line("✗ läuft ohne Administratorrechte");
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
    let output = command
        .output()
        .with_context(|| format!("{what}: Programm nicht gestartet"))?;
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

/// `C:\Program Files\llm-harness`.
fn program_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("ProgramFiles").unwrap_or("C:\\Program Files".into()))
        .join("llm-harness")
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
const MOVED: &str = "moved-to-harness-vmd";

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
        log.line("✓ laufenden Dienst gestoppt");
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
        "# Geschrieben von `harness vmd install`; nur Administratoren dürfen sie ändern.\n\
         machine = \"{machine}\"\n\
         podman = '{}'\n\
         user_sid = \"{sid}\"\n",
        podman.display()
    );
    harness_core::provider::validate_resource_name("Machine-Name", machine)?;
    if !vmd::is_user_sid(sid) {
        bail!("{sid} ist keine Nutzer-SID");
    }
    if !vmd::below_program_files(podman) || podman.display().to_string().contains('\'') {
        bail!(
            "{} liegt nicht unter Program Files; der Dienst führt als SYSTEM nur aus, was nur \
             Administratoren ändern können",
            podman.display()
        );
    }
    let source = service_exe_next_to_harness()?;
    let user_json = user_machine_json(profile, machine);
    let service_json = vmd::machine_json(machine);
    if !service_json.is_file() && !user_json.is_file() {
        let Some(size) = create else {
            bail!("Machine {machine} nicht gefunden: {}", user_json.display());
        };
        create_machine(log, podman, machine, size)?;
        if !user_json.is_file() {
            bail!(
                "podman hat die Machine angelegt, aber {} fehlt",
                user_json.display()
            );
        }
    }

    // 1. The VM stops with the user's podman, which still owns it.
    if user_json.is_file() {
        let output = Command::new(podman)
            .args(["machine", "stop", machine])
            .env("CONTAINERS_MACHINE_PROVIDER", "hyperv")
            .output()
            .context("podman machine stop")?;
        log.line(&format!(
            "✓ VM gestoppt ({})",
            String::from_utf8_lossy(&output.stdout).trim()
        ));
    }

    // 2. Lock the data directory down: owner and ACL.
    let root = vmd::service_dir()
        .parent()
        .map(Path::to_owned)
        .context("Dienstordner ohne Elternordner")?;
    fs::create_dir_all(vmd::service_dir())?;
    let icacls = system32("icacls.exe");
    step(
        log,
        "Besitzer des Datenordners: Administratoren",
        Command::new(&icacls)
            .arg(&root)
            .args(["/setowner", "*S-1-5-32-544", "/T", "/C", "/Q"]),
    )?;
    step(
        log,
        "Rechte im Datenordner zurückgesetzt",
        Command::new(&icacls)
            .arg(&root)
            .args(["/reset", "/T", "/C", "/Q"]),
    )?;
    step(
        log,
        "Datenordner nur für Administratoren beschreibbar",
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
    log.line("✓ vmd.toml und containers.conf geschrieben");

    // 3. The service binary below Program Files.
    stop_service(log);
    fs::create_dir_all(program_dir())?;
    let target = program_dir().join("harness-vmd.exe");
    fs::copy(&source, &target)
        .with_context(|| format!("{} nach {} kopieren", source.display(), target.display()))?;
    log.line(&format!("✓ {}", target.display()));

    // 4. Take the machine over, without host folder shares.
    if !service_json.is_file() {
        let text = fs::read_to_string(&user_json)?;
        let mut json: serde_json::Value = serde_json::from_str(&text)
            .with_context(|| format!("{} lesen", user_json.display()))?;
        json["Mounts"] = serde_json::Value::Array(Vec::new());
        let dir = service_json.parent().context("Machine-Ordner")?;
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
        log.line(&format!(
            "✓ Machine {machine} übernommen, ohne Freigaben ({})",
            service_json.display()
        ));
    } else {
        log.line("✓ Machine gehört dem Dienst schon");
    }

    // 5. Register (or update) and start the service.
    let sc = system32("sc.exe");
    let bin_path = format!("\"{}\"", target.display());
    if service_state().is_some() {
        step(
            log,
            "Dienst aktualisiert",
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
            "Dienst angelegt",
            Command::new(&sc).args([
                "create",
                SERVICE_NAME,
                "binPath=",
                &bin_path,
                "start=",
                "auto",
                "DisplayName=",
                "llm-harness VM",
            ]),
        )?;
    }
    let _ = Command::new(&sc)
        .args([
            "description",
            SERVICE_NAME,
            "Startet und stoppt die VM von llm-harness für einen Nutzer.",
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
        "Dienst gestartet",
        Command::new(&sc).args(["start", SERVICE_NAME]),
    )?;

    // The pipe appears once the service read its configuration.
    let deadline = Instant::now() + Duration::from_secs(15);
    while !vmd::available() {
        if Instant::now() > deadline {
            bail!(
                "Der Dienst antwortet nicht; Details in {}",
                vmd::service_dir().join("vmd.log").display()
            );
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    log.line("… VM wird gestartet");
    let answer = vmd::request(Request::Start)?;
    if !answer.ok {
        bail!("VM-Start über den Dienst: {}", answer.message);
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
    log.line(&format!(
        "… lege die VM {machine} an ({} CPUs, bis {} MiB RAM, {} GiB Platte); das dauert ein paar Minuten",
        size.cpus, size.memory, size.disk
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
        .context("podman machine init: Programm nicht gestartet")?;
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
    log.line(&format!("✓ VM {machine} angelegt"));
    Ok(())
}

/// Switches the Hyper-V feature on; DISM exits with 3010 when Windows
/// needs a restart for it, which is the usual case.
fn enable_hyperv_elevated(log: &mut Log) -> anyhow::Result<()> {
    log.line("… schalte Hyper-V ein (DISM), das dauert eine Weile");
    let output = Command::new(system32("dism.exe"))
        .args([
            "/Online",
            "/Enable-Feature",
            "/FeatureName:Microsoft-Hyper-V",
            "/All",
            "/NoRestart",
        ])
        .output()
        .context("dism.exe: Programm nicht gestartet")?;
    match output.status.code() {
        Some(0) => log.line("✓ Hyper-V eingeschaltet"),
        Some(3010) => log.line("✓ Hyper-V eingeschaltet; Windows muss neu starten"),
        code => bail!(
            "DISM endete mit {:?}: {}",
            code,
            String::from_utf8_lossy(&output.stdout).trim()
        ),
    }
    Ok(())
}

fn uninstall_elevated(log: &mut Log, machine: &str, profile: &Path) -> anyhow::Result<()> {
    harness_core::provider::validate_resource_name("Machine-Name", machine)?;
    stop_service(log);
    if service_state().is_some() {
        step(
            log,
            "Dienst entfernt",
            Command::new(system32("sc.exe")).args(["delete", SERVICE_NAME]),
        )?;
    }
    let user_json = user_machine_json(profile, machine);
    let moved = user_json.with_extension(format!("json.{MOVED}"));
    if moved.is_file() {
        fs::rename(&moved, &user_json)?;
        log.line(&format!("✓ Machine {machine} gehört wieder dir"));
    }
    let service_json = vmd::machine_json(machine);
    let _ = fs::remove_file(service_json.with_extension("ign"));
    if service_json.is_file() {
        fs::remove_file(&service_json)?;
    }
    let _ = fs::remove_file(program_dir().join("harness-vmd.exe"));
    log.line("✓ Die VM startest du jetzt wieder selbst (Admin-PowerShell: podman machine start)");
    Ok(())
}
