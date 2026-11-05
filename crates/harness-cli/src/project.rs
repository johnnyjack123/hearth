//! `harness project`: show, add, list and remove projects.

use std::env;
use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, bail};
use harness_core::config::{ConfigDir, Project};
use harness_core::lang::tr;
use harness_core::project::{ProjectName, find_repo_root};
use harness_core::provider::Provider as _;
use harness_core::rescue::{self, Unsaved};
use harness_core::tr;

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
                "{}",
                tr!(
                    "{} ist noch kein Projekt. `harness project add` legt es an.",
                    "{} is not a project yet. `harness project add` creates it.",
                    repo.display()
                )
            );
            Ok(false)
        }
    }
}

/// Registers the current repository as a project.
pub fn add(config: &ConfigDir) -> anyhow::Result<bool> {
    let repo = current_repo()?;
    for file in config.init()? {
        println!("{}", created(&file));
    }
    let project = config.create_project(&repo)?;
    println!(
        "{}",
        tr!(
            "Projekt {} angelegt: {}",
            "Project {} created: {}",
            project.name,
            project.file.display()
        )
    );
    println!();
    print_project(&project);
    Ok(true)
}

/// The line for a file `config.init()` created.
fn created(file: &Path) -> String {
    tr!("Angelegt: {}", "Created: {}", file.display())
}

/// `harness init [path] [name]`: makes a folder (by default the current
/// one) a project, with a git repository if it has none yet, offers it to
/// the account's web chats and makes sure the background service runs.
/// `name` replaces the folder name as the project's (and environment's)
/// name.
pub fn init(config: &ConfigDir, path: Option<&Path>, name: Option<&str>) -> anyhow::Result<bool> {
    let wanted = name.map(|text| {
        let name = ProjectName::slug(text);
        if name.as_str() != text.trim() {
            println!(
                "{}",
                tr!(
                    "Name der Umgebung: {name} (aus „{}“)",
                    "Name of the environment: {name} (from “{}”)",
                    text.trim()
                )
            );
        }
        name
    });
    let cwd = match path {
        Some(path) if !path.is_dir() => bail!(tr!(
            "Den Ordner {} gibt es nicht. Für einen eigenen Namen im aktuellen Ordner: \
             `harness init . <name>`",
            "The folder {} does not exist. For a name of your own in the current folder: \
             `harness init . <name>`",
            path.display()
        )),
        Some(path) => {
            std::path::absolute(path).context(tr("Ordner bestimmen", "Determining the folder"))?
        }
        None => env::current_dir().context(tr(
            "Aktuellen Ordner bestimmen",
            "Determining the current folder",
        ))?,
    };
    let repo = match find_repo_root(&cwd) {
        Some(root) => root,
        None => {
            let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
            let question = offer_repository(&cwd);
            if !interactive || !confirm(&question, true)? {
                return Err(not_a_repository(&cwd));
            }
            init_repo(&cwd)?;
            find_repo_root(&cwd).context(tr(
                "Repository nach git init nicht gefunden",
                "Repository not found after git init",
            ))?
        }
    };
    for file in config.init()? {
        println!("{}", created(&file));
    }
    let project = match config.find_project(&repo)? {
        Some(project) => {
            if let Some(wanted) = wanted.as_ref().filter(|wanted| **wanted != project.name) {
                bail!(tr!(
                    "{} ist schon das Projekt {}. Einen anderen Namen bekommt es mit \
                     `harness project remove` und danach `harness init . {wanted}`",
                    "{} is already the project {}. For another name: `harness project remove`, \
                     then `harness init . {wanted}`",
                    repo.display(),
                    project.name
                ));
            }
            println!(
                "{}",
                tr!(
                    "{} ist schon das Projekt {}.",
                    "{} is already the project {}.",
                    repo.display(),
                    project.name
                )
            );
            project
        }
        None => {
            let project = config.create_project_named(&repo, wanted.as_ref())?;
            println!(
                "{}",
                tr!(
                    "Projekt {} angelegt (Netz: {}): {}",
                    "Project {} created (network: {}): {}",
                    project.name,
                    project.network,
                    project.file.display()
                )
            );
            project
        }
    };
    let remote = config.project_config(&project.name)?.remote;
    let paired = crate::login::paired(config)?.is_some();
    if !remote.enabled {
        println!(
            "{}",
            tr(
                "Für Web-Chats nicht freigegeben (`harness project remote on` schaltet es ein).",
                "Not offered to web chats (`harness project remote on` switches it on).",
            )
        );
    } else if !paired {
        println!(
            "{}",
            tr(
                "Für Web-Chats bereit. Damit es in deinem Konto erscheint: `harness login <Server>`.",
                "Ready for web chats. To make it appear in your account: `harness login <server>`.",
            )
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
                println!(
                    "{}",
                    tr(
                        "Hintergrunddienst startet nicht, siehe `harness daemon`.",
                        "The background service does not start, see `harness daemon`.",
                    )
                );
            }
        }
        println!(
            "{}",
            tr!(
                "{} steht in der Web-App zur Auswahl: im Chat unten auf „Umgebung“.",
                "{} can be picked in the web app: in the chat, click “Environment” at the bottom.",
                project.name
            )
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
        bail!(tr!(
            "{} ist noch kein Projekt. `harness project add` legt es an.",
            "{} is not a project yet. `harness project add` creates it.",
            repo.display()
        ));
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
            "{}",
            tr!(
                "{} ist für die Web-Chats deines Kontos freigegeben (Hintergrunddienst: `harness daemon`).",
                "{} is offered to the web chats of your account (background service: `harness daemon`).",
                project.name
            )
        );
        if remote.confirm {
            println!(
                "{}",
                tr!(
                    "Neue Chats bestätigst du im Terminal mit `harness connect` ({} bisher bestätigt); \
                     der Hintergrunddienst lehnt sie ab.",
                    "You confirm new chats in the terminal with `harness connect` ({} confirmed so \
                     far); the background service refuses them.",
                    remote.chats.len()
                )
            );
        } else {
            println!(
                "{}",
                tr(
                    "Neue Chats starten ohne Bestätigung. Wer dein Server-Konto nutzt, kann hier \
                     Agents starten.",
                    "New chats start without confirmation. Whoever uses your server account can \
                     start agents here.",
                )
            );
        }
    } else {
        println!(
            "{}",
            tr!(
                "{} ist nicht für Web-Chats freigegeben. Einschalten: `harness project remote on`.",
                "{} is not offered to web chats. Switch it on: `harness project remote on`.",
                project.name
            )
        );
    }
    Ok(true)
}

