//! The connection of this computer to the harness server, so web chats can
//! use its projects: `harness daemon` keeps it in the background, `harness
//! connect` in a terminal (for watching or testing).
//!
//! The server can only ask for projects released in their project file
//! (`[remote]`, on by default since `harness init`); a project may still
//! demand a confirmation per chat, which only the terminal can give.
//! Agents run exactly as with `harness run`: in the project's instance,
//! behind its gateway, with the device's LLM access. Every step on the way
//! is reported to the chat, so the user sees that something happens.
//! Questions of the gateway go to the chat; answering them is the user's
//! decision in the browser, never the container's.

use std::collections::HashMap;
use std::io::{self, BufRead as _, IsTerminal as _, Write as _};
use std::net::TcpStream;
use std::process::{Child, ChildStdin};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, bail};
use harness_core::Provider;
use harness_core::config::{ConfigDir, HarnessConfig, Project};
use harness_core::gateway::{Event, GatewayControl, Reply, Scope, Watch};
use harness_core::pi_rpc::{self, Output, Translator};
use harness_core::project::ProjectName;
use harness_core::provider::ExecSpec;
use harness_core::server::ServerCa;
use harness_core::sync::{WORKSPACE, sanitize};
use harness_core::tunnel::{
    AgentEvent, ApprovalState, Decision, DeviceMessage, NoticeLevel, ProgressState, Prompt,
    RemoteProject, ServerMessage, TUNNEL_PATH, TUNNEL_VERSION, TakeoverReport, TakeoverRequest,
    TakeoverState, VERSION_VM_CONTROL, VmStatus, truncate,
};
use tungstenite::client::IntoClientRequest as _;
use tungstenite::http::StatusCode;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::daemon::{self, Instance};
use crate::idle::{Idle, Report, Watcher};
use crate::net::{self, Question};
use crate::progress::Progress;
use crate::status::{Connection, StatusFile};
use crate::{login, run, takeover};

/// The only machine interface harness speaks so far.
const PI_RPC: &str = "pi-rpc";

/// How long a read on the socket waits before outgoing messages get a turn.
const POLL: Duration = Duration::from_millis(100);

/// Released projects are looked up again this often, so `harness project
/// remote on|off` and new projects take effect without a restart.
const RESCAN: Duration = Duration::from_secs(10);

/// How long the server may take to greet after the connection is up.
const WELCOME_TIMEOUT: Duration = Duration::from_secs(10);

/// A question on this computer is refused after this long without answer.
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(10 * 60);

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

/// Whether a person watches: only then can a chat be confirmed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `harness connect` in a terminal.
    Foreground,
    /// `harness daemon run`, started hidden by `harnessd`.
    Background,
}

/// A log line; in the background with a time, because it goes to a file.
pub(crate) fn say(mode: Mode, text: &str) {
    match mode {
        Mode::Foreground => println!("{text}"),
        Mode::Background => println!("{} {text}", utc_stamp()),
    }
}

/// `2026-10-18 12:34:56Z`, without a date library.
fn utc_stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rest = secs % 86_400;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// `harness connect`: the connection in this terminal, until Strg+C.
pub fn connect(config: &ConfigDir, machine: &str) -> anyhow::Result<bool> {
    let Some(instance) = Instance::acquire(config)? else {
        bail!(
            "Der Hintergrunddienst hält die Verbindung schon (`harness daemon`). Zum Zuschauen \
             im Terminal erst `harness daemon stop`."
        );
    };
    let Some((client, token)) = login::paired(config)? else {
        bail!("Dieser Rechner ist mit keinem harness-Server gekoppelt: zuerst `harness login`.");
    };
    let ended = keep_connected(
        config,
        machine,
        Mode::Foreground,
        &instance,
        &client,
        &token,
    )?;
    Ok(ended == Ended::Stopped)
}

/// `harness daemon run`: the connection in the background. Returns the
/// process exit code for the launcher (0: stopped, 3: not paired).
pub fn serve_forever(config: &ConfigDir, machine: &str, mode: Mode) -> i32 {
    let instance = match Instance::acquire(config) {
        Ok(Some(instance)) => instance,
        Ok(None) => {
            say(mode, "Läuft schon (anderer Dienst oder harness connect).");
            return 0;
        }
        Err(error) => {
            say(mode, &format!("Start gescheitert: {error:#}"));
            return 1;
        }
    };
    let paired = match login::paired(config) {
        Ok(paired) => paired,
        Err(error) => {
            say(mode, &format!("Kopplung nicht lesbar: {error:#}"));
            return 1;
        }
    };
    let Some((client, token)) = paired else {
        say(
            mode,
            "Nicht gekoppelt (`harness login`); der Dienst beendet sich.",
        );
        return daemon::EXIT_UNPAIRED;
    };
    match keep_connected(config, machine, mode, &instance, &client, &token) {
        Ok(Ended::Stopped) => {
            say(mode, "Beendet.");
            0
        }
        Ok(_) => daemon::EXIT_UNPAIRED,
        Err(error) => {
            say(mode, &format!("Beendet mit Fehler: {error:#}"));
            1
        }
    }
}

