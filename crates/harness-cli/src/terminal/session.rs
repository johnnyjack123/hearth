//! The agent on a pseudo console that harness relays to the user's
//! terminal, so that harness can take the screen over for a moment when the
//! gateway asks whether the agent may reach a domain.
//!
//! Threads: one pumps the agent's output to the terminal (held back while a
//! question is shown), one keeps the gateway's watch connection, and this
//! one reads keys, follows the window size and shows the questions.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read, Stdout, Write};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use harness_core::Provider;
use harness_core::gateway::{Event, GatewayControl, Reply, Scope, Watch};
use harness_core::podman::PodmanProvider;
use harness_core::provider::ExecSpec;
use harness_core::pty::{PtyProcess, Size};

use super::Approvals;
use super::console::RawConsole;
use super::screen::{AltScreen, KeyReader};
use crate::net::{self, Question};

/// Why the session could not start; the caller then attaches directly.
pub struct NotStarted(pub String);

/// What the output pump and the question overlay share.
struct Screen {
    out: Stdout,
    /// While a question is shown, the agent's output waits here.
    paused: bool,
    held: Vec<u8>,
    alt: AltScreen,
}

impl Screen {
    fn write(&mut self, bytes: &[u8]) {
        // Rust's console stdout only takes UTF-8; the pseudo console sends
        // that, but a broken byte must not end the session.
        if self.out.write_all(bytes).is_err() {
            let _ = self
                .out
                .write_all(String::from_utf8_lossy(bytes).as_bytes());
        }
        let _ = self.out.flush();
    }
}

fn lock(screen: &Mutex<Screen>) -> MutexGuard<'_, Screen> {
    screen
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

enum GatewayEvent {
    Pending(Question),
    Resolved(u64),
}

/// Raw traffic between terminal and agent, for diagnosing terminal
/// problems: `HARNESS_TERMINAL_LOG=<file>`. It contains every key typed,
/// so it is off unless asked for.
struct Trace {
    file: Option<Mutex<File>>,
    start: Instant,
}

impl Trace {
    fn from_env() -> Trace {
        Trace {
            file: std::env::var_os("HARNESS_TERMINAL_LOG")
                .and_then(|path| File::create(path).ok())
                .map(Mutex::new),
            start: Instant::now(),
        }
    }

    fn log(&self, what: &str, bytes: &[u8]) {
        let Some(file) = &self.file else {
            return;
        };
        let text: String = String::from_utf8_lossy(bytes)
            .chars()
            .flat_map(|c| match c {
                '\x1b' => "\\e".chars().collect::<Vec<_>>(),
                c if c.is_control() => c.escape_default().collect(),
                c => vec![c],
            })
            .collect();
        let mut file = file.lock().unwrap_or_else(|p| p.into_inner());
        let _ = writeln!(
            file,
            "{:>8.3} {what} {text}",
            self.start.elapsed().as_secs_f64()
        );
    }
}

/// A question on screen.
struct Shown {
    id: u64,
    /// Whether harness switched to the alternate screen itself (and switches
    /// back), or drew over the agent's alternate screen (and has it redrawn).
    own_alt: bool,
}

