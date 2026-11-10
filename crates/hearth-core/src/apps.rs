//! Programs of the app catalog in the VM (ROADMAP Phase 3, "App-Katalog").
//!
//! Each install of an app is a volume of its own, `hearth-app-<name>-<id>`.
//! The installer writes it at `/opt/hearth-apps/<name>`, and the instances
//! of projects that switched the app on mount it read-only at the same path,
//! so absolute paths the install wrote stay valid. One install serves every
//! project and survives rebuilds of the base image; switching an app on or
//! off only recreates the instance.
//!
//! Podman cannot change a volume's labels after creating it. A finished
//! install is therefore recorded by a second, empty volume,
//! `hearth-app-<name>-<id>-ready`, carrying the version: an install that
//! broke off (error, Ctrl+C, VM stopped) has none and is never used.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub use crate::config::APPS_DIR;
use crate::config::{AppConfig, DomainPattern, Project};
use crate::gateway::{self, GatewayControl, Mode, Scope};
use crate::image::{BASE_IMAGE, fingerprint};
use crate::provider::{
    ContainerSpec, ContainerStatus, ExecSpec, Labels, Mount, MountSource, Provider, VolumeInfo,
    VolumeSpec,
};
use crate::sync::sanitize;
use crate::{Error, PROJECT_LABEL, Result, tr};

/// Helpers every recipe gets, run before its `install`.
const LIB: &str = include_str!("../defaults/apps/lib.sh");

/// `PATH` of the base image (`ENV` in its Containerfile, checked by a test).
/// hearth's tools come first, so no app can shadow the harnesses.
const IMAGE_PATH: &str = "/opt/hearth-tools/bin:/home/agent/.local/bin:/usr/local/sbin:\
                          /usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

const APP_LABEL: &str = "io.hearth.app";
const INSTALL_LABEL: &str = "io.hearth.app-install";
const RECIPE_LABEL: &str = "io.hearth.app-recipe";
const VERSION_LABEL: &str = "io.hearth.app-version";

/// Name suffix of the receipt volume of a finished install.
const READY_SUFFIX: &str = "-ready";

/// Where `hearth_version` (lib.sh) writes the installed version.
const VERSION_FILE: &str = "/tmp/hearth-app-version";

/// Seconds the installer may use the recipe's hosts. Installs such as Rust
/// take minutes on a slow line.
const INSTALL_GRANT_SECS: u64 = 1800;

/// Fingerprint of the recipe's script. Path and environment are read anew
/// for each instance, and the helpers in lib.sh do not count either: a fix
/// there would mark every app as outdated and suggest downloading them all
/// again. A recipe whose installs should be redone gets its text changed.
pub fn recipe_hash(app: &AppConfig) -> String {
    fingerprint([app.install.as_str()])
}

/// A finished install of an app in the VM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    /// App name.
    pub app: String,
    /// Start of the install in seconds since 1970, hexadecimal; later
    /// installs sort higher.
    pub id: String,
    /// Volume holding it.
    pub volume: String,
    /// Version the recipe reported, `?` if none.
    pub version: String,
    /// [`recipe_hash`] of the recipe that installed it.
    pub recipe: String,
}

impl Install {
    /// When the install started, in seconds since 1970.
    pub fn started(&self) -> Option<u64> {
        u64::from_str_radix(&self.id, 16).ok()
    }

    /// Whether the app's recipe changed since; `hearth app update` then
    /// installs it anew.
    pub fn outdated(&self, app: &AppConfig) -> bool {
        self.recipe != recipe_hash(app)
    }
}

/// An app ready for an instance: its definition and the install to mount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadyApp {
    /// The definition, for path and environment.
    pub app: AppConfig,
    /// The install the instance mounts.
    pub install: Install,
}

/// What the VM holds of apps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    /// Finished installs.
    pub ready: Vec<Install>,
    /// Volumes of installs that never finished (or still run), and receipts
    /// whose install is gone.
    pub leftovers: Vec<String>,
}

impl Inventory {
    /// Reads the app volumes of the VM.
    pub fn load(provider: &dyn Provider) -> Result<Self> {
        Ok(Self::from_volumes(&provider.volumes()?))
    }