/// Connects and reconnects until the device is revoked or a stop is asked for.
fn keep_connected(
    config: &ConfigDir,
    machine: &str,
    mode: Mode,
    instance: &Instance,
    client: &harness_core::server::ServerClient,
    token: &str,
) -> anyhow::Result<Ended> {
    let url = tunnel_url(client.url())?;
    let ca = ServerCa::from_config(&config.global()?.server)?;
    let idle = Idle::new(machine);
    let status = Arc::new(StatusFile::new(config, client.url()));
    // Watches the VM also while the server is unreachable; stops with this
    // function.
    let _watcher = Watcher::spawn(&idle, machine, mode, &status);
    let shared = Arc::new(Shared {
        config: config.clone(),
        machine: machine.to_owned(),
        mode,
        console: match mode {
            Mode::Foreground => Console::new(),
            Mode::Background => Console::none(),
        },
        idle,
        status,
    });
    let projects = offered(config);
    print_offer(mode, &projects);
    // Also through the API, so the projects show up while the tunnel is
    // still being built (or cannot be).
    if let Err(error) = client.set_projects(token, &projects) {
        say(mode, &format!("Projekte nicht gemeldet: {error}"));
    }

    let mut wait = Duration::from_secs(1);
    loop {
        match serve(&shared, instance, &url, ca.as_ref(), token) {
            Ok(Ended::Revoked) => {
                say(
                    mode,
                    "Der Server kennt dieses Gerät nicht mehr (in der Web-App entfernt?). Neu \
                     koppeln mit `harness login`.",
                );
                shared
                    .status
                    .update(|s| s.connection = Connection::Unpaired);
                return Ok(Ended::Revoked);
            }
            Ok(Ended::Stopped) => return Ok(Ended::Stopped),
            Ok(Ended::Closed) => {
                say(mode, "Der Server hat die Verbindung beendet.");
                shared.status.update(|s| {
                    s.connection = Connection::Offline;
                    s.error = Some("Der Server hat die Verbindung beendet.".to_owned());
                });
                wait = Duration::from_secs(1);
            }
            Err(error) => {
                say(
                    mode,
                    &format!("Verbindung zu {} unterbrochen: {error:#}", client.url()),
                );
                shared.status.update(|s| {
                    s.connection = Connection::Offline;
                    s.error = Some(truncate(&format!("{error:#}"), 300));
                });
            }
        }
        say(mode, &format!("Neuer Versuch in {} s …", wait.as_secs()));
        let deadline = Instant::now() + wait;
        while Instant::now() < deadline {
            if instance.stop_requested() {
                return Ok(Ended::Stopped);
            }
            // Without a connection there are no sessions to end first.
            if instance.vm_stop_requested() {
                let shared = Arc::clone(&shared);
                thread::spawn(move || stop_vm_now(&shared, StopFrom::Tray));
            }
            thread::sleep(Duration::from_millis(200));
        }
        wait = (wait * 2).min(Duration::from_secs(30));
    }
}

/// `ws(s)://…/api/device/tunnel` for the server's base URL.
fn tunnel_url(base: &str) -> anyhow::Result<String> {
    let base = base.trim_end_matches('/');
    if let Some(rest) = base.strip_prefix("https://") {
        Ok(format!("wss://{rest}{TUNNEL_PATH}"))
    } else if let Some(rest) = base.strip_prefix("http://") {
        Ok(format!("ws://{rest}{TUNNEL_PATH}"))
    } else {
        bail!("Server-Adresse {base} ist weder http:// noch https://")
    }
}

/// Released projects and their harnesses with a machine interface. Broken
/// project files are skipped instead of stopping everything.
pub fn offered(config: &ConfigDir) -> Vec<RemoteProject> {
    let mut projects = Vec::new();
    let Ok(names) = config.project_names() else {
        return projects;
    };
    for name in names {
        let remote = match config.project_config(&name) {
            Ok(raw) => raw.remote,
            Err(_) => continue,
        };
        if !remote.enabled {
            continue;
        }
        let Ok(project) = config.project(&name) else {
            continue;
        };
        let harnesses: Vec<String> = config
            .harness(&project.harness)
            .ok()
            .filter(|h| h.adapter.as_ref().is_some_and(|a| a.kind == PI_RPC))
            .map(|h| vec![h.name])
            .unwrap_or_default();
        if !harnesses.is_empty() {
            projects.push(RemoteProject {
                name: name.to_string(),
                harnesses,
            });
        }
    }
    projects
}

