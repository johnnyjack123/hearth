//! File formats of `~/.harness` (ARCHITECTURE.md §3.8, isolated mode).
//!
//! Every table rejects unknown keys: a typo such as `[netwrok]` would
//! otherwise silently fall back to a less restrictive default.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

pub use harness_gateway::DomainPattern;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};

use crate::podman::DEFAULT_MACHINE;
use crate::provider::{validate_container_path, validate_env_key, validate_memory};

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
    /// LLM APIs the harnesses need; reachable from every project, whatever
    /// its preset, because no harness works without its model.
    #[serde(default = "default_llm_apis", deserialize_with = "domain_list")]
    pub llm_apis: Vec<DomainPattern>,
    /// The harness server this computer is paired with (`harness login`).
    #[serde(default)]
    pub server: ServerConfig,
    /// The check before `harness merge`.
    #[serde(default)]
    pub review: ReviewConfig,
    /// Defaults for projects.
    #[serde(default)]
    pub defaults: Defaults,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        GlobalConfig {
            machine: default_machine(),
            llm_apis: default_llm_apis(),
            server: ServerConfig::default(),
            review: ReviewConfig::default(),
            defaults: Defaults::default(),
        }
    }
}

impl Validate for GlobalConfig {
    fn validate(&self) -> Result<(), String> {
        crate::provider::validate_resource_name("Machine-Name", &self.machine)
            .map_err(|e| e.to_string())?;
        if let Some(url) = &self.server.url
            && !(url.starts_with("http://") || url.starts_with("https://"))
        {
            return Err(format!(
                "server.url muss mit http:// oder https:// beginnen, nicht {url:?}"
            ));
        }
        if let Some(ca) = &self.server.ca
            && !ca.is_absolute()
        {
            return Err(format!(
                "server.ca muss ein absoluter Pfad sein, z. B. 'C:\\Users\\name\\caddy-root.crt', \
                 nicht {:?}",
                ca.display().to_string()
            ));
        }
        validate_harness_name(&self.defaults.harness)?;
        self.defaults.resources.validate()
    }
}

/// `[server]` in `config.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// Base URL of the paired server, e.g. `http://localhost:8080`; unset
    /// when this computer is not paired. The device token is not stored here
    /// but in the OS credential store.
    #[serde(default)]
    pub url: Option<String>,
    /// PEM file with the root certificate of a server with its own CA, e.g.
    /// Caddy's `tls internal`. For an `https://` server harness then trusts
    /// only this CA instead of the public ones, on the host and in the
    /// gateway. Stays when logging out, unlike `url`.
    #[serde(default)]
    pub ca: Option<PathBuf>,
}

/// `[review]` in `config.toml`. The fixed rules always run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewConfig {
    /// Look new dependencies up in OSV's list of malicious packages; sends
    /// their names to api.osv.dev.
    #[serde(default = "default_true")]
    pub osv: bool,
    /// Let Jev judge the added lines through the paired server, with the
    /// user's OpenRouter key; sends the agent's changes there.
    #[serde(default = "default_true")]
    pub jev: bool,
}

impl Default for ReviewConfig {
    fn default() -> Self {
        ReviewConfig {
            osv: true,
            jev: true,
        }
    }
}

fn default_true() -> bool {
    true
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
    /// Access from web chats through `harness connect`.
    #[serde(default)]
    pub remote: RemoteConfig,
    /// Programs from the app catalog the agent gets.
    #[serde(default)]
    pub apps: AppsConfig,
}

/// `[apps]` of a project.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppsConfig {
    /// Apps switched on, by name; the apps they require come along.
    #[serde(default = "default_apps")]
    pub enabled: Vec<String>,
}

impl Default for AppsConfig {
    fn default() -> Self {
        AppsConfig {
            enabled: default_apps(),
        }
    }
}

/// The C compiler used to be part of the base image; projects from before
/// the app catalog keep it, and new ones start with it.
fn default_apps() -> Vec<String> {
    vec!["c".to_owned()]
}

/// `[remote]` of a project: whether web chats may run agents in it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteConfig {
    /// Offered to web chats of the account (through the background service).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// A chat needs a confirmation on this computer before its first use;
    /// only `harness connect` in a terminal can ask.
    #[serde(default)]
    pub confirm: bool,
    /// Chats confirmed so far (conversation ids).
    #[serde(default)]
    pub chats: Vec<String>,
}

// Offered without confirmation by default: the user decided on 17.10. that
// a project should be usable from the account's chats right after `harness
// init`. The sandbox still applies; `harness project remote off` withdraws it.
impl Default for RemoteConfig {
    fn default() -> Self {
        RemoteConfig {
            enabled: true,
            confirm: false,
            chats: Vec::new(),
        }
    }
}

