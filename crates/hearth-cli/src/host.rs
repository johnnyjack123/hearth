//! The direct mode on this computer: takes the server's tool calls
//! (`host.call`), checks them with the project's rules, asks the user where
//! the rules say so and runs them (`hearth_core::host`).
//!
//! Questions wait in `host-approvals.json` for the desktop app, the tray
//! icon or `hearth host`; their answers come back as files. The chat may
//! answer only where the project allows it (`[host] browser_approvals`),
//! read fresh from the project file for every answer.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use hearth_core::config::{ConfigDir, HostConfig, HostWrites, valid_chat_id};
use hearth_core::host::{
    self, Decision, Plan, Policy, Request, Session, Shell, ToolCall, audit, inbox,
};
use hearth_core::project::ProjectName;
use hearth_core::tr;
use hearth_core::tunnel::{
    ApprovalState, DeviceMessage, HostApprovalInfo, HostCall, HostInfo, MAX_TOOL_OUTPUT, truncate,
};

use crate::connect::{Mode, say};

/// A question is denied after this long without an answer.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// How often answers left on this computer are looked for.
const INBOX_POLL: Duration = Duration::from_millis(250);

/// Who answered a question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum By {
    Computer,
    Browser,
    Timeout,
}

impl By {
    fn wire(self) -> &'static str {
        match self {
            By::Computer => "computer",
            By::Browser => "browser",
            By::Timeout => "timeout",
        }
    }
}

/// One chat's session with the rules it was made with.
struct Chat {
    project: String,
    host: HostConfig,
    session: Session,
}

/// A question waiting for its answer.
struct Waiting {
    session: String,
    call: String,
    answer: Sender<(Decision, By)>,
    info: inbox::Pending,
}

/// What a connection needs for the direct mode.
pub struct Broker {
    config: ConfigDir,
    mode: Mode,
    out: Sender<DeviceMessage>,
    chats: Mutex<HashMap<String, Chat>>,
    waiting: Mutex<HashMap<String, Waiting>>,
    running: Mutex<HashMap<(String, String), Arc<AtomicBool>>>,
    stopped: Arc<AtomicBool>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}

/// The computer's system, as the server's chat model is told.
pub fn os_name() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// What a project in the direct mode tells the server.
pub fn info(config: &ConfigDir, name: &ProjectName, host: &HostConfig) -> Option<HostInfo> {
    let project = config.project(name).ok()?;
    Some(HostInfo {
        root: project.path.display().to_string(),
        os: os_name().to_owned(),
        shell: Shell::detect().name,
        browser_approvals: host.browser_approvals,
    })
}

