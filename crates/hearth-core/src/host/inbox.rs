//! How the questions of the direct mode reach the user on this computer and
//! the answers come back, without a socket: the background service lists
//! what waits in `host-approvals.json` of the configuration folder, and
//! whoever answers (desktop app, tray icon, `hearth host approve`) leaves a
//! file in `host-decisions/`, which the service takes.
//!
//! The "for this chat" answers of every chat are listed in
//! `host-grants.json`; taking one back leaves a file in `host-revoke/`.
//!
//! Any process of the user could write there, like into any other channel
//! of the user's; the model cannot, because no tool may write into hearth's
//! folders. A command the user approved can, and that is the limit every
//! approval has (`docs/security/concept.md` §2).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Assessment, Decision, Grant, Request};

/// The list of waiting questions.
pub const PENDING_FILE: &str = "host-approvals.json";
/// The folder answers go to.
pub const DECISIONS_DIR: &str = "host-decisions";
/// The "for this chat" answers of the chats.
pub const GRANTS_FILE: &str = "host-grants.json";
/// The folder requests to take a "for this chat" answer back go to.
pub const REVOKE_DIR: &str = "host-revoke";

/// A waiting question, as the surfaces on this computer show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    /// Id to answer with (six characters, no 0/O/1/I).
    pub id: String,
    /// The project.
    pub project: String,
    /// The chat (conversation id).
    pub chat: String,
    /// The chat's title, if it has one.
    #[serde(default)]
    pub title: Option<String>,
    /// What is asked.
    pub request: Request,
    /// One line, in the language of this computer.
    pub summary: String,
    /// Why it asks.
    pub why: String,
    /// Whether "for this chat" is offered.
    #[serde(rename = "chatWide")]
    pub chat_wide: bool,
    /// Whether the chat may answer as well.
    pub browser: bool,
    /// When it was asked, Unix seconds.
    pub since: u64,
    /// Jev's rating of a command, if the server sent one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assessment: Option<Assessment>,
}

/// The "for this chat" answers of one chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatGrants {
    /// The project.
    pub project: String,
    /// The chat (conversation id).
    pub chat: String,
    /// The chat's title, if it has one.
    #[serde(default)]
    pub title: Option<String>,
    /// What the user allowed, oldest first.
    pub grants: Vec<Grant>,
}

/// An answer left for the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Answer {
    decision: Decision,
}

/// Whether `id` has the form of a question id; anything else is never a
/// file name here.
pub fn valid_id(id: &str) -> bool {
    id.len() == 6
        && id
            .bytes()
            .all(|b| matches!(b, b'A'..=b'H' | b'J'..=b'N' | b'P'..=b'Z' | b'2'..=b'9'))
}

/// A fresh question id.
pub fn new_id() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut bytes = [0u8; 6];
    if getrandom::fill(&mut bytes).is_err() {
        // Without the system's randomness the time is good enough: the id
        // only tells waiting questions apart.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.as_nanos());
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = (nanos >> (index * 8)) as u8;
        }
    }
    bytes
        .iter()
        .map(|byte| ALPHABET[usize::from(*byte) % ALPHABET.len()] as char)
        .collect()
}

