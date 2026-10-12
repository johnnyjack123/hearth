//! File formats of `~/.harness` (ARCHITECTURE.md §3.8, isolated mode).
//!
//! Every table rejects unknown keys: a typo such as `[netwrok]` would
//! otherwise silently fall back to a less restrictive default.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use serde::Deserialize;

use crate::podman::DEFAULT_MACHINE;
use crate::provider::{validate_container_path, validate_memory};

/// Harness used when neither the command line nor the project names one.
pub const DEFAULT_HARNESS: &str = "pi";

/// Home directory of the agent inside instances; `~` in harness files.
pub const AGENT_HOME: &str = "/home/agent";

/// Validation beyond what serde checks. Errors are user-facing (German).
pub(crate) trait Validate {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

/// `config.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalConfig {
    /// Podman machine the containers run in.
    #[serde(default = "default_machine")]
    pub machine: String,
    /// Defaults for projects.
    #[serde(default)]
    pub defaults: Defaults,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        GlobalConfig {
            machine: default_machine(),
            defaults: Defaults::default(),
        }
    }
}

impl Validate for GlobalConfig {
    fn validate(&self) -> Result<(), String> {
        crate::provider::validate_resource_name("Machine-Name", &self.machine)
            .map_err(|e| e.to_string())?;
        validate_harness_name(&self.defaults.harness)?;
        self.defaults.resources.validate()
    }
}

/// `[defaults]` in `config.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    /// Harness for `harness run` without an argument.
    #[serde(default = "default_harness")]
    pub harness: String,
    /// Network preset of new projects.
    #[serde(default)]
    pub network: NetworkPreset,
    /// Resource limits of projects that set none.
    #[serde(default = "default_resources")]
    pub resources: ResourcesConfig,
}

impl Default for Defaults {
    fn default() -> Self {
        Defaults {
            harness: default_harness(),
            network: NetworkPreset::default(),
            resources: default_resources(),
        }
    }
}

/// `projects/<name>.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    /// Root of the git repository on the host.
    pub path: PathBuf,
    /// Harness for `harness run` without an argument.
    #[serde(default)]
    pub default_harness: Option<String>,
    /// How the agent gets at the files.
    #[serde(default)]
    pub files: FilesConfig,
    /// Egress rules.
    #[serde(default)]
    pub network: NetworkConfig,
    /// Resource limits; unset values fall back to `config.toml`.
    #[serde(default)]
    pub resources: ResourcesConfig,
}

impl Validate for ProjectConfig {
    fn validate(&self) -> Result<(), String> {
        if !self.path.is_absolute() {
            return Err(format!(
                "path muss ein absoluter Pfad sein: {}",
                self.path.display()
            ));
        }
        if let Some(harness) = &self.default_harness {
            validate_harness_name(harness)?;
        }
        self.resources.validate()
    }
}

/// `[files]` of a project.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilesConfig {
    /// File access mode.
    #[serde(default)]
    pub mode: FileMode,
}

/// How the agent gets at the project's files. Only `isolated` exists so far;
/// `direct` (bind mount) and `host` (no sandbox) are planned (ROADMAP Phase 5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileMode {
    /// Clone in a VM volume, exchange via git bundles.
    #[default]
    Isolated,
}

impl FileMode {
    /// Name as written in the config files.
    pub fn as_str(self) -> &'static str {
        match self {
            FileMode::Isolated => "isolated",
        }
    }
}

/// `[network]` of a project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkConfig {
    /// Preset; falls back to `defaults.network` in `config.toml`.
    #[serde(default)]
    pub preset: Option<NetworkPreset>,
    /// Domains allowed in addition to the preset.
    #[serde(default)]
    pub allow: Vec<DomainPattern>,
    /// Domains blocked even if the preset or `allow` permits them.
    #[serde(default)]
    pub deny: Vec<DomainPattern>,
}

/// Network preset (ARCHITECTURE.md §3.5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkPreset {
    /// No egress at all.
    Off,
    /// Package registries, git hosts and the LLM APIs the harnesses need.
    #[default]
    Development,
    /// The public internet; host, LAN, tailnet and link-local stay blocked.
    Open,
    /// Only the project's `allow` list.
    Custom,
}

impl NetworkPreset {
    /// Name as written in the config files.
    pub fn as_str(self) -> &'static str {
        match self {
            NetworkPreset::Off => "off",
            NetworkPreset::Development => "development",
            NetworkPreset::Open => "open",
            NetworkPreset::Custom => "custom",
        }
    }
}