fn print_offer(mode: Mode, projects: &[RemoteProject]) {
    if projects.is_empty() {
        say(
            mode,
            "Noch kein Projekt für Web-Chats. Im Projektordner: `harness init`.",
        );
    } else {
        let names: Vec<String> = projects
            .iter()
            .map(|p| format!("{} ({})", p.name, p.harnesses.join(", ")))
            .collect();
        say(
            mode,
            &format!("Für Web-Chats freigegeben: {}", names.join(", ")),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ended {
    /// The server closed the connection; reconnect.
    Closed,
    /// The token is no longer valid; reconnecting is pointless.
    Revoked,
    /// `harness daemon stop` (or the end of `harness connect`).
    Stopped,
}

/// What every session thread needs.
struct Shared {
    config: ConfigDir,
    machine: String,
    mode: Mode,
    console: Console,
    /// Use of the VM, for the idle stop and the web app.
    idle: Arc<Idle>,
    /// What the tray icon shows.
    status: Arc<StatusFile>,
}

/// Runs one connection until it ends.
fn serve(
    shared: &Arc<Shared>,
    instance: &Instance,
    url: &str,
    ca: Option<&ServerCa>,
    token: &str,
) -> anyhow::Result<Ended> {
    let mut socket = match open(url, ca, token) {
        Ok(socket) => socket,
        Err(tungstenite::Error::Http(response))
            if response.status() == StatusCode::UNAUTHORIZED =>
        {
            return Ok(Ended::Revoked);
        }
        Err(error) => return Err(error).context("Verbindungsaufbau"),
    };
    let server_version = match welcome(&mut socket)? {
        Ok(version) => version,
        Err(ended) => return Ok(ended),
    };
    // The lower version of both; each side only sends what it knows.
    let version = server_version.min(TUNNEL_VERSION);
    if server_version < VERSION_VM_CONTROL {
        say(
            shared.mode,
            &format!(
                "Der Server spricht noch Protokoll {server_version}: VM-Zustand und -Stopp im \
                 Browser erst nach einem Update des Servers."
            ),
        );
    } else if server_version > TUNNEL_VERSION {
        say(
            shared.mode,
            &format!(
                "Der Server spricht Protokoll {server_version}, dieses harness {TUNNEL_VERSION}. \
                 Neue Funktionen erst nach einem Update von harness."
            ),
        );
    }
    let mut projects = offered(&shared.config);
    send(
        &mut socket,
        &DeviceMessage::Hello {
            version,
            projects: projects.clone(),
        },
    )?;
    let mut reported: Option<Report> = None;
    shared.status.update(|s| {
        s.connection = Connection::Connected;
        s.error = None;
    });
    say(
        shared.mode,
        match shared.mode {
            Mode::Foreground => {
                "Verbunden. Warte auf Nachrichten aus verknüpften Chats (Beenden: Strg+C)."
            }
            Mode::Background => "Verbunden.",
        },
    );

    let (out, outgoing) = mpsc::channel::<DeviceMessage>();
    let mut sessions: HashMap<String, SessionHandle> = HashMap::new();
    let mut rescanned = Instant::now();
    let result = loop {
        if instance.stop_requested() {
            let _ = socket.close(None);
            break Ok(Ended::Stopped);
        }
        if instance.vm_stop_requested() {
            stop_vm(shared, &mut sessions, &out, None);
        }
        match socket.read() {
            Ok(Message::Text(text)) => match serde_json::from_str::<ServerMessage>(&text) {
                Ok(message) => {
                    dispatch(shared, &mut sessions, &out, message);
                }
                Err(error) => say(
                    shared.mode,
                    &format!("Unverständliche Nachricht vom Server: {error}"),
                ),
            },
            Ok(Message::Close(frame)) => {
                if frame.is_some_and(|f| u16::from(f.code) == 4003) {
                    break Ok(Ended::Revoked);
                }
                break Ok(Ended::Closed);
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(tungstenite::Error::ConnectionClosed) => break Ok(Ended::Closed),
            Err(error) => break Err(anyhow::Error::from(error)),
        }
        let mut failed = None;
        while let Ok(message) = outgoing.try_recv() {
            if let Err(error) = send(&mut socket, &message) {
                failed = Some(error);
                break;
            }
        }
        if let Some(error) = failed {
            break Err(error);
        }
        sessions.retain(|_, session| !session.thread.is_finished());
        if rescanned.elapsed() >= RESCAN {
            rescanned = Instant::now();
            let now = offered(&shared.config);
            if now != projects {
                print_offer(shared.mode, &now);
                projects = now;
                send(
                    &mut socket,
                    &DeviceMessage::Projects {
                        projects: projects.clone(),
                    },
                )?;
            }
        }
        // Sent on change only: the stop time is a fixed instant until
        // something happens.
        let report = shared.idle.report();
        if reported != Some(report) {
            send(&mut socket, &report.message(version))?;
            reported = Some(report);
        }
    };
    // Sessions end with the connection; the next message of a chat starts
    // a new one, and pi continues the chat's session from its file.
    for session in sessions.values() {
        let _ = session.commands.send(Command::Close);
    }
    for (_, session) in sessions {
        let _ = session.thread.join();
    }
    shared.idle.end_all();
    shared.status.update(|s| s.questions.clear());
    result
}

/// Waits for the server's greeting: its protocol version, or how the
/// connection ended before.
fn welcome(socket: &mut Socket) -> anyhow::Result<Result<u32, Ended>> {
    let deadline = Instant::now() + WELCOME_TIMEOUT;
    while Instant::now() < deadline {
        match socket.read() {
            Ok(Message::Text(text)) => {
                if let Ok(ServerMessage::Welcome { version }) = serde_json::from_str(&text) {
                    return Ok(Ok(version));
                }
            }
            Ok(Message::Close(frame)) => {
                return Ok(Err(if frame.is_some_and(|f| u16::from(f.code) == 4003) {
                    Ended::Revoked
                } else {
                    Ended::Closed
                }));
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(tungstenite::Error::ConnectionClosed) => return Ok(Err(Ended::Closed)),
            Err(error) => return Err(error).context("Begrüßung des Servers"),
        }
    }
    bail!("Der Server hat nicht gegrüßt.")
}

/// Opens the tunnel. With `[server] ca` only that CA counts for an
/// `https://` server, as for every other request of harness.
fn open(url: &str, ca: Option<&ServerCa>, token: &str) -> Result<Socket, tungstenite::Error> {
    let mut request = url.into_client_request()?;
    let header = tungstenite::http::HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|e| tungstenite::Error::HttpFormat(e.into()))?;
    request.headers_mut().insert("authorization", header);
    let mut socket = match ca.filter(|_| url.starts_with("wss://")) {
        None => tungstenite::connect(request)?.0,
        Some(ca) => {
            let host = request.uri().host().unwrap_or_default().to_owned();
            let port = request.uri().port_u16().unwrap_or(443);
            let host = host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_owned();
            let stream = TcpStream::connect((host.as_str(), port))?;
            let mut builder = native_tls::TlsConnector::builder();
            builder.disable_built_in_roots(true);
            for der in ca.certificates() {
                let certificate = native_tls::Certificate::from_der(der)
                    .map_err(|e| tungstenite::Error::Tls(e.into()))?;
                builder.add_root_certificate(certificate);
            }
            let connector = builder
                .build()
                .map_err(|e| tungstenite::Error::Tls(e.into()))?;
            tungstenite::client_tls_with_config(
                request,
                stream,
                None,
                Some(tungstenite::Connector::NativeTls(connector)),
            )
            .map_err(|e| match e {
                tungstenite::HandshakeError::Failure(error) => error,
                tungstenite::HandshakeError::Interrupted(_) => tungstenite::Error::Io(
                    io::Error::new(io::ErrorKind::WouldBlock, "TLS-Handshake unterbrochen"),
                ),
            })?
            .0
        }
    };
    let stream = match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => Some(&*stream),
        MaybeTlsStream::NativeTls(stream) => Some(stream.get_ref()),
        _ => None,
    };
    if let Some(stream) = stream {
        stream.set_read_timeout(Some(POLL))?;
    }
    Ok(socket)
}

fn send(socket: &mut Socket, message: &DeviceMessage) -> anyhow::Result<()> {
    let text = serde_json::to_string(message)?;
    socket.send(Message::text(text))?;
    Ok(())
}

/// Commands from the connection to a session thread.
enum Command {
    Prompt(Box<Prompt>),
    Cancel,
    Decide(u64, Decision),
    Close,
}

struct SessionHandle {
    project: String,
    harness: String,
    commands: Sender<Command>,
    thread: JoinHandle<()>,
}

fn dispatch(
    shared: &Arc<Shared>,
    sessions: &mut HashMap<String, SessionHandle>,
    out: &Sender<DeviceMessage>,
    message: ServerMessage,
) {
    match message {
        ServerMessage::Welcome { .. } | ServerMessage::Unknown => {}
        ServerMessage::Takeover(request) => start_takeover(shared, out, request),
        ServerMessage::Settings { idle_minutes } => shared.idle.set_minutes(idle_minutes),
        ServerMessage::VmStop { request } => stop_vm(shared, sessions, out, Some(request)),
        ServerMessage::Prompt(prompt) => {
            // First sign of life in the chat: the computer has the message.
            let _ = out.send(DeviceMessage::SessionEvent {
                session: prompt.session.clone(),
                event: AgentEvent::Progress {
                    id: "received".to_owned(),
                    label: "Auf dem Rechner angekommen".to_owned(),
                    state: ProgressState::Done,
                    detail: None,
                },
            });
            // The device decides what runs here, whatever the server says;
            // read fresh, so a project from `harness init` works at once.
            let released = offered(&shared.config)
                .iter()
                .any(|p| p.name == prompt.project && p.harnesses.contains(&prompt.harness));
            if !released {
                let _ = out.send(DeviceMessage::SessionClosed {
                    session: prompt.session.clone(),
                    reason: Some(format!(
                        "Projekt {} ist auf diesem Rechner nicht für Web-Chats mit {} freigegeben.",
                        prompt.project, prompt.harness
                    )),
                });
                return;
            }
            // Until `done`, an error or the session's end (see `Emitter`).
            shared.idle.begin(&prompt.session);
            if let Some(session) = sessions.get(&prompt.session) {
                if session.project == prompt.project && session.harness == prompt.harness {
                    if session
                        .commands
                        .send(Command::Prompt(Box::new(prompt.clone())))
                        .is_ok()
                    {
                        return;
                    }
                } else {
                    let _ = session.commands.send(Command::Close);
                }
            }
            let (commands, receiver) = mpsc::channel();
            let id = prompt.session.clone();
            let handle = SessionHandle {
                project: prompt.project.clone(),
                harness: prompt.harness.clone(),
                commands,
                thread: {
                    let shared = Arc::clone(shared);
                    let out = out.clone();
                    thread::spawn(move || session(&shared, prompt, &receiver, &out))
                },
            };
            if let Some(old) = sessions.insert(id, handle) {
                let _ = old.thread.join();
            }
        }
        ServerMessage::Cancel { session } => {
            if let Some(s) = sessions.get(&session) {
                let _ = s.commands.send(Command::Cancel);
            }
        }
        ServerMessage::Close { session } => {
            if let Some(s) = sessions.remove(&session) {
                let _ = s.commands.send(Command::Close);
            }
        }
        ServerMessage::Decide {
            session,
            id,
            decision,
        } => {
            if let Some(s) = sessions.get(&session) {
                let _ = s.commands.send(Command::Decide(id, decision));
            }
        }
    }
}

/// Takes a chat's agent work over in a thread of its own; pi keeps
/// running. The server sends this only while no reply of the chat runs.
fn start_takeover(shared: &Arc<Shared>, out: &Sender<DeviceMessage>, request: TakeoverRequest) {
    let emit = Emitter {
        session: request.session.clone(),
        out: out.clone(),
        idle: Arc::clone(&shared.idle),
        status: Arc::clone(&shared.status),
    };
    // As for prompts, the device decides; read fresh.
    let released = offered(&shared.config)
        .iter()
        .any(|p| p.name == request.project && p.harnesses.contains(&request.harness));
    let name = match ProjectName::new(&request.project) {
        Ok(name) if released => name,
        _ => {
            emit.closed(Some(format!(
                "Projekt {} ist auf diesem Rechner nicht für Web-Chats freigegeben.",
                request.project
            )));
            return;
        }
    };
    shared.idle.begin(&request.session);
    let shared = Arc::clone(shared);
    thread::spawn(move || {
        let title = request
            .conversation
            .title
            .as_deref()
            .map(sanitize)
            .unwrap_or_else(|| "ohne Titel".to_owned());
        let report = takeover::takeover(
            &shared.config,
            &shared.machine,
            &name,
            false,
            &crate::progress::Terminal::default(),
            &mut |report| {
                emit.event(AgentEvent::Takeover {
                    report: report.clone(),
                });
            },
        );
        say(
            shared.mode,
            &format!("Chat „{title}“: Übernehmen {}", outcome(&report)),
        );
        emit.event(AgentEvent::Done { aborted: false });
    });
}

/// How a takeover ended, for the log.
fn outcome(report: &TakeoverReport) -> String {
    match report.state {
        TakeoverState::Merged => format!(
            "fertig, {} Commit(s) in {}",
            report.commits.unwrap_or(0),
            report.branch.as_deref().unwrap_or("?")
        ),
        TakeoverState::UpToDate => "fertig, nichts Neues".to_owned(),
        TakeoverState::Held => format!(
            "angehalten (schwere Funde), Freigabe mit harness merge --approve {}",
            report.request.as_deref().unwrap_or("?")
        ),
        TakeoverState::Conflict => "angehalten, Konflikte".to_owned(),
        TakeoverState::Refused | TakeoverState::Failed | TakeoverState::Running => format!(
            "nicht möglich: {}",
            report.reason.as_deref().unwrap_or("unbekannt")
        ),
    }
}

/// Stops the VM because the user asked, in the browser (`request` from the
/// server) or at the tray icon. Replies on their way end with that reason
/// instead of "pi wurde beendet"; the next message of a chat starts VM and
/// agent again.
fn stop_vm(
    shared: &Arc<Shared>,
    sessions: &mut HashMap<String, SessionHandle>,
    out: &Sender<DeviceMessage>,
    request: Option<String>,
) {
    let place = if request.is_some() {
        StopFrom::Browser
    } else {
        StopFrom::Tray
    };
    for session in shared.idle.answering() {
        let _ = out.send(DeviceMessage::SessionClosed {
            session: session.clone(),
            reason: Some(format!(
                "Die VM wurde {} gestoppt.",
                place.text("im", "in der")
            )),
        });
        shared.idle.end(&session);
    }
    // The threads end on their own once pi is gone; nobody waits for them.
    for (_, session) in sessions.drain() {
        let _ = session.commands.send(Command::Close);
    }
    let shared = Arc::clone(shared);
    let out = out.clone();
    thread::spawn(move || {
        let (ok, message) = stop_vm_now(&shared, place);
        if let Some(request) = request {
            let _ = out.send(DeviceMessage::VmResult {
                request,
                ok,
                message: truncate(&message, 900),
            });
        }
    });
}

/// Where the user asked to stop the VM.
#[derive(Clone, Copy)]
enum StopFrom {
    Browser,
    Tray,
}

impl StopFrom {
    /// "im Browser" or "in der Taskleiste", with the given prepositions.
    fn text(self, browser: &str, tray: &str) -> String {
        match self {
            StopFrom::Browser => format!("{browser} Browser"),
            StopFrom::Tray => format!("{tray} Taskleiste"),
        }
    }
}

/// Stops the VM through the VM service; the watcher reports the result.
fn stop_vm_now(shared: &Shared, from: StopFrom) -> (bool, String) {
    say(
        shared.mode,
        &format!("VM wird gestoppt (aus {}).", from.text("dem", "der")),
    );
    shared.idle.show(VmStatus::Stopping);
    shared.status.update(|s| {
        s.vm = VmStatus::Stopping;
        s.stop_at = None;
    });
    let (ok, message) = match crate::vm::stop() {
        Ok(message) => (true, message),
        Err(error) => (false, format!("{error:#}")),
    };
    say(shared.mode, &format!("VM-Stopp: {message}"));
    shared.idle.wake();
    (ok, message)
}

/// Events of one session to the server.
#[derive(Clone)]
struct Emitter {
    session: String,
    out: Sender<DeviceMessage>,
    idle: Arc<Idle>,
    status: Arc<StatusFile>,
}

impl Emitter {
    fn event(&self, event: AgentEvent) {
        match &event {
            AgentEvent::Done { .. } | AgentEvent::Error { .. } => self.idle.end(&self.session),
            AgentEvent::Approval {
                id, host, state, ..
            } => {
                self.idle.touch();
                // The tray icon points at open questions.
                let open = *state == ApprovalState::Pending;
                self.status
                    .question(&self.session, *id, open.then_some(host.as_str()));
            }
            _ => self.idle.touch(),
        }
        let _ = self.out.send(DeviceMessage::SessionEvent {
            session: self.session.clone(),
            event,
        });
    }

    fn closed(&self, reason: Option<String>) {
        self.idle.end(&self.session);
        self.status.session_ended(&self.session);
        let _ = self.out.send(DeviceMessage::SessionClosed {
            session: self.session.clone(),
            reason,
        });
    }
}

/// Reports the steps of preparing a session to its chat (and the log).
struct ChatProgress<'a> {
    emit: &'a Emitter,
    mode: Mode,
    title: String,
    steps: Mutex<HashMap<String, (String, Instant)>>,
}

impl ChatProgress<'_> {
    fn event(&self, id: &str, label: &str, state: ProgressState, detail: Option<String>) {
        self.emit.event(AgentEvent::Progress {
            id: id.to_owned(),
            label: label.to_owned(),
            state,
            detail,
        });
    }
}

impl Progress for ChatProgress<'_> {
    fn start(&self, id: &str, label: &str) {
        lock(&self.steps).insert(id.to_owned(), (label.to_owned(), Instant::now()));
        self.event(id, label, ProgressState::Running, None);
    }

    fn done(&self, id: &str) {
        let Some((label, started)) = lock(&self.steps).remove(id) else {
            return;
        };
        let took = format!("{:.1} s", started.elapsed().as_secs_f32());
        say(
            self.mode,
            &format!("Chat „{}“: {label} ✓ ({took})", self.title),
        );
        self.event(id, &label, ProgressState::Done, Some(took));
    }

    fn failed(&self, id: &str, error: &str) {
        let label = lock(&self.steps)
            .remove(id)
            .map_or_else(|| id.to_owned(), |(label, _)| label);
        say(
            self.mode,
            &format!("Chat „{}“: {label} ✗ {error}", self.title),
        );
        self.event(
            id,
            &label,
            ProgressState::Failed,
            Some(truncate(error, 500)),
        );
    }
}

