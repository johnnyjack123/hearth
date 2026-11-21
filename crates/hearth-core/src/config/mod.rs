//! Host configuration in `~/.hearth` (`%USERPROFILE%\.hearth`).
//!
//! The files are the source of truth and meant to be edited by hand. They are
//! read with serde and written with `toml_edit`, so comments and formatting
//! survive. Every write is re-parsed and validated first, so the harness never
//! leaves a file behind that it cannot read itself.
//!
//! Security rule (ARCHITECTURE.md §3.8): nothing here may live inside a
//! project, because agents can change project files via their commits.

mod model;
mod template;

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use toml_edit::DocumentMut;

pub use model::{
    AGENT_HOME, APPS_DIR, AdapterConfig, AppConfig, AppFile, AppSource, AppsConfig,
    DEFAULT_HARNESS, Defaults, DomainPattern, FileMode, FilesConfig, GlobalConfig, GuardKind,
    HarnessConfig, HostChange, HostConfig, HostWrites, LlmWiring, NetworkConfig, NetworkPreset,
    ProjectConfig, RemoteChange, RemoteConfig, ResourcesConfig, ReviewConfig, ServerConfig,
    valid_chat_id, valid_file_name,
};
use model::{Validate, validate_app_name, validate_harness_name};
pub use template::{AppKind, AppTemplate, MAX_PACKAGE_LEN, MAX_PACKAGES, MAX_SCRIPT};

use crate::lang::tr;
use crate::paths::{canonical, home_dir, is_within, same_path};
use crate::project::ProjectName;
use crate::provider::Resources;
use crate::{Error, Result, tr};

const CONFIG_TEMPLATE: &str = include_str!("../../defaults/config.toml");
const PROJECT_TEMPLATE: &str = include_str!("../../defaults/project.toml");

/// Harness definitions shipped with the binary, used when no file of that
/// name exists. They are not copied into the folder: a copy would freeze the
/// definition at the version of the first `init` and miss later fields.
/// A file in `harnesses/` is for deliberate changes only.
const BUILTIN_HARNESSES: &[(&str, &str)] =
    &[("pi", include_str!("../../defaults/harnesses/pi.toml"))];

/// The app catalog shipped with the binary; like the harnesses, a file of
/// the same name in `apps/` replaces an entry, and further files add apps.
const BUILTIN_APPS: &[(&str, &str)] = &[
    ("c", include_str!("../../defaults/apps/c.toml")),
    ("rust", include_str!("../../defaults/apps/rust.toml")),
    ("go", include_str!("../../defaults/apps/go.toml")),
    ("java", include_str!("../../defaults/apps/java.toml")),
    ("deno", include_str!("../../defaults/apps/deno.toml")),
    ("bun", include_str!("../../defaults/apps/bun.toml")),
    ("sqlite", include_str!("../../defaults/apps/sqlite.toml")),
];

/// The configuration folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDir {
    root: PathBuf,
}

/// One of the two domain lists in `[network]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainList {
    /// `network.allow`.
    Allow,
    /// `network.deny`.
    Deny,
}

impl DomainList {
    fn key(self) -> &'static str {
        match self {
            DomainList::Allow => "allow",
            DomainList::Deny => "deny",
        }
    }
}

/// A project with all defaults applied.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    /// Project name (file name without `.toml`).
    pub name: ProjectName,
    /// The project file.
    pub file: PathBuf,
    /// Root of the git repository on the host.
    pub path: PathBuf,
    /// Harness for `hearth run` without an argument.
    pub harness: String,
    /// File access mode.
    pub mode: FileMode,
    /// Effective network preset.
    pub network: NetworkPreset,
    /// Ask the user about hosts that are not allowed.
    pub ask: bool,
    /// Extra allowed domains.
    pub allow: Vec<DomainPattern>,
    /// Blocked domains; they win over the preset and `allow`.
    pub deny: Vec<DomainPattern>,
    /// LLM APIs from `config.toml`, reachable whatever the preset.
    pub llm_apis: Vec<DomainPattern>,
    /// Effective resource limits.
    pub resources: Resources,
    /// Apps switched on (`[apps] enabled`), without the ones they require;
    /// [`ConfigDir::project_apps`] resolves them.
    pub apps: Vec<String>,
}

