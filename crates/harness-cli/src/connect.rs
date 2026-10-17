//! `harness connect`: keeps this computer connected to the harness server,
//! so web chats linked to a released project run their agent here.
//!
//! The server can only ask for projects released with `harness project
//! remote on`, and each new chat is confirmed here first unless the project
//! says otherwise. Agents run exactly as with `harness run`: in the
//! project's instance, behind its gateway, with the device's LLM access.
//! Questions of the gateway go to the chat; answering them is the user's
//! decision in the browser, never the container's.

use std::collections::HashMap;
use std::io::{self, BufRead as _, IsTerminal as _, Write as _};
use std::net::TcpStream;
use std::process::{Child, ChildStdin};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};
use harness_core::Provider;
use harness_core::config::{ConfigDir, HarnessConfig, Project};
use harness_core::gateway::{Event, GatewayControl, Reply, Scope, Watch};
use harness_core::pi_rpc::{self, Output, Translator};
use harness_core::project::ProjectName;
use harness_core::provider::ExecSpec;
use harness_core::sync::{WORKSPACE, sanitize};
use harness_core::tunnel::{
    AgentEvent, ApprovalState, Decision, DeviceMessage, NoticeLevel, Prompt, RemoteProject,
    ServerMessage, TUNNEL_PATH, TUNNEL_VERSION,
};
use tungstenite::client::IntoClientRequest as _;
use tungstenite::http::StatusCode;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::net::{self, Question};
use crate::{login, run};

/// The only machine interface harness speaks so far.
const PI_RPC: &str = "pi-rpc";

/// How long a read on the socket waits before outgoing messages get a turn.
const POLL: Duration = Duration::from_millis(100);

/// Released projects are looked up again this often, so `harness project
/// remote on` takes effect without a restart.
const RESCAN: Duration = Duration::from_secs(10);

/// A question on this computer is refused after this long without answer.
const CONFIRM_TIMEOUT: Duration = Duration::from_secs(10 * 60);

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

