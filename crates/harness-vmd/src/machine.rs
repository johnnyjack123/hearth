//! Starting and stopping the machine as SYSTEM.
//!
//! podman and PowerShell get an environment built from scratch: HOME,
//! APPDATA, ProgramData, TEMP and CONTAINERS_CONF point into the service
//! directory, which only administrators can write. Inheriting anything
//! else could let a user process steer what podman reads or starts.

use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use harness_core::podman::MachineConfig;
use harness_core::vmd::{self, SSH_BANNER_TIMEOUT, VmState};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::log;

/// Upper bound for `podman machine start`; the first start after a host
/// reboot can take a while.
const START_TIMEOUT: Duration = Duration::from_secs(300);
const STOP_TIMEOUT: Duration = Duration::from_secs(120);
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(60);

/// How long output may still arrive after the command ended.
const OUTPUT_GRACE: Duration = Duration::from_secs(3);

/// How long SSH may take to answer after podman reports the machine started.
const SSH_READY_TIMEOUT: Duration = Duration::from_secs(90);

/// Prepares a stopped VM: its startup memory ([`startup_memory_mb`]), shut
/// down rather than saved when
/// the host shuts down, no saved state, and no Ignition items left over
/// from a failed start (they make every further start fail with 32773).
/// The machine name comes from the environment, never from the script text.
///
/// A gvproxy of an earlier run that outlived its VM (seen after the host's
/// standby) still holds the machine's pipe `podman-<name>`, and podman then
/// refuses every start ("expected pipe is not available"). With the VM off
/// it serves nothing, so it is ended.
const PREPARE_PS: &str = r#"
$ErrorActionPreference = 'Stop'
$name = $env:HARNESS_VM
$vm = Get-VM -Name $name
if ($vm.State -eq 'Saved') { Remove-VMSavedState -VMName $name }
if ($vm.State -eq 'Off') {
  $pipe = 'podman-' + $name
  Get-CimInstance Win32_Process -Filter "Name='gvproxy.exe'" |
    Where-Object { $_.CommandLine -and $_.CommandLine.Contains($pipe) } |
    ForEach-Object {
      Stop-Process -Id $_.ProcessId -Force
      "Alter gvproxy beendet: $($_.ProcessId)"
    }
}
Set-VM -Name $name -AutomaticStopAction ShutDown -AutomaticStartAction Nothing
Set-VMMemory -VMName $name -DynamicMemoryEnabled $true `
  -StartupBytes ([int64]$env:HARNESS_STARTUP_MB * 1MB) `
  -MinimumBytes ([int64]$env:HARNESS_MINIMUM_MB * 1MB) `
  -MaximumBytes ([int64]$env:HARNESS_MAXIMUM_MB * 1MB)
$ns = 'root\virtualization\v2'
$cs = Get-CimInstance -Namespace $ns -ClassName Msvm_ComputerSystem -Filter "ElementName='$name'"
$vssd = Get-CimAssociatedInstance -InputObject $cs -ResultClassName Msvm_VirtualSystemSettingData |
  Where-Object VirtualSystemType -eq 'Microsoft:Hyper-V:System:Realized'
$kvp = Get-CimAssociatedInstance -InputObject $vssd -ResultClassName Msvm_KvpExchangeComponentSettingData
$items = @($kvp.HostExchangeItems | Where-Object { $_ -match 'ignition\.config' })
if ($items.Count -gt 0) {
  $vsms = Get-CimInstance -Namespace $ns -ClassName Msvm_VirtualSystemManagementService
  Invoke-CimMethod -InputObject $vsms -MethodName RemoveKvpItems `
    -Arguments @{ TargetSystem = $cs; DataItems = [string[]]$items } | Out-Null
  "Ignition-Reste entfernt: $($items.Count)"
}
"#;

/// Resumes a VM that Hyper-V paused, e.g. during the host's standby;
/// podman does not know that state ("unknown state: quiesce") and refuses
/// to start or stop it.
const RESUME_PS: &str = r#"
$ErrorActionPreference = 'Stop'
if ((Get-VM -Name $env:HARNESS_VM).State -eq 'Paused') {
  Resume-VM -Name $env:HARNESS_VM
  'fortgesetzt'
}
"#;