/// A session: environment, pi in RPC mode, until the chat is unlinked, pi
/// exits or the connection ends.
fn session(
    shared: &Shared,
    first: Prompt,
    commands: &Receiver<Command>,
    out: &Sender<DeviceMessage>,
) {
    let emit = Emitter {
        session: first.session.clone(),
        out: out.clone(),
        idle: Arc::clone(&shared.idle),
        status: Arc::clone(&shared.status),
    };
    let title = first
        .conversation
        .title
        .clone()
        .unwrap_or_else(|| "ohne Titel".to_owned());
    match start(shared, &first, &emit, &sanitize(&title)) {
        Ok(Some(mut agent)) => {
            say(
                shared.mode,
                &format!(
                    "Chat „{}“: {} läuft im Projekt {}.",
                    sanitize(&title),
                    agent.harness.name,
                    agent.project.name
                ),
            );
            agent.prompt(&first);
            agent.serve(shared, commands);
            agent.stop();
            say(
                shared.mode,
                &format!("Chat „{}“: Sitzung beendet.", sanitize(&title)),
            );
        }
        Ok(None) => {}
        Err(error) => {
            say(
                shared.mode,
                &format!("Chat „{}“: {error:#}", sanitize(&title)),
            );
            emit.closed(Some(format!(
                "Start auf dem Rechner gescheitert: {error:#}"
            )));
        }
    }
}

