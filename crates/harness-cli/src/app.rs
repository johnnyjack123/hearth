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