    fn from_volumes(volumes: &[VolumeInfo]) -> Self {
        let ours: Vec<&VolumeInfo> = volumes
            .iter()
            .filter(|volume| volume.labels.contains_key(APP_LABEL))
            .collect();
        let mut inventory = Inventory::default();
        for volume in &ours {
            let label = |key: &str| volume.labels.get(key).cloned().unwrap_or_default();
            match volume.name.strip_suffix(READY_SUFFIX) {
                Some(data) if ours.iter().any(|other| other.name == data) => {
                    inventory.ready.push(Install {
                        app: label(APP_LABEL),
                        id: label(INSTALL_LABEL),
                        volume: data.to_owned(),
                        version: label(VERSION_LABEL),
                        recipe: label(RECIPE_LABEL),
                    });
                }
                Some(_) => inventory.leftovers.push(volume.name.clone()),
                None => {
                    let receipt = format!("{}{READY_SUFFIX}", volume.name);
                    if !ours.iter().any(|other| other.name == receipt) {
                        inventory.leftovers.push(volume.name.clone());
                    }
                }
            }
        }
        inventory
            .ready
            .sort_by(|a, b| (&a.app, &a.id).cmp(&(&b.app, &b.id)));
        inventory.leftovers.sort();
        inventory
    }

    /// The newest finished install of `app`, the one instances get.
    pub fn latest(&self, app: &str) -> Option<&Install> {
        self.ready.iter().rfind(|install| install.app == app)
    }

    /// Finished installs older than the newest of their app.
    pub fn superseded(&self) -> Vec<&Install> {
        self.ready
            .iter()
            .filter(|install| self.latest(&install.app) != Some(install))
            .collect()
    }
}

/// Removes an install. Fails while a container still mounts it; the
/// receipt only goes after the data, so nothing half-removed counts as an
/// install.
pub fn remove(provider: &dyn Provider, install: &Install) -> Result<()> {
    provider.remove_volume(&install.volume)?;
    provider.remove_volume(&format!("{}{READY_SUFFIX}", install.volume))
}

/// Installs `app` into a new volume, through the project's network and
/// gateway; `requires` are the installs of the apps it requires, mounted
/// for the install. The recipe's output goes to this process's stdout.
/// Older installs of the app are removed unless a container uses them.
pub fn install(
    provider: &dyn Provider,
    project: &Project,
    app: &AppConfig,
    requires: &[ReadyApp],
) -> Result<Install> {
    let granted = grant_hosts(provider, project, app)?;
    let installed = install_granted(provider, project, app, requires);
    // The hosts were for the installer: the agent shares the project's
    // gateway and must not keep them (security review M7). A failure here
    // leaves them until their time runs out, as before.
    let control = GatewayControl::new(provider, &project.name);
    for host in &granted {
        let _ = control.revoke(host);
    }
    installed
}

fn install_granted(
    provider: &dyn Provider,
    project: &Project,
    app: &AppConfig,
    requires: &[ReadyApp],
) -> Result<Install> {
    // A fresh volume every time: two installs within one second must not
    // share one, so the id moves on past any volume that exists.
    let taken: Vec<String> = provider
        .volumes()?
        .into_iter()
        .map(|volume| volume.name)
        .collect();
    let mut seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (id, volume) = loop {
        let id = format!("{seconds:08x}");
        let volume = format!("hearth-app-{}-{id}", app.name);
        if !taken.iter().any(|name| name.starts_with(&volume)) {
            break (id, volume);
        }
        seconds += 1;
    };
    let recipe = recipe_hash(app);
    let mut labels: Labels = [
        (APP_LABEL.to_owned(), app.name.clone()),
        (INSTALL_LABEL.to_owned(), id.clone()),
        (RECIPE_LABEL.to_owned(), recipe.clone()),
    ]
    .into();
    provider.ensure_volume(&VolumeSpec {
        name: volume.clone(),
        labels: labels.clone(),
    })?;
    let version = match run_installer(provider, project, app, &volume, requires) {
        Ok(version) => version,
        Err(error) => {
            // Failing is harmless: without a receipt it is never used, and
            // `hearth vmd clean` gets it later.
            let _ = provider.remove_volume(&volume);
            return Err(error);
        }
    };
    labels.insert(VERSION_LABEL.to_owned(), version.clone());
    provider.ensure_volume(&VolumeSpec {
        name: format!("{volume}{READY_SUFFIX}"),
        labels,
    })?;
    if let Ok(inventory) = Inventory::load(provider) {
        for old in inventory
            .superseded()
            .into_iter()
            .filter(|old| old.app == app.name)
        {
            // Still mounted by an instance until it is recreated with the
            // new install; `hearth vmd clean` gets it then.
            let _ = remove(provider, old);
        }
    }
    Ok(Install {
        app: app.name.clone(),
        id,
        volume,
        version,
        recipe,
    })
}