/// Lists all projects with their repositories and whether web chats may
/// use them.
pub fn list(config: &ConfigDir) -> anyhow::Result<bool> {
    let names = config.project_names()?;
    if names.is_empty() {
        println!(
            "{}",
            tr(
                "Noch keine Projekte. `harness init` im Ordner legt eins an.",
                "No projects yet. `harness init` in the folder creates one.",
            )
        );
        return Ok(true);
    }
    let mut all_valid = true;
    for name in names {
        match config.project(&name) {
            Ok(project) => {
                let web = match config.project_config(&name).map(|c| c.remote) {
                    Ok(remote) if remote.enabled && remote.confirm => {
                        tr("Web-Chats mit Bestätigung", "web chats with confirmation")
                    }
                    Ok(remote) if remote.enabled => tr("Web-Chats", "web chats"),
                    Ok(_) => tr("nur Terminal", "terminal only"),
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
    println!(
        "{}",
        tr!(
            "Projekt {} ({})",
            "Project {} ({})",
            project.name,
            project.path.display()
        )
    );
    let mut keep_data = keep_data;
    let mut unsaved = None;
    if !keep_data {
        let checked = crate::vm::provider(machine)
            .and_then(|provider| Ok(rescue::unsaved(&provider, &project.name)?));
        match checked {
            Ok(work) => unsaved = work.filter(Unsaved::any),
            Err(error) => {
                println!(
                    "{}",
                    tr!(
                        "Ob der Agent noch Arbeit hat, die nicht auf deinem Rechner ist, ließ sich nicht \
                         prüfen ({error:#}). Seine Daten bleiben deshalb in der VM.",
                        "Whether the agent still has work that is not on your computer could not be \
                         checked ({error:#}). Its data therefore stays in the VM."
                    )
                );
                keep_data = true;
            }
        }
    }
    let rescued_dir = config.root().join("gesichert");
    if let Some(work) = unsaved {
        println!(
            "{}",
            tr!(
                "Achtung: Der Agent hat Arbeit, die nie auf deinem Rechner ankam: {} Commit(s), {} \
                 nicht committete Datei(en). Sie wird vor dem Löschen als Git-Bundle nach {} gesichert.",
                "Caution: the agent has work that never reached your computer: {} commit(s), {} \
                 uncommitted file(s). It is saved as a Git bundle to {} before deleting.",
                work.commits,
                work.changes,
                rescued_dir.display()
            )
        );
    }
    if keep_data {
        println!(
            "{}",
            tr(
                "Instanz und Gateway werden entfernt, Arbeitskopie und Home des Agents bleiben in der VM.",
                "Instance and gateway are removed, the agent's working copy and home stay in the VM.",
            )
        );
    } else if unsaved.is_some() {
        println!(
            "{}",
            tr(
                "Danach werden Instanz, Gateway und die Daten des Agents in der VM (Arbeitskopie, \
                 Home) gelöscht. Übernehmen statt nur sichern: abbrechen und `harness sync`.",
                "After that, instance, gateway and the agent's data in the VM (working copy, home) \
                 are deleted. To take the work over instead of only saving it: cancel and run \
                 `harness sync`.",
            )
        );
    } else {
        println!(
            "{}",
            tr(
                "Instanz, Gateway und die Daten des Agents in der VM (Arbeitskopie, Home) werden \
                 gelöscht. Seine Commits sind alle schon auf deinem Rechner.",
                "Instance, gateway and the agent's data in the VM (working copy, home) are \
                 deleted. All its commits are on your computer already.",
            )
        );
    }
    println!(
        "{}",
        tr(
            "Dein Ordner und seine Branches bleiben, wie sie sind.",
            "Your folder and its branches stay as they are.",
        )
    );
    if !yes {
        if !std::io::stdin().is_terminal() {
            bail!(tr(
                "Ohne Terminal nur mit --yes",
                "Without a terminal only with --yes"
            ));
        }
        if !confirm(tr("Projekt entfernen?", "Remove the project?"), false)? {
            println!("{}", tr("Abgebrochen.", "Cancelled."));
            return Ok(false);
        }
    }
    if unsaved.is_some() {
        let stamp = crate::connect::utc_stamp()
            .replace(' ', "_")
            .replace([':', 'Z'], "");
        let target = rescued_dir.join(format!("{}-{stamp}.bundle", project.name));
        let saved = std::fs::create_dir_all(&rescued_dir)
            .map_err(anyhow::Error::from)
            .and_then(|()| crate::vm::provider(machine))
            .and_then(|provider| Ok(rescue::save(&provider, &project.name, &target)?));
        match saved {
            Ok(()) => println!(
                "{}",
                tr!(
                    "Gesichert in {}. Holen: git fetch \"{}\" \"refs/heads/*:refs/remotes/gesichert/*\"",
                    "Saved in {}. To fetch: git fetch \"{}\" \"refs/heads/*:refs/remotes/gesichert/*\"",
                    target.display(),
                    target.display()
                )
            ),
            Err(error) => {
                println!(
                    "{}",
                    tr!(
                        "Sichern gescheitert ({error:#}); die Daten des Agents bleiben in der VM.",
                        "Saving failed ({error:#}); the agent's data stays in the VM."
                    )
                );
                keep_data = true;
            }
        }
    }
    let cleaned = remove_from_vm(machine, &project.name, keep_data);
    config.remove_project(&project.name)?;
    crate::login::announce_projects(config);
    match cleaned {
        Ok(()) => println!(
            "{}",
            tr!("Projekt {} entfernt.", "Project {} removed.", project.name)
        ),
        Err(error) => println!(
            "{}",
            tr!(
                "Projekt {} entfernt. In der VM ist noch etwas übrig ({error:#}); ein neues Projekt \
                 mit diesem Namen übernimmt es.",
                "Project {} removed. Something is left in the VM ({error:#}); a new project with \
                 this name takes it over.",
                project.name
            )
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
        // The harnesses' sessions and settings of this project.
        let prefix = name.config_prefix();
        for volume in provider.volumes()? {
            if volume.name.starts_with(&prefix) {
                provider.remove_volume(&volume.name)?;
            }
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
        None => bail!(no_project(&repo)),
    }
}

/// "… is no project yet", for errors.
fn no_project(repo: &Path) -> String {
    tr!(
        "{} ist noch kein Projekt. `harness project add` legt es an",
        "{} is not a project yet. `harness project add` creates it",
        repo.display()
    )
}

/// Asks a yes/no question on the terminal.
pub fn confirm(question: &str, default: bool) -> anyhow::Result<bool> {
    let choices = if default {
        tr("[J/n]", "[Y/n]")
    } else {
        tr("[j/N]", "[y/N]")
    };
    print!("{question} {choices} ");
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
            &tr!(
                "{files} Dateien kommen in den ersten Commit. Wirklich alle?",
                "{files} files go into the first commit. Really all of them?"
            ),
            false,
        )?
    {
        bail!(tr(
            "Abgebrochen. Das Repository ist angelegt, die Dateien sind vorgemerkt; mit einer \
             .gitignore ausdünnen und selbst committen",
            "Cancelled. The repository is created and the files are staged; thin them out with \
             a .gitignore and commit yourself"
        ));
    }
    run(&["commit", "--quiet", "--allow-empty", "-m", "Erster Stand"]).context(tr(
        "Erster Commit. Fehlt die Git-Identität? git config --global user.name/user.email",
        "First commit. Is the Git identity missing? git config --global user.name/user.email",
    ))?;
    println!(
        "{}",
        tr!(
            "Git-Repository angelegt, {files} Dateien im ersten Commit.",
            "Git repository created, {files} files in the first commit."
        )
    );
    Ok(())
}

/// The question whether to make `dir` a git repository.
fn offer_repository(dir: &Path) -> String {
    tr!(
        "{} ist kein Git-Repository; der Agent tauscht Arbeit über Git aus. Jetzt eins \
         anlegen und alle Dateien als ersten Commit übernehmen?",
        "{} is not a Git repository; the agent exchanges work through Git. Create one now and \
         put all files into a first commit?",
        dir.display()
    )
}

/// The project of the current folder for `harness`, `run` and `shell`. On
/// a terminal it offers to set the folder up instead of failing: a git
/// repository first if needed, then the project.
pub fn resolve_or_offer(config: &ConfigDir) -> anyhow::Result<Project> {
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let cwd = env::current_dir().context(tr(
        "Aktuellen Ordner bestimmen",
        "Determining the current folder",
    ))?;
    let repo = match find_repo_root(&cwd) {
        Some(root) => root,
        None if interactive => {
            if !confirm(&offer_repository(&cwd), false)? {
                bail!(tr("Abgebrochen", "Cancelled"));
            }
            init_repo(&cwd)?;
            find_repo_root(&cwd).context(tr(
                "Repository nach git init nicht gefunden",
                "Repository not found after git init",
            ))?
        }
        None => return Err(not_a_repository(&cwd)),
    };
    if let Some(project) = config.find_project(&repo)? {
        return Ok(project);
    }
    if !interactive {
        bail!(no_project(&repo));
    }
    if !confirm(
        &tr!(
            "{} als Projekt anlegen?",
            "Create {} as a project?",
            repo.display()
        ),
        true,
    )? {
        bail!(tr("Abgebrochen", "Cancelled"));
    }
    config.init()?;
    let project = config.create_project(&repo)?;
    println!(
        "{}",
        tr!(
            "Projekt {} angelegt (Netz: {}). Einstellungen: harness project show",
            "Project {} created (network: {}). Settings: harness project show",
            project.name,
            project.network
        )
    );
    Ok(project)
}

fn current_repo() -> anyhow::Result<PathBuf> {
    let cwd = env::current_dir().context(tr(
        "Aktuellen Ordner bestimmen",
        "Determining the current folder",
    ))?;
    find_repo_root(&cwd).ok_or_else(|| not_a_repository(&cwd))
}

fn not_a_repository(dir: &Path) -> anyhow::Error {
    anyhow::anyhow!(tr!(
        "{} liegt in keinem Git-Repository. Projekte sind Git-Repositories, weil der \
         Austausch mit dem Agent über Git läuft",
        "{} is not in a Git repository. Projects are Git repositories, because the \
         exchange with the agent runs through Git",
        dir.display()
    ))
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
        network.push_str(tr(", ohne Nachfragen", ", without asking"));
    }
    if !project.allow.is_empty() {
        network.push_str(&tr!(
            ", zusätzlich erlaubt: {}",
            ", also allowed: {}",
            list(&project.allow)
        ));
    }
    if !project.deny.is_empty() {
        network.push_str(&tr!(", gesperrt: {}", ", blocked: {}", list(&project.deny)));
    }
    let resources = &project.resources;
    let unlimited = tr("unbegrenzt", "unlimited");
    println!("{}", tr!("Projekt {name}", "Project {name}"));
    println!("  Repository:  {}", project.path.display());
    println!(
        "  {}{}",
        tr("Datei:       ", "File:        "),
        project.file.display()
    );
    println!("  Harness:     {}", project.harness);
    println!(
        "  {}",
        tr!(
            "Dateien:     {} (Klon im Volume {}, Branch {})",
            "Files:       {} (clone in volume {}, branch {})",
            project.mode.as_str(),
            name.workspace_volume(),
            name.agent_branch()
        )
    );
    println!("  {}{network}", tr("Netzwerk:    ", "Network:     "));
    println!(
        "  {}",
        tr!(
            "Ressourcen:  Speicher {}, CPUs {}, höchstens {} Prozesse",
            "Resources:   memory {}, CPUs {}, at most {} processes",
            resources.memory.as_deref().unwrap_or(unlimited),
            resources
                .cpus
                .map_or_else(|| unlimited.to_owned(), |c| c.to_string()),
            resources.pids_limit
        )
    );
    println!(
        "  {}",
        tr!(
            "Container:   {} im internen Netz {}, Gateway {}",
            "Container:   {} in the internal network {}, gateway {}",
            name.instance(),
            name.network(),
            name.gateway()
        )
    );
}