/// A running harness for one session.
struct Agent {
    emit: Emitter,
    project: Project,
    harness: HarnessConfig,
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    responses: Receiver<pi_rpc::Response>,
    reader: JoinHandle<()>,
    /// pi provider per server credential, for the model chosen in the chat.
    providers: HashMap<String, String>,
    model: Option<(String, String)>,
    first_prompt: bool,
    next_id: u64,
    approvals: Option<ApprovalBridge>,
    /// Set while the harness has the message but has not answered yet.
    thinking: Arc<Mutex<bool>>,
    /// Holds back the end of a reply so autosync can run first.
    reply_end: ReplyEnd,
    /// Signals from the reader that a reply ended and autosync is due.
    finished: Receiver<()>,
}

/// With autosync, the reader does not pass a reply's `done` on but
/// reports the end here; the session takes the work over, then ends the
/// reply. So the chat shows the takeover as part of the reply.
#[derive(Clone)]
struct ReplyEnd {
    autosync: Arc<AtomicBool>,
    finished: Sender<()>,
}

fn start(
    shared: &Shared,
    prompt: &Prompt,
    emit: &Emitter,
    title: &str,
) -> anyhow::Result<Option<Agent>> {
    let name = ProjectName::new(&prompt.project)?;
    if let Some(refusal) = refused(shared, &name, prompt)? {
        emit.closed(Some(refusal));
        return Ok(None);
    }
    let progress = ChatProgress {
        emit,
        mode: shared.mode,
        title: title.to_owned(),
        steps: Mutex::new(HashMap::new()),
    };
    let project = shared.config.project(&name)?;
    let prepared = run::prepare_project(
        &shared.config,
        &shared.machine,
        project,
        Some(&prompt.harness),
        &progress,
    )?;
    let harness = prepared
        .harnesses
        .iter()
        .find(|h| h.name == prompt.harness)
        .cloned()
        .context("Harness fehlt nach der Vorbereitung")?;
    let Some(adapter) = harness.adapter.clone().filter(|a| a.kind == PI_RPC) else {
        bail!("{} hat keine Anbindung für den Web-Chat", harness.name);
    };
    let mut base = adapter.command.clone();
    base.extend(pi_rpc::session_args(
        &prompt.session,
        prompt.conversation.title.as_deref(),
    ));
    let ready = run::ready_harness(&shared.config, &prepared, &harness, base, &progress)?;
    let providers = ready
        .models
        .map(|m| m.by_credential.into_iter().collect())
        .unwrap_or_default();

    let spec = ExecSpec {
        container: prepared.instance(),
        command: ready.command,
        workdir: Some(WORKSPACE.to_owned()),
        env: harness.env.clone(),
        interactive: true,
        tty: false,
    };
    let mut child = crate::progress::step(
        &progress,
        "harness",
        &format!("{} starten", harness.name),
        || Ok(prepared.provider.exec_piped(&spec)?),
    )?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        bail!("keine Verbindung zu {}", harness.name);
    };
    let stdin = Arc::new(Mutex::new(stdin));
    let (responses_tx, responses) = mpsc::channel();
    let thinking = Arc::new(Mutex::new(false));
    let (finished_tx, finished) = mpsc::channel();
    let reply_end = ReplyEnd {
        autosync: Arc::new(AtomicBool::new(false)),
        finished: finished_tx,
    };
    let reader = {
        let emit = emit.clone();
        let stdin = Arc::clone(&stdin);
        let thinking = Arc::clone(&thinking);
        let name = harness.name.clone();
        let reply_end = reply_end.clone();
        thread::spawn(move || {
            read_agent(
                stdout,
                &emit,
                &stdin,
                &responses_tx,
                &thinking,
                &reply_end,
                &name,
            );
        })
    };
    let approvals = if prepared.project.ask {
        ApprovalBridge::start(&prepared.provider, &prepared.project, emit)
    } else {
        None
    };
    Ok(Some(Agent {
        emit: emit.clone(),
        project: prepared.project,
        harness,
        child,
        stdin,
        responses,
        reader,
        providers,
        model: None,
        first_prompt: true,
        next_id: 0,
        approvals,
        thinking,
        reply_end,
        finished,
    }))
}

