//! Markers of hearth commands that use a machine's VM (`hearth run`,
//! `shell`, `net watch` …), so the background service does not stop an idle
//! VM under a terminal session.
//!
//! Each command holds a file in `<local data>/busy/<machine>/` open without
//! delete sharing. Windows refuses to delete a file that is open that way,
//! so whoever counts simply tries to delete every marker: one that goes away
//! belonged to a process that has ended, also one that crashed; one that
//! stays is in use.

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::lang::tr;
use crate::{Error, Result, paths, tr};

/// Longest label read from a marker; it is only shown in the log.
const MAX_LABEL: usize = 100;

/// Directory of the markers for `machine`.
pub fn dir(machine: &str) -> Result<PathBuf> {
    let valid = !machine.is_empty()
        && machine.len() <= 64
        && machine
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !machine.starts_with('.');
    if !valid {
        return Err(Error::Unsupported(tr!(
            "Ungültiger Machine-Name für Belegt-Marker: {machine:?}",
            "Invalid machine name for an in-use marker: {machine:?}"
        )));
    }
    Ok(paths::local_data_dir()?.join("busy").join(machine))
}

/// A held marker; dropping it releases it.
#[derive(Debug)]
pub struct Marker {
    file: Option<File>,
    path: PathBuf,
}

impl Marker {
    /// Marks the VM of `machine` as used by this process until the marker is
    /// dropped or the process ends. `label` says what uses it, e.g.
    /// `hearth run`.
    pub fn acquire(machine: &str, label: &str) -> Result<Marker> {
        Marker::acquire_in(&dir(machine)?, label)
    }

    /// [`Marker::acquire`] in an explicit directory (tests).
    pub fn acquire_in(dir: &Path, label: &str) -> Result<Marker> {
        fs::create_dir_all(dir)
            .map_err(|e| Error::io(tr("Belegt-Ordner anlegen", "Creating the in-use folder"), e))?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let path = dir.join(format!("{}-{nanos}.busy", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt as _;
            use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
            // Readable for the label, but not deletable while open.
            options.share_mode(FILE_SHARE_READ);
        }
        let mut file = options
            .open(&path)
            .map_err(|e| Error::io(tr("Belegt-Marker anlegen", "Creating the in-use marker"), e))?;
        let _ = file.write_all(label.as_bytes());
        let _ = file.flush();
        Ok(Marker {
            file: Some(file),
            path,
        })
    }
}

impl Drop for Marker {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

/// Labels of the other processes that use the VM of `machine` right now;
/// markers of ended processes are removed on the way. This process's own
/// markers do not count: it knows what it does itself.
pub fn users(machine: &str) -> Vec<String> {
    dir(machine)
        .map(|dir| users_in(&dir, Some(std::process::id())))
        .unwrap_or_default()
}

/// [`users`] in an explicit directory, without the markers of process
/// `except` (tests pass `None`).
pub fn users_in(dir: &Path, except: Option<u32>) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let own = except.map(|pid| format!("{pid}-"));
    let mut users = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "busy") {
            continue;
        }
        let name = entry.file_name();
        if own
            .as_deref()
            .is_some_and(|own| name.to_string_lossy().starts_with(own))
        {
            continue;
        }
        if released(&path) {
            continue;
        }
        let label = fs::read(&path)
            .map(|bytes| {
                let text = String::from_utf8_lossy(&bytes);
                text.chars()
                    .filter(|c| !c.is_control())
                    .take(MAX_LABEL)
                    .collect::<String>()
            })
            .unwrap_or_default();
        users.push(if label.is_empty() {
            "hearth".to_owned()
        } else {
            label
        });
    }
    users.sort();
    users
}

/// Whether the marker at `path` is no longer held; removes it if so.
#[cfg(windows)]
fn released(path: &Path) -> bool {
    // Fails with a sharing violation while the holder keeps it open.
    fs::remove_file(path).is_ok()
}

/// Without Windows' share modes a marker cannot tell whether its holder
/// still runs; markers of crashed processes then stay until removed by hand.
#[cfg(not(windows))]
fn released(_path: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_names_stay_inside_the_directory() {
        assert!(dir("hearth").is_ok());
        assert!(dir("podman-machine-default").is_ok());
        assert!(dir("..").is_err());
        assert!(dir("a/b").is_err());
        assert!(dir("a\\b").is_err());
        assert!(dir("").is_err());
    }

    #[test]
    fn held_markers_count_and_released_ones_vanish() {
        let temp = tempfile::tempdir().unwrap();
        assert!(users_in(temp.path(), None).is_empty());
        let run = Marker::acquire_in(temp.path(), "hearth run").unwrap();
        let shell = Marker::acquire_in(temp.path(), "hearth shell").unwrap();
        assert_eq!(users_in(temp.path(), None), ["hearth run", "hearth shell"]);
        // A process does not count itself.
        assert!(users_in(temp.path(), Some(std::process::id())).is_empty());
        drop(run);
        assert_eq!(users_in(temp.path(), None), ["hearth shell"]);
        drop(shell);
        assert!(users_in(temp.path(), None).is_empty());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }

    #[cfg(windows)]
    #[test]
    fn markers_of_ended_processes_are_cleaned_up() {
        let temp = tempfile::tempdir().unwrap();
        // What a crashed process leaves behind: a marker nobody holds.
        fs::write(temp.path().join("4242-1.busy"), "hearth run").unwrap();
        fs::write(temp.path().join("notes.txt"), "kein Marker").unwrap();
        assert!(users_in(temp.path(), None).is_empty());
        assert!(!temp.path().join("4242-1.busy").exists());
        assert!(temp.path().join("notes.txt").exists());
    }
}
