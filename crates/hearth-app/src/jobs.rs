//! Longer actions run the `hearth` command next to the app, as in a
//! terminal: login, init, VM, service, removing a project. Their output
//! shows in the window while they run. The logic stays in one place (the
//! CLI), and a slow step never blocks the window.

use std::io::{BufRead as _, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use eframe::egui;

/// The `hearth` program next to this one.
pub fn hearth_program() -> PathBuf {
    let name = if cfg!(windows) {
        "hearth.exe"
    } else {
        "hearth"
    };
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name)))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

/// How a job ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Still running.
    Running,
    /// Ended with success.
    Done,
    /// Ended with an error.
    Failed,
}

/// One action and what it printed.
pub struct Job {
    /// What it does, for people.
    pub title: String,
    state: Arc<Mutex<(Vec<String>, Outcome)>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Job {
    /// The lines so far.
    pub fn lines(&self) -> Vec<String> {
        lock(&self.state).0.clone()
    }

    /// How it stands.
    pub fn outcome(&self) -> Outcome {
        lock(&self.state).1
    }
}

/// The actions of this window, newest last.
#[derive(Default)]
pub struct Jobs {
    jobs: Vec<Job>,
}

impl Jobs {
    /// Starts `hearth <args>` and repaints the window as it prints.
    pub fn start(&mut self, ctx: &egui::Context, title: impl Into<String>, args: &[&str]) {
        let title = title.into();
        let state = Arc::new(Mutex::new((Vec::new(), Outcome::Running)));
        let mut command = Command::new(hearth_program());
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            // No console window flashing up behind the app.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        match command.spawn() {
            Err(error) => {
                lock(&state).0.push(hearth_core::tr!(
                    "hearth startet nicht: {error}",
                    "hearth does not start: {error}"
                ));
                lock(&state).1 = Outcome::Failed;
            }
            Ok(mut child) => {
                let pipes: Vec<Box<dyn Read + Send>> = [
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
                .collect();
                let readers: Vec<_> = pipes
                    .into_iter()
                    .map(|pipe| {
                        let state = Arc::clone(&state);
                        let ctx = ctx.clone();
                        thread::spawn(move || {
                            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                                lock(&state).0.push(line);
                                ctx.request_repaint();
                            }
                        })
                    })
                    .collect();
                let state_done = Arc::clone(&state);
                let ctx = ctx.clone();
                thread::spawn(move || {
                    let ok = child.wait().is_ok_and(|status| status.success());
                    for reader in readers {
                        let _ = reader.join();
                    }
                    lock(&state_done).1 = if ok { Outcome::Done } else { Outcome::Failed };
                    ctx.request_repaint();
                });
            }
        }
        self.jobs.push(Job { title, state });
        // A long session keeps only the latest few.
        if self.jobs.len() > 8 {
            self.jobs.remove(0);
        }
    }

    /// Whether any job runs.
    pub fn busy(&self) -> bool {
        self.jobs
            .iter()
            .any(|job| job.outcome() == Outcome::Running)
    }

    /// Whether a job with this title runs.
    pub fn running(&self, title: &str) -> bool {
        self.jobs
            .iter()
            .any(|job| job.title == title && job.outcome() == Outcome::Running)
    }

    /// The jobs, newest last.
    pub fn all(&self) -> &[Job] {
        &self.jobs
    }

    /// Forgets the finished ones.
    pub fn clear_finished(&mut self) {
        self.jobs.retain(|job| job.outcome() == Outcome::Running);
    }
}

/// Opens a terminal window that runs `hearth <args>` in `dir`, for what
/// needs one (pi's interface, a shell, a merge to confirm).
pub fn open_terminal(dir: &std::path::Path, args: &[&str]) -> Result<(), String> {
    let program = hearth_program();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        // Windows Terminal if installed, else a classic console window.
        let terminal = std::env::var_os("LOCALAPPDATA")
            .map(|dir| PathBuf::from(dir).join(r"Microsoft\WindowsApps\wt.exe"))
            .filter(|path| path.is_file());
        let result = match terminal {
            Some(wt) => Command::new(wt)
                .arg("-d")
                .arg(dir)
                .arg(&program)
                .args(args)
                .spawn(),
            None => Command::new(&program)
                .args(args)
                .current_dir(dir)
                .creation_flags(CREATE_NEW_CONSOLE)
                .spawn(),
        };
        result.map(|_| ()).map_err(|error| error.to_string())
    }
    #[cfg(target_os = "macos")]
    {
        // Terminal.app runs a small script; arguments are quoted for the shell.
        let quote = |text: &str| format!("'{}'", text.replace('\'', "'\\''"));
        let line = std::iter::once(quote(&program.to_string_lossy()))
            .chain(args.iter().map(|arg| quote(arg)))
            .collect::<Vec<_>>()
            .join(" ");
        let script = std::env::temp_dir().join(format!("hearth-{}.command", std::process::id()));
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\ncd {}\nexec {line}\n",
                quote(&dir.to_string_lossy())
            ),
        )
        .map_err(|error| error.to_string())?;
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700));
        Command::new("open")
            .arg("-a")
            .arg("Terminal")
            .arg(&script)
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // The desktop's own terminal where it says so, else common ones.
        let candidates = [
            "x-terminal-emulator",
            "kgx",
            "gnome-terminal",
            "konsole",
            "xfce4-terminal",
            "kitty",
            "alacritty",
            "foot",
            "xterm",
        ];
        for terminal in candidates {
            let Some(path) = hearth_core::paths::find_on_path(terminal) else {
                continue;
            };
            let mut command = Command::new(path);
            command.current_dir(dir);
            match terminal {
                "gnome-terminal" | "kgx" => command.arg("--"),
                _ => command.arg("-e"),
            };
            command.arg(&program).args(args);
            return command
                .spawn()
                .map(|_| ())
                .map_err(|error| error.to_string());
        }
        Err(hearth_core::lang::tr(
            "Kein Terminalprogramm gefunden",
            "No terminal program found",
        )
        .to_owned())
    }
}
