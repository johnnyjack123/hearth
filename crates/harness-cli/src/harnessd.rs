//! `harnessd`: starts `harness daemon run` without a console window, writes
//! its output to `daemon.log` in the configuration folder and restarts it
//! after a crash. Windows starts it at logon (`harness daemon install`).
//!
//! It stays this small on purpose and shares no code with `harness`: a
//! window-less (GUI subsystem) program cannot be the CLI itself, and
//! everything that talks to the server or the VM lives in `harness`.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Must match `daemon::EXIT_UNPAIRED`: restarting would not help.
const EXIT_UNPAIRED: i32 = 3;

/// The log is moved aside once it reaches this size.
const MAX_LOG: u64 = 5 * 1024 * 1024;

/// Same rule as `ConfigDir::default_root` in harness-core.
fn config_root() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("HARNESS_HOME") {
        return Some(PathBuf::from(home));
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(home).join(".harness"))
}

fn main() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let harness = exe.with_file_name(if cfg!(windows) {
        "harness.exe"
    } else {
        "harness"
    });
    let Some(root) = config_root() else {
        return;
    };
    let _ = fs::create_dir_all(&root);
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
        let mut command = Command::new(&harness);
        command
            .args(["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(file)
            .stderr(err);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            // A console without a window: harness and the programs it starts
            // (podman, ssh) share it, so none of them opens a window.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let started = Instant::now();
        let Ok(status) = command.status() else {
            return;
        };
        // 0: stopped on request or already running elsewhere.
        if status.success() || status.code() == Some(EXIT_UNPAIRED) {
            return;
        }
        // A crash after a long run is a new problem, not a crash loop.
        if started.elapsed() > Duration::from_secs(600) {
            wait = Duration::from_secs(5);
        }
        thread::sleep(wait);
        wait = (wait * 2).min(Duration::from_secs(120));
    }
}
