//! What carries over from llm-harness, as hearth was called before 0.2.0:
//! the configuration folder and the device token. Everything else (the VM,
//! its volumes, the services) starts anew under the new names.

use std::fs;
use std::io;
use std::path::Path;

/// The configuration folder in the home folder before 0.2.0.
pub const OLD_CONFIG_DIR: &str = ".harness";

/// The Credential Manager service of device tokens before 0.2.0.
pub const OLD_KEYRING_SERVICE: &str = "llm-harness";

/// The VM's name before 0.2.0; old configurations name it.
const OLD_MACHINE: &str = "harness";

/// Folders and files of the old configuration worth keeping: projects, own
/// apps and their files, own harness definitions and the server's app
/// templates. Logs, rescued bundles and requests stay behind.
const KEPT: &[&str] = &[
    "projects",
    "apps",
    "app-files",
    "harnesses",
    "server-apps.json",
];

/// Copies what matters of `~/.harness` into a new configuration folder that
/// does not exist yet. The old folder stays untouched, so the old version
/// keeps working until it is removed.
///
/// # Errors
/// I/O errors while copying.
///
/// # Returns
/// Whether anything was copied.
pub fn adopt_old_config(old_root: &Path, new_root: &Path) -> io::Result<bool> {
    if new_root.exists() || !old_root.join("config.toml").is_file() {
        return Ok(false);
    }
    fs::create_dir_all(new_root)?;
    let config = fs::read_to_string(old_root.join("config.toml"))?;
    fs::write(new_root.join("config.toml"), with_new_machine(&config))?;
    for name in KEPT {
        let from = old_root.join(name);
        if from.is_dir() {
            copy_dir(&from, &new_root.join(name))?;
        } else if from.is_file() {
            fs::copy(&from, new_root.join(name))?;
        }
    }
    Ok(true)
}

/// An old `config.toml` with the VM under its new name; everything else,
/// comments included, stays as it was.
fn with_new_machine(config: &str) -> String {
    config
        .lines()
        .map(|line| {
            let key = line.trim_start();
            if key.starts_with("machine") && key.contains(&format!("\"{OLD_MACHINE}\"")) {
                line.replace(
                    &format!("\"{OLD_MACHINE}\""),
                    &format!("\"{}\"", crate::podman::machine::DEFAULT_MACHINE),
                )
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + if config.ends_with('\n') { "\n" } else { "" }
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        // Links are not followed: the copy must not reach outside the folder.
        if kind.is_dir() {
            copy_dir(&entry.path(), &to.join(entry.file_name()))?;
        } else if kind.is_file() {
            fs::copy(entry.path(), to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_projects_and_renames_the_vm() {
        let base = std::env::temp_dir().join(format!("hearth-legacy-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let old = base.join("old");
        fs::create_dir_all(old.join("projects")).unwrap();
        fs::write(
            old.join("config.toml"),
            "# comment\n[vm]\nmachine = \"harness\"\n",
        )
        .unwrap();
        fs::write(old.join("projects/demo.toml"), "name = \"demo\"\n").unwrap();
        fs::write(old.join("daemon.log"), "old log").unwrap();

        let new = base.join("new");
        assert!(adopt_old_config(&old, &new).unwrap());
        assert_eq!(
            fs::read_to_string(new.join("config.toml")).unwrap(),
            "# comment\n[vm]\nmachine = \"hearth\"\n"
        );
        assert!(new.join("projects/demo.toml").is_file());
        assert!(!new.join("daemon.log").exists());
        // Only once: an existing folder is never overwritten.
        assert!(!adopt_old_config(&old, &new).unwrap());
        assert!(old.join("config.toml").is_file());
        fs::remove_dir_all(&base).unwrap();
    }
}
