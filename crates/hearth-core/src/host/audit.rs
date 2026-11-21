//! The record of the direct mode: every tool call with what was decided and
//! by whom, one JSON line each in `logs/host-<project>.jsonl` of the
//! configuration folder. File contents never go in, only paths and commands.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The record grows to this size, then the older half moves aside.
const MAX_LOG: u64 = 5 * 1024 * 1024;

/// One entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Milliseconds since 1970.
    pub at: u64,
    /// The chat (conversation id).
    pub chat: String,
    /// The tool.
    pub tool: String,
    /// The path or command, as it was checked.
    pub target: String,
    /// `free`, `once`, `chat`, `deny`, `refused` or `expired`.
    pub decision: String,
    /// Who decided: `rules`, `app`, `terminal`, `browser` or `timeout`.
    pub by: String,
    /// Exit code of a command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<i32>,
    /// Whether the call failed.
    #[serde(default)]
    pub error: bool,
}

impl Entry {
    /// An entry stamped now.
    pub fn now(chat: &str, tool: &str, target: &str, decision: &str, by: &str) -> Entry {
        Entry {
            at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |time| u64::try_from(time.as_millis()).unwrap_or(0)),
            chat: chat.to_owned(),
            tool: tool.to_owned(),
            target: target.chars().take(2_000).collect(),
            decision: decision.to_owned(),
            by: by.to_owned(),
            exit: None,
            error: false,
        }
    }
}

/// Where the record of `project` lives.
pub fn path(config_root: &Path, project: &str) -> PathBuf {
    config_root
        .join("logs")
        .join(format!("host-{project}.jsonl"))
}

/// Appends `entry`; a record that cannot be written is not worth failing
/// the call for, so errors only go to the caller's log.
///
/// # Errors
///
/// When the folder or file cannot be written.
pub fn record(config_root: &Path, project: &str, entry: &Entry) -> std::io::Result<()> {
    let file = path(config_root, project);
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    if fs::metadata(&file).is_ok_and(|meta| meta.len() > MAX_LOG) {
        let _ = fs::rename(&file, file.with_extension("jsonl.old"));
    }
    let mut line = serde_json::to_string(entry).map_err(std::io::Error::other)?;
    line.push('\n');
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)?
        .write_all(line.as_bytes())
}

/// The last `count` entries of `project`, newest last.
pub fn recent(config_root: &Path, project: &str, count: usize) -> Vec<Entry> {
    let Ok(text) = fs::read_to_string(path(config_root, project)) else {
        return Vec::new();
    };
    let entries: Vec<Entry> = text
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let skip = entries.len().saturating_sub(count);
    entries.into_iter().skip(skip).collect()
}