impl fmt::Display for NetworkPreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `[resources]` of a project or `[defaults.resources]`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourcesConfig {
    /// CPU limit; unset means no limit (or the default).
    #[serde(default)]
    pub cpus: Option<f64>,
    /// Memory limit in podman notation (`512m`, `2g`).
    #[serde(default)]
    pub memory: Option<String>,
}

impl Validate for ResourcesConfig {
    fn validate(&self) -> Result<(), String> {
        if let Some(cpus) = self.cpus
            && !(cpus.is_finite() && cpus > 0.0)
        {
            return Err(format!("cpus muss größer als 0 sein, nicht {cpus}"));
        }
        if let Some(memory) = &self.memory {
            validate_memory(memory).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// `harnesses/<name>.toml` (ARCHITECTURE.md §3.7).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessConfig {
    /// Must match the file name.
    pub name: String,
    /// One line for listings.
    #[serde(default)]
    pub description: Option<String>,
    /// Shell command that installs the harness into the tools volume.
    pub install: String,
    /// Program and arguments that start the harness.
    pub command: Vec<String>,
    /// Paths (`~/…` or absolute) kept in a per-harness volume shared by all
    /// projects, e.g. logins.
    #[serde(default)]
    pub persist: Vec<String>,
    /// Machine interface for the web chat (Phase 4).
    #[serde(default)]
    pub adapter: Option<AdapterConfig>,
}

impl HarnessConfig {
    /// `persist` entries as absolute container paths (`~` → [`AGENT_HOME`]).
    pub fn persist_paths(&self) -> Vec<String> {
        self.persist.iter().map(|p| expand_home(p)).collect()
    }

    /// Volume holding this harness's persisted paths.
    pub fn config_volume(&self) -> String {
        format!("harness-cfg-{}", self.name)
    }
}

impl Validate for HarnessConfig {
    fn validate(&self) -> Result<(), String> {
        validate_harness_name(&self.name)?;
        if self.install.trim().is_empty() {
            return Err("install darf nicht leer sein".to_owned());
        }
        if self.command.first().is_none_or(|c| c.trim().is_empty()) {
            return Err("command darf nicht leer sein".to_owned());
        }
        for path in &self.persist {
            let expanded = expand_home(path);
            validate_container_path("persist", &expanded).map_err(|e| e.to_string())?;
            if expanded == AGENT_HOME || expanded == "/" {
                return Err(format!(
                    "persist darf nicht das ganze Home-Verzeichnis sein: {path}"
                ));
            }
        }
        if let Some(adapter) = &self.adapter
            && adapter.command.is_empty()
        {
            return Err("adapter.command darf nicht leer sein".to_owned());
        }
        Ok(())
    }
}

/// `[adapter]` of a harness.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterConfig {
    /// Adapter implementation, e.g. `pi-rpc`.
    pub kind: String,
    /// Program and arguments for machine mode.
    pub command: Vec<String>,
}

/// A domain in an allow or deny list: `example.com` or `*.example.com`
/// (subdomains only). Stored lowercase.
///
/// IP literals are rejected on purpose: the gateway decides about
/// addresses itself after resolving names (phase 0, 6.4).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct DomainPattern {
    wildcard: bool,
    domain: String,
}

impl DomainPattern {
    /// Whether `host` is covered by this pattern.
    pub fn matches(&self, host: &str) -> bool {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if self.wildcard {
            host.strip_suffix(&self.domain)
                .is_some_and(|rest| rest.len() > 1 && rest.ends_with('.'))
        } else {
            host == self.domain
        }
    }
}

impl FromStr for DomainPattern {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let lower = text.trim().to_ascii_lowercase();
        let (wildcard, domain) = match lower.strip_prefix("*.") {
            Some(rest) => (true, rest.to_owned()),
            None => (false, lower),
        };
        let labels: Vec<&str> = domain.split('.').collect();
        let label_ok = |l: &&str| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        };
        let tld_ok = labels
            .last()
            .is_some_and(|tld| tld.chars().any(|c| c.is_ascii_alphabetic()));
        if labels.len() < 2 || !labels.iter().all(label_ok) || !tld_ok || domain.len() > 253 {
            return Err(format!(
                "ungültige Domain {text:?}, erwartet z. B. \"api.example.com\" oder \"*.example.com\""
            ));
        }
        Ok(DomainPattern { wildcard, domain })
    }
}

impl TryFrom<String> for DomainPattern {
    type Error = String;

    fn try_from(text: String) -> Result<Self, String> {
        text.parse()
    }
}