/// Runs `spec` (with TTY) relayed through a pseudo console. Questions of
/// the gateway are shown if `approvals` is given and the project asks.
pub fn run(
    provider: &PodmanProvider,
    spec: &ExecSpec,
    approvals: Option<Approvals<'_>>,
) -> Result<anyhow::Result<i32>, NotStarted> {
    let command = provider
        .exec_command(spec)
        .map_err(|e| NotStarted(e.to_string()))?;
    let approvals = approvals.filter(|a| a.project.ask);
    // Opened before the terminal goes raw, so a failure reads normally.
    let watch = match &approvals {
        Some(a) => match GatewayControl::new(provider, &a.project.name).watch() {
            Ok(watch) => Some(watch),
            Err(error) => {
                println!(
                    "Rückfragen des Gateways gehen hier nicht ({error}); nutze `harness net watch`."
                );
                None
            }
        },
        None => None,
    };
    let mut console = RawConsole::enable().map_err(|e| NotStarted(e.to_string()))?;
    let size = console.size().map_err(|e| NotStarted(e.to_string()))?;
    // The pseudo console paints from the top left of the window; what was
    // on screen moves into the scrollback instead of being painted over.
    {
        let mut out = io::stdout();
        let _ = out.write_all("\r\n".repeat(usize::from(size.rows)).as_bytes());
        let _ = out.write_all(b"\x1b[H");
        let _ = out.flush();
    }
    let mut pty = PtyProcess::spawn(&command, size).map_err(|e| NotStarted(e.to_string()))?;
    let output = pty.take_output().expect("fresh pty has its output");
    let input = pty.take_input().expect("fresh pty has its input");

    let screen = Mutex::new(Screen {
        out: io::stdout(),
        paused: false,
        held: Vec::new(),
        alt: AltScreen::default(),
    });
    let mut notes = Vec::new();
    let trace = Trace::from_env();
    trace.log("size", format!("{}x{}", size.cols, size.rows).as_bytes());
    let result = thread::scope(|scope| {
        scope.spawn(|| pump(output, &screen, &trace));
        let (events_tx, events) = mpsc::channel();
        let (decisions, decisions_rx) = mpsc::channel();
        if let Some(watch) = watch {
            scope.spawn(move || watcher(watch, &events_tx, &decisions_rx));
        } else {
            drop(events_tx);
        }
        let mut session = Session {
            pty,
            input,
            screen: &screen,
            approvals,
            size,
            queue: VecDeque::new(),
            shown: None,
            notes: &mut notes,
            trace: &trace,
            keys: KeyReader::default(),
        };
        let result = session.run(&mut console, &events, &decisions);
        if session.shown.is_some() {
            session.hide();
        }
        drop(decisions);
        // Ends the output once it is drained; the pump is still reading.
        session.pty.close();
        result
    });
    drop(console);
    for note in notes {
        println!("{note}");
    }
    Ok(result)
}

fn pump(mut output: File, screen: &Mutex<Screen>, trace: &Trace) {
    let mut buffer = vec![0u8; 16 * 1024];
    loop {
        let n = match output.read(&mut buffer) {
            Ok(0) | Err(_) => {
                trace.log("out", b"<end>");
                return;
            }
            Ok(n) => n,
        };
        let chunk = &buffer[..n];
        trace.log("out", chunk);
        let mut screen = lock(screen);
        screen.alt.feed(chunk);
        if screen.paused {
            screen.held.extend_from_slice(chunk);
        } else {
            let started = Instant::now();
            screen.write(chunk);
            let took = started.elapsed();
            if took > Duration::from_millis(50) {
                trace.log("slow", format!("{} ms", took.as_millis()).as_bytes());
            }
        }
    }
}

/// Keeps the watch connection: forwards questions and their resolution,
/// sends the user's decisions, pings.
fn watcher(
    mut watch: Watch,
    events: &Sender<GatewayEvent>,
    decisions: &Receiver<(u64, Option<Scope>)>,
) {
    let mut last_ping = Instant::now();
    loop {
        match decisions.try_recv() {
            Ok((id, decision)) => {
                if watch.decide(id, decision).is_err() {
                    return;
                }
            }
            Err(TryRecvError::Disconnected) => return,
            Err(TryRecvError::Empty) => {}
        }
        if last_ping.elapsed() >= Watch::PING_INTERVAL {
            if watch.ping().is_err() {
                return;
            }
            last_ping = Instant::now();
        }
        let event = match watch.next(Duration::from_millis(100)) {
            Ok(Some(Reply::Event(Event::Pending { id, host, port }))) => {
                GatewayEvent::Pending(Question { id, host, port })
            }
            Ok(Some(Reply::Event(Event::Resolved { id, .. }))) => GatewayEvent::Resolved(id),
            Ok(_) => continue,
            Err(_) => return,
        };
        if events.send(event).is_err() {
            return;
        }
    }
}

struct Session<'a> {
    pty: PtyProcess,
    input: File,
    screen: &'a Mutex<Screen>,
    approvals: Option<Approvals<'a>>,
    size: Size,
    queue: VecDeque<Question>,
    shown: Option<Shown>,
    notes: &'a mut Vec<String>,
    trace: &'a Trace,
    keys: KeyReader,
}

