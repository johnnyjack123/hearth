//! `hearthd`: starts `hearth daemon run` without a console window, writes
//! its output to `daemon.log` in the configuration folder and restarts it
//! after a crash. On Windows it also shows the tray icon. Windows starts it
//! at logon (`hearth daemon install`).
//!
//! It shares no code with `hearth`: a window-less (GUI subsystem) program
//! cannot be the CLI itself, and everything that talks to the server or the
//! VM lives in `hearth`. What the tray shows comes from the service's
//! status file; what it asks for goes through named events.

#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(windows)]
#[path = "hearthd/icon.rs"]
mod icon;
#[cfg(windows)]
#[path = "hearthd/menu.rs"]
mod menu;
#[cfg(windows)]
#[path = "hearthd/tray.rs"]
mod tray;

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

/// Must match `daemon::EXIT_UNPAIRED`: restarting would not help.
const EXIT_UNPAIRED: i32 = 3;

/// The log is moved aside once it reaches this size.
const MAX_LOG: u64 = 5 * 1024 * 1024;

/// After "quit", the service gets this long before it is ended by force.
const QUIT_GRACE: Duration = Duration::from_secs(15);

/// Same rule as `ConfigDir::default_root` in hearth-core.
fn config_root() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("HEARTH_HOME") {
        return Some(PathBuf::from(home));
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join(".hearth"))
}

/// Suffix of the service's named kernel objects; the same as
/// `daemon::object_suffix` in hearth, which creates them.
fn object_suffix(root: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in root.to_string_lossy().to_lowercase().bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Runs `hearth daemon run` and restarts it after crashes, until it ends
/// for good (stopped on request, already running elsewhere, not paired,
/// cannot start) or `quit` is set. `pid` holds the running service's
/// process id (0 between runs), so the tray can tell its status file from
/// an old one.
fn supervise(harness: &Path, root: &Path, quit: &AtomicBool, pid: &AtomicU32) {
    let _ = fs::create_dir_all(root);
    let log = root.join("daemon.log");
    let mut wait = Duration::from_secs(5);
    loop {
        if fs::metadata(&log).is_ok_and(|m| m.len() > MAX_LOG) {
            let _ = fs::rename(&log, root.join("daemon.log.old"));
        }
        let Ok(file) = OpenOptions::new().create(true).append(true).open(&log) else {
            return;
        };
        let Ok(err) = file.try_clone() else {
            return;
        };
        let mut command = Command::new(harness);
        command
            .args(["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(file)
            .stderr(err);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            // A console without a window: hearth and the programs it starts
            // (podman, ssh) share it, so none of them opens a window.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let started = Instant::now();
        let Ok(mut child) = command.spawn() else {
            return;
        };
        pid.store(child.id(), Ordering::SeqCst);
        let mut quit_since: Option<Instant> = None;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {}
                Err(_) => break None,
            }
            if quit.load(Ordering::SeqCst) {
                let since = *quit_since.get_or_insert_with(Instant::now);
                // The tray asked the service to stop; it gets a moment.
                if since.elapsed() > QUIT_GRACE {
                    let _ = child.kill();
                }
            }
            thread::sleep(Duration::from_millis(200));
        };
        pid.store(0, Ordering::SeqCst);
        if quit.load(Ordering::SeqCst) {
            return;
        }
        match status {
            // 0: stopped on request or already running elsewhere.
            Some(status) if status.success() || status.code() == Some(EXIT_UNPAIRED) => return,
            _ => {}
        }
        // A crash after a long run is a new problem, not a crash loop.
        if started.elapsed() > Duration::from_secs(600) {
            wait = Duration::from_secs(5);
        }
        let deadline = Instant::now() + wait;
        while Instant::now() < deadline {
            if quit.load(Ordering::SeqCst) {
                return;
            }
            thread::sleep(Duration::from_millis(200));
        }
        wait = (wait * 2).min(Duration::from_secs(120));
    }
}

fn main() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let harness = exe.with_file_name(if cfg!(windows) {
        "hearth.exe"
    } else {
        "hearth"
    });
    let Some(root) = config_root() else {
        return;
    };
    #[cfg(windows)]
    tray::run(&harness, &root);
    #[cfg(not(windows))]
    supervise(&harness, &root, &AtomicBool::new(false), &AtomicU32::new(0));
}
