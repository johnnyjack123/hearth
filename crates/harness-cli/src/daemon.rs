//! `harness daemon`: the background service that keeps this computer
//! connected to the harness server, so web chats can use its projects
//! without a terminal.
//!
//! `harnessd.exe` (a window-less launcher next to `harness.exe`) starts
//! `harness daemon run` hidden, writes its output to `daemon.log` in the
//! configuration folder and restarts it after a crash. Windows starts the
//! launcher at logon through the user's Run key; no admin rights needed.
//! A named mutex per configuration folder keeps one connection per
//! computer, because two would keep replacing each other on the server.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::bail;
use harness_core::config::ConfigDir;
use harness_core::lang::tr;
use harness_core::tr;

/// Exit code of `harness daemon run` when reconnecting makes no sense
/// (not paired, device removed); the launcher then stops too.
pub const EXIT_UNPAIRED: i32 = 3;

/// Name of the value in the Run key.
const RUN_VALUE: &str = "llm-harness";

/// Log of the background service, in the configuration folder.
pub fn log_path(config: &ConfigDir) -> PathBuf {
    config.root().join("daemon.log")
}

/// Whether the service of the real profile may be started automatically.
/// Test runs with `HARNESS_HOME` must not leave a service behind.
pub fn automatic() -> bool {
    std::env::var_os("HARNESS_HOME").is_none()
}

/// Suffix of the named kernel objects: one service per configuration
/// folder, so a test with `HARNESS_HOME` does not collide with the user's.
/// harnessd computes the same; keep both in step.
fn object_suffix(config: &ConfigDir) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in config.root().to_string_lossy().to_lowercase().bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(windows)]
mod sys {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt as _;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, OpenEventW, OpenMutexW, ResetEvent,
        SYNCHRONIZATION_SYNCHRONIZE, SetEvent, WaitForSingleObject,
    };

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    fn wide(text: &str) -> Vec<u16> {
        OsStr::new(text).encode_wide().chain([0]).collect()
    }

    /// An owned kernel handle.
    pub struct Handle(HANDLE);

    // The handles are only waited on and signalled, which Windows allows
    // from any thread.
    unsafe impl Send for Handle {}
    unsafe impl Sync for Handle {}

    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }

    /// Creates the service's mutex; `None` if another process holds it.
    pub fn create_mutex(name: &str) -> std::io::Result<Option<Handle>> {
        let name = wide(name);
        let handle = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            drop(Handle(handle));
            return Ok(None);
        }
        Ok(Some(Handle(handle)))
    }

    /// Whether some process holds the mutex.
    pub fn mutex_exists(name: &str) -> bool {
        let name = wide(name);
        let handle = unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, 0, name.as_ptr()) };
        if handle.is_null() {
            return false;
        }
        drop(Handle(handle));
        true
    }

    /// Creates (or opens) a named event and clears it. A manual-reset event
    /// stays set once signalled (stop); an auto-reset one is consumed by the
    /// check that sees it (one request).
    pub fn create_event(name: &str, manual_reset: bool) -> std::io::Result<Handle> {
        let name = wide(name);
        let handle =
            unsafe { CreateEventW(std::ptr::null(), i32::from(manual_reset), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        unsafe {
            ResetEvent(handle);
        }
        Ok(Handle(handle))
    }

    /// Whether the event is set.
    pub fn is_set(event: &Handle) -> bool {
        unsafe { WaitForSingleObject(event.0, 0) == WAIT_OBJECT_0 }
    }

    /// Sets the named event; false if nobody created it.
    pub fn signal(name: &str) -> bool {
        let name = wide(name);
        let handle = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        if handle.is_null() {
            return false;
        }
        let event = Handle(handle);
        unsafe { SetEvent(event.0) != 0 }
    }

    /// Writes the Run value that starts `command` at logon.
    pub fn set_autostart(value: &str, command: &str) -> std::io::Result<()> {
        let key = wide(RUN_KEY);
        let name = wide(value);
        let data = wide(command);
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                data.as_ptr().cast(),
                u32::try_from(data.len() * 2).unwrap_or(u32::MAX),
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(std::io::Error::from_raw_os_error(status as i32))
        }
    }

    /// Removes the Run value; fine if it is not there.
    pub fn remove_autostart(value: &str) {
        let key = wide(RUN_KEY);
        let name = wide(value);
        unsafe {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr());
        }
    }

    /// The command in the Run value, if set.
    pub fn autostart(value: &str) -> Option<String> {
        let key = wide(RUN_KEY);
        let name = wide(value);
        let mut buffer = vec![0u16; 1024];
        let mut size = u32::try_from(buffer.len() * 2).unwrap_or(u32::MAX);
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
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
}