/// Shrinks the VM's disk file to what the guest uses: `Optimize-VHD -Mode
/// Full` reclaims the blocks the guest trimmed before. It needs the VM off,
/// and Hyper-V may hold the file for some seconds after podman reports the
/// stop (30.10.: "in use" 2 s after it), so that error is tried again for a
/// minute. The path comes from the environment, never from the script text.
const COMPACT_PS: &str = r#"
$ErrorActionPreference = 'Stop'
$path = $env:HARNESS_DISK
$before = (Get-Item -LiteralPath $path).Length
for ($try = 1; ; $try++) {
  try {
    Optimize-VHD -Path $path -Mode Full
    break
  } catch {
    $inUse = $_.FullyQualifiedErrorId -like 'ObjectInUse*' -or $_.Exception.Message -like '*0x80070020*'
    if (-not $inUse -or $try -ge 12) { throw }
    Start-Sleep -Seconds 5
  }
}
$after = (Get-Item -LiteralPath $path).Length
"$before $after"
"#;

/// How long the compaction may take.
const COMPACT_TIMEOUT: Duration = Duration::from_secs(900);

/// Growth of the disk file since the last compaction from which an idle
/// stop compacts again: below that, the minutes it takes gain little.
const COMPACT_GROWTH: u64 = 1 << 30;

/// An idle stop compacts at most once in this time.
const COMPACT_INTERVAL: Duration = Duration::from_secs(24 * 3600);

/// The last compaction, kept in the service dir.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Compacted {
    /// When, in seconds since 1970.
    at: u64,
    /// Size of the disk file afterwards, in bytes.
    size: u64,
}

impl Compacted {
    fn path() -> PathBuf {
        vmd::service_dir().join("compacted.json")
    }

    fn load() -> Option<Compacted> {
        serde_json::from_str(&fs::read_to_string(Self::path()).ok()?).ok()
    }

    fn save(self) {
        if let Ok(text) = serde_json::to_string(&self) {
            let _ = fs::write(Self::path(), text);
        }
    }
}

/// Whether an idle stop should compact a disk file of `size` bytes: never
/// compacted yet, or grown by [`COMPACT_GROWTH`] and not compacted within
/// [`COMPACT_INTERVAL`].
fn compaction_due(size: u64, last: Option<Compacted>, now: u64) -> bool {
    last.is_none_or(|last| {
        now.saturating_sub(last.at) >= COMPACT_INTERVAL.as_secs()
            && size >= last.size.saturating_add(COMPACT_GROWTH)
    })
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn file_size(path: &Path) -> u64 {
    fs::metadata(path).map_or(0, |m| m.len())
}

/// Memory Windows keeps for itself when the VM starts with what is free
/// (the user's choice, 2.11.: rather a smaller VM than a slow Windows).
const HOST_RESERVE_MB: u64 = 1024;

/// What Hyper-V gives the VM at its start, in MB.
///
/// On ARM64 the guest cannot take memory Hyper-V adds later: its kernel
/// says "hv_balloon: Memory hot add disabled on ARM64" (seen 1.11.), and
/// the VM stayed at 936 MB with a maximum of 4 GB until the OOM killer
/// struck. So it starts with what Windows can spare, between the configured
/// startup and maximum; what Linux does not use, the balloon driver hands
/// back. Elsewhere dynamic memory grows the VM when it needs more, and a
/// small start fails less often for lack of free memory.
///
/// `available_mb` is Windows' available memory, if known.
fn startup_memory_mb(config: &Config, available_mb: Option<u64>, grows_later: bool) -> u64 {
    let floor = u64::from(config.startup_memory_mb);
    let Some(available) = available_mb.filter(|_| !grows_later) else {
        return floor;
    };
    let spare = available.saturating_sub(HOST_RESERVE_MB);
    let startup = spare.clamp(floor, u64::from(config.maximum_memory_mb).max(floor));
    // Hyper-V wants whole 2 MB pages; round steps read better in the log.
    (startup - startup % 128).max(floor)
}

/// Windows' available physical memory (what Task Manager shows as
/// available, standby cache included) in MB.
#[cfg(windows)]
fn available_memory_mb() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        // SAFETY: a plain C struct, all zeros is a valid value.
        ..unsafe { std::mem::zeroed() }
    };
    // SAFETY: `status` is a valid MEMORYSTATUSEX with its length set.
    (unsafe { GlobalMemoryStatusEx(&mut status) } != 0).then_some(status.ullAvailPhys / (1 << 20))
}

#[cfg(not(windows))]
fn available_memory_mb() -> Option<u64> {
    None
}

/// Hard power-off after a failed start or stop.
const TURN_OFF_PS: &str = r#"
Stop-VM -Name $env:HARNESS_VM -TurnOff -Force -ErrorAction SilentlyContinue
"#;

