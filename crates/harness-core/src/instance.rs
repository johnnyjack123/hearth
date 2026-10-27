//! The container an agent works in (ARCHITECTURE.md §3.3, isolated mode):
//! one per project, only in the project's internal network, with
//!
//! - `/workspace`: the agent's clone (volume per project)
//! - `/home/agent`: caches and history (volume per project)
//! - `/opt/harness-tools`: installed harnesses (shared volume, read-only)
//! - `/opt/harness-apps/<app>`: the apps the project switched on (one
//!   volume per install, read-only, see [`crate::apps`])
//! - the harness's persisted path, e.g. `~/.pi` (volume per harness)

use crate::apps::{self, ReadyApp};
use crate::config::{AGENT_HOME, HarnessConfig, Project};
use crate::gateway::{self, GatewayControl, Mode, Scope};
use crate::image::{BASE_IMAGE, CONTEXT_HASH_LABEL, ImageStatus, fingerprint};
use crate::project::ProjectName;
use crate::provider::{
    ContainerSpec, ContainerStatus, ExecSpec, Labels, Mount, MountSource, Provider, VolumeSpec,
};
use crate::sync::WORKSPACE;
use crate::{Error, PROJECT_LABEL, Result};

/// Volume with the installed harnesses, shared by all projects.
pub const TOOLS_VOLUME: &str = "harness-tools";

/// Where the tools volume is mounted; its `bin` is on the base image's PATH.
pub const TOOLS_DIR: &str = "/opt/harness-tools";

/// Records the mounts an instance was created with, so a change (e.g. a
/// further harness) recreates it instead of silently missing a volume.
const MOUNTS_LABEL: &str = "io.llm-harness.mounts";

/// Fingerprint of the environment the apps set, so a change (another
/// version folder, a new variable) recreates the instance. Missing on
/// instances without apps.
const ENV_LABEL: &str = "io.llm-harness.env";

/// The registry harnesses are installed from; granted to the installer for
/// a while if the project's rules do not allow it anyway.
const INSTALL_HOST: &str = "registry.npmjs.org";

/// Seconds the installer may use [`INSTALL_HOST`] in restrictive presets.
const INSTALL_GRANT_SECS: u64 = 900;

/// Whether a project's instance exists and runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceState {
    /// No container.
    Missing,
    /// Created or stopped.
    Stopped,
    /// Running.
    Running,
}

/// State of the project's instance.
pub fn state(provider: &dyn Provider, name: &ProjectName) -> Result<InstanceState> {
    Ok(match provider.container(&name.instance())? {
        None => InstanceState::Missing,
        Some(info) if info.status == ContainerStatus::Running => InstanceState::Running,
        Some(_) => InstanceState::Stopped,
    })
}

/// The mounts of an instance with these harnesses and apps.
pub fn mounts(name: &ProjectName, harnesses: &[&HarnessConfig], apps: &[ReadyApp]) -> Vec<Mount> {
    let mount = |source, target: &str, read_only| Mount {
        source,
        target: target.to_owned(),
        read_only,
    };
    let mut mounts = vec![
        mount(
            MountSource::Volume(name.workspace_volume()),
            WORKSPACE,
            false,
        ),
        mount(MountSource::Volume(name.home_volume()), AGENT_HOME, false),
        // Instances only run harnesses; installing them is the installer's job.
        mount(
            MountSource::Volume(TOOLS_VOLUME.to_owned()),
            TOOLS_DIR,
            true,
        ),
    ];
    mounts.extend(apps::app_mounts(apps));
    for harness in harnesses {
        if let Some(path) = harness.persist_paths().into_iter().next()
            && !mounts.iter().any(|m| m.target == path)
        {
            mounts.push(mount(
                MountSource::Volume(harness.config_volume()),
                &path,
                false,
            ));
        }
    }
    mounts
}

