//! `harness project`: show, add and list projects.

use std::env;
use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Asks a yes/no question on the terminal.
pub fn confirm(question: &str, default: bool) -> anyhow::Result<bool> {
    print!("{question} {} ", if default { "[J/n]" } else { "[j/N]" });
    std::io::Write::flush(&mut std::io::stdout())?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(match answer.trim().to_lowercase().as_str() {
        "" => default,
        "j" | "ja" | "y" | "yes" => true,
        _ => false,
    })
}

/// Files above which `git init` asks once more before committing them all.
const MANY_FILES: usize = 2000;

/// `git init` in `dir` and a first commit of everything not ignored; the
/// agent's clone needs a commit to start from.
fn init_repo(dir: &Path) -> anyhow::Result<()> {
    let git = harness_core::sync::find_git()?;
    let run = |args: &[&str]| -> anyhow::Result<String> {
        let output = Command::new(&git)
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .with_context(|| format!("git {}", args.join(" ")))?;
        if !output.status.success() {
            bail!(
                "git {}: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    };
    run(&["init", "--quiet"])?;
    run(&["add", "--all"])?;
    let files = run(&["diff", "--cached", "--name-only"])?.lines().count();
    if files > MANY_FILES
        && !confirm(
            &format!("{files} Dateien kommen in den ersten Commit. Wirklich alle?"),
            false,
        )?
    {
        bail!(
            "Abgebrochen. Das Repository ist angelegt, die Dateien sind vorgemerkt; mit einer \
             .gitignore ausdünnen und selbst committen"
        );
    }
    run(&["commit", "--quiet", "--allow-empty", "-m", "Erster Stand"]).context(
        "Erster Commit. Fehlt die Git-Identität? git config --global user.name/user.email",
    )?;
    println!("Git-Repository angelegt, {files} Dateien im ersten Commit.");
    Ok(())
}

/// The project of the current folder for `harness`, `run` and `shell`. On
/// a terminal it offers to set the folder up instead of failing: a git
/// repository first if needed, then the project.
pub fn resolve_or_offer(config: &ConfigDir) -> anyhow::Result<Project> {
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let cwd = env::current_dir().context("Aktuellen Ordner bestimmen")?;
    let repo = match find_repo_root(&cwd) {
        Some(root) => root,
        None if interactive => {
            let question = format!(
                "{} ist kein Git-Repository; der Agent tauscht Arbeit über Git aus. Jetzt eins \
                 anlegen und alle Dateien als ersten Commit übernehmen?",
                cwd.display()
            );
            if !confirm(&question, false)? {
                bail!("Abgebrochen");
            }
            init_repo(&cwd)?;
            find_repo_root(&cwd).context("Repository nach git init nicht gefunden")?
        }
        None => return Err(not_a_repository(&cwd)),
    };
    if let Some(project) = config.find_project(&repo)? {
        return Ok(project);
    }
    if !interactive {
        bail!(
            "{} ist noch kein Projekt. `harness project add` legt es an",
            repo.display()
        );
    }
    if !confirm(&format!("{} als Projekt anlegen?", repo.display()), true)? {
        bail!("Abgebrochen");
    }
    config.init()?;
    let project = config.create_project(&repo)?;
    println!(
        "Projekt {} angelegt (Netz: {}). Einstellungen: harness project show",
        project.name, project.network
    );
    Ok(project)
}

fn current_repo() -> anyhow::Result<PathBuf> {
    let cwd = env::current_dir().context("Aktuellen Ordner bestimmen")?;
    find_repo_root(&cwd).ok_or_else(|| not_a_repository(&cwd))
}

fn not_a_repository(dir: &Path) -> anyhow::Error {
    anyhow::anyhow!(
        "{} liegt in keinem Git-Repository. Projekte sind Git-Repositories, weil der \
         Austausch mit dem Agent über Git läuft",
        dir.display()
    )
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