/// Grants the recipe's hosts for a while unless the project allows them
/// anyway (like the npm registry for installing a harness). Returns the
/// hosts it granted, to take back once the install is done.
fn grant_hosts(
    provider: &dyn Provider,
    project: &Project,
    app: &AppConfig,
) -> Result<Vec<DomainPattern>> {
    let policy = gateway::policy(project);
    if policy.mode == Mode::Open {
        return Ok(Vec::new());
    }
    let control = GatewayControl::new(provider, &project.name);
    let mut granted = Vec::new();
    for host in &app.hosts {
        if !policy.allow.contains(host) {
            control.allow(host, Scope::For(INSTALL_GRANT_SECS))?;
            granted.push(host.clone());
        }
    }
    Ok(granted)
}

/// Name of the container that installs `app`; one install per app at a time.
fn installer_name(app: &AppConfig) -> String {
    format!("hearth-app-install-{}", app.name)
}

/// Runs the recipe in a short-lived container that is hardened like an
/// instance but may write the new volume. Returns the reported version.
fn run_installer(
    provider: &dyn Provider,
    project: &Project,
    app: &AppConfig,
    volume: &str,
    requires: &[ReadyApp],
) -> Result<String> {
    let container = installer_name(app);
    match provider.container(&container)? {
        Some(info) if info.status == ContainerStatus::Running => {
            return Err(Error::Unsupported(tr!(
                "{} wird gerade schon installiert (Container {container}); bitte warten und \
                 dann erneut starten",
                "{} is being installed already (container {container}); please wait and \
                 then start again",
                app.title
            )));
        }
        Some(_) => provider.remove_container(&container)?,
        None => {}
    }
    let mut mounts = vec![Mount {
        source: MountSource::Volume(volume.to_owned()),
        target: app.dir(),
        read_only: false,
    }];
    mounts.extend(app_mounts(requires));
    let mut env = environment(requires.iter().map(|ready| &ready.app))?;
    let work = format!("{}/.hearth-work", app.dir());
    env.extend(
        [
            ("PREFIX", app.dir()),
            ("WORK", work.clone()),
            ("HEARTH_VERSION_FILE", VERSION_FILE.to_owned()),
            ("HOME", "/tmp".to_owned()),
        ]
        .map(|(key, value)| (key.to_owned(), value)),
    );
    // The approved copy of a program file goes in before the script runs.
    let program = app
        .file
        .as_ref()
        .map(|file| (file, format!("{work}/{}", file.name)));
    if let Some((_, target)) = &program {
        env.insert("HEARTH_FILE".to_owned(), target.clone());
    }
    provider.create_container(&ContainerSpec {
        name: container.clone(),
        image: BASE_IMAGE.tag.to_owned(),
        network: project.name.network(),
        mounts,
        env,
        labels: [(PROJECT_LABEL.to_owned(), project.name.to_string())].into(),
        workdir: Some("/tmp".to_owned()),
        ..ContainerSpec::default()
    })?;
    let exec = |command: Vec<String>| ExecSpec {
        container: container.clone(),
        command,
        ..ExecSpec::default()
    };
    let result = provider.start_container(&container).and_then(|()| {
        if let Some((file, target)) = &program {
            if !file.host.is_file() {
                return Err(Error::Unsupported(tr!(
                    "Die freigegebene Datei von {} fehlt ({}); bitte mit hearth app add neu \
                     freigeben",
                    "The approved file of {} is missing ({}); please approve it again with \
                     hearth app add",
                    app.title,
                    file.host.display()
                )));
            }
            let mkdir = vec!["mkdir".into(), "-p".into(), work.clone()];
            provider.exec(&exec(mkdir))?;
            provider.copy_into(&container, &file.host, target)?;
        }
        let script = format!("{LIB}\n{}\n", app.install);
        let code = provider.exec(&exec(vec!["/bin/sh".into(), "-c".into(), script]))?;
        if code != 0 {
            return Err(Error::Unsupported(tr!(
                "Installation von {} fehlgeschlagen (Exit-Code {code})",
                "Installation of {} failed (exit code {code})",
                app.title
            )));
        }
        let finish = r#"rm -rf "$WORK"; cat "$HEARTH_VERSION_FILE" 2>/dev/null || true"#;
        let output =
            provider.exec_output(&exec(vec!["/bin/sh".into(), "-c".into(), finish.into()]))?;
        Ok(clean_version(&output.stdout))
    });
    let removed = provider.remove_container(&container);
    let version = result?;
    removed.map(|()| version)
}

