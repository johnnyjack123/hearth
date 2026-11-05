//! State of a running gateway: rules, approvals, open questions, watchers
//! and the connection log, shared by all connection threads.

use std::collections::VecDeque;
use std::io::{self, BufRead, Read, Write};
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::address;
use crate::control::{Event, GrantInfo, LogEntry, MAX_LINE, Request, Scope, Status};
use crate::llm::LlmUpstream;
use crate::pattern::{DomainPattern, Host};
use crate::policy::{Mode, Policy, Reason};
use crate::upstream::Connector;

/// Tunables of a [`Gateway`].
#[derive(Debug, Clone)]
pub struct Options {
    /// How long a request waits for the user before it fails.
    pub ask_timeout: Duration,
    /// Addresses the gateway may connect to. Always [`address::is_public`]
    /// in production; tests swap it to reach servers on loopback.
    pub address_filter: fn(IpAddr) -> bool,
    /// Name resolution; the system resolver in production, a fixed table in
    /// tests.
    pub resolve: fn(&str, u16) -> io::Result<Vec<SocketAddr>>,
    /// Concurrent proxy connections; more get `503`.
    pub max_connections: usize,
    /// Entries kept for `status`.
    pub log_capacity: usize,
    /// Print every connection attempt to stdout (`podman logs`).
    pub print_log: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            ask_timeout: Duration::from_secs(120),
            address_filter: address::is_public,
            resolve: system_resolve,
            max_connections: 200,
            log_capacity: 200,
            print_log: false,
        }
    }
}

fn system_resolve(name: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    Ok((name, port).to_socket_addrs()?.collect())
}

/// Handle to the shared state; cheap to clone.
#[derive(Clone)]
pub struct Gateway {
    inner: Arc<Inner>,
}

struct Inner {
    options: Options,
    state: Mutex<State>,
    connections: AtomicUsize,
}

#[derive(Default)]
struct State {
    policy: Policy,
    grants: Vec<Grant>,
    denied: Vec<DomainPattern>,
    pending: Vec<Arc<Pending>>,
    next_id: u64,
    watchers: Vec<Watcher>,
    next_watcher: u64,
    log: VecDeque<LogEntry>,
    llm: Option<(LlmUpstream, Connector)>,
    project: Option<String>,
}

struct Grant {
    pattern: DomainPattern,
    until: Option<Instant>,
}

struct Watcher {
    id: u64,
    tx: SyncSender<String>,
}

/// A question to the user that one or more connections wait on.
pub(crate) struct Pending {
    id: u64,
    host: Host,
    port: u16,
    outcome: Mutex<Option<(bool, Reason)>>,
    decided: Condvar,
}

/// What to do with a new connection.
pub(crate) enum Verdict {
    Allow(Reason),
    Deny(Reason),
    Ask(Arc<Pending>),
}

/// Events queued per watcher; a watcher that falls this far behind is
/// dropped instead of slowing down the proxy.
const WATCH_QUEUE: usize = 1000;

impl Gateway {
    /// A gateway that blocks everything until it receives a policy.
    pub fn new(options: Options) -> Gateway {
        Gateway {
            inner: Arc::new(Inner {
                options,
                state: Mutex::new(State {
                    next_id: 1,
                    ..State::default()
                }),
                connections: AtomicUsize::new(0),
            }),
        }
    }

