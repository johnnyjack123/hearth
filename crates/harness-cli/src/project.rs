//! `harness project`: show, add, list and remove projects.

use std::env;
use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, bail};
use harness_core::config::{ConfigDir, Project};
use harness_core::project::{ProjectName, find_repo_root};
use harness_core::provider::Provider as _;

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

/// `harness init [name]`: makes the current folder a project (with a git
/// repository, if it has none yet), offers it to the account's web chats
/// and makes sure the background service runs. `name` replaces the folder
/// name as the project's (and environment's) name.
pub fn init(config: &ConfigDir, name: Option<&str>) -> anyhow::Result<bool> {
    let wanted = name.map(|text| {
        let name = ProjectName::slug(text);
        if name.as_str() != text.trim() {
            println!("Name der Umgebung: {name} (aus „{}“)", text.trim());
        }
        name
    });
    let cwd = env::current_dir().context("Aktuellen Ordner bestimmen")?;
    let repo = match find_repo_root(&cwd) {
        Some(root) => root,
        None => {
            let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
            let question = format!(
                "{} ist kein Git-Repository; der Agent tauscht Arbeit über Git aus. Jetzt eins \
                 anlegen und alle Dateien als ersten Commit übernehmen?",
                cwd.display()
            );
            if !interactive || !confirm(&question, true)? {
                return Err(not_a_repository(&cwd));
            }
            init_repo(&cwd)?;
            find_repo_root(&cwd).context("Repository nach git init nicht gefunden")?
        }
    };
    for file in config.init()? {
        println!("Angelegt: {}", file.display());
    }
    let project = match config.find_project(&repo)? {
        Some(project) => {
            if let Some(wanted) = wanted.as_ref().filter(|wanted| **wanted != project.name) {
                bail!(
                    "{} ist schon das Projekt {}. Einen anderen Namen bekommt es mit \
                     `harness project remove` und danach `harness init {wanted}`",
                    repo.display(),
                    project.name
                );
            }
            println!("{} ist schon das Projekt {}.", repo.display(), project.name);
            project
        }
        None => {
            let project = config.create_project_named(&repo, wanted.as_ref())?;
            println!(
                "Projekt {} angelegt (Netz: {}): {}",
                project.name,
                project.network,
                project.file.display()
            );
            project
        }
    };
    let remote = config.project_config(&project.name)?.remote;
    let paired = crate::login::paired(config)?.is_some();
    if !remote.enabled {
        println!("Für Web-Chats nicht freigegeben (`harness project remote on` schaltet es ein).");
    } else if !paired {
        println!(
            "Für Web-Chats bereit. Damit es in deinem Konto erscheint: `harness login <Server>`."
        );
    } else {
        crate::login::announce_projects(config);
        if crate::daemon::automatic() && !crate::daemon::running(config) {
            let started = if crate::daemon::autostart().is_some() {
                crate::daemon::start(config)?
            } else {
                crate::daemon::install(config)?
            };
            if !started {
                println!("Hintergrunddienst startet nicht, siehe `harness daemon`.");
            }
        }
        println!(
            "{} steht in der Web-App zur Auswahl: im Chat unten auf „Umgebung“.",
            project.name
        );
    }
    Ok(true)
}

/// `harness project remote [on|off]`: offers the project of the current
/// repository to web chats, or stops that.
pub fn remote(
    config: &ConfigDir,
    enabled: Option<bool>,
    confirm: Option<bool>,
) -> anyhow::Result<bool> {
    let repo = current_repo()?;
    let Some(project) = config.find_project(&repo)? else {
        bail!(
            "{} ist noch kein Projekt. `harness project add` legt es an.",
            repo.display()
        );
    };
    let remote = if enabled.is_some() || confirm.is_some() {
        let remote = config.set_remote(&project.name, enabled, confirm)?;
        crate::login::announce_projects(config);
        remote
    } else {
        config.project_config(&project.name)?.remote
    };
    if remote.enabled {
        println!(
            "{} ist für die Web-Chats deines Kontos freigegeben (Hintergrunddienst: `harness daemon`).",
            project.name
        );
        if remote.confirm {
            println!(
                "Neue Chats bestätigst du im Terminal mit `harness connect` ({} bisher bestätigt); \
                 der Hintergrunddienst lehnt sie ab.",
                remote.chats.len()
            );
        } else {
            println!(
                "Neue Chats starten ohne Bestätigung. Wer dein Server-Konto nutzt, kann hier \
                 Agents starten."
            );
        }
    } else {
        println!(
            "{} ist nicht für Web-Chats freigegeben. Einschalten: `harness project remote on`.",
            project.name
        );
    }
    Ok(true)
}