/// Hyper-V's view of the VM: `Running`, `Off`, `Saved`, …
const STATE_PS: &str = r#"
$ErrorActionPreference = 'Stop'
(Get-VM -Name $env:HARNESS_VM).State.ToString()
"#;

/// Runs a command, capturing stdout and stderr, and kills it after
/// `timeout`. Returns success and the combined output.
fn run(mut command: Command, timeout: Duration) -> (bool, String) {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return (false, format!("Start fehlgeschlagen: {e}")),
    };
    // Output is collected as it arrives: a process the command started can
    // inherit the pipes and keep them open long after the command ended
    // (cmd and its children, possibly a helper of podman), so the readers
    // are not waited for beyond a short grace period.
    let output = Arc::new(Mutex::new(Vec::<u8>::new()));
    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|mut stream| {
        let output = Arc::clone(&output);
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            while let Ok(n) = stream.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                output
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .extend_from_slice(&buffer[..n]);
            }
        })
    })
    .collect();
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(200)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let grace = Instant::now() + OUTPUT_GRACE;
    while readers.iter().any(|r| !r.is_finished()) && Instant::now() < grace {
        std::thread::sleep(Duration::from_millis(50));
    }
    let output =
        String::from_utf8_lossy(&output.lock().unwrap_or_else(|p| p.into_inner())).into_owned();
    match status {
        Some(status) => (status.success(), output),
        None => (
            false,
            format!("nach {} s abgebrochen\n{output}", timeout.as_secs()),
        ),
    }
}

/// The last lines of command output for messages and the log.
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(6)..].join(" | ")
}

/// The machine the service owns.
pub struct Vm {
    config: Config,
}

impl Vm {
    /// Control of the configured machine.
    pub fn new(config: Config) -> Vm {
        Vm { config }
    }

    fn dir(name: &str) -> PathBuf {
        vmd::service_dir().join(name)
    }

    /// The environment for podman and PowerShell: what Windows itself needs
    /// from the service's own (SYSTEM) environment, plus the private dirs.
    fn base_env(&self) -> Vec<(OsString, OsString)> {
        let mut env: Vec<(OsString, OsString)> = [
            "SystemRoot",
            "SystemDrive",
            "windir",
            "ComSpec",
            "PATHEXT",
            "ProgramFiles",
            "CommonProgramFiles",
            "NUMBER_OF_PROCESSORS",
            "PROCESSOR_ARCHITECTURE",
            "OS",
            "COMPUTERNAME",
            "PSModulePath",
        ]
        .into_iter()
        .filter_map(|key| std::env::var_os(key).map(|value| (key.into(), value)))
        .collect();
        let system32 = Path::new(&std::env::var_os("SystemRoot").unwrap_or("C:\\Windows".into()))
            .join("System32");
        let mut path = vec![
            system32.clone(),
            system32.join("WindowsPowerShell").join("v1.0"),
        ];
        if let Some(dir) = self.config.podman.parent() {
            path.push(dir.to_owned());
        }
        env.push((
            "PATH".into(),
            std::env::join_paths(path).unwrap_or_default(),
        ));
        let home = vmd::podman_home();
        for (key, value) in [
            ("HOME", home.clone()),
            ("USERPROFILE", home),
            ("APPDATA", Self::dir("appdata")),
            ("LOCALAPPDATA", Self::dir("localappdata")),
            ("ProgramData", Self::dir("programdata")),
            ("TEMP", Self::dir("tmp")),
            ("TMP", Self::dir("tmp")),
            (
                "CONTAINERS_CONF",
                vmd::service_dir().join("containers.conf"),
            ),
        ] {
            env.push((key.into(), value.into_os_string()));
        }
        env.push(("CONTAINERS_MACHINE_PROVIDER".into(), "hyperv".into()));
        env.push(("HARNESS_VM".into(), self.config.machine.clone().into()));
        env
    }

    /// Creates the private directories podman uses.
    pub fn prepare_dirs(&self) {
        for dir in [
            vmd::podman_home(),
            Self::dir("appdata"),
            Self::dir("localappdata"),
            Self::dir("programdata"),
            Self::dir("tmp"),
        ] {
            let _ = fs::create_dir_all(dir);
        }
    }

    fn podman(&self, args: &[&str]) -> Command {
        let mut command = Command::new(&self.config.podman);
        command.env_clear().envs(self.base_env()).args(args);
        command
    }

