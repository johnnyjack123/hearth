//! What the background service tells its tray icon (`harnessd`): a small
//! JSON file next to the log, rewritten whenever something changes. The
//! tray only reads it; what the tray asks for goes through named events
//! (`daemon::Instance`).

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use harness_core::config::ConfigDir;
use harness_core::tunnel::VmStatus;
use serde::Serialize;

/// File name in the configuration folder; `harnessd` reads the same.
pub const FILE: &str = "daemon-status.json";

/// The connection to the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Connection {
    /// Being built (or rebuilt after an interruption).
    Connecting,
    /// Up and greeted.
    Connected,
    /// Interrupted; a new attempt follows.
    Offline,
    /// The server no longer knows this device; `harness login` pairs again.
    Unpaired,
}

/// An open question of a project's gateway, so the tray can point at it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Question {
    /// Chat (conversation id) whose agent asked.
    pub session: String,
    /// Question id.
    pub id: u64,
    /// Host the agent wants to reach.
    pub host: String,
}

/// The service's state as the tray shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    /// Process id of the service; the tray ignores a file of another.
    pub pid: u32,
    /// The connection.
    pub connection: Connection,
    /// The server's address, which also serves the web app.
    pub server: String,
    /// Why the connection is down, if it is.
    pub error: Option<String>,
    /// The VM.
    pub vm: VmStatus,
    /// Agent replies on their way.
    pub replies: u32,
    /// harness commands in a terminal.
    pub terminals: u32,
    /// When the idle VM stops, Unix seconds.
    #[serde(rename = "stopAt")]
    pub stop_at: Option<u64>,
    /// Open questions of gateways.
    pub questions: Vec<Question>,
    /// This harness's version.
    pub version: String,
    /// The server's program version, once it said (protocol 10).
    #[serde(rename = "serverVersion")]
    pub server_version: Option<String>,
}

/// The status file and what was last written to it.
pub struct StatusFile {
    path: PathBuf,
    status: Mutex<Status>,
    written: Mutex<String>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The moment `duration` from now, as Unix seconds.
pub fn unix_in(duration: Duration) -> u64 {
    (SystemTime::now() + duration)
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl StatusFile {
    /// The file of `config`'s service, connecting to `server`.
    pub fn new(config: &ConfigDir, server: &str) -> StatusFile {
        let file = StatusFile {
            path: config.root().join(FILE),
            status: Mutex::new(Status {
                pid: std::process::id(),
                connection: Connection::Connecting,
                server: server.to_owned(),
                error: None,
                vm: VmStatus::Unknown,
                replies: 0,
                terminals: 0,
                stop_at: None,
                questions: Vec::new(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                server_version: None,
            }),
            written: Mutex::new(String::new()),
        };
        file.update(|_| {});
        file
    }

    /// Changes the status and writes the file if anything changed.
    pub fn update(&self, change: impl FnOnce(&mut Status)) {
        let mut status = lock(&self.status);
        change(&mut status);
        let Ok(text) = serde_json::to_string_pretty(&*status) else {
            return;
        };
        let mut written = lock(&self.written);
        if *written == text {
            return;
        }
        // Written aside and renamed, so the tray never reads half a file.
        let temporary = self.path.with_extension("json.tmp");
        if fs::write(&temporary, &text).is_ok() && fs::rename(&temporary, &self.path).is_ok() {
            *written = text;
        }
    }

    /// Notes a gateway question, or its end.
    pub fn question(&self, session: &str, id: u64, host: Option<&str>) {
        self.update(|status| {
            status
                .questions
                .retain(|q| !(q.session == session && q.id == id));
            if let Some(host) = host {
                status.questions.push(Question {
                    session: session.to_owned(),
                    id,
                    host: host.to_owned(),
                });
            }
        });
    }

    /// Forgets the questions of a session that ended.
    pub fn session_ended(&self, session: &str) {
        self.update(|status| status.questions.retain(|q| q.session != session));
    }
}

impl Drop for StatusFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_follows_every_change() {
        let temp = tempfile::tempdir().unwrap();
        let config = ConfigDir::new(temp.path().to_path_buf());
        let file = StatusFile::new(&config, "http://pi:8085");
        let read = || {
            serde_json::from_str::<serde_json::Value>(
                &fs::read_to_string(temp.path().join(FILE)).unwrap(),
            )
            .unwrap()
        };
        assert_eq!(read()["connection"], "connecting");
        file.update(|s| {
            s.connection = Connection::Connected;
            s.vm = VmStatus::Running;
        });
        file.question("chat-1", 3, Some("example.com"));
        let now = read();
        assert_eq!(now["connection"], "connected");
        assert_eq!(now["vm"], "running");
        assert_eq!(now["questions"][0]["host"], "example.com");
        file.question("chat-1", 3, None);
        assert_eq!(read()["questions"].as_array().unwrap().len(), 0);
        file.question("chat-2", 1, Some("a.example"));
        file.session_ended("chat-2");
        assert_eq!(read()["questions"].as_array().unwrap().len(), 0);
        drop(file);
        assert!(!temp.path().join(FILE).exists());
    }
}