fn describe(mounts: &[Mount]) -> String {
    mounts
        .iter()
        .map(|m| {
            let MountSource::Volume(source) = &m.source;
            let mode = if m.read_only { ":ro" } else { "" };
            format!("{source}:{}{mode}", m.target)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Starts the project's instance, creating volumes and container as needed.
/// The container is recreated when the base image, the mounts or the apps'
/// environment changed; all data lives in volumes, the root filesystem is
/// read-only anyway. Returns whether the container was (re)created.
///
/// The project's network must exist, i.e. [`gateway::ensure_running`] runs
/// first, and `apps` must be installed.
pub fn ensure_running(
    provider: &dyn Provider,
    project: &Project,
    harnesses: &[&HarnessConfig],
    apps: &[ReadyApp],
) -> Result<bool> {
    match BASE_IMAGE.status(provider)? {
        ImageStatus::Current => {}
        ImageStatus::Missing => {
            return Err(Error::Unsupported(format!(
                "Base-Image {} fehlt → harness image build",
                BASE_IMAGE.tag
            )));
        }
        ImageStatus::Outdated => {
            return Err(Error::Unsupported(format!(
                "Base-Image {} ist veraltet → harness image build base",
                BASE_IMAGE.tag
            )));
        }
    }
    let name = &project.name;
    let mounts = mounts(name, harnesses, apps);
    let description = describe(&mounts);
    let hash = BASE_IMAGE.context_hash();
    let container = name.instance();
    let env = apps::environment(apps.iter().map(|ready| &ready.app))?;
    let env_hash = (!env.is_empty()).then(|| {
        fingerprint(
            env.iter()
                .flat_map(|(key, value)| [key.as_str(), value.as_str()]),
        )
    });

    let current = provider.container(&container)?;
    let fits = current.as_ref().is_some_and(|info| {
        info.labels.get(CONTEXT_HASH_LABEL) == Some(&hash)
            && info.labels.get(MOUNTS_LABEL) == Some(&description)
            && info.labels.get(ENV_LABEL) == env_hash.as_ref()
    });
    if fits {
        if current.is_some_and(|info| info.status != ContainerStatus::Running) {
            provider.start_container(&container)?;
        }
        return Ok(false);
    }
    if current.is_some() {
        provider.remove_container(&container)?;
    }
    for mount in &mounts {
        let MountSource::Volume(volume) = &mount.source;
        provider.ensure_volume(&VolumeSpec {
            name: volume.clone(),
            ..VolumeSpec::default()
        })?;
    }
    let mut labels: Labels = [
        (PROJECT_LABEL.to_owned(), name.to_string()),
        (CONTEXT_HASH_LABEL.to_owned(), hash),
        (MOUNTS_LABEL.to_owned(), description),
    ]
    .into();
    if let Some(env_hash) = env_hash {
        labels.insert(ENV_LABEL.to_owned(), env_hash);
    }
    provider.create_container(&ContainerSpec {
        name: container.clone(),
        image: BASE_IMAGE.tag.to_owned(),
        network: name.network(),
        mounts,
        env,
        labels,
        workdir: Some(WORKSPACE.to_owned()),
        resources: project.resources.clone(),
        ..ContainerSpec::default()
    })?;
    provider.start_container(&container)?;
    Ok(true)
}

/// Stops the project's instance; volumes and the container stay.
pub fn stop(provider: &dyn Provider, name: &ProjectName) -> Result<()> {
    provider.stop_container(&name.instance())
}

/// Whether the harness's program is installed (looked up on the instance's
/// PATH, which includes the tools volume).
pub fn harness_installed(
    provider: &dyn Provider,
    name: &ProjectName,
    harness: &HarnessConfig,
) -> Result<bool> {
    let program = harness.command.first().cloned().unwrap_or_default();
    let output = provider.exec_output(&ExecSpec {
        container: name.instance(),
        command: vec![
            "sh".into(),
            "-c".into(),
            r#"command -v "$1" >/dev/null"#.into(),
            "sh".into(),
            program,
        ],
        ..ExecSpec::default()
    })?;
    Ok(output.code == 0)
}

/// Installs a harness into the tools volume.
///
/// Runs `install` from the harness definition in a short-lived container
/// that is hardened like an instance but may write the tools volume. It uses
/// the project's network and gateway; if the project's rules do not allow
/// the npm registry, it is granted for a few minutes.
pub fn install_harness(
    provider: &dyn Provider,
    project: &Project,
    harness: &HarnessConfig,
) -> Result<()> {
    let name = &project.name;
    let policy = gateway::policy(project);
    let allowed = policy.mode == Mode::Open || policy.allow.iter().any(|p| p.matches(INSTALL_HOST));
    if !allowed {
        let pattern = INSTALL_HOST.parse().expect("valid built-in domain");
        GatewayControl::new(provider, name).allow(&pattern, Scope::For(INSTALL_GRANT_SECS))?;
    }

    let code = with_installer(provider, project, |installer| {
        provider.exec(&ExecSpec {
            container: installer.to_owned(),
            command: vec!["sh".into(), "-c".into(), harness.install.clone()],
            ..ExecSpec::default()
        })
    })?;
    match code {
        0 => Ok(()),
        code => Err(Error::Unsupported(format!(
            "Installation von {} fehlgeschlagen (Exit-Code {code})",
            harness.name
        ))),
    }
}

/// Runs `work` with a short-lived container that is hardened like an
/// instance but may write the tools volume, and removes it afterwards.
fn with_installer<T>(
    provider: &dyn Provider,
    project: &Project,
    work: impl FnOnce(&str) -> Result<T>,
) -> Result<T> {
    let name = &project.name;
    provider.ensure_volume(&VolumeSpec {
        name: TOOLS_VOLUME.to_owned(),
        ..VolumeSpec::default()
    })?;
    let installer = name.installer();
    provider.remove_container(&installer)?;
    // The root filesystem is read-only: npm installs into the tools volume
    // and keeps its cache in /tmp.
    let env = [
        ("NPM_CONFIG_PREFIX", TOOLS_DIR),
        ("npm_config_cache", "/tmp/npm-cache"),
        ("HOME", "/tmp"),
    ]
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .into();
    provider.create_container(&ContainerSpec {
        name: installer.clone(),
        image: BASE_IMAGE.tag.to_owned(),
        network: name.network(),
        mounts: vec![Mount {
            source: MountSource::Volume(TOOLS_VOLUME.to_owned()),
            target: TOOLS_DIR.to_owned(),
            read_only: false,
        }],
        env,
        labels: [(PROJECT_LABEL.to_owned(), name.to_string())].into(),
        workdir: Some("/tmp".to_owned()),
        ..ContainerSpec::default()
    })?;
    let result = provider
        .start_container(&installer)
        .and_then(|()| work(&installer));
    let removed = provider.remove_container(&installer);
    let value = result?;
    removed.map(|()| value)
}

/// Whether a file exists in the tools volume, seen from the instance.
pub fn tool_file_exists(provider: &dyn Provider, name: &ProjectName, path: &str) -> Result<bool> {
    let output = provider.exec_output(&ExecSpec {
        container: name.instance(),
        command: vec![
            "sh".into(),
            "-c".into(),
            r#"[ -f "$1" ]"#.into(),
            "sh".into(),
            path.to_owned(),
        ],
        ..ExecSpec::default()
    })?;
    Ok(output.code == 0)
}

/// Writes a file of harness into the tools volume, where instances can
/// read but not change it. `path` must lie below [`TOOLS_DIR`].
pub fn write_tool_file(
    provider: &dyn Provider,
    project: &Project,
    path: &str,
    contents: &str,
) -> Result<()> {
    let inside = path
        .strip_prefix(TOOLS_DIR)
        .is_some_and(|rest| rest.starts_with('/') && !rest.split('/').any(|p| p == ".."));
    if !inside {
        return Err(Error::invalid(
            "Pfad im Tools-Volume",
            path,
            "muss unter /opt/harness-tools liegen",
        ));
    }
    let script = r#"set -e
mkdir -p "$(dirname "$1")"
printf '%s' "$2" > "$1.partial"
mv -f "$1.partial" "$1""#;
    let output = with_installer(provider, project, |installer| {
        provider.exec_output(&ExecSpec {
            container: installer.to_owned(),
            command: vec![
                "sh".into(),
                "-c".into(),
                script.into(),
                "sh".into(),
                path.to_owned(),
                contents.to_owned(),
            ],
            ..ExecSpec::default()
        })
    })?;
    if output.code == 0 {
        Ok(())
    } else {
        Err(Error::Unsupported(format!(
            "{path} schreiben: {}",
            crate::sync::sanitize(output.stderr.trim())
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pi() -> HarnessConfig {
        toml::from_str(include_str!("../defaults/harnesses/pi.toml")).unwrap()
    }

    #[test]
    fn instance_mounts() {
        let name = ProjectName::new("demo").unwrap();
        let mounts = mounts(&name, &[&pi()], &[]);
        let described = describe(&mounts);
        assert_eq!(
            described,
            "harness-ws-demo:/workspace harness-home-demo:/home/agent \
             harness-tools:/opt/harness-tools:ro \
             harness-cfg-pi:/home/agent/.pi"
        );
        // The same harness twice does not mount twice.
        assert_eq!(
            super::mounts(&name, &[&pi(), &pi()], &[]).len(),
            mounts.len()
        );
        assert_eq!(super::mounts(&name, &[], &[]).len(), 3);

        let go: crate::config::AppConfig =
            toml::from_str(include_str!("../defaults/apps/go.toml")).unwrap();
        let ready = ReadyApp {
            app: go,
            install: apps::Install {
                app: "go".into(),
                id: "6a000000".into(),
                volume: "harness-app-go-6a000000".into(),
                version: "1.25.3".into(),
                recipe: String::new(),
            },
        };
        assert_eq!(
            describe(&super::mounts(&name, &[&pi()], &[ready])),
            "harness-ws-demo:/workspace harness-home-demo:/home/agent \
             harness-tools:/opt/harness-tools:ro \
             harness-app-go-6a000000:/opt/harness-apps/go:ro \
             harness-cfg-pi:/home/agent/.pi"
        );
    }
}