/// `harness connect`.
pub fn connect(config: &ConfigDir, machine: &str) -> anyhow::Result<bool> {
    let Some((client, token)) = login::paired(config)? else {
        bail!("Dieser Rechner ist mit keinem harness-Server gekoppelt: zuerst `harness login`.");
    };
    let url = tunnel_url(client.url())?;
    let shared = Arc::new(Shared {
        config: config.clone(),
        machine: machine.to_owned(),
        console: Console::new(),
    });
    let projects = offered(config);
    print_offer(&projects);

    let mut wait = Duration::from_secs(1);
    loop {
        match serve(&shared, &url, &token) {
            Ok(Ended::Revoked) => {
                println!(
                    "Der Server kennt dieses Gerät nicht mehr (in der Web-App entfernt?). \
                     Neu koppeln mit `harness login`."
                );
                return Ok(false);
            }
            Ok(Ended::Closed) => {
                println!("Der Server hat die Verbindung beendet.");
                wait = Duration::from_secs(1);
            }
            Err(error) => println!("Verbindung zu {} unterbrochen: {error:#}", client.url()),
        }
        println!("Neuer Versuch in {} s …", wait.as_secs());
        thread::sleep(wait);
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
/// project files are skipped with a note instead of stopping everything.
fn offered(config: &ConfigDir) -> Vec<RemoteProject> {
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

fn print_offer(projects: &[RemoteProject]) {
    if projects.is_empty() {
        println!(
            "Noch kein Projekt für Web-Chats freigegeben. Im Projektordner: \
             `harness project remote on`."
        );
    } else {
        let names: Vec<String> = projects
            .iter()
            .map(|p| format!("{} ({})", p.name, p.harnesses.join(", ")))
            .collect();
        println!("Für Web-Chats freigegeben: {}", names.join(", "));
    }
}

enum Ended {
    /// The server closed the connection; reconnect.
    Closed,
    /// The token is no longer valid; reconnecting is pointless.
    Revoked,
}

/// What every session thread needs.
struct Shared {
    config: ConfigDir,
    machine: String,
    console: Console,
}

/// Runs one connection until it ends.
fn serve(shared: &Arc<Shared>, url: &str, token: &str) -> anyhow::Result<Ended> {
    let mut socket = match open(url, token) {
        Ok(socket) => socket,
        Err(tungstenite::Error::Http(response))
            if response.status() == StatusCode::UNAUTHORIZED =>
        {
            return Ok(Ended::Revoked);
        }
        Err(error) => return Err(error).context("Verbindungsaufbau"),
    };
    let mut projects = offered(&shared.config);
    send(
        &mut socket,
        &DeviceMessage::Hello {
            version: TUNNEL_VERSION,
            projects: projects.clone(),
        },
    )?;
    println!("Verbunden. Warte auf Nachrichten aus verknüpften Chats (Beenden: Strg+C).");

    let (out, outgoing) = mpsc::channel::<DeviceMessage>();
    let mut sessions: HashMap<String, SessionHandle> = HashMap::new();
    let mut rescanned = Instant::now();
    let result = loop {
        match socket.read() {
            Ok(Message::Text(text)) => match serde_json::from_str::<ServerMessage>(&text) {
                Ok(message) => {
                    dispatch(shared, &projects, &mut sessions, &out, message);
                }
                Err(error) => println!("Unverständliche Nachricht vom Server: {error}"),
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
                print_offer(&now);
                projects = now;
                send(
                    &mut socket,
                    &DeviceMessage::Projects {
                        projects: projects.clone(),
                    },
                )?;
            }
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
    result
}

fn open(url: &str, token: &str) -> Result<Socket, tungstenite::Error> {
    let mut request = url.into_client_request()?;
    let header = tungstenite::http::HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|e| tungstenite::Error::HttpFormat(e.into()))?;
    request.headers_mut().insert("authorization", header);
    let (mut socket, _) = tungstenite::connect(request)?;
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
    projects: &[RemoteProject],
    sessions: &mut HashMap<String, SessionHandle>,
    out: &Sender<DeviceMessage>,
    message: ServerMessage,
) {
    match message {
        ServerMessage::Welcome { version } if version != TUNNEL_VERSION => println!(
            "Der Server spricht Protokoll {version}, dieses harness {TUNNEL_VERSION}. \
             Bitte harness oder den Server aktualisieren."
        ),
        ServerMessage::Welcome { .. } | ServerMessage::Unknown => {}
        ServerMessage::Prompt(prompt) => {
            // The device decides what runs here, whatever the server says.
            let released = projects
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

/// Events of one session to the server.
#[derive(Clone)]
struct Emitter {
    session: String,
    out: Sender<DeviceMessage>,
}

impl Emitter {
    fn event(&self, event: AgentEvent) {
        let _ = self.out.send(DeviceMessage::SessionEvent {
            session: self.session.clone(),
            event,
        });
    }

    fn closed(&self, reason: Option<String>) {
        let _ = self.out.send(DeviceMessage::SessionClosed {
            session: self.session.clone(),
            reason,
        });
    }
}

/// A session: confirmation, environment, pi in RPC mode, until the chat
/// is unlinked, pi exits or the connection ends.
fn session(
    shared: &Shared,
    first: Prompt,
    commands: &Receiver<Command>,
    out: &Sender<DeviceMessage>,
) {
    let emit = Emitter {
        session: first.session.clone(),
        out: out.clone(),
    };
    let title = first
        .conversation
        .title
        .clone()
        .unwrap_or_else(|| "ohne Titel".to_owned());
    match start(shared, &first, &emit) {
        Ok(Some(mut agent)) => {
            println!(
                "Chat „{}“: {} läuft im Projekt {}.",
                sanitize(&title),
                agent.harness.name,
                agent.project.name
            );
            agent.prompt(&first);
            agent.serve(shared, commands);
            agent.stop();
            println!("Chat „{}“: Sitzung beendet.", sanitize(&title));
        }
        Ok(None) => {}
        Err(error) => {
            println!("Chat „{}“: {error:#}", sanitize(&title));
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
}

fn start(shared: &Shared, prompt: &Prompt, emit: &Emitter) -> anyhow::Result<Option<Agent>> {
    let name = ProjectName::new(&prompt.project)?;
    if !confirmed(shared, &name, prompt)? {
        emit.closed(Some("Auf dem Rechner nicht bestätigt.".to_owned()));
        return Ok(None);
    }
    emit.event(AgentEvent::notice(
        NoticeLevel::Info,
        format!("Umgebung für {} wird vorbereitet …", prompt.project),
    ));
    let project = shared.config.project(&name)?;
    let prepared = run::prepare_project(
        &shared.config,
        &shared.machine,
        project,
        Some(&prompt.harness),
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
    let ready = run::ready_harness(&shared.config, &prepared, &harness, base)?;
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
    let mut child = prepared.provider.exec_piped(&spec)?;
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.kill();
        bail!("keine Verbindung zu {}", harness.name);
    };
    let stdin = Arc::new(Mutex::new(stdin));
    let (responses_tx, responses) = mpsc::channel();
    let reader = {
        let emit = emit.clone();
        let stdin = Arc::clone(&stdin);
        thread::spawn(move || read_agent(stdout, &emit, &stdin, &responses_tx))
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
    }))
}

/// Whether the chat may use the project: released, and confirmed here
/// (now or earlier) unless the project waives that.
fn confirmed(shared: &Shared, name: &ProjectName, prompt: &Prompt) -> anyhow::Result<bool> {
    let remote = shared.config.project_config(name)?.remote;
    if !remote.enabled {
        return Ok(false);
    }
    if !remote.confirm || remote.chats.contains(&prompt.conversation.id) {
        return Ok(true);
    }
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
) {
    let mut translator = Translator::new();
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
                Output::Event(event) => emit.event(event),
                Output::Reply(reply) => write_line(stdin, &reply),
                Output::Response(response) => {
                    if response.success {
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
            if let Some(history) = &prompt.history
                && self.session_is_new()
            {
                text = pi_rpc::with_history(history, &text);
            }
        }
        let id = self.id("prompt");
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
    questions: Arc<Mutex<HashMap<u64, Question>>>,
    thread: Option<JoinHandle<()>>,
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
        let Some(question) = lock(&self.questions).remove(&id) else {
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
        let Some((scope, note)) = net::decide(config, project, &question, key) else {
            return;
        };
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
    questions: &Mutex<HashMap<u64, Question>>,
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
                lock(questions).insert(id, Question { id, host, port });
            }
            Ok(Some(Reply::Event(Event::Resolved { id, .. }))) => {
                let question = lock(questions).remove(&id);
                let (host, port) = question
                    .map(|q| (q.host.to_string(), q.port))
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