    pub(crate) fn options(&self) -> &Options {
        &self.inner.options
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A panicking connection thread must not take the gateway down.
        self.inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Counts a new connection; `false` if the limit is reached.
    pub(crate) fn connection_opened(&self) -> bool {
        let before = self.inner.connections.fetch_add(1, Ordering::SeqCst);
        if before >= self.inner.options.max_connections {
            self.connection_closed();
            false
        } else {
            true
        }
    }

    pub(crate) fn connection_closed(&self) {
        self.inner.connections.fetch_sub(1, Ordering::SeqCst);
    }

    /// The rules' decision about `host` without asking anyone: `None` where
    /// [`Gateway::check`] would ask, so the caller may still find the host
    /// allowed another way first.
    pub(crate) fn check_quietly(&self, host: &Host) -> Option<Result<Reason, Reason>> {
        self.lock().evaluate(host, Instant::now())
    }

    /// Decides about a connection to `host`. Unknown hosts become a question
    /// if the project asks and someone watches; concurrent requests to the
    /// same host share one question.
    pub(crate) fn check(&self, host: &Host, port: u16) -> Verdict {
        let mut state = self.lock();
        match state.evaluate(host, Instant::now()) {
            Some(Ok(reason)) => Verdict::Allow(reason),
            Some(Err(reason)) => Verdict::Deny(reason),
            None => {
                if let Some(pending) = state.pending.iter().find(|p| p.host == *host) {
                    return Verdict::Ask(pending.clone());
                }
                let pending = Arc::new(Pending {
                    id: state.next_id,
                    host: host.clone(),
                    port,
                    outcome: Mutex::new(None),
                    decided: Condvar::new(),
                });
                state.next_id += 1;
                state.pending.push(pending.clone());
                state.broadcast(&Event::Pending {
                    id: pending.id,
                    host: host.clone(),
                    port,
                });
                Verdict::Ask(pending)
            }
        }
    }

    /// Blocks until the question is answered or times out.
    pub(crate) fn await_decision(&self, pending: &Pending) -> (bool, Reason) {
        let outcome = pending
            .outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let (outcome, _) = pending
            .decided
            .wait_timeout_while(outcome, self.inner.options.ask_timeout, |o| o.is_none())
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(decided) = *outcome {
            return decided;
        }
        drop(outcome);
        self.lock().resolve(pending, false, Reason::Unanswered);
        pending
            .outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .unwrap_or((false, Reason::Unanswered))
    }

    /// Where `llm.harness` forwards to and how, if the host has set it.
    pub(crate) fn llm(&self) -> Option<(LlmUpstream, Connector)> {
        self.lock().llm.clone()
    }

    /// The project the host named, see [`Request::Project`].
    pub(crate) fn project(&self) -> Option<String> {
        self.lock().project.clone()
    }

    /// Records a connection attempt and tells the watchers.
    pub(crate) fn record(&self, allowed: bool, host: &Host, port: u16, reason: Reason) {
        let entry = LogEntry {
            time_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64),
            allowed,
            host: host.clone(),
            port,
            reason,
        };
        if self.inner.options.print_log {
            println!("{entry}");
        }
        let mut state = self.lock();
        if state.log.len() >= self.inner.options.log_capacity {
            state.log.pop_front();
        }
        state.log.push_back(entry.clone());
        state.broadcast(&Event::Log(entry));
    }

