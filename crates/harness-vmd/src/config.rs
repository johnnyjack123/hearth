//! `vmd.toml` in the service directory, written by `harness vmd install`.
//! Only administrators can change it, and the service still checks every
//! value: it decides what runs as SYSTEM and who may talk to the service.

use std::fs;
use std::path::{Path, PathBuf};

use harness_core::provider::validate_resource_name;
use harness_core::vmd::{below_program_files, is_user_sid};
use serde::Deserialize;

/// Configuration of the service.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// The Podman machine the service owns.
    pub machine: String,
    /// podman.exe; must lie below Program Files, which only administrators
    /// can write.
    pub podman: PathBuf,
    /// The one user allowed on the pipe besides administrators.
    pub user_sid: String,
    /// Memory Hyper-V reserves when the VM starts. Small, because the
    /// start fails when the host lacks it (spike §12.1, finding 2); on
    /// ARM64 the VM starts with more when Windows has it free, because it
    /// cannot grow later (`machine::startup_memory_mb`).
    #[serde(default = "default_startup")]
    pub startup_memory_mb: u32,
    /// Lower bound of dynamic memory.
    #[serde(default = "default_minimum")]
    pub minimum_memory_mb: u32,
    /// Upper bound of dynamic memory.
    #[serde(default = "default_maximum")]
    pub maximum_memory_mb: u32,
}

fn default_startup() -> u32 {
    1024
}

fn default_minimum() -> u32 {
    512
}

fn default_maximum() -> u32 {
    4096
}

impl Config {
    /// Reads and validates `path`.
    pub fn load(path: &Path) -> Result<Config, String> {
        let text =
            fs::read_to_string(path).map_err(|e| format!("{} lesen: {e}", path.display()))?;
        let config: Config =
            toml::from_str(&text).map_err(|e| format!("{} ist ungültig: {e}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    /// Checks the values the service relies on.
    pub fn validate(&self) -> Result<(), String> {
        validate_resource_name("Machine-Name", &self.machine).map_err(|e| e.to_string())?;
        if !is_user_sid(&self.user_sid) {
            return Err(format!("user_sid {:?} ist keine Nutzer-SID", self.user_sid));
        }
        if !below_program_files(&self.podman) {
            return Err(format!(
                "podman {} liegt nicht unter Program Files; als SYSTEM wird nur ausgeführt, was \
                 nur Administratoren ändern können",
                self.podman.display()
            ));
        }
        let (min, start, max) = (
            self.minimum_memory_mb,
            self.startup_memory_mb,
            self.maximum_memory_mb,
        );
        if !(256..=start).contains(&min) || start > max || max > 65536 {
            return Err(format!(
                "Speicher muss 256 ≤ minimum ({min}) ≤ startup ({start}) ≤ maximum ({max}) ≤ 65536 MB erfüllen"
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        toml::from_str(
            r#"
            machine = "harness"
            podman = 'C:\Program Files\RedHat\Podman\podman.exe'
            user_sid = "S-1-5-21-1004336348-1177238915-682003330-1001"
            "#,
        )
        .unwrap()
    }

    #[test]
    fn valid_config_and_defaults() {
        let config = config();
        config.validate().unwrap();
        assert_eq!(config.startup_memory_mb, 1024);
        assert_eq!(config.maximum_memory_mb, 4096);
    }

    #[test]
    fn values_that_would_run_or_admit_too_much_are_refused() {
        for sid in [
            "S-1-5-18",
            "S-1-1-0",
            "S-1-5-21-1-2-3-4)(A;;GA;;;WD",
            "S-1-5-21-1-2-3",
        ] {
            let bad = Config {
                user_sid: sid.into(),
                ..config()
            };
            assert!(bad.validate().is_err(), "{sid} accepted");
        }
        for podman in [
            r"C:\Users\x\podman.exe",
            r"C:\Program Files\..\Users\x\podman.exe",
            r"podman.exe",
            r"\\server\Program Files\podman.exe",
        ] {
            let bad = Config {
                podman: podman.into(),
                ..config()
            };
            assert!(bad.validate().is_err(), "{podman} accepted");
        }
        let bad = Config {
            machine: "-x".into(),
            ..config()
        };
        assert!(bad.validate().is_err());
        let bad = Config {
            startup_memory_mb: 8192,
            ..config()
        };
        assert!(bad.validate().is_err());
        assert!(toml::from_str::<Config>("machine = 'h'\nextra = 1\n").is_err());
    }
}