impl ConfigDir {
    /// `HEARTH_HOME` if set, else `~/.hearth`.
    pub fn default_root() -> Result<PathBuf> {
        match env::var_os("HEARTH_HOME") {
            Some(dir) => Ok(PathBuf::from(dir)),
            None => Ok(home_dir()?.join(".hearth")),
        }
    }

    /// The configuration folder at [`ConfigDir::default_root`].
    pub fn open_default() -> Result<Self> {
        Ok(Self::new(Self::default_root()?))
    }

    /// A configuration folder at an explicit location (tests, `--config`).
    pub fn new(root: PathBuf) -> Self {
        ConfigDir { root }
    }

    /// The folder itself.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `config.toml`.
    pub fn config_file(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    /// Folder with one file per project.
    pub fn projects_dir(&self) -> PathBuf {
        self.root.join("projects")
    }

    /// File of the named project.
    pub fn project_file(&self, name: &ProjectName) -> PathBuf {
        self.projects_dir().join(format!("{name}.toml"))
    }

    /// Folder with harness definitions.
    pub fn harnesses_dir(&self) -> PathBuf {
        self.root.join("harnesses")
    }

    /// Folder with app recipes of one's own (the catalog is built in).
    pub fn apps_dir(&self) -> PathBuf {
        self.root.join("apps")
    }

    /// Copies of the program files the user approved, one folder per app
    /// (`[file]` of a recipe).
    pub fn app_files_dir(&self) -> PathBuf {
        self.root.join("app-files")
    }

    /// Copy of the account's app templates, written by the background
    /// service whenever the server sends them, read by every start.
    pub fn templates_file(&self) -> PathBuf {
        self.root.join("server-apps.json")
    }

    /// The account's app templates as last received. A missing or broken
    /// file means none, and a template this hearth cannot read (a newer
    /// server's) is left out: neither may stop a project from starting.
    pub fn templates(&self) -> Vec<AppTemplate> {
        let Ok(Some(text)) = read_optional(&self.templates_file()) else {
            return Vec::new();
        };
        let Ok(values) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
            return Vec::new();
        };
        values
            .into_iter()
            .filter_map(|value| serde_json::from_value(value).ok())
            .collect()
    }

    /// Replaces the copy of the account's app templates.
    pub fn save_templates(&self, templates: &[AppTemplate]) -> Result<()> {
        fs::create_dir_all(&self.root)
            .map_err(|e| Error::io(tr!("{} anlegen", "Creating {}", self.root.display()), e))?;
        let text = serde_json::to_string_pretty(templates).map_err(|e| {
            Error::Unsupported(tr!(
                "Vorlagen nicht speicherbar: {e}",
                "Templates cannot be saved: {e}"
            ))
        })?;
        write_atomic(&self.templates_file(), &text, true)
    }

    /// Creates the folder layout and writes default files that do not exist
    /// yet. Existing files are never touched. Returns the files written.
    pub fn init(&self) -> Result<Vec<PathBuf>> {
        for dir in [self.projects_dir(), self.harnesses_dir()] {
            fs::create_dir_all(&dir)
                .map_err(|e| Error::io(tr!("{} anlegen", "Creating {}", dir.display()), e))?;
        }
        let mut written = Vec::new();
        let file = self.config_file();
        if !file.exists() {
            write_atomic(&file, CONFIG_TEMPLATE, false)?;
            written.push(file);
        }
        Ok(written)
    }

    /// `config.toml`, or the built-in defaults if it does not exist.
    pub fn global(&self) -> Result<GlobalConfig> {
        match read_optional(&self.config_file())? {
            Some(text) => parse(&self.config_file(), &text),
            None => Ok(GlobalConfig::default()),
        }
    }