/// The running service's hold on its configuration folder, with the
/// events that ask it to stop and (from the tray icon) to stop the VM.
pub struct Instance {
    #[cfg(windows)]
    _mutex: sys::Handle,
    #[cfg(windows)]
    stop: sys::Handle,
    #[cfg(windows)]
    vm_stop: sys::Handle,
}

impl Instance {
    /// Takes the connection of this configuration folder; `None` if a
    /// service or `harness connect` already has it.
    #[cfg(windows)]
    pub fn acquire(config: &ConfigDir) -> anyhow::Result<Option<Instance>> {
        let suffix = object_suffix(config);
        let Some(mutex) = sys::create_mutex(&format!(r"Local\llm-harness-daemon-{suffix}"))? else {
            return Ok(None);
        };
        let stop = sys::create_event(&format!(r"Local\llm-harness-daemon-stop-{suffix}"), true)?;
        // harnessd (the tray icon) signals it; the name must match there.
        let vm_stop =
            sys::create_event(&format!(r"Local\llm-harness-daemon-vmstop-{suffix}"), false)?;
        Ok(Some(Instance {
            _mutex: mutex,
            stop,
            vm_stop,
        }))
    }

    /// Without Windows there is no second process to keep out.
    #[cfg(not(windows))]
    pub fn acquire(_config: &ConfigDir) -> anyhow::Result<Option<Instance>> {
        Ok(Some(Instance {}))
    }

    /// Whether the tray icon asked to stop the VM since the last check.
    pub fn vm_stop_requested(&self) -> bool {
        #[cfg(windows)]
        {
            sys::is_set(&self.vm_stop)
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    /// Whether `harness daemon stop` asked this process to end.
    pub fn stop_requested(&self) -> bool {
        #[cfg(windows)]
        {
            sys::is_set(&self.stop)
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
}

/// Whether a service (or `harness connect`) holds this configuration's
/// connection.
pub fn running(config: &ConfigDir) -> bool {
    #[cfg(windows)]
    {
        sys::mutex_exists(&format!(
            r"Local\llm-harness-daemon-{}",
            object_suffix(config)
        ))
    }
    #[cfg(not(windows))]
    {
        let _ = config;
        false
    }
}

fn launcher() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let launcher = exe.with_file_name(if cfg!(windows) {
        "harnessd.exe"
    } else {
        "harnessd"
    });
    if !launcher.exists() {
        bail!(tr!(
            "{} fehlt neben harness. Mit `cargo build` entsteht es mit.",
            "{} is missing next to harness. `cargo build` builds it as well.",
            launcher.display()
        ));
    }
    Ok(launcher)
}

fn wait_until(timeout: Duration, condition: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    condition()
}

/// Starts the service unless it runs; returns whether it runs afterwards.
pub fn start(config: &ConfigDir) -> anyhow::Result<bool> {
    if running(config) {
        return Ok(true);
    }
    let launcher = launcher()?;
    let mut command = std::process::Command::new(&launcher);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // Its own process group and no console: closing this terminal must
        // not end the service.
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    command.spawn()?;
    Ok(wait_until(Duration::from_secs(8), || running(config)))
}

/// Asks the service to stop and waits for it.
pub fn stop(config: &ConfigDir) -> bool {
    #[cfg(windows)]
    {
        if !sys::signal(&format!(
            r"Local\llm-harness-daemon-stop-{}",
            object_suffix(config)
        )) {
            return !running(config);
        }
    }
    wait_until(Duration::from_secs(15), || !running(config))
}

/// Starts the service at logon from now on and right away.
pub fn install(config: &ConfigDir) -> anyhow::Result<bool> {
    if !automatic() {
        bail!(tr(
            "Der Autostart gilt nur für ~/.harness. Mit HARNESS_HOME `harness daemon start` \
             oder `harness connect` nutzen",
            "The autostart only applies to ~/.harness. With HARNESS_HOME, use `harness daemon \
             start` or `harness connect`"
        ));
    }
    let launcher = launcher()?;
    #[cfg(windows)]
    sys::set_autostart(RUN_VALUE, &format!("\"{}\"", launcher.display()))?;
    start(config)
}

/// Stops the service and removes its autostart.
pub fn uninstall(config: &ConfigDir) -> bool {
    #[cfg(windows)]
    sys::remove_autostart(RUN_VALUE);
    stop(config)
}

/// The launcher Windows starts at logon, if the autostart is set.
pub fn autostart() -> Option<String> {
    #[cfg(windows)]
    {
        sys::autostart(RUN_VALUE)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// The last lines of the service's log.
pub fn log_tail(path: &Path, lines: usize) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..]
        .iter()
        .map(|line| harness_core::sync::sanitize(line))
        .collect()
}

/// Commands of `harness daemon`. The help texts follow the user's
/// language, so they are attributes rather than doc comments.
#[derive(clap::Subcommand)]
pub enum DaemonCommand {
    #[command(about = tr(
        "Zeigt, ob der Dienst läuft, den Autostart und das Ende des Protokolls (Standard)",
        "Shows whether the service runs, the autostart and the end of the log (default)"
    ))]
    Status,
    #[command(about = tr(
        "Startet den Dienst im Hintergrund",
        "Starts the service in the background"
    ))]
    Start,
    #[command(about = tr("Beendet den Dienst", "Stops the service"))]
    Stop,
    #[command(about = tr(
        "Startet den Dienst ab jetzt bei jeder Anmeldung (ohne Admin)",
        "Starts the service at every sign-in from now on (without admin)"
    ))]
    Install,
    #[command(about = tr(
        "Beendet den Dienst und entfernt den Autostart",
        "Stops the service and removes the autostart"
    ))]
    Uninstall,
    #[command(hide = true, about = tr(
        "Der Dienst selbst; startet harnessd.exe, nicht von Hand aufrufen",
        "The service itself; started by harnessd.exe, do not call by hand"
    ))]
    Run,
}

