//! `harness app`: the app catalog and the apps of a project (ROADMAP
//! Phase 3). The project file says which apps are on; the VM holds one
//! install of each, shared by all projects (`harness_core::apps`).

use std::time::Duration;

use anyhow::bail;
use clap::Subcommand;
use harness_core::apps::{self, Install, Inventory, ReadyApp};
use harness_core::config::{AppConfig, ConfigDir, Project};
use harness_core::gateway;
use harness_core::podman::PodmanProvider;
use harness_core::project::ProjectName;
use harness_core::tunnel::{AppInfo, AppsReport, ProjectApps, RemoteProject, truncate};
use harness_core::vmd;

use crate::progress::{Progress, Terminal, step};
use crate::{login, project};

/// Subcommands of `harness app`.
#[derive(Subcommand)]
pub enum AppCommand {
    /// Zeigt den Katalog und welche Apps das Projekt hat (Standard)
    List,
    /// Schaltet Apps im Projekt ein; der nächste Start installiert, was fehlt
    On {
        /// Namen aus `harness app list`
        #[arg(required = true)]
        names: Vec<String>,
    },
    /// Schaltet Apps im Projekt aus
    Off {
        /// Namen aus `harness app list`
        #[arg(required = true)]
        names: Vec<String>,
    },
    /// Installiert jetzt, was dem Projekt fehlt (Standard: alle eingeschalteten Apps)
    Install {
        /// Nur diese Apps (müssen nicht eingeschaltet sein)
        names: Vec<String>,
    },
    /// Installiert Apps neu, in der aktuellen Version (Standard: alle eingeschalteten)
    Update {
        /// Nur diese Apps
        names: Vec<String>,
    },
    /// Löscht Apps aus der VM (Platz); Projekte, die sie eingeschaltet haben, installieren beim nächsten Start neu
    Remove {
        /// Namen aus `harness app list`
        #[arg(required = true)]
        names: Vec<String>,
    },
}

/// `harness app [--project NAME] [COMMAND]`.
pub fn run(
    config: &ConfigDir,
    machine: &str,
    project_name: Option<&str>,
    command: Option<AppCommand>,
) -> anyhow::Result<bool> {
    match command.unwrap_or(AppCommand::List) {
        AppCommand::List => list(config, machine, project_name),
        AppCommand::On { names } => switch(config, project_name, &names, true),
        AppCommand::Off { names } => switch(config, project_name, &names, false),
        AppCommand::Install { names } => install(config, machine, project_name, &names, false),
        AppCommand::Update { names } => install(config, machine, project_name, &names, true),
        AppCommand::Remove { names } => remove(machine, &names),
    }
}

/// Most apps the browser may switch on for one project at once.
const MAX_REMOTE_APPS: usize = 100;

/// The apps of this computer for the browser: every app it knows, which the
/// offered `projects` switched on, and the installed versions if the VM's
/// `inventory` is known. A broken recipe of the user's is left out rather
/// than hiding all the others.
pub(crate) fn report(
    config: &ConfigDir,
    projects: &[RemoteProject],
    inventory: Option<&Inventory>,
) -> AppsReport {
    let catalog = config
        .app_names()
        .unwrap_or_default()
        .iter()
        .filter_map(|name| config.app(name).ok())
        .map(|app| {
            let install = inventory.and_then(|inventory| inventory.latest(&app.name));
            AppInfo {
                version: install.map(|install| install.version.clone()),
                outdated: install.is_some_and(|install| install.outdated(&app)),
                description: app.description.as_deref().map(|d| truncate(d, 300)),
                name: app.name,
                title: app.title,
                requires: app.requires,
            }
        })
        .collect();
    let projects = projects
        .iter()
        .filter_map(|offered| {
            let name = ProjectName::new(&offered.name).ok()?;
            let project = config.project(&name).ok()?;
            Some(ProjectApps {
                name: offered.name.clone(),
                enabled: project.apps,
            })
        })
        .collect();
    AppsReport {
        catalog,
        versions: inventory.is_some(),
        projects,
    }
}

/// Switches a project's apps as the browser asks: only for a project the
/// computer offers to web chats, only apps it knows. Returns whether it
/// worked and what to tell the user (German).
pub(crate) fn apply_from_browser(
    config: &ConfigDir,
    offered: &[RemoteProject],
    project: &str,
    enabled: &[String],
) -> (bool, String) {
    if !offered.iter().any(|candidate| candidate.name == project) {
        return (
            false,
            format!("Projekt {project} ist auf diesem Rechner nicht für Web-Chats freigegeben."),
        );
    }
    if enabled.len() > MAX_REMOTE_APPS {
        return (false, "Zu viele Apps auf einmal.".to_owned());
    }
    let switched = ProjectName::new(project)
        .map_err(anyhow::Error::from)
        .and_then(|name| {
            let current = config.project(&name)?.apps;
            let off: Vec<String> = current
                .into_iter()
                .filter(|app| !enabled.contains(app))
                .collect();
            Ok(config.set_apps(&name, enabled, &off)?)
        })
        .and_then(|project| Ok(config.project_apps(&project)?));
    match switched {
        Ok(active) => {
            let titles: Vec<&str> = active.iter().map(|app| app.title.as_str()).collect();
            let list = if titles.is_empty() {
                "keine".to_owned()
            } else {
                titles.join(", ")
            };
            (
                true,
                format!(
                    "Apps von {project}: {list}. Gilt ab dem nächsten Start der Umgebung; was \
                     fehlt, wird dann installiert."
                ),
            )
        }
        Err(error) => (
            false,
            format!("Apps von {project} nicht geändert: {error:#}"),
        ),
    }
}