    /// Names of all project files, sorted.
    pub fn project_names(&self) -> Result<Vec<ProjectName>> {
        let dir = self.projects_dir();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::io(tr!("{} lesen", "Reading {}", dir.display()), e)),
        };
        let mut names = Vec::new();
        for entry in entries {
            let path = entry
                .map_err(|e| Error::io(tr!("{} lesen", "Reading {}", dir.display()), e))?
                .path();
            if path.extension().is_none_or(|ext| ext != "toml") {
                continue;
            }
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let name = ProjectName::new(&stem).map_err(|e| Error::ConfigInvalid {
                path: path.clone(),
                message: tr!(
                    "Dateiname ist kein gültiger Projektname: {e}",
                    "File name is not a valid project name: {e}"
                ),
            })?;
            names.push(name);
        }
        names.sort();
        Ok(names)
    }

    /// The raw project file.
    pub fn project_config(&self, name: &ProjectName) -> Result<ProjectConfig> {
        let file = self.project_file(name);
        let text = read_optional(&file)?.ok_or_else(|| Error::ProjectUnknown(name.to_string()))?;
        parse(&file, &text)
    }

    /// The named project with defaults from `config.toml` applied.
    pub fn project(&self, name: &ProjectName) -> Result<Project> {
        let config = self.project_config(name)?;
        let global = self.global()?;
        self.resolve(name, config, &global)
    }

    /// The project whose repository is `repo_root`, if any.
    ///
    /// A broken project file only fails the lookup if no valid file matches,
    /// because it might be the one that should have matched; two files for the
    /// same repository are an error.
    pub fn find_project(&self, repo_root: &Path) -> Result<Option<Project>> {
        let global = self.global()?;
        let mut found: Option<Project> = None;
        let mut broken: Option<Error> = None;
        for name in self.project_names()? {
            let config = match self.project_config(&name) {
                Ok(config) => config,
                Err(error) => {
                    broken.get_or_insert(error);
                    continue;
                }
            };
            if !same_path(&config.path, repo_root) {
                continue;
            }
            if let Some(first) = &found {
                return Err(Error::Unsupported(tr!(
                    "{} gehört zu zwei Projekten: {} und {}",
                    "{} belongs to two projects: {} and {}",
                    repo_root.display(),
                    first.name,
                    name
                )));
            }
            found = Some(self.resolve(&name, config, &global)?);
        }
        match (found, broken) {
            (Some(project), _) => Ok(Some(project)),
            (None, Some(error)) => Err(error),
            (None, None) => Ok(None),
        }
    }

    /// Registers `repo_root` as a new project and writes its file from the
    /// commented template. The name is derived from the folder name, with a
    /// numeric suffix if another project already uses it.
    pub fn create_project(&self, repo_root: &Path) -> Result<Project> {
        self.create_project_named(repo_root, None)
    }

    /// [`ConfigDir::create_project`] with a name the user chose instead of
    /// the folder's; fails if another project has it.
    pub fn create_project_named(
        &self,
        repo_root: &Path,
        name: Option<&ProjectName>,
    ) -> Result<Project> {
        let path = canonical(repo_root)?;
        if !path.join(".git").exists() {
            return Err(Error::Unsupported(tr!(
                "{} ist kein Git-Repository (kein .git gefunden)",
                "{} is not a Git repository (no .git found)",
                path.display()
            )));
        }
        self.check_separate_from(&path)?;
        if let Some(existing) = self.find_project(&path)? {
            return Err(Error::Unsupported(tr!(
                "{} ist bereits Projekt {}",
                "{} is project {} already",
                path.display(),
                existing.name
            )));
        }
        let global = self.global()?;
        let name = match name {
            Some(name) if self.project_file(name).exists() => {
                return Err(Error::Unsupported(tr!(
                    "Ein Projekt {name} gibt es schon; bitte einen anderen Namen wählen",
                    "A project {name} exists already; please choose another name"
                )));
            }
            Some(name) => name.clone(),
            None => {
                let base = ProjectName::derive(&path);
                std::iter::once(base.clone())
                    .chain((2..100).map(|n| base.with_suffix(n)))
                    .find(|name| !self.project_file(name).exists())
                    .ok_or_else(|| {
                        Error::Unsupported(tr!(
                            "Kein freier Projektname für {base} gefunden",
                            "No free project name found for {base}"
                        ))
                    })?
            }
        };

        let template = PROJECT_TEMPLATE.replace("{name}", name.as_str());
        let mut doc: DocumentMut = template
            .parse()
            .map_err(|e| template_error("project.toml", e))?;
        doc["path"] = toml_edit::value(path.to_string_lossy().into_owned());
        doc["network"]["preset"] = toml_edit::value(global.defaults.network.as_str());
        let file = self.project_file(&name);
        let config: ProjectConfig = parse(&file, &doc.to_string())?;
        fs::create_dir_all(self.projects_dir()).map_err(|e| {
            Error::io(
                tr!("{} anlegen", "Creating {}", self.projects_dir().display()),
                e,
            )
        })?;
        write_atomic(&file, &doc.to_string(), false)?;
        self.resolve(&name, config, &global)
    }

    /// Deletes a project's file; the repository and anything in the VM stay.
    pub fn remove_project(&self, name: &ProjectName) -> Result<()> {
        let file = self.project_file(name);
        if !file.exists() {
            return Err(Error::ProjectUnknown(name.to_string()));
        }
        fs::remove_file(&file)
            .map_err(|e| Error::io(tr!("{} löschen", "Deleting {}", file.display()), e))
    }

    /// Changes a project file in place, keeping comments and formatting. The
    /// result is validated before it is written.
    pub fn update_project(
        &self,
        name: &ProjectName,
        edit: impl FnOnce(&mut DocumentMut) -> Result<()>,
    ) -> Result<Project> {
        let file = self.project_file(name);
        let text = read_optional(&file)?.ok_or_else(|| Error::ProjectUnknown(name.to_string()))?;
        let mut doc: DocumentMut = text.parse().map_err(|e| Error::ConfigInvalid {
            path: file.clone(),
            message: format!("{e}"),
        })?;
        edit(&mut doc)?;
        let config: ProjectConfig = parse(&file, &doc.to_string())?;
        self.check_separate_from(&config.path)?;
        write_atomic(&file, &doc.to_string(), true)?;
        self.resolve(name, config, &self.global()?)
    }

    /// Sets (or with `None` removes) `[server] url` in `config.toml`,
    /// creating the configuration folder if needed. Other keys of
    /// `[server]` (`ca`) stay; the table goes once it is empty.
    pub fn set_server_url(&self, url: Option<&str>) -> Result<GlobalConfig> {
        self.init()?;
        let file = self.config_file();
        let text = read_optional(&file)?.unwrap_or_default();
        let mut doc: DocumentMut = text.parse().map_err(|e| Error::ConfigInvalid {
            path: file.clone(),
            message: format!("{e}"),
        })?;
        match url {
            Some(url) => doc["server"]["url"] = toml_edit::value(url),
            None => {
                let empty = match doc.get_mut("server").and_then(|s| s.as_table_like_mut()) {
                    Some(server) => {
                        server.remove("url");
                        server.is_empty()
                    }
                    None => false,
                };
                if empty {
                    doc.remove("server");
                }
            }
        }
        let global: GlobalConfig = parse(&file, &doc.to_string())?;
        write_atomic(&file, &doc.to_string(), true)?;
        Ok(global)
    }

    /// Sets `network.preset` of a project.
    pub fn set_network_preset(&self, name: &ProjectName, preset: NetworkPreset) -> Result<Project> {
        self.update_project(name, |doc| {
            doc["network"]["preset"] = toml_edit::value(preset.as_str());
            Ok(())
        })
    }

    /// Changes `[remote]` of a project: whether web chats may use it,
    /// whether new chats need a confirmation and whether they may merge
    /// without an approval here. `None` leaves a value as is.
    pub fn set_remote(&self, name: &ProjectName, change: RemoteChange) -> Result<RemoteConfig> {
        self.update_project(name, |doc| {
            for (key, value) in [
                ("enabled", change.enabled),
                ("confirm", change.confirm),
                ("merge", change.merge),
            ] {
                if let Some(value) = value {
                    doc["remote"][key] = toml_edit::value(value);
                }
            }
            Ok(())
        })?;
        Ok(self.project_config(name)?.remote)
    }

    /// Changes `[host]` of a project (the direct mode).
    pub fn set_host(&self, name: &ProjectName, change: HostChange) -> Result<HostConfig> {
        self.update_project(name, |doc| {
            for (key, value) in [
                ("enabled", change.enabled),
                ("browser_approvals", change.browser_approvals),
            ] {
                if let Some(value) = value {
                    doc["host"][key] = toml_edit::value(value);
                }
            }
            if let Some(writes) = change.writes {
                doc["host"]["writes"] = toml_edit::value(match writes {
                    HostWrites::Free => "free",
                    HostWrites::Ask => "ask",
                });
            }
            Ok(())
        })?;
        Ok(self.project_config(name)?.host)
    }

    /// Adds a chat confirmed on this computer to `remote.chats`.
    pub fn remember_remote_chat(&self, name: &ProjectName, chat: &str) -> Result<()> {
        if !valid_chat_id(chat) {
            return Err(Error::invalid(
                tr("Chat-ID", "chat ID"),
                chat,
                tr(
                    "nur Buchstaben, Ziffern und '-'",
                    "only letters, digits and '-'",
                ),
            ));
        }
        if self
            .project_config(name)?
            .remote
            .chats
            .iter()
            .any(|c| c == chat)
        {
            return Ok(());
        }
        let file = self.project_file(name);
        self.update_project(name, |doc| {
            let item = &mut doc["remote"]["chats"];
            if item.is_none() {
                *item = toml_edit::value(toml_edit::Array::new());
            }
            let array = item.as_array_mut().ok_or_else(|| Error::ConfigInvalid {
                path: file.clone(),
                message: tr("remote.chats ist keine Liste", "remote.chats is not a list")
                    .to_owned(),
            })?;
            array.push(chat);
            Ok(())
        })?;
        Ok(())
    }

    /// Adds a domain to `network.allow` or `network.deny` of a project.
    /// Returns the project and whether the file changed (`false` if the
    /// pattern was already listed).
    pub fn add_network_domain(
        &self,
        name: &ProjectName,
        list: DomainList,
        pattern: &DomainPattern,
    ) -> Result<(Project, bool)> {
        let config = self.project_config(name)?;
        let present = match list {
            DomainList::Allow => &config.network.allow,
            DomainList::Deny => &config.network.deny,
        };
        if present.contains(pattern) {
            return Ok((self.resolve(name, config, &self.global()?)?, false));
        }
        let key = list.key();
        let file = self.project_file(name);
        let project = self.update_project(name, |doc| {
            let item = &mut doc["network"][key];
            if item.is_none() {
                *item = toml_edit::value(toml_edit::Array::new());
            }
            let array = item.as_array_mut().ok_or_else(|| Error::ConfigInvalid {
                path: file.clone(),
                message: tr!(
                    "network.{key} ist keine Liste",
                    "network.{key} is not a list"
                ),
            })?;
            array.push(pattern.to_string());
            Ok(())
        })?;
        Ok((project, true))
    }

    /// The named harness: `harnesses/<name>.toml`, else the built-in one.
    pub fn harness(&self, name: &str) -> Result<HarnessConfig> {
        validate_harness_name(name).map_err(|message| Error::ConfigInvalid {
            path: self.harnesses_dir(),
            message,
        })?;
        let file = self.harnesses_dir().join(format!("{name}.toml"));
        let text = match read_optional(&file)? {
            Some(text) => text,
            None => BUILTIN_HARNESSES
                .iter()
                .find(|(builtin, _)| *builtin == name)
                .map(|(_, text)| (*text).to_owned())
                .ok_or_else(|| Error::HarnessUnknown(name.to_owned()))?,
        };
        let harness: HarnessConfig = parse(&file, &text)?;
        if harness.name != name {
            return Err(Error::ConfigInvalid {
                path: file,
                message: tr!(
                    "name = {:?} passt nicht zum Dateinamen {name}.toml",
                    "name = {:?} does not match the file name {name}.toml",
                    harness.name
                ),
            });
        }
        Ok(harness)
    }

    /// The named app: `apps/<name>.toml`, else the catalog's, else the
    /// account's template. What exists on this computer wins, so a template
    /// from the browser can never replace a recipe of the catalog or a file.
    pub fn app(&self, name: &str) -> Result<AppConfig> {
        validate_app_name(name).map_err(|message| Error::ConfigInvalid {
            path: self.apps_dir(),
            message,
        })?;
        let file = self.apps_dir().join(format!("{name}.toml"));
        let (text, source) = match read_optional(&file)? {
            Some(text) => (text, AppSource::Local),
            None => match BUILTIN_APPS.iter().find(|(builtin, _)| *builtin == name) {
                Some((_, text)) => ((*text).to_owned(), AppSource::Builtin),
                None => {
                    let template = self
                        .templates()
                        .into_iter()
                        .find(|template| template.name == name)
                        .ok_or_else(|| Error::AppUnknown(name.to_owned()))?;
                    return template.to_app().map_err(|message| Error::ConfigInvalid {
                        path: self.templates_file(),
                        message: tr!("Vorlage {name}: {message}", "Template {name}: {message}"),
                    });
                }
            },
        };
        let mut app: AppConfig = parse(&file, &text)?;
        app.source = source;
        if let Some(program) = &mut app.file {
            program.host = self.app_files_dir().join(name).join(&program.name);
        }
        if app.name != name {
            return Err(Error::ConfigInvalid {
                path: file,
                message: tr!(
                    "name = {:?} passt nicht zum Dateinamen {name}.toml",
                    "name = {:?} does not match the file name {name}.toml",
                    app.name
                ),
            });
        }
        Ok(app)
    }

    /// All apps: the catalog in its order, then the files in `apps/` that
    /// add apps, by name, then the account's templates of other names.
    pub fn apps(&self) -> Result<Vec<AppConfig>> {
        self.app_names()?
            .iter()
            .map(|name| self.app(name))
            .collect()
    }

    /// Names of [`ConfigDir::apps`], without reading the recipes.
    pub fn app_names(&self) -> Result<Vec<String>> {
        let mut names: Vec<String> = BUILTIN_APPS
            .iter()
            .map(|(name, _)| (*name).to_owned())
            .collect();
        let dir = self.apps_dir();
        let mut own = Vec::new();
        match fs::read_dir(&dir) {
            Ok(entries) => {
                for entry in entries {
                    let path = entry
                        .map_err(|e| Error::io(tr!("{} lesen", "Reading {}", dir.display()), e))?
                        .path();
                    if path.extension().is_some_and(|ext| ext == "toml")
                        && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                        && !names.iter().any(|name| name == stem)
                    {
                        own.push(stem.to_owned());
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::io(tr!("{} lesen", "Reading {}", dir.display()), e)),
        }
        own.sort();
        names.extend(own);
        let mut templates: Vec<String> = self
            .templates()
            .into_iter()
            .map(|template| template.name)
            .filter(|name| !names.contains(name))
            .collect();
        templates.sort();
        templates.dedup();
        names.extend(templates);
        Ok(names)
    }

    /// The apps a project gets: the ones it switched on and what they
    /// require, each once, required ones first.
    pub fn project_apps(&self, project: &Project) -> Result<Vec<AppConfig>> {
        self.resolve_apps(&project.apps)
    }

    /// `names` with the apps they require, each once, required ones first.
    pub fn resolve_apps(&self, names: &[String]) -> Result<Vec<AppConfig>> {
        let mut resolved = Vec::new();
        for name in names {
            self.add_app(name, &mut resolved, &mut Vec::new())?;
        }
        Ok(resolved)
    }

    fn add_app(
        &self,
        name: &str,
        resolved: &mut Vec<AppConfig>,
        chain: &mut Vec<String>,
    ) -> Result<()> {
        if resolved.iter().any(|app| app.name == name) {
            return Ok(());
        }
        if chain.iter().any(|link| link == name) {
            return Err(Error::Unsupported(tr!(
                "Apps verlangen sich gegenseitig: {} → {name}",
                "Apps require each other: {} → {name}",
                chain.join(" → ")
            )));
        }
        let app = self.app(name)?;
        chain.push(name.to_owned());
        for required in &app.requires {
            self.add_app(required, resolved, chain)?;
        }
        chain.pop();
        resolved.push(app);
        Ok(())
    }

    /// Switches apps of a project on and off (`[apps] enabled`). Apps
    /// switched on must exist; off works for any name, also for an app whose
    /// file is gone.
    pub fn set_apps(&self, name: &ProjectName, on: &[String], off: &[String]) -> Result<Project> {
        for app in on {
            self.app(app)?;
        }
        let file = self.project_file(name);
        self.update_project(name, |doc| {
            let item = &mut doc["apps"]["enabled"];
            if item.is_none() {
                // Without the key the default applies; it must not get lost.
                *item = toml_edit::value(
                    AppsConfig::default()
                        .enabled
                        .iter()
                        .collect::<toml_edit::Array>(),
                );
            }
            let array = item.as_array_mut().ok_or_else(|| Error::ConfigInvalid {
                path: file.clone(),
                message: tr("apps.enabled ist keine Liste", "apps.enabled is not a list")
                    .to_owned(),
            })?;
            array.retain(|value| !off.iter().any(|app| value.as_str() == Some(app)));
            for app in on {
                if !array.iter().any(|value| value.as_str() == Some(app)) {
                    array.push(app.as_str());
                }
            }
            Ok(())
        })
    }

    /// Refuses a project that contains this folder or lies inside it.
    fn check_separate_from(&self, project_path: &Path) -> Result<()> {
        if is_within(&self.root, project_path) || is_within(project_path, &self.root) {
            return Err(Error::Unsupported(tr!(
                "{} überschneidet sich mit dem Konfigurationsordner {}. Ein Agent könnte sonst \
                 über seine Commits die eigenen Regeln ändern",
                "{} overlaps with the configuration folder {}. An agent could otherwise change \
                 its own rules through its commits",
                project_path.display(),
                self.root.display()
            )));
        }
        Ok(())
    }

    fn resolve(
        &self,
        name: &ProjectName,
        config: ProjectConfig,
        global: &GlobalConfig,
    ) -> Result<Project> {
        // Also checked on every load, not just on create: the file may have
        // been edited by hand.
        self.check_separate_from(&config.path)?;
        let defaults = &global.defaults;
        Ok(Project {
            name: name.clone(),
            file: self.project_file(name),
            harness: config
                .default_harness
                .unwrap_or_else(|| defaults.harness.clone()),
            mode: config.files.mode,
            network: config.network.preset.unwrap_or(defaults.network),
            ask: config.network.ask,
            allow: config.network.allow,
            deny: config.network.deny,
            llm_apis: global.llm_apis.clone(),
            resources: Resources {
                cpus: config.resources.cpus.or(defaults.resources.cpus),
                memory: config
                    .resources
                    .memory
                    .or_else(|| defaults.resources.memory.clone()),
                ..Resources::default()
            },
            apps: config.apps.enabled,
            path: config.path,
        })
    }
}

/// Checks a recipe built in code like one read from a file.
pub(crate) fn check_app(app: &AppConfig) -> std::result::Result<(), String> {
    app.validate()
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io(tr!("{} lesen", "Reading {}", path.display()), e)),
    }
}