/// Why the chat may not use the project, if it may not: not released, or
/// the project wants a confirmation that is missing (only a terminal can
/// give it).
fn refused(shared: &Shared, name: &ProjectName, prompt: &Prompt) -> anyhow::Result<Option<String>> {
    let remote = shared.config.project_config(name)?.remote;
    if !remote.enabled {
        return Ok(Some(format!(
            "Projekt {name} ist auf diesem Rechner nicht für Web-Chats freigegeben \
             (`harness project remote on`)."
        )));
    }
    if !remote.confirm || remote.chats.contains(&prompt.conversation.id) {
        return Ok(None);
    }
    if shared.mode == Mode::Background {
        return Ok(Some(format!(
            "Projekt {name} verlangt eine Bestätigung pro Chat (confirm = true). Die geht nur mit \
             `harness connect` im Terminal, nicht mit dem Hintergrunddienst."
        )));
    }
    Ok((!confirmed_here(shared, name, prompt)?)
        .then(|| "Auf dem Rechner nicht bestätigt.".to_owned()))
}

/// Asks in the terminal of `harness connect`; a yes is remembered.
fn confirmed_here(shared: &Shared, name: &ProjectName, prompt: &Prompt) -> anyhow::Result<bool> {
    let title = prompt
        .conversation
        .title
        .as_deref()
        .map(sanitize)
        .unwrap_or_else(|| "ohne Titel".to_owned());
    let question = format!(
        "\nChat „{title}“ möchte im Projekt {} mit {} arbeiten.\n\
         Erste Nachricht: {}\n\
         Erlauben? Der Chat wird gemerkt und fragt nicht wieder. [j/n] ",
        prompt.project,
        prompt.harness,
        sanitize(&prompt.text.chars().take(200).collect::<String>())
    );
    let Some(answer) = shared.console.ask(&question) else {
        println!("Keine Antwort, abgelehnt.");
        return Ok(false);
    };
    if !matches!(
        answer.trim().to_lowercase().as_str(),
        "j" | "ja" | "y" | "yes"
    ) {
        println!("Abgelehnt.");
        return Ok(false);
    }
    shared
        .config
        .remember_remote_chat(name, &prompt.conversation.id)?;
    Ok(true)
}

