//! Runs an approved command of the direct mode: in the project folder,
//! without input, with a time limit and bounded output, and ends the whole
//! process tree when it is cancelled or runs too long.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Bytes of output kept: the start and the end, the middle is dropped.
pub const MAX_OUTPUT: usize = 60_000;

/// The shell that runs commands, as the model is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    /// Program.
    pub program: PathBuf,
    /// Arguments before the command text.
    pub args: Vec<String>,
    /// Name for the model and the approval, e.g. `bash` or `PowerShell`.
    pub name: String,
}

impl Shell {
    /// The shell of this computer: bash on Linux and macOS (`sh` without
    /// it), Git Bash on Windows when installed, else Windows PowerShell.
    pub fn detect() -> Shell {
        #[cfg(windows)]
        {
            let git_bash = [
                std::env::var_os("ProgramFiles").map(PathBuf::from),
                std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Programs")),
            ]
            .into_iter()
            .flatten()
            .map(|dir| dir.join("Git").join("bin").join("bash.exe"))
            .find(|path| path.is_file());
            if let Some(program) = git_bash {
                return Shell {
                    program,
                    args: vec!["-c".to_owned()],
                    name: "bash (Git Bash on Windows)".to_owned(),
                };
            }
            let system = std::env::var_os("SystemRoot")
                .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
            Shell {
                program: system.join(r"System32\WindowsPowerShell\v1.0\powershell.exe"),
                args: ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"]
                    .map(str::to_owned)
                    .to_vec(),
                name: "Windows PowerShell".to_owned(),
            }
        }
        #[cfg(not(windows))]
        {
            let bash = ["/bin/bash", "/usr/bin/bash", "/usr/local/bin/bash"]
                .into_iter()
                .map(PathBuf::from)
                .find(|path| path.is_file());
            match bash {
                Some(program) => Shell {
                    program,
                    args: vec!["-c".to_owned()],
                    name: "bash".to_owned(),
                },
                None => Shell {
                    program: PathBuf::from("/bin/sh"),
                    args: vec!["-c".to_owned()],
                    name: "sh".to_owned(),
                },
            }
        }
    }
}

/// How a command ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Exit code; `None` when it was ended (timeout, cancel) or died of a signal.
    pub code: Option<i32>,
    /// Standard output and error interleaved as they came, bounded.
    pub output: String,
    /// Whether the time limit ended it.
    pub timed_out: bool,
    /// Whether a cancel ended it.
    pub cancelled: bool,
    /// How long it ran.
    pub elapsed: Duration,
}

/// Keeps the first and the last bytes of a stream.
#[derive(Default)]
struct Bounded {
    head: Vec<u8>,
    tail: Vec<u8>,
    dropped: usize,
}

impl Bounded {
    fn push(&mut self, bytes: &[u8]) {
        let half = MAX_OUTPUT / 2;
        let take = bytes.len().min(half.saturating_sub(self.head.len()));
        self.head.extend_from_slice(&bytes[..take]);
        let rest = &bytes[take..];
        if rest.is_empty() {
            return;
        }
        self.tail.extend_from_slice(rest);
        if self.tail.len() > half {
            let excess = self.tail.len() - half;
            self.tail.drain(..excess);
            self.dropped += excess;
        }
    }

    fn text(&self) -> String {
        let head = String::from_utf8_lossy(&self.head);
        if self.tail.is_empty() {
            return head.into_owned();
        }
        let tail = String::from_utf8_lossy(&self.tail);
        if self.dropped == 0 {
            return format!("{head}{tail}");
        }
        format!(
            "{head}\n[… {} bytes of output left out …]\n{tail}",
            self.dropped
        )
    }
}

/// Runs `command` with `shell` in `cwd`. Variables of hearth itself are
/// removed from the environment; everything else is the user's, because
/// the command runs as the user anyway.
///
/// # Errors
///
/// When the shell cannot be started.
pub fn run(
    shell: &Shell,
    command: &str,
    cwd: &Path,
    timeout: Duration,
    cancel: &AtomicBool,
) -> std::io::Result<Outcome> {
    let started = Instant::now();
    let mut process = Command::new(&shell.program);
    process
        .args(&shell.args)
        .arg(command)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, _) in std::env::vars_os() {
        if name
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("HEARTH_")
        {
            process.env_remove(name);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        // Its own group, so ending it ends what it started too.
        process.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        process.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }
    let mut child = process.spawn()?;
    let output = Arc::new(Mutex::new(Bounded::default()));
    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|mut pipe| {
        let output = Arc::clone(&output);
        thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            while let Ok(read) = pipe.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                if let Ok(mut output) = output.lock() {
                    output.push(&buffer[..read]);
                }
            }
        })
    })
    .collect();

    let (mut timed_out, mut cancelled) = (false, false);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
        } else if started.elapsed() >= timeout {
            timed_out = true;
        }
        if cancelled || timed_out {
            kill_tree(&mut child);
            break child.wait().ok();
        }
        thread::sleep(Duration::from_millis(25));
    };
    // A grandchild may hold the pipes open; it is gone after kill_tree, but
    // a command that left a daemon behind must not hang the call.
    let deadline = Instant::now() + Duration::from_secs(2);
    for reader in readers {
        while !reader.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
    }
    let output = output
        .lock()
        .map(|output| output.text())
        .unwrap_or_default();
    Ok(Outcome {
        code: if timed_out || cancelled {
            None
        } else {
            status.and_then(|status| status.code())
        },
        output,
        timed_out,
        cancelled,
        elapsed: started.elapsed(),
    })
}

/// Ends the command and everything it started.
fn kill_tree(child: &mut Child) {
    #[cfg(unix)]
    if let Ok(group) = libc::pid_t::try_from(child.id()) {
        // SAFETY: plain syscall; the group's id is the child's pid
        // (process_group(0)), and a negative pid names the whole group.
        unsafe {
            libc::kill(-group, libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let system = std::env::var_os("SystemRoot")
            .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        let _ = Command::new(system.join(r"System32\taskkill.exe"))
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    let _ = child.kill();
}