    /// Serves one control connection until it says `bye`, closes, or stays
    /// silent past the reader's timeout. Answers and events go through one
    /// writer thread, so they never interleave within a line.
    pub fn serve_control<R, W>(&self, mut reader: R, writer: W)
    where
        R: BufRead,
        W: Write + Send + 'static,
    {
        let (tx, rx) = mpsc::sync_channel::<String>(WATCH_QUEUE);
        let writer = thread::spawn(move || {
            let mut writer = writer;
            for line in rx {
                if writer
                    .write_all(format!("{line}\n").as_bytes())
                    .and_then(|()| writer.flush())
                    .is_err()
                {
                    break;
                }
            }
        });

        let mut watcher = None;
        let mut line = String::new();
        loop {
            line.clear();
            match reader
                .by_ref()
                .take(MAX_LINE as u64 + 1)
                .read_line(&mut line)
            {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            if !line.ends_with('\n') {
                // Over-long line or EOF mid-line: the peer is not speaking
                // the protocol.
                let _ = tx.send("error Zeile zu lang oder unvollständig".to_owned());
                break;
            }
            let text = line.trim_end_matches(['\r', '\n']);
            if text.is_empty() {
                continue;
            }
            let replies = match text.parse::<Request>() {
                Ok(Request::Bye) => {
                    let _ = tx.send("ok".to_owned());
                    break;
                }
                Ok(request) => self.handle(request, &tx, &mut watcher),
                Err(message) => vec![format!("error {message}")],
            };
            if replies.into_iter().any(|reply| tx.send(reply).is_err()) {
                break;
            }
        }
        if let Some(id) = watcher {
            self.lock().remove_watcher(id);
        }
        drop(tx);
        let _ = writer.join();
    }

    /// Executes a request and returns the answer lines.
    fn handle(
        &self,
        request: Request,
        tx: &SyncSender<String>,
        watcher: &mut Option<u64>,
    ) -> Vec<String> {
        let mut state = self.lock();
        let now = Instant::now();
        let result = match request {
            Request::Policy(policy) => {
                state.policy = policy;
                Ok(Vec::new())
            }
            Request::Allow {
                scope: Scope::Once, ..
            } => Err("once gibt es nur bei decide".to_owned()),
            Request::Allow { pattern, scope } => state.allow(pattern, scope, now),
            Request::Deny(pattern) => {
                state.deny(pattern);
                Ok(Vec::new())
            }
            Request::Revoke(pattern) => {
                state.grants.retain(|grant| grant.pattern != pattern);
                Ok(Vec::new())
            }
            Request::Decide { id, allow } => state.decide(id, allow, now),
            Request::Watch => {
                if watcher.is_none() {
                    let id = state.next_watcher;
                    state.next_watcher += 1;
                    state.watchers.push(Watcher { id, tx: tx.clone() });
                    *watcher = Some(id);
                }
                // Questions asked before this watcher connected.
                Ok(state
                    .pending
                    .iter()
                    .map(|p| {
                        Event::Pending {
                            id: p.id,
                            host: p.host.clone(),
                            port: p.port,
                        }
                        .to_string()
                    })
                    .collect())
            }
            Request::Status => Ok(state.status(now).to_lines()),
            Request::Llm(None) => {
                state.llm = None;
                Ok(Vec::new())
            }
            // A refused upstream also removes the previous one: requests
            // must not keep going to a server the host has moved away from.
            Request::Llm(Some(upstream)) => {
                let connector = Connector::for_upstream(&upstream);
                state.llm = None;
                connector.map(|connector| {
                    state.llm = Some((upstream, connector));
                    Vec::new()
                })
            }
            Request::Project(name) => {
                state.project = Some(name);
                Ok(Vec::new())
            }
            Request::Ping | Request::Bye => Ok(Vec::new()),
        };
        // Rules changed: questions they now answer are answered.
        state.reevaluate_pending(now);
        match result {
            Ok(mut lines) => {
                // Events of `watch` follow its `ok`; status data precedes it.
                if lines.first().is_some_and(|l| l.starts_with("event ")) {
                    lines.insert(0, "ok".to_owned());
                } else {
                    lines.push("ok".to_owned());
                }
                lines
            }
            Err(message) => vec![format!("error {message}")],
        }
    }
}

impl State {
    /// `Some(Ok)` allows, `Some(Err)` blocks, `None` means ask the user.
    fn evaluate(&self, host: &Host, now: Instant) -> Option<Result<Reason, Reason>> {
        let name = host.name();
        if let Some(name) = name {
            if self.policy.deny.iter().any(|p| p.matches(name)) {
                return Some(Err(Reason::DenyList));
            }
            if self.denied.iter().any(|p| p.matches(name)) {
                return Some(Err(Reason::DeniedByUser));
            }
        }
        if self.policy.mode == Mode::Open {
            return Some(Ok(Reason::Open));
        }
        let Some(name) = name else {
            return Some(Err(Reason::IpLiteral));
        };
        if self.policy.allow.iter().any(|p| p.matches(name)) {
            return Some(Ok(Reason::Allowlist));
        }
        let granted = self
            .grants
            .iter()
            .any(|g| g.until.is_none_or(|until| until > now) && g.pattern.matches(name));
        if granted {
            return Some(Ok(Reason::Grant));
        }
        if self.policy.ask && !self.watchers.is_empty() {
            None
        } else {
            Some(Err(Reason::NotAllowed))
        }
    }

