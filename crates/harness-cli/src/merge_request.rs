//! Merges waiting for the user's approval in the terminal: when the check
//! finds something severe while a web chat takes over the agent's work, the
//! chat shows an id, and only `harness merge --approve <id>` on this
//! computer lets the merge through. The request pins the checked commit, so
//! the agent cannot slip in more after the check.

use std::collections::hash_map::RandomState;
use std::fs;
use std::hash::{BuildHasher as _, Hasher as _};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context as _;
use harness_core::config::ConfigDir;
use harness_core::project::ProjectName;
use harness_core::tr;
use serde::{Deserialize, Serialize};

/// Without 0/O and 1/I, so an id read off the screen cannot be mistyped.
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const ID_LENGTH: usize = 6;

/// A merge waiting for approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeRequest {
    /// The id the chat shows.
    pub id: String,
    /// Project name.
    pub project: String,
    /// Agent branch on the host.
    pub branch: String,
    /// Commit of the agent branch that was checked.
    pub commit: String,
    /// When it was asked for, Unix seconds.
    pub created: u64,
}

fn dir(config: &ConfigDir) -> PathBuf {
    config.root().join("merge-requests")
}

/// Whether `id` can be a request id; anything else never reaches the file
/// system. Also the ids of program file requests (`app_request`).
pub(crate) fn valid(id: &str) -> bool {
    id.len() == ID_LENGTH && id.bytes().all(|b| ALPHABET.contains(&b))
}

/// A new short id, as the chat shows it.
pub(crate) fn new_id() -> String {
    // Not a secret: approving needs this computer's terminal anyway. The id
    // only has to be short and unlikely to repeat.
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    hasher.write_u32(std::process::id());
    let mut bits = hasher.finish();
    (0..ID_LENGTH)
        .map(|_| {
            let c = ALPHABET[(bits % ALPHABET.len() as u64) as usize] as char;
            bits /= ALPHABET.len() as u64;
            c
        })
        .collect()
}

/// Stores a request for `branch` at `commit` and returns its id. An older
/// request of the same project is replaced: only the newest state can be
/// approved. Its errors end up in a chat's takeover report, so they stay
/// German.
pub fn create(
    config: &ConfigDir,
    project: &ProjectName,
    branch: &str,
    commit: &str,
) -> anyhow::Result<String> {
    let dir = dir(config);
    fs::create_dir_all(&dir).with_context(|| format!("{} anlegen", dir.display()))?;
    for old in list(config) {
        if old.project == project.as_str() {
            remove(config, &old.id);
        }
    }
    let request = MergeRequest {
        id: new_id(),
        project: project.to_string(),
        branch: branch.to_owned(),
        commit: commit.to_owned(),
        created: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    };
    let file = dir.join(format!("{}.json", request.id));
    fs::write(&file, serde_json::to_vec_pretty(&request)?)
        .with_context(|| format!("{} schreiben", file.display()))?;
    Ok(request.id)
}

/// The request with this id, if there is one. Case does not matter.
pub fn load(config: &ConfigDir, id: &str) -> anyhow::Result<Option<MergeRequest>> {
    let id = id.trim().to_uppercase();
    if !valid(&id) {
        return Ok(None);
    }
    let file = dir(config).join(format!("{id}.json"));
    match fs::read(&file) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).with_context(|| {
            tr!("{} ist beschädigt", "{} is damaged", file.display())
        })?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| tr!("{} lesen", "Reading {}", file.display())),
    }
}

/// All stored requests; unreadable files are skipped.
pub fn list(config: &ConfigDir) -> Vec<MergeRequest> {
    let Ok(entries) = fs::read_dir(dir(config)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| fs::read(entry.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<MergeRequest>(&bytes).ok())
        .filter(|request| valid(&request.id))
        .collect()
}

/// Removes the request, if it exists.
pub fn remove(config: &ConfigDir, id: &str) {
    if valid(id) {
        let _ = fs::remove_file(dir(config).join(format!("{id}.json")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_stored_replaced_and_removed() {
        let temp = tempfile::tempdir().unwrap();
        let config = ConfigDir::new(temp.path().to_path_buf());
        let demo = ProjectName::new("demo").unwrap();
        let first = create(&config, &demo, "agent/demo", "aaa").unwrap();
        assert!(valid(&first));
        let loaded = load(&config, &first.to_lowercase()).unwrap().unwrap();
        assert_eq!(loaded.commit, "aaa");
        // The newer request of the same project replaces the older one.
        let second = create(&config, &demo, "agent/demo", "bbb").unwrap();
        assert!(load(&config, &first).unwrap().is_none() || first == second);
        assert_eq!(list(&config).len(), 1);
        remove(&config, &second);
        assert!(load(&config, &second).unwrap().is_none());
    }

    #[test]
    fn ids_never_become_paths() {
        let temp = tempfile::tempdir().unwrap();
        let config = ConfigDir::new(temp.path().to_path_buf());
        for id in ["../../x", "ABC", "ABCDE0", "ABCDEFG", ""] {
            assert!(load(&config, id).unwrap().is_none(), "{id}");
        }
    }
}