/// Reads pi's stdout until it ends: events to the chat, answers to pi,
/// command responses to the session.
fn read_agent(
    stdout: std::process::ChildStdout,
    emit: &Emitter,
    stdin: &Mutex<ChildStdin>,
    responses: &Sender<pi_rpc::Response>,
    thinking: &Mutex<bool>,
    reply_end: &ReplyEnd,
    name: &str,
) {
    let mut translator = Translator::new();
    // From pi's model data; the chat shows how full the context is.
    let mut context_window = None;
    let mut reader = io::BufReader::new(stdout);
    let mut line = Vec::new();
    // Split on LF only: pi's JSON strings may contain U+2028, which a
    // generic line reader would treat as a line end.
    while let Ok(n) = reader.read_until(b'\n', &mut line) {
        if n == 0 {
            break;
        }
        let text = String::from_utf8_lossy(&line);
        for output in translator.feed(&text) {
            match output {
                Output::Event(event) => {
                    // The first visible sign of an answer ends "thinking".
                    let answers = !matches!(
                        event,
                        AgentEvent::Usage { .. } | AgentEvent::TurnStart | AgentEvent::TurnEnd
                    );
                    if answers && std::mem::take(&mut *lock(thinking)) {
                        emit.event(AgentEvent::Progress {
                            id: "thinking".to_owned(),
                            label: format!("{name} denkt nach"),
                            state: ProgressState::Done,
                            detail: None,
                        });
                    }
                    let event = match event {
                        AgentEvent::Usage {
                            input_tokens,
                            output_tokens,
                            cached_input_tokens,
                            cost,
                            context_window: None,
                        } => AgentEvent::Usage {
                            input_tokens,
                            output_tokens,
                            cached_input_tokens,
                            cost,
                            context_window,
                        },
                        other => other,
                    };
                    if event == (AgentEvent::Done { aborted: false })
                        && reply_end.autosync.load(Ordering::SeqCst)
                        && reply_end.finished.send(()).is_ok()
                    {
                        continue;
                    }
                    emit.event(event);
                }
                Output::Reply(reply) => write_line(stdin, &reply),
                Output::Response(response) => {
                    if response.success {
                        let window = match response.command.as_str() {
                            "get_state" => response.data.get("model"),
                            "set_model" => Some(&response.data),
                            _ => None,
                        }
                        .and_then(|model| model.get("contextWindow"))
                        .and_then(serde_json::Value::as_u64);
                        if window.is_some() {
                            context_window = window;
                        }
                        let _ = responses.send(response);
                    } else if response.command == "prompt" {
                        emit.event(AgentEvent::error(format!(
                            "pi hat die Nachricht abgelehnt: {}",
                            response.error.as_deref().unwrap_or("unbekannter Fehler")
                        )));
                    } else {
                        emit.event(AgentEvent::notice(
                            NoticeLevel::Warning,
                            format!(
                                "pi: {} gescheitert: {}",
                                response.command,
                                response.error.as_deref().unwrap_or("unbekannter Fehler")
                            ),
                        ));
                    }
                }
            }
        }
        line.clear();
    }
    emit.event(AgentEvent::error("pi wurde beendet."));
}