impl fmt::Display for DomainPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.wildcard {
            write!(f, "*.{}", self.domain)
        } else {
            f.write_str(&self.domain)
        }
    }
}

/// Harness names become file and volume names.
pub(crate) fn validate_harness_name(name: &str) -> Result<(), String> {
    let valid = (1..=30).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if valid {
        Ok(())
    } else {
        Err(format!(
            "ungültiger Harness-Name {name:?}: 1 bis 30 Zeichen, Kleinbuchstaben, Ziffern und '-'"
        ))
    }
}

fn expand_home(path: &str) -> String {
    match path.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{AGENT_HOME}{rest}"),
        _ => path.to_owned(),
    }
}

fn default_machine() -> String {
    DEFAULT_MACHINE.to_owned()
}

fn default_harness() -> String {
    DEFAULT_HARNESS.to_owned()
}

/// Same values as `defaults/config.toml`, so a missing file and a fresh one
/// behave alike.
fn default_resources() -> ResourcesConfig {
    ResourcesConfig {
        cpus: None,
        memory: Some("3g".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_patterns() {
        let exact: DomainPattern = "API.DeepSeek.com".parse().unwrap();
        assert_eq!(exact.to_string(), "api.deepseek.com");
        assert!(exact.matches("api.deepseek.com"));
        assert!(exact.matches("API.deepseek.com."));
        assert!(!exact.matches("x.api.deepseek.com"));

        let wildcard: DomainPattern = "*.githubusercontent.com".parse().unwrap();
        assert!(wildcard.matches("objects.githubusercontent.com"));
        assert!(wildcard.matches("a.b.githubusercontent.com"));
        assert!(!wildcard.matches("githubusercontent.com"));
        assert!(!wildcard.matches("evilgithubusercontent.com"));

        for bad in [
            "",
            "localhost",
            "1.2.3.4",
            "*.com.",
            "a..b",
            "-a.com",
            "a.com:443",
            "https://a.com",
            "*",
            "a.*.com",
            "ex ample.com",
        ] {
            assert!(bad.parse::<DomainPattern>().is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn harness_validation() {
        let pi: HarnessConfig =
            toml::from_str(include_str!("../../defaults/harnesses/pi.toml")).unwrap();
        pi.validate().unwrap();
        assert_eq!(pi.persist_paths(), ["/home/agent/.pi"]);
        assert_eq!(pi.config_volume(), "harness-cfg-pi");

        let mut bad = pi.clone();
        bad.persist = vec!["~".into()];
        assert!(bad.validate().is_err());
        bad.persist = vec!["relative/path".into()];
        assert!(bad.validate().is_err());
        let mut bad = pi.clone();
        bad.command = vec![];
        assert!(bad.validate().is_err());
        let mut bad = pi;
        bad.name = "Pi".into();
        assert!(bad.validate().is_err());
    }

    #[test]
    fn templates_parse_and_match_code_defaults() {
        let global: GlobalConfig =
            toml::from_str(include_str!("../../defaults/config.toml")).unwrap();
        global.validate().unwrap();
        assert_eq!(global, GlobalConfig::default());
        assert_eq!(GlobalConfig::default(), toml::from_str("").unwrap());

        let project =
            include_str!("../../defaults/project.toml").replace("path = \"\"", "path = '/repo'");
        let project: ProjectConfig = toml::from_str(&project).unwrap();
        assert_eq!(project.network.preset, Some(NetworkPreset::Development));
        assert_eq!(project.files.mode, FileMode::Isolated);
    }

    #[test]
    fn unknown_keys_and_modes_are_rejected() {
        let typo = "path = '/repo'\n[netwrok]\npreset = 'off'\n";
        assert!(toml::from_str::<ProjectConfig>(typo).is_err());
        let direct = "path = '/repo'\n[files]\nmode = 'direct'\n";
        assert!(toml::from_str::<ProjectConfig>(direct).is_err());
        let bad_domain = "path = '/repo'\n[network]\nallow = ['1.1.1.1']\n";
        assert!(toml::from_str::<ProjectConfig>(bad_domain).is_err());
    }

    #[test]
    fn resource_validation() {
        let ok = ResourcesConfig {
            cpus: Some(1.5),
            memory: Some("512m".into()),
        };
        assert!(ok.validate().is_ok());
        let bad = ResourcesConfig {
            cpus: Some(0.0),
            memory: None,
        };
        assert!(bad.validate().is_err());
        let bad = ResourcesConfig {
            cpus: None,
            memory: Some("lots".into()),
        };
        assert!(bad.validate().is_err());
    }
}
