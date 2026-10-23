//! Stops the VM when nothing has used it for the idle time chosen in the
//! web app (ROADMAP Phase 3). Runs beside the server connection of the
//! background service and of `harness connect`, also while the server is
//! unreachable.
//!
//! Idle means: no agent reply on its way, no harness command in a terminal
//! (`harness_core::busy`), and, checked right before stopping, no running
//! container that harness did not create (someone uses the VM by hand).
//! Agents waiting for the next message do not count: their sessions end
//! with the VM, and the next message starts both again.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use harness_core::busy::{self, Marker};
use harness_core::podman::PodmanProvider;
use harness_core::tunnel::{
    DEFAULT_IDLE_MINUTES, DeviceMessage, MAX_IDLE_MINUTES, VERSION_VM_CONTROL, VmActivity, VmStatus,
};
use harness_core::{MANAGED_LABEL, Provider as _};

use crate::connect::{Mode, say};

/// How often the watcher looks at the VM (the VM service answers fast).
const CHECK: Duration = Duration::from_secs(15);

/// What the watcher saw last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    /// The VM.
    pub vm: VmStatus,
    /// Agent replies on their way.
    pub replies: u32,
    /// harness commands in a terminal.
    pub terminals: u32,
    /// When the idle VM will be stopped, if it will.
    pub stop_at: Option<Instant>,
}

impl Report {
    /// The status message for a connection that speaks `version`.
    pub fn message(&self, version: u32) -> DeviceMessage {
        if version < VERSION_VM_CONTROL {
            // Protocol 1 knows no "stopping".
            let vm = match self.vm {
                VmStatus::Stopping => VmStatus::Running,
                vm => vm,
            };
            return DeviceMessage::Status { vm, activity: None };
        }
        let now = Instant::now();
        DeviceMessage::Status {
            vm: self.vm,
            activity: Some(VmActivity {
                replies: self.replies,
                terminals: self.terminals,
                stop_in: self
                    .stop_at
                    .map(|at| at.saturating_duration_since(now).as_secs()),
            }),
        }
    }
}

struct Usage {
    last: Instant,
    replies: HashSet<String>,
    /// Held while replies run, so watchers of other configurations
    /// (`HARNESS_HOME`) see that the VM is in use.
    marker: Option<Marker>,
}

impl Usage {
    fn mark(&mut self, machine: &str) {
        if self.replies.is_empty() {
            self.marker = None;
        } else if self.marker.is_none() {
            self.marker = Marker::acquire(machine, "harness daemon (Chat)").ok();
        }
    }
}

enum Signal {
    Wake,
    Stop,
}