impl Broker {
    /// A broker that sends to the server through `out`; it watches for
    /// answers on this computer until it is dropped.
    pub fn start(config: ConfigDir, mode: Mode, out: Sender<DeviceMessage>) -> Arc<Broker> {
        // Answers of an earlier run wait for nobody.
        inbox::clear(config.root());
        let broker = Arc::new(Broker {
            config,
            mode,
            out,
            chats: Mutex::new(HashMap::new()),
            waiting: Mutex::new(HashMap::new()),
            running: Mutex::new(HashMap::new()),
            stopped: Arc::new(AtomicBool::new(false)),
        });
        let watcher = Arc::downgrade(&broker);
        let stopped = Arc::clone(&broker.stopped);
        thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                thread::sleep(INBOX_POLL);
                let Some(broker) = watcher.upgrade() else {
                    return;
                };
                broker.take_answers_here();
            }
        });
        broker
    }

    /// Ends everything: waiting questions are denied, running commands end.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        for waiting in lock(&self.waiting).values() {
            let _ = waiting.answer.send((Decision::Deny, By::Timeout));
        }
        for cancel in lock(&self.running).values() {
            cancel.store(true, Ordering::Relaxed);
        }
        inbox::clear(self.config.root());
    }

    fn take_answers_here(&self) {
        let waiting = lock(&self.waiting);
        for (id, entry) in waiting.iter() {
            if let Some(decision) = inbox::take(self.config.root(), id) {
                let _ = entry.answer.send((decision, By::Computer));
            }
        }
    }

    fn publish(&self) {
        let pending: Vec<inbox::Pending> = lock(&self.waiting)
            .values()
            .map(|waiting| waiting.info.clone())
            .collect();
        if let Err(error) = inbox::write_pending(self.config.root(), &pending) {
            say(
                self.mode,
                &tr!(
                    "Offene Freigaben nicht gespeichert: {error}",
                    "Waiting approvals not saved: {error}"
                ),
            );
        }
    }

    /// A tool call from the server; answered on its own thread.
    pub fn call(self: &Arc<Self>, call: HostCall) {
        let broker = Arc::clone(self);
        thread::spawn(move || {
            let (session, id) = (call.session.clone(), call.call.clone());
            let output = broker.handle(&call);
            let _ = broker.out.send(DeviceMessage::HostResult {
                session,
                call: id,
                output: truncate(&output.text, MAX_TOOL_OUTPUT),
                is_error: output.is_error,
                exit_code: output.exit_code,
            });
        });
    }

    /// Ends a call: a waiting question counts as denied, a running command
    /// ends.
    pub fn cancel(&self, session: &str, call: &str) {
        for waiting in lock(&self.waiting).values() {
            if waiting.session == session && waiting.call == call {
                let _ = waiting.answer.send((Decision::Deny, By::Timeout));
            }
        }
        if let Some(cancel) = lock(&self.running).get(&(session.to_owned(), call.to_owned())) {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    /// The chat's answer; taken only where the project allows answers from
    /// the chat, read fresh from its file.
    pub fn decide_from_browser(&self, session: &str, approval: &str, decision: Decision) {
        let waiting = lock(&self.waiting);
        let Some(entry) = waiting
            .get(approval)
            .filter(|entry| entry.session == session)
        else {
            return;
        };
        let allowed = ProjectName::new(&entry.info.project)
            .ok()
            .and_then(|name| self.config.project_config(&name).ok())
            .is_some_and(|project| project.host.enabled && project.host.browser_approvals);
        if allowed {
            let _ = entry.answer.send((decision, By::Browser));
        } else {
            say(
                self.mode,
                &tr!(
                    "Antwort aus dem Browser auf {approval} ignoriert: Das Projekt erlaubt Freigaben nur an diesem Rechner.",
                    "Answer from the browser to {approval} ignored: the project allows approvals on this computer only."
                ),
            );
        }
    }

    /// Forgets a chat's "for this chat" answers (chat unlinked or deleted).
    pub fn close(&self, session: &str) {
        lock(&self.chats).remove(session);
    }

    fn record(&self, project: &str, entry: &audit::Entry) {
        if let Err(error) = audit::record(self.config.root(), project, entry) {
            say(
                self.mode,
                &tr!(
                    "Protokoll nicht geschrieben: {error}",
                    "Record not written: {error}"
                ),
            );
        }
    }

    /// The project's direct mode, if it may run: offered to web chats and
    /// switched on here.
    fn host_config(&self, project: &str) -> Result<(ProjectName, HostConfig), String> {
        let name = ProjectName::new(project).map_err(|error| error.to_string())?;
        let raw = self
            .config
            .project_config(&name)
            .map_err(|error| error.to_string())?;
        if !raw.remote.enabled || !raw.host.enabled {
            return Err(format!(
                "The direct mode is off for project {project} on the computer; tools cannot run."
            ));
        }
        Ok((name, raw.host))
    }

    /// A copy of the chat's session to plan or run with; made anew when the
    /// project's rules changed (its "for this chat" answers go with it).
    fn session_for(
        &self,
        call: &HostCall,
        name: &ProjectName,
        host: &HostConfig,
    ) -> Result<Session, String> {
        let mut chats = lock(&self.chats);
        if let Some(chat) = chats.get(&call.session)
            && chat.project == call.project
            && chat.host == *host
        {
            return Ok(chat.session.clone());
        }
        let project = self
            .config
            .project(name)
            .map_err(|error| error.to_string())?;
        let policy = Policy::new(
            &project.path,
            &host::hearth_folders(self.config.root()),
            host.writes == HostWrites::Ask,
        )
        .map_err(|error| error.0)?;
        let session = Session::new(policy, Shell::detect());
        chats.insert(
            call.session.clone(),
            Chat {
                project: call.project.clone(),
                host: host.clone(),
                session: session.clone(),
            },
        );
        Ok(session)
    }

    fn handle(&self, call: &HostCall) -> host::Output {
        if !valid_chat_id(&call.session) {
            return host::Output::error("Invalid chat id.");
        }
        let (name, config) = match self.host_config(&call.project) {
            Ok(found) => found,
            Err(text) => return host::Output::error(text),
        };
        let Some(tool) = call.parse() else {
            return host::Output::error("Unknown tool or invalid input.");
        };
        let session = match self.session_for(call, &name, &config) {
            Ok(session) => session,
            Err(text) => return host::Output::error(text),
        };
        let target = describe(&tool);
        let (decision, by) = match session.plan(&tool) {
            Plan::Run => ("free", "rules"),
            Plan::Refuse(text) => {
                self.record(
                    &call.project,
                    &audit::Entry::now(&call.session, tool.name(), &target, "refused", "rules"),
                );
                return host::Output::error(text);
            }
            Plan::Ask(request) => match self.ask(call, &config, &request) {
                (Decision::Deny, by) => {
                    self.record(
                        &call.project,
                        &audit::Entry::now(&call.session, tool.name(), &target, "deny", by.wire()),
                    );
                    return host::Output::error(match by {
                        By::Timeout => {
                            "Nobody answered the approval in time; the call did not run."
                        }
                        _ => {
                            "The user denied this. Do not retry it; ask the user what to do instead."
                        }
                    });
                }
                (decision, by) => {
                    if let Some(chat) = lock(&self.chats).get_mut(&call.session) {
                        chat.session.grant(&request, decision);
                    }
                    (
                        if decision == Decision::Chat {
                            "chat"
                        } else {
                            "once"
                        },
                        by.wire(),
                    )
                }
            },
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let key = (call.session.clone(), call.call.clone());
        lock(&self.running).insert(key.clone(), Arc::clone(&cancel));
        let output = session.execute(&tool, &cancel);
        lock(&self.running).remove(&key);
        let mut entry = audit::Entry::now(&call.session, tool.name(), &target, decision, by);
        entry.exit = output.exit_code;
        entry.error = output.is_error;
        self.record(&call.project, &entry);
        if matches!(tool, ToolCall::Shell { .. }) {
            say(
                self.mode,
                &tr!(
                    "{}: Befehl ausgeführt ({by}): {}",
                    "{}: command run ({by}): {}",
                    call.project,
                    truncate(&target, 200)
                ),
            );
        }
        output
    }

    /// Asks the user and waits for the answer.
    fn ask(&self, call: &HostCall, config: &HostConfig, request: &Request) -> (Decision, By) {
        let id = loop {
            let id = inbox::new_id();
            if !lock(&self.waiting).contains_key(&id) {
                break id;
            }
        };
        let info = inbox::Pending {
            id: id.clone(),
            project: call.project.clone(),
            chat: call.session.clone(),
            title: call.conversation.title.clone(),
            request: request.clone(),
            summary: request.summary(),
            why: request.why(),
            chat_wide: request.chat_wide(),
            browser: config.browser_approvals,
            since: now_secs(),
        };
        let (answer, answers) = mpsc::channel();
        lock(&self.waiting).insert(
            id.clone(),
            Waiting {
                session: call.session.clone(),
                call: call.call.clone(),
                answer,
                info: info.clone(),
            },
        );
        self.publish();
        notify_desktop(&info);
        let _ = self
            .out
            .send(approval_message(call, &info, ApprovalState::Pending, None));
        say(
            self.mode,
            &tr!(
                "{}: Freigabe {id} wartet: {}",
                "{}: approval {id} waits: {}",
                call.project,
                truncate(&info.summary, 200)
            ),
        );

        let started = Instant::now();
        let (decision, by) = loop {
            match answers.recv_timeout(Duration::from_millis(500)) {
                Ok(answer) => break answer,
                Err(RecvTimeoutError::Disconnected) => break (Decision::Deny, By::Timeout),
                Err(RecvTimeoutError::Timeout) if started.elapsed() >= ANSWER_TIMEOUT => {
                    break (Decision::Deny, By::Timeout);
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        };
        // "For this chat" only where it is offered.
        let decision = if decision == Decision::Chat && !request.chat_wide() {
            Decision::Once
        } else {
            decision
        };
        lock(&self.waiting).remove(&id);
        self.publish();
        let _ = self.out.send(approval_message(
            call,
            &info,
            ApprovalState::Resolved,
            Some((decision, by)),
        ));
        (decision, by)
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn approval_message(
    call: &HostCall,
    info: &inbox::Pending,
    state: ApprovalState,
    answer: Option<(Decision, By)>,
) -> DeviceMessage {
    DeviceMessage::HostApproval {
        session: call.session.clone(),
        info: HostApprovalInfo {
            approval: info.id.clone(),
            call: call.call.clone(),
            state,
            request: info.request.clone(),
            summary: truncate(&info.summary, 480),
            why: truncate(&info.why, 980),
            chat_wide: info.chat_wide,
            browser: info.browser,
            decision: answer.map(|(decision, _)| decision),
            by: answer.map(|(_, by)| by.wire().to_owned()),
        },
    }
}

/// Tells the user on the desktop that a question waits. On Windows the
/// tray icon does that (it reads `host-approvals.json`); elsewhere there is
/// no tray yet, so the service itself asks the desktop: `notify-send` with
/// an "Open" action that starts the app (Linux), or a notification through
/// `osascript` (macOS). Best effort: without them the app and `hearth
/// host` still show the question.
fn notify_desktop(info: &inbox::Pending) {
    if cfg!(windows) || std::env::var_os("HEARTH_TRAY_QUIET").is_some() {
        return;
    }
    let title = "Hearth";
    let body = tr!(
        "{}: {} – wartet auf deine Freigabe",
        "{}: {} – waits for your approval",
        info.project,
        info.summary
    );
    let app = std::env::current_exe()
        .ok()
        .map(|exe| exe.with_file_name("hearth-app"));
    thread::spawn(move || {
        if cfg!(target_os = "macos") {
            let quote = |text: &str| text.replace('\\', "\\\\").replace('"', "\\\"");
            let _ = std::process::Command::new("osascript")
                .arg("-e")
                .arg(format!(
                    "display notification \"{}\" with title \"{title}\"",
                    quote(&body)
                ))
                .output();
            return;
        }
        let open = hearth_core::lang::tr("Öffnen", "Open");
        // --wait and --action need libnotify 0.8; older ones fail on them,
        // then the plain notification follows.
        let answered = std::process::Command::new("notify-send")
            .args(["--app-name=Hearth", "--urgency=critical", "--wait"])
            .arg(format!("--action=open={open}"))
            .args([title, &body])
            .output();
        match answered {
            Ok(output) if output.status.success() => {
                if String::from_utf8_lossy(&output.stdout).trim() == "open"
                    && let Some(app) = app.filter(|app| app.is_file())
                {
                    let _ = std::process::Command::new(app)
                        .args(["--page", "approvals"])
                        .spawn();
                }
            }
            _ => {
                let _ = std::process::Command::new("notify-send")
                    .args(["--app-name=Hearth", "--urgency=critical", title, &body])
                    .output();
            }
        }
    });
}

/// The path or command of a call, for the record.
fn describe(tool: &ToolCall) -> String {
    match tool {
        ToolCall::Read { path, .. }
        | ToolCall::Write { path, .. }
        | ToolCall::Edit { path, .. } => path.clone(),
        ToolCall::List { path } => path.clone().unwrap_or_else(|| ".".to_owned()),
        ToolCall::Find { pattern, path } | ToolCall::Grep { pattern, path, .. } => {
            format!("{} in {}", pattern, path.as_deref().unwrap_or("."))
        }
        ToolCall::Shell { command, .. } => command.clone(),
    }
}