/// The version as a label value and for display: it comes from whatever
/// the recipe downloaded, so only a short, plain word passes.
fn clean_version(raw: &str) -> String {
    let version = sanitize(raw.lines().next().unwrap_or_default().trim());
    let plain = !version.is_empty()
        && version.len() <= 40
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'));
    if plain { version } else { "?".to_owned() }
}

/// Read-only mounts of these apps at their folders.
pub fn app_mounts(apps: &[ReadyApp]) -> Vec<Mount> {
    apps.iter()
        .map(|ready| Mount {
            source: MountSource::Volume(ready.install.volume.clone()),
            target: ready.app.dir(),
            read_only: true,
        })
        .collect()
}

/// What an instance's environment gets for these apps: `PATH` with their
/// folders after hearth's tools, `HEARTH_APPS_PATH` with the same folders
/// for login shells (`/etc/profile` resets `PATH`), and their variables.
/// Empty without apps, so their instances keep the image's environment.
pub fn environment<'a>(
    apps: impl IntoIterator<Item = &'a AppConfig>,
) -> Result<BTreeMap<String, String>> {
    let mut env = BTreeMap::new();
    let mut owners: BTreeMap<String, &str> = BTreeMap::new();
    let mut dirs: Vec<String> = Vec::new();
    for app in apps {
        for dir in app.path_dirs() {
            if !dirs.contains(&dir) {
                dirs.push(dir);
            }
        }
        for (key, value) in app.env_vars() {
            if let Some(existing) = env.get(&key)
                && *existing != value
            {
                return Err(Error::Unsupported(tr!(
                    "{} und {} setzen {key} verschieden; eine von beiden ausschalten",
                    "{} and {} set {key} differently; switch one of them off",
                    owners[&key],
                    app.title
                )));
            }
            owners.insert(key.clone(), &app.title);
            env.insert(key, value);
        }
    }
    if !dirs.is_empty() {
        let joined = dirs.join(":");
        let (tools, rest) = IMAGE_PATH
            .split_once(':')
            .expect("the image's PATH has several entries");
        env.insert("PATH".to_owned(), format!("{tools}:{joined}:{rest}"));
        env.insert("HEARTH_APPS_PATH".to_owned(), joined);
    }
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, ConfigDir};

    fn app(name: &str) -> AppConfig {
        ConfigDir::new(std::env::temp_dir().join("hearth-apps-test-none"))
            .app(name)
            .unwrap()
    }

    fn volume(name: &str, labels: &[(&str, &str)]) -> VolumeInfo {
        VolumeInfo {
            name: name.to_owned(),
            labels: labels
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn image_path_matches_the_containerfile() {
        let containerfile = include_str!("../../../images/base/Containerfile");
        let path = containerfile
            .lines()
            .find_map(|line| line.trim().strip_prefix("PATH="))
            .unwrap()
            .trim_end_matches(" \\");
        assert_eq!(path, IMAGE_PATH);
        let profile = include_str!("../../../images/base/profile.sh");
        assert!(profile.contains("HEARTH_APPS_PATH"));
    }

    #[test]
    fn inventory_pairs_installs_with_receipts() {
        let labels = |app: &'static str, id: &'static str| {
            vec![
                (APP_LABEL, app),
                (INSTALL_LABEL, id),
                (RECIPE_LABEL, "r1"),
                (VERSION_LABEL, "1.0"),
            ]
        };
        let volumes = [
            volume("hearth-ws-demo", &[]),
            volume("hearth-app-go-6a000000", &labels("go", "6a000000")),
            volume("hearth-app-go-6a000000-ready", &labels("go", "6a000000")),
            volume("hearth-app-go-6b000000", &labels("go", "6b000000")),
            volume("hearth-app-go-6b000000-ready", &labels("go", "6b000000")),
            // broke off: no receipt
            volume("hearth-app-rust-6c000000", &labels("rust", "6c000000")),
            // receipt whose install is gone
            volume("hearth-app-c-6a000000-ready", &labels("c", "6a000000")),
        ];
        let inventory = Inventory::from_volumes(&volumes);
        assert_eq!(inventory.ready.len(), 2);
        assert_eq!(
            inventory.latest("go").unwrap().volume,
            "hearth-app-go-6b000000"
        );
        assert_eq!(inventory.latest("go").unwrap().version, "1.0");
        assert!(inventory.latest("rust").is_none());
        assert!(inventory.latest("c").is_none());
        let old: Vec<_> = inventory.superseded().iter().map(|i| &i.volume).collect();
        assert_eq!(old, ["hearth-app-go-6a000000"]);
        assert_eq!(
            inventory.leftovers,
            ["hearth-app-c-6a000000-ready", "hearth-app-rust-6c000000"]
        );
    }

    #[test]
    fn environment_of_apps() {
        assert!(environment([]).unwrap().is_empty());
        let rust = app("rust");
        let c = app("c");
        let env = environment([&c, &rust]).unwrap();
        assert_eq!(
            env["PATH"],
            "/opt/hearth-tools/bin:/opt/hearth-apps/c/bin:/opt/hearth-apps/rust/cargo/bin:\
             /home/agent/.cargo/bin:/home/agent/.local/bin:/usr/local/sbin:/usr/local/bin:\
             /usr/sbin:/usr/bin:/sbin:/bin"
        );
        assert_eq!(
            env["HEARTH_APPS_PATH"],
            "/opt/hearth-apps/c/bin:/opt/hearth-apps/rust/cargo/bin:/home/agent/.cargo/bin"
        );
        assert_eq!(env["RUSTUP_HOME"], "/opt/hearth-apps/rust/rustup");
        assert_eq!(env["CARGO_HOME"], "/home/agent/.cargo");

        let mut other = app("java");
        other.title = "Anderes Java".into();
        other.env.insert("JAVA_HOME".into(), "/elsewhere".into());
        let error = environment([&app("java"), &other]).unwrap_err().to_string();
        assert!(error.contains("JAVA_HOME"), "{error}");
    }

    #[test]
    fn versions_are_plain_words() {
        assert_eq!(clean_version("1.99.1\n"), "1.99.1");
        assert_eq!(clean_version("14.2.0-19+deb13u1"), "14.2.0-19+deb13u1");
        assert_eq!(clean_version(""), "?");
        assert_eq!(clean_version("1.0 \u{1b}[31mred"), "?");
        assert_eq!(clean_version(&"9".repeat(41)), "?");
    }

    #[test]
    fn recipe_hash_follows_the_script() {
        let go = app("go");
        let mut changed = go.clone();
        assert_eq!(recipe_hash(&go), recipe_hash(&changed));
        changed.path.push("extra".into());
        assert_eq!(recipe_hash(&go), recipe_hash(&changed));
        changed.install.push_str("\ntrue");
        assert_ne!(recipe_hash(&go), recipe_hash(&changed));
    }
}