fn write_line(stdin: &Mutex<ChildStdin>, line: &str) {
    let mut stdin = lock(stdin);
    let _ = writeln!(stdin, "{line}").and_then(|()| stdin.flush());
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Agent {
    fn id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}-{}", self.next_id)
    }

    fn send(&self, line: &str) {
        write_line(&self.stdin, line);
    }

    fn prompt(&mut self, prompt: &Prompt) {
        self.reply_end
            .autosync
            .store(prompt.autosync, Ordering::SeqCst);
        if let Some(model) = &prompt.model {
            match self.providers.get(&model.credential_id) {
                Some(provider) => {
                    let wanted = (provider.clone(), model.model_id.clone());
                    if self.model.as_ref() != Some(&wanted) {
                        let id = self.id("model");
                        self.send(&pi_rpc::set_model(&id, &wanted.0, &wanted.1));
                        self.model = Some(wanted);
                    }
                }
                None => self.emit.event(AgentEvent::notice(
                    NoticeLevel::Warning,
                    "Das im Chat gewählte Modell ist für pi nicht angebunden, pi nutzt sein \
                     zuletzt gewähltes Modell.",
                )),
            }
        }
        let mut text = prompt.text.clone();
        if self.first_prompt {
            self.first_prompt = false;
            // Asked even without history: the answer also carries the
            // model's context window.
            let new_session = self.session_is_new();
            if let Some(history) = &prompt.history
                && new_session
            {
                text = pi_rpc::with_history(history, &text);
            }
        }
        let id = self.id("prompt");
        *lock(&self.thinking) = true;
        self.emit.event(AgentEvent::Progress {
            id: "thinking".to_owned(),
            label: format!("{} denkt nach", self.harness.name),
            state: ProgressState::Running,
            detail: None,
        });
        self.send(&pi_rpc::prompt(&id, &text, false));
    }

    /// Whether pi's session for this chat has no messages yet, so the chat's
    /// history has to be passed on.
    fn session_is_new(&mut self) -> bool {
        let id = self.id("state");
        self.send(&pi_rpc::get_state(&id));
        let deadline = Instant::now() + Duration::from_secs(15);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match self.responses.recv_timeout(left) {
                Ok(response) if response.id.as_deref() == Some(id.as_str()) => {
                    return response
                        .data
                        .get("messageCount")
                        .and_then(serde_json::Value::as_u64)
                        == Some(0);
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        false
    }

    fn serve(&mut self, shared: &Shared, commands: &Receiver<Command>) {
        loop {
            match commands.recv_timeout(Duration::from_millis(500)) {
                Ok(Command::Prompt(prompt)) => self.prompt(&prompt),
                Ok(Command::Cancel) => self.send(&pi_rpc::abort()),
                Ok(Command::Decide(id, decision)) => {
                    if let Some(bridge) = &self.approvals {
                        bridge.decide(&shared.config, &self.project, id, decision);
                    }
                }
                Ok(Command::Close) | Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
            if self.finished.try_recv().is_ok() {
                self.autosync(shared);
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                self.emit.closed(Some(format!(
                    "{} wurde beendet (Exit-Code {}).",
                    self.harness.name,
                    status.code().unwrap_or(-1)
                )));
                return;
            }
        }
    }

    /// Takes the reply's work over, then ends the reply. Only a result worth
    /// a look reaches the chat; a reply that changed nothing stays as it is.
    fn autosync(&self, shared: &Shared) {
        let step = |state| AgentEvent::Progress {
            id: "autosync".to_owned(),
            label: "Arbeit übernehmen (Autosync)".to_owned(),
            state,
            detail: None,
        };
        self.emit.event(step(ProgressState::Running));
        let report = takeover::takeover(
            &shared.config,
            &shared.machine,
            &self.project.name,
            true,
            &crate::progress::Terminal::default(),
            &mut |_| {},
        );
        let failed = matches!(report.state, TakeoverState::Failed);
        self.emit.event(step(if failed {
            ProgressState::Failed
        } else {
            ProgressState::Done
        }));
        let quiet = report.state == TakeoverState::UpToDate
            && report.committed.is_none()
            && report.notes.is_empty();
        if !quiet {
            self.emit.event(AgentEvent::Takeover {
                report: report.clone(),
            });
        }
        say(
            shared.mode,
            &format!(
                "Projekt {}: Autosync {}",
                self.project.name,
                outcome(&report)
            ),
        );
        self.emit.event(AgentEvent::Done { aborted: false });
    }

    /// Ends pi: closing stdin asks it to shut down, the rest is force.
    fn stop(mut self) {
        drop(self.approvals.take());
        drop(self.stdin);
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = self.reader.join();
    }
}

/// Watches the project's gateway and turns its questions into chat events;
/// answers from the chat go back the same way.
struct ApprovalBridge {
    decisions: Sender<(u64, Option<Scope>)>,
    questions: Arc<Mutex<HashMap<u64, Asked>>>,
    thread: Option<JoinHandle<()>>,
}

/// An open question of the gateway. It stays until the gateway reports it
/// resolved, so that event can still name the host.
struct Asked {
    question: Question,
    /// Answered from the chat; a second answer is ignored.
    answered: bool,
}

impl ApprovalBridge {
    fn start(provider: &dyn Provider, project: &Project, emit: &Emitter) -> Option<Self> {
        let watch = match GatewayControl::new(provider, &project.name).watch() {
            Ok(watch) => watch,
            Err(error) => {
                emit.event(AgentEvent::notice(
                    NoticeLevel::Warning,
                    format!(
                        "Rückfragen des Gateways gehen nicht in den Chat ({error}). \
                         Auf dem Rechner: `harness net watch`."
                    ),
                ));
                return None;
            }
        };
        let (decisions, receiver) = mpsc::channel();
        let questions = Arc::new(Mutex::new(HashMap::new()));
        let thread = {
            let emit = emit.clone();
            let questions = Arc::clone(&questions);
            thread::spawn(move || watch_gateway(watch, &emit, &questions, &receiver))
        };
        Some(ApprovalBridge {
            decisions,
            questions,
            thread: Some(thread),
        })
    }

    fn decide(&self, config: &ConfigDir, project: &Project, id: u64, decision: Decision) {
        let mut questions = lock(&self.questions);
        let Some(asked) = questions.get_mut(&id).filter(|asked| !asked.answered) else {
            return;
        };
        // The same answers as in `harness net watch`; "always" also writes
        // the host into the project's allow list.
        let key = match decision {
            Decision::Once => "e",
            Decision::Minutes => "m",
            Decision::Session => "s",
            Decision::Always => "i",
            Decision::Deny => "n",
        };
        let Some((scope, note)) = net::decide(config, project, &asked.question, key) else {
            return;
        };
        asked.answered = true;
        drop(questions);
        if let Some(note) = note {
            println!("{note}");
        }
        let _ = self.decisions.send((id, scope));
    }
}

impl Drop for ApprovalBridge {
    fn drop(&mut self) {
        // Closing the channel ends the watcher, which says bye to the gateway.
        let (dead, _) = mpsc::channel();
        self.decisions = dead;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn watch_gateway(
    mut watch: Watch,
    emit: &Emitter,
    questions: &Mutex<HashMap<u64, Asked>>,
    decisions: &Receiver<(u64, Option<Scope>)>,
) {
    let mut last_ping = Instant::now();
    loop {
        match decisions.try_recv() {
            Ok((id, scope)) => {
                if watch.decide(id, scope).is_err() {
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
        match watch.next(Duration::from_millis(100)) {
            Ok(Some(Reply::Event(Event::Pending { id, host, port }))) => {
                emit.event(AgentEvent::Approval {
                    id,
                    host: host.to_string(),
                    port,
                    state: ApprovalState::Pending,
                });
                let question = Question { id, host, port };
                lock(questions).insert(
                    id,
                    Asked {
                        question,
                        answered: false,
                    },
                );
            }
            Ok(Some(Reply::Event(Event::Resolved { id, .. }))) => {
                let asked = lock(questions).remove(&id);
                let (host, port) = asked
                    .map(|asked| (asked.question.host.to_string(), asked.question.port))
                    .unwrap_or_else(|| ("?".to_owned(), 1));
                emit.event(AgentEvent::Approval {
                    id,
                    host,
                    port,
                    state: ApprovalState::Resolved,
                });
            }
            Ok(_) => {}
            Err(_) => return,
        }
    }
}

/// Questions on this terminal, one at a time, from any session thread.
struct Console {
    asking: Mutex<()>,
    lines: Mutex<Option<Receiver<String>>>,
}

impl Console {
    /// No questions at all, for the background service.
    fn none() -> Self {
        Console {
            asking: Mutex::new(()),
            lines: Mutex::new(None),
        }
    }

    fn new() -> Self {
        let lines = io::stdin().is_terminal().then(|| {
            let (tx, rx) = mpsc::channel();
            thread::spawn(move || {
                for line in io::stdin().lock().lines() {
                    let Ok(line) = line else { break };
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
            rx
        });
        Console {
            asking: Mutex::new(()),
            lines: Mutex::new(lines),
        }
    }

    /// The answer to `question`, or `None` without a terminal or in time.
    fn ask(&self, question: &str) -> Option<String> {
        let _turn = lock(&self.asking);
        let lines = lock(&self.lines);
        let lines = lines.as_ref()?;
        // Lines typed while nobody asked are no answer.
        while lines.try_recv().is_ok() {}
        print!("{question}");
        let _ = io::stdout().flush();
        lines.recv_timeout(CONFIRM_TIMEOUT).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_urls() {
        assert_eq!(
            tunnel_url("http://localhost:8080/").unwrap(),
            "ws://localhost:8080/api/device/tunnel"
        );
        assert_eq!(
            tunnel_url("https://harness.example.com").unwrap(),
            "wss://harness.example.com/api/device/tunnel"
        );
        assert!(tunnel_url("ftp://x").is_err());
    }
}
