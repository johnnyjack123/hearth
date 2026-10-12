//! Well-known locations on the host.

use std::env;
use std::path::PathBuf;

use crate::{Error, Result};

/// Directory for machine-local state that is not configuration: the isolated
/// podman home, build contexts and the exchange folder.
///
/// Windows: `%LOCALAPPDATA%\llm-harness`. Elsewhere: `$XDG_DATA_HOME/llm-harness`
/// or `~/.local/share/llm-harness`.
///
/// Note that `<dir>/exchange` is shared into the VM; nothing secret may be
/// written there.
pub fn local_data_dir() -> Result<PathBuf> {
    if let Some(dir) = env::var_os("LOCALAPPDATA") {
        return Ok(PathBuf::from(dir).join("llm-harness"));
    }
    if let Some(dir) = env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(dir).join("llm-harness"));
    }
    Ok(home_dir()?.join(".local").join("share").join("llm-harness"))
}

/// The user's home directory (`%USERPROFILE%` on Windows, `$HOME` elsewhere).
pub fn home_dir() -> Result<PathBuf> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| Error::Unsupported("Weder USERPROFILE noch HOME ist gesetzt".to_owned()))
}
