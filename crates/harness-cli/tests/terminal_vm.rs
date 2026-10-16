//! `harness shell` in a scripted terminal (a pseudo console of the test):
//! terminal size and resizing, Ctrl+C, and the gateway's questions answered
//! in the same terminal, also while the program uses the alternate screen.
//!
//! Ignored by default; needs the running VM, both images and internet:
//! `cargo test -p harness-cli --test terminal_vm -- --ignored --nocapture`

#![cfg(windows)]

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use harness_core::Provider;
use harness_core::podman::{DEFAULT_MACHINE, PodmanProvider};
use harness_core::project::ProjectName;
use harness_core::pty::{PtyProcess, Size};

const HARNESS: &str = env!("CARGO_BIN_EXE_harness");

struct Cleanup {
    provider: PodmanProvider,
    name: ProjectName,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        let p = &self.provider;
        let name = &self.name;
        let _ = p.remove_container(&name.instance());
        let _ = p.remove_container(&name.gateway());
        let _ = p.remove_volume(&name.workspace_volume());
        let _ = p.remove_volume(&name.home_volume());
        let _ = p.remove_network(&name.network());
        if let Ok(exchange) = p.exchange()
            && let Ok(dir) = exchange.host_path(name.exchange_dir())
        {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// Output of the scripted terminal, without escape sequences and spaces
/// (the console may draw runs of spaces as cursor movements).
fn plain(raw: &str) -> String {
    let mut text = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('\x40'..='\x7e').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC up to BEL or ST.
                    while let Some(c) = chars.next() {
                        if c == '\x07' || (c == '\x1b' && chars.next_if_eq(&'\\').is_some()) {
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if !c.is_whitespace() {
            text.push(c);
        }
    }
    text
}

struct Terminal {
    pty: PtyProcess,
    input: fs::File,
    output: Arc<Mutex<String>>,
    /// How much of the plain output earlier waits consumed.
    seen: usize,
}

impl Terminal {
    fn send(&mut self, text: &str) {
        self.input.write_all(text.as_bytes()).unwrap();
        self.input.flush().unwrap();
    }

    /// Waits until `needle` (compared without whitespace) shows up after
    /// what earlier waits found.
    fn expect(&mut self, needle: &str, timeout: Duration) {
        let needle: String = needle.chars().filter(|c| !c.is_whitespace()).collect();
        let deadline = Instant::now() + timeout;
        loop {
            let text = plain(&self.output.lock().unwrap());
            if let Some(pos) = text.get(self.seen..).and_then(|t| t.find(&needle)) {
                self.seen += pos + needle.len();
                return;
            }
            if Instant::now() > deadline {
                if let Some(path) = std::env::var_os("TERMINAL_VM_LOG") {
                    let _ = fs::write(path, self.output.lock().unwrap().as_bytes());
                }
                let tail: String = text
                    .chars()
                    .rev()
                    .take(1500)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                panic!("{needle:?} did not appear; end of output:\n{tail}");
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn absent_so_far(&self, needle: &str) -> bool {
        !plain(&self.output.lock().unwrap()).contains(needle)
    }
}

/// A git repository registered as project in a fresh `HARNESS_HOME`.
struct TestProject {
    _tmp: tempfile::TempDir,
    home: std::path::PathBuf,
    repo: std::path::PathBuf,
    _cleanup: Cleanup,
}

fn test_project(prefix: &str) -> TestProject {
    let machine = std::env::var("HARNESS_MACHINE").unwrap_or_else(|_| DEFAULT_MACHINE.into());
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let folder = format!("{prefix}-{}", std::process::id());
    let repo = tmp.path().join(&folder);
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet", "--initial-branch=main"]);
    git(&repo, &["config", "user.name", "Test"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "core.autocrlf", "false"]);
    fs::write(repo.join("README.md"), "test\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "--quiet", "-m", "initial"]);
    let added = Command::new(HARNESS)
        .args(["project", "add"])
        .current_dir(&repo)
        .env("HARNESS_HOME", &home)
        .output()
        .unwrap();
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    TestProject {
        _cleanup: Cleanup {
            provider: PodmanProvider::for_machine(&machine).unwrap(),
            name: ProjectName::new(&folder).unwrap(),
        },
        _tmp: tmp,
        home,
        repo,
    }
}

impl Terminal {
    /// Starts `harness <args>` in the project, on a 100×30 terminal.
    fn start(project: &TestProject, args: &[&str]) -> (Terminal, thread::JoinHandle<()>) {
        let mut command = Command::new(HARNESS);
        command
            .args(args)
            .current_dir(&project.repo)
            .env("HARNESS_HOME", &project.home)
            .env_remove("HARNESS_DIRECT_TERMINAL");
        let size = Size {
            cols: 100,
            rows: 30,
        };
        let mut pty = PtyProcess::spawn(&command, size).unwrap();
        let mut reader = pty.take_output().unwrap();
        let input = pty.take_input().unwrap();
        let output = Arc::new(Mutex::new(String::new()));
        let collected = Arc::clone(&output);
        let pump = thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                collected
                    .lock()
                    .unwrap()
                    .push_str(&String::from_utf8_lossy(&buffer[..n]));
            }
        });
        let term = Terminal {
            pty,
            input,
            output,
            seen: 0,
        };
        (term, pump)
    }

    /// Waits for the exit code, closes the terminal and keeps its output in
    /// `TERMINAL_VM_LOG` (escape sequences included) if that is set.
    fn finish(self, pump: thread::JoinHandle<()>) -> Option<u32> {
        let code = self.pty.wait(Some(Duration::from_secs(30))).unwrap();
        self.pty.close();
        pump.join().unwrap();
        if let Some(path) = std::env::var_os("TERMINAL_VM_LOG") {
            fs::write(path, self.output.lock().unwrap().as_bytes()).unwrap();
        }
        code
    }
}

#[test]
#[ignore = "needs the running Podman machine, both images and internet access"]
fn shell_with_questions_in_the_same_terminal() {
    let project = test_project("termtest");
    let (mut term, pump) = Terminal::start(&project, &["shell"]);

    // Arithmetic in the commands keeps their echo from matching too early.
    term.send("echo READY-$((40+2))\r");
    term.expect("READY-42", Duration::from_secs(90));

    term.send("echo SIZE-$(stty size | tr ' ' x)\r");
    term.expect("SIZE-30x100", Duration::from_secs(10));
    term.pty
        .resize(Size {
            cols: 120,
            rows: 40,
        })
        .unwrap();
    thread::sleep(Duration::from_millis(1500));
    term.send("echo SIZE-$(stty size | tr ' ' x)\r");
    term.expect("SIZE-40x120", Duration::from_secs(10));

    // Ctrl+C reaches the program in the container, not harness.
    term.send("sleep 30; echo AFTER-$((1+1))\r");
    thread::sleep(Duration::from_secs(2));
    let interrupted = Instant::now();
    term.send("\x03");
    term.send("echo RC-$((6*7))\r");
    term.expect("RC-42", Duration::from_secs(10));
    assert!(interrupted.elapsed() < Duration::from_secs(10));
    assert!(term.absent_so_far("AFTER-2"));

    // A domain outside the preset: the question appears here, "e" allows once.
    term.send(
        "curl -sS --max-time 60 -o /dev/null -w 'HTTP-%{http_code}\\n' https://example.net/\r",
    );
    term.expect(
        "Ein Programm in der Sandbox möchte example.net:443 erreichen",
        Duration::from_secs(30),
    );
    term.send("e");
    term.expect("HTTP-200", Duration::from_secs(30));

    // Esc refuses.
    term.send("curl -sS --max-time 60 -o /dev/null https://example.org/ || echo DENIED-$((3+4))\r");
    term.expect(
        "Ein Programm in der Sandbox möchte example.org:443 erreichen",
        Duration::from_secs(30),
    );
    term.send("\x1b");
    term.expect("DENIED-7", Duration::from_secs(30));

    // While the program uses the alternate screen, the question is drawn
    // over it and the program goes on afterwards.
    term.send("printf '\\033[?1049h'; curl -sS --max-time 60 -o /dev/null -w 'ALT-%{http_code}\\n' https://example.com/; printf '\\033[?1049l'\r");
    term.expect(
        "Ein Programm in der Sandbox möchte example.com:443 erreichen",
        Duration::from_secs(30),
    );
    term.send("e");
    // Keys typed together with the answer are not passed on.
    thread::sleep(Duration::from_secs(1));
    term.send("echo BACK-$((5+5))\r");
    term.expect("BACK-10", Duration::from_secs(30));

    term.send("exit\r");
    assert_eq!(term.finish(pump), Some(0));
}

/// pi's own interface through the relay. Without `harness login` in the
/// test's HARNESS_HOME, `harness run` removes harness's entries from pi's
/// models.json in the shared volume `harness-cfg-pi`; the next paired run
/// writes them again. Not while the user works with pi.
#[test]
#[ignore = "needs the running Podman machine and both images; touches pi's shared config"]
fn pi_draws_and_exits_in_the_relayed_terminal() {
    let project = test_project("termpi");
    let (mut term, pump) = Terminal::start(&project, &["run", "pi"]);
    // pi switches bracketed paste on once its interface is up.
    let deadline = Instant::now() + Duration::from_secs(180);
    while !term.output.lock().unwrap().contains("\x1b[?2004h") {
        assert!(Instant::now() < deadline, "pi did not start");
        thread::sleep(Duration::from_millis(200));
    }
    thread::sleep(Duration::from_secs(3));
    // pi's own requests (version check, telemetry) are switched off in its
    // definition; otherwise every start would ask about pi.dev.
    assert!(term.absent_so_far("RückfragedesGateways"));

    // A question while pi's fullscreen interface runs: pi's `!` runs a
    // command, the question is drawn over pi, and pi shows the result.
    term.send("!curl -sS -o /dev/null -w 'PI-%{http_code}' https://example.net/");
    thread::sleep(Duration::from_millis(500));
    term.send("\r");
    term.expect(
        "Ein Programm in der Sandbox möchte example.net:443 erreichen",
        Duration::from_secs(30),
    );
    term.send("e");
    term.expect("PI-200", Duration::from_secs(30));
    thread::sleep(Duration::from_secs(1));

    // Ctrl+D on an empty editor ends pi.
    term.send("\x04");
    term.expect("pi beendet", Duration::from_secs(30));
    assert_eq!(term.finish(pump), Some(0));
}