/// `harness daemon [COMMAND]`.
pub fn run(
    config: &ConfigDir,
    machine: &str,
    command: Option<DaemonCommand>,
) -> anyhow::Result<bool> {
    match command.unwrap_or(DaemonCommand::Status) {
        DaemonCommand::Status => {
            status(config);
            Ok(true)
        }
        DaemonCommand::Start => {
            let ok = start(config)?;
            println!(
                "{}",
                if ok {
                    tr("Hintergrunddienst läuft.", "Background service is running.")
                } else {
                    tr(
                        "Hintergrunddienst startet nicht, siehe `harness daemon`.",
                        "Background service does not start, see `harness daemon`.",
                    )
                }
            );
            Ok(ok)
        }
        DaemonCommand::Stop => {
            let ok = stop(config);
            println!(
                "{}",
                if ok {
                    tr("Hintergrunddienst beendet.", "Background service stopped.")
                } else {
                    tr(
                        "Hintergrunddienst reagiert nicht.",
                        "Background service does not respond.",
                    )
                }
            );
            Ok(ok)
        }
        DaemonCommand::Install => {
            let ok = install(config)?;
            println!(
                "{}",
                if ok {
                    tr(
                        "Autostart eingerichtet. Der Dienst läuft.",
                        "Autostart set up. The service is running.",
                    )
                } else {
                    tr(
                        "Autostart eingerichtet. Der Dienst startet nicht, siehe `harness daemon`.",
                        "Autostart set up. The service does not start, see `harness daemon`.",
                    )
                }
            );
            Ok(ok)
        }
        DaemonCommand::Uninstall => {
            let ok = uninstall(config);
            println!(
                "{}",
                tr(
                    "Autostart entfernt, Dienst beendet.",
                    "Autostart removed, service stopped.",
                )
            );
            Ok(ok)
        }
        DaemonCommand::Run => {
            // A VM that runs already (an update of harness restarts only the
            // service) gets its swap now, the others when harness starts them.
            let swap_machine = machine.to_owned();
            std::thread::spawn(move || {
                if let Ok(provider) =
                    harness_core::podman::PodmanProvider::for_machine(&swap_machine)
                    && harness_core::vmd::ssh_answers(
                        provider.machine().ssh_port,
                        std::time::Duration::from_secs(3),
                    )
                    && let Some(note) = crate::vm::ensure_swap(&provider)
                {
                    println!("{} {note}", crate::connect::utc_stamp());
                }
            });
            let code =
                crate::connect::serve_forever(config, machine, crate::connect::Mode::Background);
            std::process::exit(code);
        }
    }
}

fn status(config: &ConfigDir) {
    println!(
        "{} {}",
        tr("Hintergrunddienst:", "Background service:"),
        if running(config) {
            tr("läuft", "running")
        } else {
            tr("läuft nicht", "not running")
        }
    );
    match autostart() {
        Some(command) => println!("Autostart: {command}"),
        None => println!(
            "{}",
            tr(
                "Autostart: aus (`harness daemon install`)",
                "Autostart: off (`harness daemon install`)",
            )
        ),
    }
    let log = log_path(config);
    let tail = log_tail(&log, 15);
    if !tail.is_empty() {
        println!();
        println!("{}:", log.display());
        for line in tail {
            println!("  {line}");
        }
    }
}