    fn allow(
        &mut self,
        pattern: DomainPattern,
        scope: Scope,
        now: Instant,
    ) -> Result<Vec<String>, String> {
        // Deny wins at evaluation anyway; this only tells the user early.
        let blocked = if pattern.is_wildcard() {
            self.policy.deny.contains(&pattern)
        } else {
            let host = pattern.to_string();
            self.policy.deny.iter().any(|p| p.matches(&host))
        };
        if blocked {
            return Err(format!(
                "{pattern} steht in der deny-Liste des Projekts und bleibt gesperrt"
            ));
        }
        self.denied.retain(|p| *p != pattern);
        self.grants.retain(|g| g.pattern != pattern);
        let until = match scope {
            Scope::For(seconds) => Some(now + Duration::from_secs(seconds)),
            Scope::Session | Scope::Once => None,
        };
        self.grants.push(Grant { pattern, until });
        Ok(Vec::new())
    }

    fn deny(&mut self, pattern: DomainPattern) {
        self.grants.retain(|g| g.pattern != pattern);
        if !self.denied.contains(&pattern) {
            self.denied.push(pattern);
        }
    }

    fn decide(
        &mut self,
        id: u64,
        allow: Option<Scope>,
        now: Instant,
    ) -> Result<Vec<String>, String> {
        let pending = self
            .pending
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| format!("Anfrage {id} ist nicht (mehr) offen"))?;
        // Approvals and refusals beyond this one request are stored per host;
        // hosts that are no valid domain (single labels) only get `once`.
        let pattern = pending
            .host
            .name()
            .and_then(|name| name.parse::<DomainPattern>().ok());
        match allow {
            Some(scope) => {
                if let (Some(pattern), Scope::For(_) | Scope::Session) = (pattern, scope) {
                    self.allow(pattern, scope, now)?;
                }
                self.resolve(&pending, true, Reason::Approved);
            }
            None => {
                if let Some(pattern) = pattern {
                    self.deny(pattern);
                }
                self.resolve(&pending, false, Reason::DeniedByUser);
            }
        }
        Ok(Vec::new())
    }

    /// Answers a question once; later calls are no-ops.
    fn resolve(&mut self, pending: &Pending, allowed: bool, reason: Reason) {
        let mut outcome = pending
            .outcome
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if outcome.is_some() {
            return;
        }
        *outcome = Some((allowed, reason));
        pending.decided.notify_all();
        drop(outcome);
        self.pending.retain(|p| p.id != pending.id);
        self.broadcast(&Event::Resolved {
            id: pending.id,
            allowed,
            reason,
        });
    }

    fn reevaluate_pending(&mut self, now: Instant) {
        for pending in self.pending.clone() {
            match self.evaluate(&pending.host, now) {
                Some(Ok(reason)) => self.resolve(&pending, true, reason),
                Some(Err(reason)) => self.resolve(&pending, false, reason),
                None => {}
            }
        }
    }

    fn remove_watcher(&mut self, id: u64) {
        self.watchers.retain(|w| w.id != id);
        // Nobody left to answer: fail the waiting requests now instead of
        // after the timeout.
        if self.watchers.is_empty() {
            for pending in self.pending.clone() {
                self.resolve(&pending, false, Reason::Unanswered);
            }
        }
    }

    fn broadcast(&mut self, event: &Event) {
        let line = event.to_string();
        self.watchers.retain(|w| match w.tx.try_send(line.clone()) {
            Ok(()) => true,
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
        });
    }

    fn status(&mut self, now: Instant) -> Status {
        self.grants
            .retain(|g| g.until.is_none_or(|until| until > now));
        Status {
            policy: self.policy.clone(),
            grants: self
                .grants
                .iter()
                .map(|g| GrantInfo {
                    pattern: g.pattern.clone(),
                    remaining_secs: g.until.map(|until| (until - now).as_secs().max(1)),
                })
                .collect(),
            denied: self.denied.clone(),
            pending: self
                .pending
                .iter()
                .map(|p| (p.id, p.host.clone(), p.port))
                .collect(),
            watchers: self.watchers.len(),
            llm: self.llm.as_ref().map(|(upstream, _)| upstream.url()),
            log: self.log.iter().cloned().collect(),
        }
    }
}

#[cfg(test)]
mod tests;