/// Activity and idle time, shared by the connection, its sessions and the
/// watcher.
pub struct Idle {
    machine: String,
    usage: Mutex<Usage>,
    minutes: AtomicU32,
    report: Mutex<Report>,
    signals: Mutex<Option<Sender<Signal>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Idle {
    /// Nothing used yet; the idle time is the default until the server sends
    /// the user's choice.
    pub fn new(machine: &str) -> Arc<Idle> {
        Arc::new(Idle {
            machine: machine.to_owned(),
            usage: Mutex::new(Usage {
                last: Instant::now(),
                replies: HashSet::new(),
                marker: None,
            }),
            minutes: AtomicU32::new(DEFAULT_IDLE_MINUTES),
            report: Mutex::new(Report {
                vm: VmStatus::Unknown,
                replies: 0,
                terminals: 0,
                stop_at: None,
            }),
            signals: Mutex::new(None),
        })
    }

    /// Something happened (a message, an agent event).
    pub fn touch(&self) {
        lock(&self.usage).last = Instant::now();
    }

    /// A reply of `session` is on its way; the VM counts as used until it
    /// ends.
    pub fn begin(&self, session: &str) {
        let mut usage = lock(&self.usage);
        usage.last = Instant::now();
        if usage.replies.insert(session.to_owned()) {
            usage.mark(&self.machine);
            drop(usage);
            self.wake();
        }
    }

    /// The reply of `session` ended.
    pub fn end(&self, session: &str) {
        let mut usage = lock(&self.usage);
        usage.last = Instant::now();
        if usage.replies.remove(session) {
            usage.mark(&self.machine);
            drop(usage);
            self.wake();
        }
    }

    /// Sessions with a reply on its way.
    pub fn answering(&self) -> Vec<String> {
        lock(&self.usage).replies.iter().cloned().collect()
    }

    /// Ends all replies, e.g. with the connection.
    pub fn end_all(&self) {
        let mut usage = lock(&self.usage);
        usage.last = Instant::now();
        usage.replies.clear();
        usage.mark(&self.machine);
    }

    /// The idle time chosen in the web app; 0 means never.
    pub fn set_minutes(&self, minutes: u32) {
        self.minutes
            .store(minutes.min(MAX_IDLE_MINUTES), Ordering::SeqCst);
        // So the browser sees the new stop time right away.
        self.wake();
    }

    /// What the watcher saw last.
    pub fn report(&self) -> Report {
        *lock(&self.report)
    }

    /// Shows `vm` until the watcher looks again, e.g. during a stop.
    pub fn show(&self, vm: VmStatus) {
        let mut report = lock(&self.report);
        report.vm = vm;
        report.stop_at = None;
    }

    /// Asks the watcher to look now.
    pub fn wake(&self) {
        if let Some(signals) = lock(&self.signals).as_ref() {
            let _ = signals.send(Signal::Wake);
        }
    }

    fn usage(&self) -> (u32, Instant) {
        let usage = lock(&self.usage);
        (
            u32::try_from(usage.replies.len()).unwrap_or(u32::MAX),
            usage.last,
        )
    }
}

/// The watcher thread; stops when dropped.
pub struct Watcher {
    signals: Sender<Signal>,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    /// Starts watching the VM of `machine`.
    pub fn spawn(idle: &Arc<Idle>, machine: &str, mode: Mode) -> Watcher {
        let (signals, receiver) = mpsc::channel();
        *lock(&idle.signals) = Some(signals.clone());
        let thread = {
            let idle = Arc::clone(idle);
            let machine = machine.to_owned();
            thread::spawn(move || watch(&idle, &machine, mode, &receiver))
        };
        Watcher {
            signals,
            thread: Some(thread),
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.signals.send(Signal::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn watch(idle: &Idle, machine: &str, mode: Mode, signals: &Receiver<Signal>) {
    let mut was_running = false;
    loop {
        let vm = crate::vm::status();
        let terminals = busy::users(machine);
        let running = vm == VmStatus::Running;
        // A VM that just came up counts as used from now on, also one
        // started by hand.
        if (running && !was_running) || !terminals.is_empty() {
            idle.touch();
        }
        was_running = running;
        let (replies, last) = idle.usage();
        let minutes = idle.minutes.load(Ordering::SeqCst);
        let stop_at = (running && replies == 0 && terminals.is_empty() && minutes > 0)
            .then(|| last + Duration::from_secs(u64::from(minutes) * 60));
        *lock(&idle.report) = Report {
            vm,
            replies,
            terminals: u32::try_from(terminals.len()).unwrap_or(u32::MAX),
            stop_at,
        };
        if stop_at.is_some_and(|at| Instant::now() >= at) {
            stop_idle(idle, machine, mode, minutes);
            continue;
        }
        match signals.recv_timeout(CHECK) {
            Ok(Signal::Wake) | Err(RecvTimeoutError::Timeout) => {}
            Ok(Signal::Stop) | Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

fn stop_idle(idle: &Idle, machine: &str, mode: Mode, minutes: u32) {
    match foreign_containers(machine) {
        Ok(names) if !names.is_empty() => {
            say(
                mode,
                &format!(
                    "VM bleibt an: Es laufen Container, die harness nicht angelegt hat ({}).",
                    names.join(", ")
                ),
            );
            idle.touch();
            return;
        }
        Ok(_) => {}
        Err(error) => {
            say(
                mode,
                &format!("VM bleibt an: laufende Container nicht lesbar ({error:#})."),
            );
            idle.touch();
            return;
        }
    }
    say(
        mode,
        &format!("VM seit {minutes} min unbenutzt, wird gestoppt."),
    );
    idle.show(VmStatus::Stopping);
    match crate::vm::stop() {
        Ok(message) => say(mode, &format!("VM gestoppt: {message}")),
        Err(error) => {
            say(mode, &format!("VM-Stopp gescheitert: {error:#}"));
            // Next try after another idle period, not every few seconds.
            idle.touch();
        }
    }
}

/// Running containers without harness's label, by name.
fn foreign_containers(machine: &str) -> anyhow::Result<Vec<String>> {
    let provider = PodmanProvider::for_machine(machine)?;
    Ok(provider
        .running_containers()?
        .into_iter()
        .filter(|c| !c.labels.contains_key(MANAGED_LABEL))
        .map(|c| harness_core::sync::sanitize(&c.name))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_keep_the_vm_busy_until_they_end() {
        let idle = Idle::new("harness-test-idle");
        idle.begin("a");
        idle.begin("b");
        assert_eq!(idle.usage().0, 2);
        // Other harness processes see the replies through the marker; this
        // one does not count itself.
        assert!(lock(&idle.usage).marker.is_some());
        assert!(busy::users("harness-test-idle").is_empty());
        let mut answering = idle.answering();
        answering.sort();
        assert_eq!(answering, ["a", "b"]);
        idle.end("a");
        assert_eq!(idle.usage().0, 1);
        idle.end_all();
        assert_eq!(idle.usage().0, 0);
        assert!(lock(&idle.usage).marker.is_none());
        if let Ok(dir) = busy::dir("harness-test-idle") {
            let _ = std::fs::remove_dir(dir);
        }
    }

    #[test]
    fn idle_time_is_capped() {
        let idle = Idle::new("harness-test-idle");
        idle.set_minutes(10_000_000);
        assert_eq!(idle.minutes.load(Ordering::SeqCst), MAX_IDLE_MINUTES);
    }

    #[test]
    fn status_follows_the_protocol_version() {
        let report = Report {
            vm: VmStatus::Stopping,
            replies: 0,
            terminals: 1,
            stop_at: Some(Instant::now() + Duration::from_secs(90)),
        };
        assert_eq!(
            report.message(1),
            DeviceMessage::Status {
                vm: VmStatus::Running,
                activity: None
            }
        );
        let DeviceMessage::Status {
            vm,
            activity: Some(activity),
        } = report.message(2)
        else {
            panic!("no activity in protocol 2")
        };
        assert_eq!(vm, VmStatus::Stopping);
        assert_eq!(activity.terminals, 1);
        assert!(matches!(activity.stop_in, Some(88..=90)));
    }
}
