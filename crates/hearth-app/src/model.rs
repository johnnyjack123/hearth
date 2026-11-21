//! What the window shows, read from the configuration folder: the projects,
//! the background service's status file, the questions of the direct mode,
//! held merges and requested program files. Everything here is files the
//! CLI and the service write; the app reads them about once a second.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hearth_core::config::{ConfigDir, HostConfig};
use hearth_core::host::inbox;
use serde::Deserialize;

/// One project as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRow {
    /// Name.
    pub name: String,
    /// Folder on this computer.
    pub path: PathBuf,
    /// Offered to web chats.
    pub remote: bool,
    /// Web chats may take the sandbox's work over without an approval here.
    pub merge: bool,
    /// The direct mode.
    pub host: HostConfig,
    /// Network preset of the sandbox.
    pub network: String,
    /// Apps of the sandbox.
    pub apps: Vec<String>,
    /// Extra allowed domains of the sandbox's network.
    pub allow: Vec<String>,
    /// Blocked domains.
    pub deny: Vec<String>,
}

/// A program of the app catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogApp {
    /// Name for `hearth app on`.
    pub name: String,
    /// Name for people.
    pub title: String,
    /// What it brings.
    pub description: Option<String>,
}

/// The background service's status file (`daemon-status.json`), as far
/// as the window needs it; unknown or missing fields are fine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct ServiceStatus {
    /// `connecting`, `connected`, `offline` or `unpaired`.
    pub connection: String,
    /// The server's address.
    pub server: String,
    /// Why the connection is down.
    pub error: Option<String>,
    /// `running`, `stopped`, `starting`, `stopping` or `unknown`.
    pub vm: String,
    /// Agent replies on their way.
    pub replies: u32,
    /// hearth commands in a terminal.
    pub terminals: u32,
    /// This hearth's version.
    pub version: String,
    /// The server's version.
    #[serde(rename = "serverVersion")]
    pub server_version: Option<String>,
}

/// A merge a web chat held for findings (`merge-requests/<ID>.json`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MergeRequest {
    /// Id to approve with.
    pub id: String,
    /// The project.
    pub project: String,
    /// When it was asked for, Unix seconds.
    #[serde(default)]
    pub created: u64,
}

/// A program file the browser asked for (`app-requests/<ID>.json`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FileRequest {
    /// Id to approve with.
    pub id: String,
    /// Name for people.
    pub title: String,
    /// The file.
    pub path: String,
    /// What it is.
    #[serde(default)]
    pub kind: String,
    /// Its size, in bytes.
    #[serde(default)]
    pub size: u64,
}

/// Everything the window shows.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// The projects, by name.
    pub projects: Vec<ProjectRow>,
    /// Projects whose file could not be read, with the reason.
    pub broken: Vec<(String, String)>,
    /// The service's status, if it runs (its file exists).
    pub service: Option<ServiceStatus>,
    /// The server this computer is paired with, from `config.toml`.
    pub server: Option<String>,
    /// Questions of the direct mode.
    pub approvals: Vec<inbox::Pending>,
    /// Held merges.
    pub merges: Vec<MergeRequest>,
    /// Requested program files.
    pub files: Vec<FileRequest>,
    /// Programs a sandbox project can switch on.
    pub catalog: Vec<CatalogApp>,
}

impl Snapshot {
    /// What waits for the user, for the badge in the navigation.
    pub fn waiting(&self) -> usize {
        self.approvals.len() + self.merges.len() + self.files.len()
    }
}

fn read_all<T: for<'de> Deserialize<'de>>(dir: &Path) -> Vec<T> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| fs::read_to_string(entry.path()).ok())
        .filter_map(|text| serde_json::from_str(&text).ok())
        .collect()
}

/// Reads everything from the configuration folder.
pub fn load(config: &ConfigDir) -> Snapshot {
    let root = config.root();
    let mut snapshot = Snapshot {
        server: config.global().ok().and_then(|global| global.server.url),
        service: fs::read_to_string(root.join("daemon-status.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok()),
        approvals: inbox::read_pending(root),
        merges: read_all(&root.join("merge-requests")),
        files: read_all(&root.join("app-requests")),
        ..Snapshot::default()
    };
    snapshot.merges.sort_by_key(|request| request.created);
    for name in config.project_names().unwrap_or_default() {
        match config.project_config(&name) {
            Ok(raw) => snapshot.projects.push(ProjectRow {
                name: name.to_string(),
                path: raw.path.clone(),
                remote: raw.remote.enabled,
                merge: raw.remote.merge,
                host: raw.host.clone(),
                // The effective preset: the project's own, else config.toml's.
                network: config
                    .project(&name)
                    .map(|project| project.network.to_string())
                    .unwrap_or_default(),
                apps: raw.apps.enabled.clone(),
                allow: raw.network.allow.iter().map(ToString::to_string).collect(),
                deny: raw.network.deny.iter().map(ToString::to_string).collect(),
            }),
            Err(error) => snapshot.broken.push((name.to_string(), error.to_string())),
        }
    }
    snapshot.projects.sort_by(|a, b| a.name.cmp(&b.name));
    snapshot.catalog = config
        .apps()
        .unwrap_or_default()
        .into_iter()
        .map(|app| CatalogApp {
            name: app.name,
            title: app.title,
            description: app.description,
        })
        .collect();
    snapshot
}

/// Reloads the snapshot at most once per `interval`.
pub struct Refresher {
    interval: Duration,
    last: Option<Instant>,
}

impl Refresher {
    /// Reloads every `interval`.
    pub fn new(interval: Duration) -> Self {
        Refresher {
            interval,
            last: None,
        }
    }

    /// Whether it is time, and notes that it reloads now.
    pub fn due(&mut self) -> bool {
        if self.last.is_some_and(|last| last.elapsed() < self.interval) {
            return false;
        }
        self.last = Some(Instant::now());
        true
    }

    /// Reloads at the next frame.
    pub fn soon(&mut self) {
        self.last = None;
    }
}

/// The last `count` lines of a text file.
pub fn tail(path: &Path, count: usize) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(count)..]
        .iter()
        .map(|line| (*line).to_owned())
        .collect()
}