impl Session<'_> {
    fn run(
        &mut self,
        console: &mut RawConsole,
        events: &Receiver<GatewayEvent>,
        decisions: &Sender<(u64, Option<Scope>)>,
    ) -> anyhow::Result<i32> {
        loop {
            if let Some(code) = self.pty.wait(Some(Duration::ZERO))? {
                return Ok(code as i32);
            }
            while let Ok(event) = events.try_recv() {
                match event {
                    GatewayEvent::Pending(question) => self.queue.push_back(question),
                    GatewayEvent::Resolved(id) => {
                        self.queue.retain(|q| q.id != id);
                        if self.shown.as_ref().is_some_and(|s| s.id == id) {
                            self.hide();
                        }
                    }
                }
            }
            if self.shown.is_none() && !self.queue.is_empty() {
                self.show();
            }

            let now = console.size()?;
            if now != self.size {
                self.size = now;
                self.pty.resize(now)?;
                if self.shown.is_some() {
                    self.draw_question();
                }
            }

            let text = console.read(Duration::from_millis(30))?;
            if !text.is_empty() {
                self.trace.log("in", text.as_bytes());
            }
            if self.shown.is_none() {
                // The agent may stop reading for a while; a full pipe must
                // not freeze the terminal, so a failed write ends nothing.
                let _ = self.input.write_all(text.as_bytes());
                continue;
            }
            let keys = if text.is_empty() {
                self.keys.idle()
            } else {
                self.keys.feed(&text)
            };
            for key in keys {
                if self.answer(key, decisions) {
                    break;
                }
            }
        }
    }

    /// Handles a key while a question is shown; true once it was answered.
    fn answer(&mut self, key: char, decisions: &Sender<(u64, Option<Scope>)>) -> bool {
        let (Some(shown), Some(approvals)) = (&self.shown, &self.approvals) else {
            return false;
        };
        let key = match key {
            // Esc and Ctrl+C refuse.
            '\x1b' | '\x03' => "n".to_owned(),
            other => other.to_lowercase().to_string(),
        };
        let Some(question) = self.queue.iter().find(|q| q.id == shown.id) else {
            return false;
        };
        let Some((decision, note)) =
            net::decide(approvals.config, approvals.project, question, &key)
        else {
            return false;
        };
        let id = shown.id;
        self.notes.extend(note);
        let _ = decisions.send((id, decision));
        // Not waiting for the gateway's confirmation: if the connection
        // dropped, the user would be stuck on the question.
        self.queue.retain(|q| q.id != id);
        self.hide();
        true
    }

    fn show(&mut self) {
        let Some(question) = self.queue.front() else {
            return;
        };
        let mut screen = lock(self.screen);
        screen.paused = true;
        let own_alt = !screen.alt.active();
        if own_alt {
            screen.write(b"\x1b[?1049h");
        }
        self.shown = Some(Shown {
            id: question.id,
            own_alt,
        });
        drop(screen);
        self.draw_question();
    }

    fn draw_question(&self) {
        let Some(question) = self
            .shown
            .as_ref()
            .and_then(|s| self.queue.iter().find(|q| q.id == s.id))
        else {
            return;
        };
        let more = match self.queue.len() {
            0 | 1 => String::new(),
            n => format!(" ({} weitere warten)", n - 1),
        };
        let text = format!(
            "\x1b[0m\x1b[H\x1b[2J\r\n  \x1b[1mharness · Rückfrage des Gateways\x1b[0m{more}\r\n\r\n  \
             Der Agent möchte {}:{} erreichen.\r\n\r\n  {}\r\n\r\n  \
             Bis du antwortest, wartet die Anfrage, höchstens 2 Minuten. Esc lehnt ab.\r\n",
            question.host,
            question.port,
            net::answers(),
        );
        lock(self.screen).write(text.as_bytes());
    }

    fn hide(&mut self) {
        let Some(shown) = self.shown.take() else {
            return;
        };
        self.keys.reset();
        let mut screen = lock(self.screen);
        if shown.own_alt {
            // Brings back the agent's screen exactly as it was.
            screen.write(b"\x1b[?1049l");
        } else {
            screen.write(b"\x1b[0m\x1b[H\x1b[2J");
        }
        let held = std::mem::take(&mut screen.held);
        screen.write(&held);
        screen.paused = false;
        drop(screen);
        if !shown.own_alt && self.size.rows > 1 {
            // The agent's alternate screen was drawn over; a size change
            // makes the console and the agent paint it again.
            let smaller = Size {
                rows: self.size.rows - 1,
                ..self.size
            };
            let _ = self.pty.resize(smaller);
            thread::sleep(Duration::from_millis(50));
            let _ = self.pty.resize(self.size);
        }
    }
}
