//! `harness project`: show, add and list projects.

use std::env;
use std::path::PathBuf;

use anyhow::{Context as _, bail};
use harness_core::config::{ConfigDir, Project};
use harness_core::project::{ProjectName, find_repo_root};

/// Prints the effective settings of the project for the current directory.
pub fn show(config: &ConfigDir) -> anyhow::Result<bool> {
    let repo = current_repo()?;
    match config.find_project(&repo)? {
        Some(project) => {
            print_project(&project);
            Ok(true)
        }
        None => {
            println!(
                "{} ist noch kein Projekt. `harness project add` legt es an.",
                repo.display()
            );
            Ok(false)
        }
    }
}

/// Registers the current repository as a project.
pub fn add(config: &ConfigDir) -> anyhow::Result<bool> {
    let repo = current_repo()?;
    for file in config.init()? {
        println!("Angelegt: {}", file.display());
    }
    let project = config.create_project(&repo)?;
    println!(
        "Projekt {} angelegt: {}",
        project.name,
        project.file.display()
    );
    println!();
    print_project(&project);
    Ok(true)
}

/// Lists all projects with their repositories.
pub fn list(config: &ConfigDir) -> anyhow::Result<bool> {
    let names = config.project_names()?;
    if names.is_empty() {
        println!("Noch keine Projekte. `harness project add` im Repository legt eins an.");
        return Ok(true);
    }
    let mut all_valid = true;
    for name in names {
        match config.project(&name) {
            Ok(project) => println!("{:<24} {}", project.name, project.path.display()),
            Err(error) => {
                all_valid = false;
                println!("{name:<24} ✗ {error}");
            }
        }
    }
    Ok(all_valid)
}

/// The project named on the command line, else the one of the current
/// repository.
pub fn resolve(config: &ConfigDir, name: Option<&str>) -> anyhow::Result<Project> {
    if let Some(name) = name {
        return Ok(config.project(&ProjectName::new(name)?)?);
    }
    let repo = current_repo()?;
    match config.find_project(&repo)? {
        Some(project) => Ok(project),
        None => bail!(
            "{} ist noch kein Projekt. `harness project add` legt es an",
            repo.display()
        ),
    }
}

fn current_repo() -> anyhow::Result<PathBuf> {
    let cwd = env::current_dir().context("Aktuellen Ordner bestimmen")?;
    match find_repo_root(&cwd) {
        Some(root) => Ok(root),
        None => bail!(
            "{} liegt in keinem Git-Repository. Projekte sind Git-Repositories, weil der \
             Austausch mit dem Agent über Git läuft",
            cwd.display()
        ),
    }
}

fn print_project(project: &Project) {
    let name = &project.name;
    let list = |domains: &[harness_core::config::DomainPattern]| {
        domains
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut network = project.network.to_string();
    if !project.ask {
        network.push_str(", ohne Nachfragen");
    }
    if !project.allow.is_empty() {
        network.push_str(&format!(", zusätzlich erlaubt: {}", list(&project.allow)));
    }
    if !project.deny.is_empty() {
        network.push_str(&format!(", gesperrt: {}", list(&project.deny)));
    }
    let resources = &project.resources;
    println!("Projekt {name}");
    println!("  Repository:  {}", project.path.display());
    println!("  Datei:       {}", project.file.display());
    println!("  Harness:     {}", project.harness);
    println!(
        "  Dateien:     {} (Klon im Volume {}, Branch {})",
        project.mode.as_str(),
        name.workspace_volume(),
        name.agent_branch()
    );
    println!("  Netzwerk:    {network}");
    println!(
        "  Ressourcen:  Speicher {}, CPUs {}, höchstens {} Prozesse",
        resources.memory.as_deref().unwrap_or("unbegrenzt"),
        resources
            .cpus
            .map_or_else(|| "unbegrenzt".to_owned(), |c| c.to_string()),
        resources.pids_limit
    );
    println!(
        "  Container:   {} im internen Netz {}, Gateway {}",
        name.instance(),
        name.network(),
        name.gateway()
    );
}
