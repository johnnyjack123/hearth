//! Program files the browser asked for (app catalog, step 3b), waiting in
//! `app-requests/` for the user's approval on this computer: the tray icon
//! asks, or `hearth app approve <id>` in a terminal. Until then nothing of
//! the file leaves the computer.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context as _;
use hearth_core::app_file::FileInfo;
use hearth_core::config::ConfigDir;
use serde::{Deserialize, Serialize};

use crate::merge_request::{new_id, valid};

/// Requests waiting at most; the browser cannot pile up more.
pub const MAX_PENDING: usize = 10;

/// A program file waiting for approval. The tray icon reads these files
/// too (`hearthd/menu.rs`), without sharing this code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRequest {
    /// The id the browser shows.
    pub id: String,
    /// App name it becomes.
    pub name: String,
    /// Name for people.
    pub title: String,
    /// The file on this computer.
    pub path: String,
    /// What it was when asked for, in German.
    pub kind: String,
    /// Its size then, in bytes.
    pub size: u64,
    /// When it was asked for, Unix seconds.
    pub created: u64,
}

fn dir(config: &ConfigDir) -> PathBuf {
    config.root().join("app-requests")
}

/// Stores a request and returns it. One of the same name is replaced. Its
/// errors go to the browser that asked, so they stay German.
pub fn create(
    config: &ConfigDir,
    name: &str,
    title: &str,
    path: &str,
    info: FileInfo,
) -> anyhow::Result<FileRequest> {
    let dir = dir(config);
    fs::create_dir_all(&dir).with_context(|| format!("{} anlegen", dir.display()))?;
    let others: Vec<FileRequest> = list(config)
        .into_iter()
        .filter(|request| {
            if request.name == name {
                remove(config, &request.id);
                false
            } else {
                true
            }
        })
        .collect();
    anyhow::ensure!(
        others.len() < MAX_PENDING,
        "Schon {MAX_PENDING} Programme warten auf Freigabe; erst diese bestätigen oder ablehnen \
         (hearth app list)"
    );
    let request = FileRequest {
        id: new_id(),
        name: name.to_owned(),
        title: title.to_owned(),
        path: path.to_owned(),
        kind: info.kind.label(),
        size: info.size,
        created: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    };
    let file = dir.join(format!("{}.json", request.id));
    fs::write(&file, serde_json::to_vec_pretty(&request)?)
        .with_context(|| format!("{} schreiben", file.display()))?;
    Ok(request)
}

/// The request with this id, if there is one. Case does not matter.
pub fn load(config: &ConfigDir, id: &str) -> Option<FileRequest> {
    let id = id.trim().to_uppercase();
    if !valid(&id) {
        return None;
    }
    let bytes = fs::read(dir(config).join(format!("{id}.json"))).ok()?;
    serde_json::from_slice::<FileRequest>(&bytes)
        .ok()
        .filter(|request| request.id == id)
}

/// All waiting requests, oldest first; unreadable files are skipped.
pub fn list(config: &ConfigDir) -> Vec<FileRequest> {
    let Ok(entries) = fs::read_dir(dir(config)) else {
        return Vec::new();
    };
    let mut requests: Vec<FileRequest> = entries
        .flatten()
        .filter_map(|entry| fs::read(entry.path()).ok())
        .filter_map(|bytes| serde_json::from_slice::<FileRequest>(&bytes).ok())
        .filter(|request| valid(&request.id))
        .collect();
    requests.sort_by_key(|request| request.created);
    requests
}

/// Removes the request, if it exists.
pub fn remove(config: &ConfigDir, id: &str) {
    if valid(id) {
        let _ = fs::remove_file(dir(config).join(format!("{id}.json")));
    }
}

#[cfg(test)]
mod tests {
    use hearth_core::app_file::FileKind;

    use super::*;

    #[test]
    fn requests_wait_replace_each_other_by_name_and_are_limited() {
        let temp = tempfile::tempdir().unwrap();
        let config = ConfigDir::new(temp.path().to_path_buf());
        let info = FileInfo {
            kind: FileKind::Zip,
            size: 3,
        };
        let first = create(&config, "tool", "Tool", "C:/t.zip", info).unwrap();
        assert_eq!(load(&config, &first.id.to_lowercase()).unwrap(), first);
        let second = create(&config, "tool", "Tool 2", "C:/t2.zip", info).unwrap();
        assert_eq!(list(&config), vec![second.clone()]);
        for n in 1..MAX_PENDING {
            create(&config, &format!("tool{n}"), "T", "C:/x.zip", info).unwrap();
        }
        assert!(create(&config, "one-more", "T", "C:/x.zip", info).is_err());
        remove(&config, &second.id);
        assert!(load(&config, &second.id).is_none());
        assert!(load(&config, "../../x").is_none());
    }
}