/// Writes the list of waiting questions (the service does).
///
/// # Errors
///
/// When the file cannot be written.
pub fn write_pending(config_root: &Path, pending: &[Pending]) -> io::Result<()> {
    let path = config_root.join(PENDING_FILE);
    if pending.is_empty() {
        return match fs::remove_file(&path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    }
    let text = serde_json::to_string_pretty(pending).map_err(io::Error::other)?;
    write_atomically(&path, text.as_bytes())
}

/// The waiting questions; none when the file is missing or unreadable.
pub fn read_pending(config_root: &Path) -> Vec<Pending> {
    fs::read_to_string(config_root.join(PENDING_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn decision_path(config_root: &Path, id: &str) -> PathBuf {
    config_root.join(DECISIONS_DIR).join(format!("{id}.json"))
}

/// Leaves an answer for the service.
///
/// # Errors
///
/// An invalid id, or the file cannot be written.
pub fn submit(config_root: &Path, id: &str, decision: Decision) -> io::Result<()> {
    if !valid_id(id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid approval id",
        ));
    }
    let path = decision_path(config_root, id);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string(&Answer { decision }).map_err(io::Error::other)?;
    write_atomically(&path, text.as_bytes())
}

/// Takes the answer to `id` if one was left; it is removed.
pub fn take(config_root: &Path, id: &str) -> Option<Decision> {
    if !valid_id(id) {
        return None;
    }
    let path = decision_path(config_root, id);
    let text = fs::read_to_string(&path).ok()?;
    let _ = fs::remove_file(&path);
    serde_json::from_str::<Answer>(&text)
        .ok()
        .map(|answer| answer.decision)
}

/// Writes the "for this chat" answers of all chats (the service does).
///
/// # Errors
///
/// When the file cannot be written.
pub fn write_grants(config_root: &Path, chats: &[ChatGrants]) -> io::Result<()> {
    let path = config_root.join(GRANTS_FILE);
    if chats.is_empty() {
        return match fs::remove_file(&path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    }
    let text = serde_json::to_string_pretty(chats).map_err(io::Error::other)?;
    write_atomically(&path, text.as_bytes())
}

/// The "for this chat" answers of all chats; none when the file is missing
/// or unreadable.
pub fn read_grants(config_root: &Path) -> Vec<ChatGrants> {
    fs::read_to_string(config_root.join(GRANTS_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Asks the service to take the "for this chat" answer `id` back.
///
/// # Errors
///
/// An invalid id, or the file cannot be written.
pub fn request_revoke(config_root: &Path, id: &str) -> io::Result<()> {
    if !valid_id(id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid grant id",
        ));
    }
    let dir = config_root.join(REVOKE_DIR);
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(id), b"")
}

/// Takes the requests to take answers back; they are removed.
pub fn take_revocations(config_root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(config_root.join(REVOKE_DIR)) else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let _ = fs::remove_file(entry.path());
        if valid_id(&name) {
            ids.push(name);
        }
    }
    ids
}

/// Removes answers nobody waits for any more (from a service that ended),
/// and the grants of its chats, which ended with it.
pub fn clear(config_root: &Path) {
    for dir in [DECISIONS_DIR, REVOKE_DIR] {
        if let Ok(entries) = fs::read_dir(config_root.join(dir)) {
            for entry in entries.flatten() {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let _ = fs::remove_file(config_root.join(PENDING_FILE));
    let _ = fs::remove_file(config_root.join(GRANTS_FILE));
}

/// Writes through a temporary file, so a reader never sees half of it.
fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_go_through_files() {
        let tmp = tempfile::tempdir().unwrap();
        let id = new_id();
        assert!(valid_id(&id), "{id}");
        assert_eq!(take(tmp.path(), &id), None);
        submit(tmp.path(), &id, Decision::Chat).unwrap();
        assert_eq!(take(tmp.path(), &id), Some(Decision::Chat));
        // Taken once.
        assert_eq!(take(tmp.path(), &id), None);
        assert!(submit(tmp.path(), "../x", Decision::Once).is_err());
        assert!(!valid_id("ABCDE0"));
    }

    #[test]
    fn the_list_of_waiting_questions_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let pending = Pending {
            id: "ABC234".to_owned(),
            project: "demo".to_owned(),
            chat: "chat-1".to_owned(),
            title: Some("Build".to_owned()),
            request: Request::Command {
                command: "cargo test".to_owned(),
                shell: "bash".to_owned(),
                cwd: "/x".to_owned(),
            },
            summary: "Run command: cargo test".to_owned(),
            why: "…".to_owned(),
            chat_wide: true,
            browser: false,
            since: 1,
            assessment: Assessment::from_wire(0.8, "jev"),
        };
        write_pending(tmp.path(), std::slice::from_ref(&pending)).unwrap();
        assert_eq!(read_pending(tmp.path()), vec![pending]);
        write_pending(tmp.path(), &[]).unwrap();
        assert!(read_pending(tmp.path()).is_empty());
    }

    #[test]
    fn grants_are_listed_and_taken_back_through_files() {
        let tmp = tempfile::tempdir().unwrap();
        let chats = vec![ChatGrants {
            project: "demo".to_owned(),
            chat: "chat-1".to_owned(),
            title: None,
            grants: vec![Grant {
                id: "ABC234".to_owned(),
                kind: super::super::GrantKind::Command,
                target: "cargo test".to_owned(),
            }],
        }];
        write_grants(tmp.path(), &chats).unwrap();
        assert_eq!(read_grants(tmp.path()), chats);
        request_revoke(tmp.path(), "ABC234").unwrap();
        assert!(request_revoke(tmp.path(), "../x").is_err());
        assert_eq!(take_revocations(tmp.path()), vec!["ABC234".to_owned()]);
        assert!(take_revocations(tmp.path()).is_empty());
        clear(tmp.path());
        assert!(read_grants(tmp.path()).is_empty());
    }
}