/// Deletes every install of the named apps that no container mounts.
fn remove(machine: &str, names: &[String]) -> anyhow::Result<bool> {
    let provider = crate::vm::provider(machine)?;
    let inventory = Inventory::load(&provider)?;
    let mut all_gone = true;
    for name in names {
        let installs: Vec<&Install> = inventory
            .ready
            .iter()
            .filter(|install| install.app == *name)
            .collect();
        if installs.is_empty() {
            println!("{name}: nicht installiert.");
            continue;
        }
        for install in installs {
            match apps::remove(&provider, install) {
                Ok(()) => println!("{name} {}: gelöscht.", install.version),
                Err(error) => {
                    all_gone = false;
                    println!(
                        "{name} {}: bleibt, weil eine Instanz sie noch einhängt (auch gestoppte). \
                         Erst in den Projekten ausschalten (`harness app off {name}`) und sie \
                         einmal starten. {error}",
                        install.version
                    );
                }
            }
        }
    }
    Ok(all_gone)
}

/// The project's apps, installing those the VM lacks, one step each.
pub(crate) fn ready(
    config: &ConfigDir,
    provider: &PodmanProvider,
    project: &Project,
    progress: &dyn Progress,
) -> anyhow::Result<Vec<ReadyApp>> {
    let wanted = config.project_apps(project)?;
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let inventory = Inventory::load(provider)?;
    let mut ready: Vec<ReadyApp> = Vec::new();
    for app in wanted {
        // A newer recipe does not reinstall by itself (downloads, minutes);
        // `harness app list` points it out.
        let install = match inventory.latest(&app.name) {
            Some(install) => install.clone(),
            None => install_one(config, provider, project, &app, &ready, progress)?,
        };
        ready.push(ReadyApp { app, install });
    }
    Ok(ready)
}

/// Installs `app` as a step; `installed` must hold the apps it requires.
fn install_one(
    config: &ConfigDir,
    provider: &PodmanProvider,
    project: &Project,
    app: &AppConfig,
    installed: &[ReadyApp],
    progress: &dyn Progress,
) -> anyhow::Result<Install> {
    let needed: Vec<String> = config
        .resolve_apps(&app.requires)?
        .into_iter()
        .map(|required| required.name)
        .collect();
    let requires: Vec<ReadyApp> = installed
        .iter()
        .filter(|ready| needed.contains(&ready.app.name))
        .cloned()
        .collect();
    step(
        progress,
        &format!("app-{}", app.name),
        &format!("{} installieren (einmalig, für alle Projekte)", app.title),
        || {
            // The recipe's output follows on the next lines.
            println!();
            Ok(apps::install(provider, project, app, &requires)?)
        },
    )
}

fn list(config: &ConfigDir, machine: &str, project_name: Option<&str>) -> anyhow::Result<bool> {
    let catalog = config.apps()?;
    // Outside a project the catalog alone is still worth showing.
    let project = match project_name {
        Some(_) => Some(project::resolve(config, project_name)?),
        None => project::resolve(config, None).ok(),
    };
    let active: Vec<AppConfig> = match &project {
        Some(project) => config.project_apps(project)?,
        None => Vec::new(),
    };
    let inventory = running_provider(machine)
        .map(|provider| Inventory::load(&provider))
        .transpose()?;

    match &project {
        Some(project) => println!("Apps für Projekt {}:", project.name),
        None => println!("App-Katalog (kein Projekt in diesem Ordner):"),
    }
    let mut outdated = false;
    for app in &catalog {
        let state = match &project {
            Some(project) if project.apps.contains(&app.name) => "an".to_owned(),
            Some(_) if active.iter().any(|a| a.name == app.name) => {
                let by: Vec<&str> = active
                    .iter()
                    .filter(|a| a.requires.contains(&app.name))
                    .map(|a| a.title.as_str())
                    .collect();
                format!("an (für {})", by.join(", "))
            }
            Some(_) => "aus".to_owned(),
            None => String::new(),
        };
        let installed = match &inventory {
            Some(inventory) => match inventory.latest(&app.name) {
                Some(install) if install.outdated(app) => {
                    outdated = true;
                    format!("{} *", install.version)
                }
                Some(install) => install.version.clone(),
                None => "–".to_owned(),
            },
            None => String::new(),
        };
        println!(
            "  {:<8} {:<10} {:<16} {:<18} {}",
            app.name,
            app.title,
            state,
            installed,
            app.description.as_deref().unwrap_or_default()
        );
    }
    match &inventory {
        None => {
            println!(
                "(Die VM läuft nicht; installierte Versionen zeigt die Liste, wenn sie läuft.)"
            )
        }
        Some(_) => println!("(Versionen: installiert in der VM, – nicht installiert)"),
    }
    if outdated {
        println!("* Das Rezept ist seither neuer; `harness app update <name>` installiert neu.");
    }
    if project.is_some() {
        println!();
        println!(
            "Einschalten mit `harness app on <name>`; der nächste Start (harness, Web-Chat) \
             installiert, was fehlt, einmal für alle Projekte."
        );
    }
    Ok(true)
}