/// Lists all projects with their repositories and whether web chats may
/// use them.
pub fn list(config: &ConfigDir) -> anyhow::Result<bool> {
    let names = config.project_names()?;
    if names.is_empty() {
        println!("Noch keine Projekte. `harness init` im Ordner legt eins an.");
        return Ok(true);
    }
    let mut all_valid = true;
    for name in names {
        match config.project(&name) {
            Ok(project) => {
                let web = match config.project_config(&name).map(|c| c.remote) {
                    Ok(remote) if remote.enabled && remote.confirm => "Web-Chats mit Bestätigung",
                    Ok(remote) if remote.enabled => "Web-Chats",
                    Ok(_) => "nur Terminal",
                    Err(_) => "?",
                };
                println!(
                    "{:<24} {:<26} {}",
                    project.name,
                    web,
                    project.path.display()
                );
            }
            Err(error) => {
                all_valid = false;
                println!("{name:<24} ✗ {error}");
            }
        }
    }
    Ok(all_valid)
}

/// `harness project remove [name]`: forgets a project. Its instance and
/// gateway go, and unless `keep_data` the agent's clone and home in the VM
/// too; the repository on the host stays as it is. Web chats no longer
/// offer it.
pub fn remove(
    config: &ConfigDir,
    machine: &str,
    name: Option<&str>,
    keep_data: bool,
    yes: bool,
) -> anyhow::Result<bool> {
    let project = resolve(config, name)?;
    println!("Projekt {} ({})", project.name, project.path.display());
    if keep_data {
        println!(
            "Instanz und Gateway werden entfernt, Arbeitskopie und Home des Agents bleiben in der VM."
        );
    } else {
        println!(
            "Instanz, Gateway und die Daten des Agents in der VM (Arbeitskopie, Home) werden \
             gelöscht. Was der Agent noch nicht übergeben hat, ist dann weg (vorher `harness sync`)."
        );
    }
    println!("Dein Ordner und seine Branches bleiben, wie sie sind.");
    if !yes {
        if !std::io::stdin().is_terminal() {
            bail!("Ohne Terminal nur mit --yes");
        }
        if !confirm("Projekt entfernen?", false)? {
            println!("Abgebrochen.");
            return Ok(false);
        }
    }
    let cleaned = remove_from_vm(machine, &project.name, keep_data);
    config.remove_project(&project.name)?;
    crate::login::announce_projects(config);
    match cleaned {
        Ok(()) => println!("Projekt {} entfernt.", project.name),
        Err(error) => println!(
            "Projekt {} entfernt. In der VM ist noch etwas übrig ({error:#}); ein neues Projekt \
             mit diesem Namen übernimmt es.",
            project.name
        ),
    }
    Ok(true)
}

/// Removes what a project has in the VM, starting the VM if needed.
fn remove_from_vm(machine: &str, name: &ProjectName, keep_data: bool) -> anyhow::Result<()> {
    let provider = crate::vm::provider(machine)?;
    for container in [name.instance(), name.installer(), name.gateway()] {
        provider.remove_container(&container)?;
    }
    provider.remove_network(&name.network())?;
    if !keep_data {
        for volume in [name.workspace_volume(), name.home_volume()] {
            provider.remove_volume(&volume)?;
        }
    }
    Ok(())
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