    fn powershell(&self, script: &str) -> Command {
        let system32 = Path::new(&std::env::var_os("SystemRoot").unwrap_or("C:\\Windows".into()))
            .join("System32");
        let mut command = Command::new(
            system32
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe"),
        );
        command
            .env_clear()
            .envs(self.base_env())
            .env(
                "HARNESS_STARTUP_MB",
                self.config.startup_memory_mb.to_string(),
            )
            .env(
                "HARNESS_MINIMUM_MB",
                self.config.minimum_memory_mb.to_string(),
            )
            .env(
                "HARNESS_MAXIMUM_MB",
                self.config.maximum_memory_mb.to_string(),
            )
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ]);
        command
    }

    fn ssh_port(&self) -> Option<u16> {
        MachineConfig::from_path(&vmd::machine_json(&self.config.machine))
            .ok()
            .map(|machine| machine.ssh_port)
    }

    /// Hyper-V's state and whether SSH answers.
    pub fn state(&self) -> VmState {
        let (ok, output) = run(self.powershell(STATE_PS), SCRIPT_TIMEOUT);
        if !ok {
            return VmState::Unknown;
        }
        match output.trim() {
            "Running" => match self.ssh_port() {
                Some(port) if vmd::ssh_answers(port, SSH_BANNER_TIMEOUT) => VmState::Running,
                _ => VmState::Unresponsive,
            },
            "Off" | "Saved" => VmState::Stopped,
            _ => VmState::Unknown,
        }
    }

    /// Starts the machine unless it already answers; restarts it if it runs
    /// but SSH does not answer.
    pub fn start(&self) -> Result<String, String> {
        let (_, resumed) = run(self.powershell(RESUME_PS), SCRIPT_TIMEOUT);
        if resumed.contains("fortgesetzt") {
            // Whether it still answers decides below, as for any running VM.
            log::line("VM war pausiert (Standby?) und läuft weiter");
        }
        match self.state() {
            VmState::Running => return Ok("Die VM läuft bereits.".to_owned()),
            VmState::Unresponsive => {
                log::line("VM läuft, SSH antwortet nicht: Neustart");
                self.stop()?;
            }
            VmState::Stopped | VmState::Unknown => {}
        }
        let available = available_memory_mb();
        let startup =
            startup_memory_mb(&self.config, available, cfg!(not(target_arch = "aarch64")));
        log::line(&format!(
            "Startspeicher der VM: {startup} MB (Windows: {} MB verfügbar)",
            available.map_or("?".to_owned(), |mb| mb.to_string())
        ));
        let mut prepare = self.powershell(PREPARE_PS);
        prepare.env("HARNESS_STARTUP_MB", startup.to_string());
        let (ok, output) = run(prepare, SCRIPT_TIMEOUT);
        if !ok {
            return Err(format!("Vorbereitung fehlgeschlagen: {}", tail(&output)));
        }
        if !output.trim().is_empty() {
            log::line(&format!("Vorbereitung: {}", tail(&output)));
        }
        let started = Instant::now();
        let (ok, output) = run(
            self.podman(&["machine", "start", &self.config.machine]),
            START_TIMEOUT,
        );
        log::line(&format!(
            "podman machine start ({} s): {}",
            started.elapsed().as_secs(),
            tail(&output)
        ));
        if !ok {
            self.clean_up_failed_start();
            return Err(format!(
                "Start fehlgeschlagen: {}{}",
                tail(&output),
                start_hint(&output)
            ));
        }
        let port = self.ssh_port().ok_or("SSH-Port der Machine unbekannt")?;
        let deadline = Instant::now() + SSH_READY_TIMEOUT;
        while !vmd::ssh_answers(port, SSH_BANNER_TIMEOUT) {
            if Instant::now() > deadline {
                return Err("Die VM läuft, aber SSH antwortet nicht.".to_owned());
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        Ok(format!(
            "Die VM ist gestartet ({} s).",
            started.elapsed().as_secs()
        ))
    }

    /// After a failed start: power off and remove what would block the
    /// next one (spike §12.3).
    fn clean_up_failed_start(&self) {
        let _ = run(self.powershell(TURN_OFF_PS), SCRIPT_TIMEOUT);
        // Resets podman's own "starting" state of the machine.
        let _ = run(
            self.podman(&["machine", "stop", &self.config.machine]),
            STOP_TIMEOUT,
        );
        let _ = run(self.powershell(PREPARE_PS), SCRIPT_TIMEOUT);
    }

    /// Gives the space of deleted files back to Windows: the VM off, its
    /// disk file compacted, and the VM on again if it ran before. The CLI
    /// trims inside the VM first (`PodmanProvider::trim`); the machine's key
    /// belongs to the user, and Windows' ssh refuses it to SYSTEM.
    pub fn compact(&self) -> Result<String, String> {
        let disk = self.disk()?;
        let was_running = self.state() == VmState::Running;
        if was_running {
            self.stop()?;
        }
        let shrunk = self.shrink(&disk)?;
        if !was_running {
            return Ok(shrunk);
        }
        // A start that fails right after the compaction (seen once on
        // 27.10., the next one worked) must not hide that the disk shrank;
        // the next harness command starts the VM anyway.
        Ok(match self.start() {
            Ok(_) => shrunk,
            Err(error) => {
                format!("{shrunk} Die VM startet beim nächsten harness-Befehl wieder ({error}).")
            }
        })
    }

    /// Stops the VM after the user's idle time and then compacts its disk
    /// file if that is due ([`compaction_due`]). The VM is idle, so the
    /// minutes that takes bother nobody; a start in the meantime waits.
    pub fn stop_idle(&self) -> Result<String, String> {
        let due = self
            .disk()
            .ok()
            .filter(|disk| compaction_due(file_size(disk), Compacted::load(), unix_now()));
        let stopped = self.stop()?;
        let Some(disk) = due else {
            return Ok(stopped);
        };
        log::line("Leerlauf: Festplatte der VM wird verkleinert");
        Ok(match self.shrink(&disk) {
            Ok(shrunk) => format!("{stopped} {shrunk}"),
            Err(error) => format!("{stopped} {error}"),
        })
    }

    fn disk(&self) -> Result<PathBuf, String> {
        MachineConfig::from_path(&vmd::machine_json(&self.config.machine))
            .ok()
            .and_then(|machine| machine.disk)
            .ok_or_else(|| "Festplatte der VM unbekannt (die Machine-JSON nennt keine)".to_owned())
    }

    /// Compacts the disk file of the stopped VM.
    fn shrink(&self, disk: &Path) -> Result<String, String> {
        let mut command = self.powershell(COMPACT_PS);
        command.env("HARNESS_DISK", disk);
        let (ok, output) = run(command, COMPACT_TIMEOUT);
        log::line(&format!("Optimize-VHD: {}", tail(&output)));
        // Also after a failure: the next idle stop tries again only once
        // the disk has grown.
        Compacted {
            at: unix_now(),
            size: file_size(disk),
        }
        .save();
        if ok {
            Ok(compacted(&output))
        } else {
            Err(format!("Verkleinern fehlgeschlagen: {}", tail(&output)))
        }
    }

    /// Stops the machine; powers it off if podman cannot.
    pub fn stop(&self) -> Result<String, String> {
        let (ok, output) = run(
            self.podman(&["machine", "stop", &self.config.machine]),
            STOP_TIMEOUT,
        );
        log::line(&format!("podman machine stop: {}", tail(&output)));
        if ok {
            return Ok("Die VM ist gestoppt.".to_owned());
        }
        let (off, output) = run(self.powershell(TURN_OFF_PS), SCRIPT_TIMEOUT);
        if off && self.state() == VmState::Stopped {
            Ok("Die VM wurde hart ausgeschaltet.".to_owned())
        } else {
            Err(format!("Stopp fehlgeschlagen: {}", tail(&output)))
        }
    }
}

/// The message for a compacted disk, from the script's `<before> <after>`
/// (bytes).
fn compacted(output: &str) -> String {
    let sizes: Vec<f64> = output
        .split_whitespace()
        .rev()
        .take(2)
        .filter_map(|word| word.parse::<f64>().ok())
        .collect();
    let gb = |bytes: f64| format!("{:.1} GB", bytes / 1e9).replace('.', ",");
    match sizes[..] {
        [after, before] => format!("Festplatte der VM: {} → {}.", gb(before), gb(after)),
        _ => "Festplatte der VM verkleinert.".to_owned(),
    }
}

/// A hint for Hyper-V's job errors 32778 ("out of memory") and 32788, which
/// say nothing by themselves; on this machine 32788 has always meant too
/// little free memory as well (event 3030 in Hyper-V-Worker-Admin).
fn start_hint(output: &str) -> &'static str {
    if output.contains("32788") || output.contains("32778") {
        " (Vermutlich zu wenig freier Arbeitsspeicher: Die VM braucht beim Start 1 GB. \
             Programme schließen und erneut versuchen.)"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_error_32788_gets_a_hint() {
        assert_eq!(
            compacted("7881299968 3328599654\r\n"),
            "Festplatte der VM: 7,9 GB → 3,3 GB."
        );
        assert_eq!(compacted("oops"), "Festplatte der VM verkleinert.");
        let hint = start_hint("Job failed with error code: 32788 ()");
        assert!(hint.contains("Arbeitsspeicher"), "{hint}");
        assert!(!hint.contains("  "), "{hint}");
        assert_eq!(start_hint("error code: 32778 ()"), hint);
        assert_eq!(start_hint("ssh: handshake failed"), "");
    }

    #[test]
    fn idle_stops_compact_once_the_disk_has_grown() {
        const GB: u64 = 1 << 30;
        const DAY: u64 = 24 * 3600;
        let now = 100 * DAY;
        assert!(compaction_due(9 * GB, None, now));
        let last = Some(Compacted {
            at: now - 2 * DAY,
            size: 5 * GB,
        });
        assert!(compaction_due(6 * GB, last, now));
        assert!(!compaction_due(6 * GB - 1, last, now));
        let recent = Some(Compacted {
            at: now - DAY / 2,
            size: 5 * GB,
        });
        assert!(!compaction_due(9 * GB, recent, now));
        let record = Compacted {
            at: now,
            size: 5 * GB,
        };
        let text = serde_json::to_string(&record).unwrap();
        assert_eq!(serde_json::from_str::<Compacted>(&text).unwrap(), record);
    }

    #[test]
    fn arm_vms_start_with_what_windows_can_spare() {
        let config = Config {
            machine: "harness".into(),
            podman: r"C:\Program Files\RedHat\Podman\podman.exe".into(),
            user_sid: "S-1-5-21-1-2-3-1001".into(),
            startup_memory_mb: 1024,
            minimum_memory_mb: 512,
            maximum_memory_mb: 4096,
        };
        // Plenty free: the maximum.
        assert_eq!(startup_memory_mb(&config, Some(9000), false), 4096);
        // What is left after 1 GB for Windows, in steps of 128 MB.
        assert_eq!(startup_memory_mb(&config, Some(3000), false), 1920);
        // 2 GB available (as on 1.11.) or less: never below the configured start.
        assert_eq!(startup_memory_mb(&config, Some(2091), false), 1024);
        assert_eq!(startup_memory_mb(&config, Some(900), false), 1024);
        assert_eq!(startup_memory_mb(&config, None, false), 1024);
        // Where the VM grows later, it starts small.
        assert_eq!(startup_memory_mb(&config, Some(9000), true), 1024);
    }

    #[test]
    fn commands_run_with_a_timeout() {
        let mut slow = Command::new("cmd.exe");
        slow.args(["/d", "/c", "ping -n 30 127.0.0.1 > nul"]);
        let started = Instant::now();
        let (ok, output) = run(slow, Duration::from_secs(1));
        assert!(!ok);
        assert!(output.contains("abgebrochen"), "{output}");
        assert!(started.elapsed() < Duration::from_secs(10));

        let mut quick = Command::new("cmd.exe");
        quick.args(["/d", "/c", "echo out& echo err 1>&2"]);
        let (ok, output) = run(quick, Duration::from_secs(10));
        assert!(ok);
        assert!(output.contains("out") && output.contains("err"), "{output}");
    }

    #[test]
    fn environment_points_into_the_service_dir() {
        let vm = Vm::new(Config {
            machine: "harness".into(),
            podman: r"C:\Program Files\RedHat\Podman\podman.exe".into(),
            user_sid: "S-1-5-21-1-2-3-1001".into(),
            startup_memory_mb: 1024,
            minimum_memory_mb: 512,
            maximum_memory_mb: 4096,
        });
        let env = vm.base_env();
        let get = |key: &str| {
            env.iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| PathBuf::from(v))
        };
        for key in [
            "HOME",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "ProgramData",
            "TEMP",
            "CONTAINERS_CONF",
        ] {
            let value = get(key).unwrap_or_else(|| panic!("{key} missing"));
            assert!(value.starts_with(vmd::service_dir()), "{key} = {value:?}");
        }
        assert!(get("XDG_CONFIG_HOME").is_none());
        assert!(get("CONTAINER_HOST").is_none());
        assert_eq!(tail("a\n\nb\nc\n"), "a | b | c");
    }
}