/// A provider if the VM runs; `None` without starting it.
fn running_provider(machine: &str) -> Option<PodmanProvider> {
    let provider = PodmanProvider::for_machine(machine).ok()?;
    vmd::ssh_answers(provider.machine().ssh_port, Duration::from_secs(2)).then_some(provider)
}

fn switch(
    config: &ConfigDir,
    project_name: Option<&str>,
    names: &[String],
    on: bool,
) -> anyhow::Result<bool> {
    let project = project::resolve(config, project_name)?;
    let project = if on {
        config.set_apps(&project.name, names, &[])?
    } else {
        config.set_apps(&project.name, &[], names)?
    };
    let active = config.project_apps(&project)?;
    let titles: Vec<&str> = active.iter().map(|app| app.title.as_str()).collect();
    println!(
        "Projekt {}: {}",
        project.name,
        if titles.is_empty() {
            "keine Apps".to_owned()
        } else {
            titles.join(", ")
        }
    );
    if !on {
        for name in names {
            if let Some(app) = active.iter().find(|app| app.name == *name) {
                let by: Vec<&str> = active
                    .iter()
                    .filter(|other| other.requires.contains(name))
                    .map(|other| other.title.as_str())
                    .collect();
                println!("{} bleibt, weil {} es braucht.", app.title, by.join(", "));
            }
        }
    }
    println!(
        "Gilt ab dem nächsten Start der Instanz (harness, harness shell, Web-Chat); eine \
         laufende wird dabei neu angelegt, Arbeitskopie und Home bleiben."
    );
    Ok(true)
}

fn install(
    config: &ConfigDir,
    machine: &str,
    project_name: Option<&str>,
    names: &[String],
    again: bool,
) -> anyhow::Result<bool> {
    let project = project::resolve(config, project_name)?;
    let wanted = if names.is_empty() {
        config.project_apps(&project)?
    } else {
        config.resolve_apps(names)?
    };
    if wanted.is_empty() {
        bail!(
            "Projekt {} hat keine Apps eingeschaltet (`harness app on <name>`)",
            project.name
        );
    }
    let progress = Terminal::default();
    let provider = crate::vm::provider_with(machine, &progress)?;
    let (llm, _) = login::gateway_llm(config, &provider)?;
    step(
        &progress,
        "gateway",
        &format!("Gateway {}", project.name.gateway()),
        || Ok(gateway::ensure_running(&provider, &project, llm.as_ref())?),
    )?;
    let inventory = Inventory::load(&provider)?;
    let mut ready: Vec<ReadyApp> = Vec::new();
    for app in wanted {
        // Only the apps asked for are installed again, not what they require.
        let asked = names.is_empty() || names.contains(&app.name);
        let install = match inventory.latest(&app.name) {
            Some(install) if !(again && asked) => {
                println!("{}: {} ist schon installiert.", app.title, install.version);
                install.clone()
            }
            _ => {
                let install = install_one(config, &provider, &project, &app, &ready, &progress)?;
                println!("{}: {} installiert.", app.title, install.version);
                install
            }
        };
        ready.push(ReadyApp { app, install });
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_browser_switches_only_offered_projects_and_known_apps() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("demo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let config = ConfigDir::new(dir.path().join("home"));
        let project = config.create_project(&repo).unwrap();
        let name = project.name.to_string();
        let offered = vec![RemoteProject {
            name: name.clone(),
            harnesses: vec!["pi".into()],
        }];

        let (ok, message) = apply_from_browser(&config, &offered, &name, &["rust".into()]);
        assert!(ok, "{message}");
        assert!(message.contains("C/C++, Rust"), "{message}");
        assert_eq!(config.project(&project.name).unwrap().apps, ["rust"]);

        // Unknown apps and projects the computer does not offer change nothing.
        let (ok, message) = apply_from_browser(&config, &offered, &name, &["cobol".into()]);
        assert!(!ok && message.contains("cobol"), "{message}");
        let (ok, _) = apply_from_browser(&config, &[], &name, &[]);
        assert!(!ok);
        assert_eq!(config.project(&project.name).unwrap().apps, ["rust"]);

        let report = report(&config, &offered, None);
        assert!(!report.versions);
        assert_eq!(report.projects.len(), 1);
        assert_eq!(report.projects[0].enabled, ["rust"]);
        assert_eq!(report.catalog.len(), 7);
        assert!(report.catalog.iter().all(|app| app.version.is_none()));
        let rust = report
            .catalog
            .iter()
            .find(|app| app.name == "rust")
            .unwrap();
        assert_eq!(rust.requires, ["c"]);
    }
}