/// Whether `id` looks like a conversation id of the server (UUID).
pub fn valid_chat_id(id: &str) -> bool {
    (1..=100).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
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
        if let Some(bad) = self.remote.chats.iter().find(|id| !valid_chat_id(id)) {
            return Err(format!("remote.chats: ungültige Chat-ID {bad:?}"));
        }
        for app in &self.apps.enabled {
            validate_app_name(app).map_err(|e| format!("apps.enabled: {e}"))?;
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkConfig {
    /// Preset; falls back to `defaults.network` in `config.toml`.
    #[serde(default)]
    pub preset: Option<NetworkPreset>,
    /// Hold requests to hosts that are not allowed and ask the user
    /// (`harness net watch`) instead of refusing them right away.
    #[serde(default = "default_ask")]
    pub ask: bool,
    /// Domains allowed in addition to the preset.
    #[serde(default, deserialize_with = "domain_list")]
    pub allow: Vec<DomainPattern>,
    /// Domains blocked even if the preset or `allow` permits them.
    #[serde(default, deserialize_with = "domain_list")]
    pub deny: Vec<DomainPattern>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        NetworkConfig {
            preset: None,
            ask: default_ask(),
            allow: Vec::new(),
            deny: Vec::new(),
        }
    }
}

/// Network preset (ARCHITECTURE.md §3.5). The LLM APIs from `llm_apis` are
/// reachable in all of them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkPreset {
    /// Nothing but the LLM APIs; `allow` is ignored.
    Off,
    /// Package registries and git hosts, plus `allow`.
    #[default]
    Development,
    /// The public internet; host, LAN, tailnet and VM stay blocked.
    Open,
    /// Only the project's `allow` list.
    Custom,
}

impl FromStr for NetworkPreset {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        [
            NetworkPreset::Off,
            NetworkPreset::Development,
            NetworkPreset::Open,
            NetworkPreset::Custom,
        ]
        .into_iter()
        .find(|p| p.as_str() == text)
        .ok_or_else(|| {
            format!(
                "unbekannte Voreinstellung {text:?}, erwartet off, development, open oder custom"
            )
        })
    }
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
    /// Path (`~/…` or absolute) kept in a per-harness volume shared by all
    /// projects, e.g. logins. At most one, since it is one volume.
    #[serde(default)]
    pub persist: Vec<String>,
    /// How harness points the harness at `llm.harness` after `harness login`.
    #[serde(default)]
    pub llm: Option<LlmWiring>,
    /// Guard against prompt injection that harness loads into the harness.
    #[serde(default)]
    pub guard: Option<GuardKind>,
    /// Option that appends text to the harness's system prompt; harness
    /// passes its notes about the sandbox with it (`briefing`).
    #[serde(default)]
    pub prompt_flag: Option<String>,
    /// Environment variables for the harness's process.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Machine interface for the web chat (Phase 4).
    #[serde(default)]
    pub adapter: Option<AdapterConfig>,
}

/// Which guard against prompt injection a harness gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuardKind {
    /// pi: the extension `pi/guard.ts`, loaded with `-e`.
    Pi,
}

/// How a harness is configured to reach its model through the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LlmWiring {
    /// pi: `~/.pi/agent/models.json` with the account's providers.
    Pi,
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
        if self.persist.len() > 1 {
            return Err(
                "persist darf nur einen Pfad enthalten (ein Volume pro Harness)".to_owned(),
            );
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
        for key in self.env.keys() {
            validate_env_key(key).map_err(|e| e.to_string())?;
        }
        if let Some(adapter) = &self.adapter
            && adapter.command.is_empty()
        {
            return Err("adapter.command darf nicht leer sein".to_owned());
        }
        Ok(())
    }
}

/// `apps/<name>.toml`: a program of the app catalog (ROADMAP Phase 3). It is
/// installed once into a volume of its own and mounted read-only at
/// `/opt/harness-apps/<name>` into the instances of projects that switch it
/// on (`crate::apps`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    /// Must match the file name.
    pub name: String,
    /// Name for people, e.g. `C/C++`.
    pub title: String,
    /// One line for listings.
    #[serde(default)]
    pub description: Option<String>,
    /// Apps this one needs; they are mounted with it and while it installs.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Hosts the installer may reach, granted for the install only.
    #[serde(default, deserialize_with = "domain_list")]
    pub hosts: Vec<DomainPattern>,
    /// Folders put on `PATH`: relative to the app's folder, or `~/…` for
    /// what the agent installs with the app's tools (`cargo install`).
    #[serde(default)]
    pub path: Vec<String>,
    /// Further environment variables; `{prefix}` is the app's folder and a
    /// leading `~` the agent's home.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Shell script that installs the app into `$PREFIX` (`apps/lib.sh`
    /// documents what it gets).
    pub install: String,
}

/// Where apps are mounted in instances, one folder each.
pub const APPS_DIR: &str = "/opt/harness-apps";

/// Variables the instance sets itself; an app must not override them.
const RESERVED_ENV: &[&str] = &["PATH", "HARNESS_APPS_PATH", "HOME", "USER"];

impl AppConfig {
    /// The app's folder in instances and in the installer.
    pub fn dir(&self) -> String {
        format!("{APPS_DIR}/{}", self.name)
    }