fn parse<T: DeserializeOwned + Validate>(path: &Path, text: &str) -> Result<T> {
    let invalid = |message: String| Error::ConfigInvalid {
        path: path.to_owned(),
        message,
    };
    let value: T = toml::from_str(text).map_err(|e| invalid(e.to_string()))?;
    value.validate().map_err(invalid)?;
    Ok(value)
}

/// Writes via a temporary file and rename, so readers never see half a file.
/// Without `replace`, an existing file is an error (no silent overwrite when
/// two processes create the same project).
fn write_atomic(path: &Path, text: &str, replace: bool) -> Result<()> {
    if !replace && path.exists() {
        return Err(Error::Unsupported(tr!(
            "{} existiert bereits",
            "{} exists already",
            path.display()
        )));
    }
    let mut partial = path.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    fs::write(&partial, text)
        .map_err(|e| Error::io(tr!("{} schreiben", "Writing {}", partial.display()), e))?;
    fs::rename(&partial, path)
        .map_err(|e| Error::io(tr!("{} schreiben", "Writing {}", path.display()), e))
}

fn template_error(name: &str, error: toml_edit::TomlError) -> Error {
    Error::Unsupported(tr!(
        "Eingebaute Vorlage {name} ist ungültig: {error}",
        "Built-in template {name} is invalid: {error}"
    ))
}

#[cfg(test)]
mod tests;