    /// `path` as absolute folders.
    pub fn path_dirs(&self) -> Vec<String> {
        self.path
            .iter()
            .map(|dir| {
                if dir.starts_with("~/") {
                    expand_home(dir)
                } else {
                    format!("{}/{dir}", self.dir())
                }
            })
            .collect()
    }

    /// `env` with `{prefix}` and a leading `~` expanded.
    pub fn env_vars(&self) -> BTreeMap<String, String> {
        self.env
            .iter()
            .map(|(key, value)| {
                let value = value.replace("{prefix}", &self.dir());
                (key.clone(), expand_home(&value))
            })
            .collect()
    }
}

impl Validate for AppConfig {
    fn validate(&self) -> Result<(), String> {
        validate_app_name(&self.name)?;
        if self.title.trim().is_empty() || self.title.chars().count() > 60 {
            return Err("title muss 1 bis 60 Zeichen lang sein".to_owned());
        }
        if self.install.trim().is_empty() {
            return Err("install darf nicht leer sein".to_owned());
        }
        for required in &self.requires {
            validate_app_name(required).map_err(|e| format!("requires: {e}"))?;
            if *required == self.name {
                return Err("requires darf die App nicht selbst enthalten".to_owned());
            }
        }
        for dir in &self.path {
            // Only "~/…" counts as home: "~foo" would be another user's.
            let relative = dir.strip_prefix("~/").unwrap_or(dir);
            let valid = !relative.is_empty()
                && !relative.starts_with(['/', '~'])
                && !relative.contains(['\\', ':', ',', '\n', '\0'])
                && !relative
                    .split('/')
                    .any(|part| part.is_empty() || part == "..");
            if !valid {
                return Err(format!(
                    "path: {dir:?} muss ein Ordner unter der App (z. B. \"bin\") oder unter ~/ sein"
                ));
            }
        }
        for (key, value) in &self.env {
            validate_env_key(key).map_err(|e| e.to_string())?;
            if RESERVED_ENV.contains(&key.as_str()) {
                return Err(format!(
                    "env: {key} setzt harness selbst (für den PATH gibt es path)"
                ));
            }
            if value.contains(['\n', '\0']) {
                return Err(format!("env: {key} enthält einen Zeilenumbruch"));
            }
        }
        Ok(())
    }
}

/// App names become volume and container names (`crate::apps`); the same
/// rules as harness names.
pub(crate) fn validate_app_name(name: &str) -> Result<(), String> {
    validate_harness_name(name).map_err(|_| {
        format!("ungültiger App-Name {name:?}: 1 bis 30 Zeichen, Kleinbuchstaben, Ziffern und '-'")
    })
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

/// Domain lists use the gateway's own pattern type, so the host and the
/// gateway can never disagree about what a pattern means.
fn domain_list<'de, D>(deserializer: D) -> Result<Vec<DomainPattern>, D::Error>
where
    D: Deserializer<'de>,
{
    Vec::<String>::deserialize(deserializer)?
        .iter()
        .map(|text| text.parse().map_err(D::Error::custom))
        .collect()
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

/// DeepSeek, the provider pi is set up with (docs/ROADMAP.md, package 6).
fn default_llm_apis() -> Vec<DomainPattern> {
    vec!["api.deepseek.com".parse().expect("valid built-in domain")]
}

fn default_ask() -> bool {
    true
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
    fn domain_lists() {
        // Pattern rules themselves are tested in harness-gateway.
        let text = "path = '/repo'\n[network]\nallow = ['API.example.com', '*.example.org']\n";
        let project: ProjectConfig = toml::from_str(text).unwrap();
        assert_eq!(project.network.allow[0].to_string(), "api.example.com");
        assert!(project.network.allow[1].matches("a.example.org"));
        assert!(project.network.ask);

        let global: GlobalConfig = toml::from_str("llm_apis = []\n").unwrap();
        assert!(global.llm_apis.is_empty());
        assert!(toml::from_str::<GlobalConfig>("llm_apis = ['localhost']\n").is_err());
    }

    #[test]
    fn presets_parse() {
        for preset in ["off", "development", "open", "custom"] {
            assert_eq!(preset.parse::<NetworkPreset>().unwrap().as_str(), preset);
        }
        assert!("closed".parse::<NetworkPreset>().is_err());
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
        assert_eq!(
            project.network,
            NetworkConfig {
                preset: Some(NetworkPreset::Development),
                ..NetworkConfig::default()
            }
        );
        assert_eq!(project.remote, RemoteConfig::default());
        assert_eq!(
            toml::from_str::<ProjectConfig>("path = '/repo'")
                .unwrap()
                .remote,
            RemoteConfig::default()
        );
        let odd = "path = '/repo'\n[remote]\nchats = ['a b']\n";
        assert!(
            toml::from_str::<ProjectConfig>(odd)
                .unwrap()
                .validate()
                .is_err()
        );
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
