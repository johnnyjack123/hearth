//! `hearth app`: the app catalog and the apps of a project (ROADMAP
//! Phase 3). The project file says which apps are on; the VM holds one
//! install of each, shared by all projects (`hearth_core::apps`).

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::bail;
use clap::Subcommand;
use hearth_core::Error;
use hearth_core::app_file;
use hearth_core::apps::{self, Install, Inventory, ReadyApp};
use hearth_core::config::{AppConfig, AppSource, AppTemplate, ConfigDir, Project};
use hearth_core::gateway;
use hearth_core::lang::{self, Lang, tr};
use hearth_core::podman::PodmanProvider;
use hearth_core::project::ProjectName;
use hearth_core::sync::sanitize;
use hearth_core::tr;
use hearth_core::tunnel::{AppInfo, AppsReport, PendingFile, ProjectApps, RemoteProject, truncate};
use hearth_core::vmd;

use crate::app_request::{self, FileRequest};
use crate::progress::{Progress, Terminal, step};
use crate::{login, project};

/// Subcommands of `hearth app`. The help texts follow the user's language
/// (hearth_core::lang), so they are attributes rather than doc comments.
#[derive(Subcommand)]
pub enum AppCommand {
    #[command(about = tr(
        "Zeigt den Katalog und welche Apps das Projekt hat (Standard)",
        "Shows the catalog and which apps the project has (default)"
    ))]
    List,
    #[command(about = tr(
        "Schaltet Apps im Projekt ein; der nächste Start installiert, was fehlt",
        "Switches apps on in the project; the next start installs what is missing"
    ))]
    On {
        #[arg(required = true, help = tr(
            "Namen aus `hearth app list`",
            "Names from `hearth app list`"
        ))]
        names: Vec<String>,
    },
    #[command(about = tr(
        "Schaltet Apps im Projekt aus",
        "Switches apps off in the project"
    ))]
    Off {
        #[arg(required = true, help = tr(
            "Namen aus `hearth app list`",
            "Names from `hearth app list`"
        ))]
        names: Vec<String>,
    },
    #[command(about = tr(
        "Installiert jetzt, was dem Projekt fehlt (Standard: alle eingeschalteten Apps)",
        "Installs now what the project lacks (default: all apps switched on)"
    ))]
    Install {
        #[arg(help = tr(
            "Nur diese Apps (müssen nicht eingeschaltet sein)",
            "Only these apps (need not be switched on)"
        ))]
        names: Vec<String>,
    },
    #[command(about = tr(
        "Installiert Apps neu, in der aktuellen Version (Standard: alle eingeschalteten)",
        "Installs apps again, in the current version (default: all switched on)"
    ))]
    Update {
        #[arg(help = tr("Nur diese Apps", "Only these apps"))]
        names: Vec<String>,
    },
    #[command(about = tr(
        "Löscht Apps aus der VM (Platz); Projekte, die sie eingeschaltet haben, installieren beim nächsten Start neu",
        "Deletes apps from the VM (space); projects that have them switched on install them again on the next start"
    ))]
    Remove {
        #[arg(required = true, help = tr(
            "Namen aus `hearth app list`",
            "Names from `hearth app list`"
        ))]
        names: Vec<String>,
    },
    #[command(about = tr(
        "Macht eine Linux-Programmdatei dieses Rechners zur App (Programm, AppImage, .deb oder Archiv)",
        "Makes a Linux program file of this computer an app (program, AppImage, .deb or archive)"
    ))]
    Add {
        #[arg(help = tr("Die Datei", "The file"))]
        path: PathBuf,
        #[arg(long, help = tr(
            "Kurzname der App (Standard: aus dem Dateinamen)",
            "Short name of the app (default: from the file name)"
        ))]
        name: Option<String>,
        #[arg(long, help = tr(
            "Name für Menschen (Standard: der Dateiname)",
            "Name for people (default: the file name)"
        ))]
        title: Option<String>,
    },
    #[command(about = tr(
        "Gibt eine Programmdatei frei, die der Browser angefragt hat",
        "Approves a program file the browser asked for"
    ))]
    Approve {
        #[arg(help = tr(
            "Die ID aus dem Browser oder `hearth app list`",
            "The ID from the browser or `hearth app list`"
        ))]
        id: String,
        #[arg(long, help = tr(
            "Ohne Rückfrage (das Tray-Symbol hat schon gefragt)",
            "Without asking (the tray icon has asked already)"
        ))]
        yes: bool,
    },
    #[command(about = tr(
        "Lehnt eine angefragte Programmdatei ab",
        "Refuses a program file the browser asked for"
    ))]
    Deny {
        #[arg(help = tr(
            "Die ID aus dem Browser oder `hearth app list`",
            "The ID from the browser or `hearth app list`"
        ))]
        id: String,
    },
    #[command(about = tr(
        "Vergisst Apps aus Programmdateien (Rezept und Kopie) und schaltet sie in allen Projekten aus",
        "Forgets apps from program files (recipe and copy) and switches them off in all projects"
    ))]
    Forget {
        #[arg(required = true, help = tr(
            "Namen aus `hearth app list`",
            "Names from `hearth app list`"
        ))]
        names: Vec<String>,
    },
}

/// `hearth app [--project NAME] [COMMAND]`.
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
        AppCommand::Add { path, name, title } => add(config, &path, name, title),
        AppCommand::Approve { id, yes } => approve(config, &id, yes),
        AppCommand::Deny { id } => deny(config, &id),
        AppCommand::Forget { names } => forget(config, &names),
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
                source: Some(app.source),
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
    let pending = app_request::list(config)
        .into_iter()
        .map(|request| PendingFile {
            id: request.id,
            name: request.name,
            title: request.title,
            path: truncate(&request.path, 900),
            kind: request.kind,
        })
        .collect();
    AppsReport {
        catalog,
        versions: inventory.is_some(),
        projects,
        pending,
    }
}

/// Whether `name` may become the app of a program file: free, or already
/// one of a program file (a new file replaces it). Says why not in `lang`:
/// the browser reads German, the terminal the user's language.
fn file_app_name_free(config: &ConfigDir, name: &str, lang: Lang) -> Result<(), String> {
    match config.app(name) {
        Ok(app) if app.file.is_some() => Ok(()),
        Ok(app) => {
            let source = match app.source {
                AppSource::Builtin => lang.pick("Katalog", "catalog"),
                AppSource::Local => {
                    lang.pick("Rezept auf diesem Rechner", "recipe on this computer")
                }
                AppSource::Server => lang.pick(
                    "eigenes Programm aus dem Browser",
                    "custom program from the browser",
                ),
            };
            Err(tr!(in lang;
                "Den Namen {name} hat schon {} ({source}); bitte einen anderen wählen.",
                "The name {name} is taken by {} ({source}); please pick another one.",
                app.title
            ))
        }
        Err(Error::AppUnknown(_)) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

/// A program file the browser asks for: checked here (only absolute paths
/// to Linux program files, free names), then stored as a request for the
/// user to approve on this computer. Returns whether it waits, what to tell
/// the user (German, the browser translates on its own) and the request's
/// id. Details only this computer should know go to `log`.
pub(crate) fn request_from_browser(
    config: &ConfigDir,
    path: &str,
    name: &str,
    title: &str,
    log: &dyn Fn(&str),
) -> (bool, String, Option<String>) {
    let title = title.trim();
    let checked = (|| -> Result<FileRequest, String> {
        if title.is_empty() || title.chars().count() > 60 {
            return Err("Der Name muss 1 bis 60 Zeichen lang sein.".to_owned());
        }
        // The title shows up in the approval dialog; it must not fake lines
        // or reorder text there.
        if title.chars().any(invisible_or_control) {
            return Err("Der Name darf keine Steuer- oder Formatzeichen enthalten.".to_owned());
        }
        file_app_name_free(config, name, Lang::De)?;
        let file = Path::new(path);
        if path.contains('\0') || !app_file::on_local_drive(file) {
            return Err(format!(
                "{} ist kein vollständiger Pfad auf einem Laufwerk dieses Rechners (z. B. \
                 C:\\Tools\\programm); Netzwerkpfade und -laufwerke gehen nicht.",
                sanitize(path)
            ));
        }
        // Why a file does not qualify stays here: the browser must not learn
        // which files exist on this computer.
        let info = app_file::inspect(file).map_err(|reason| {
            log(&format!(
                "Programmdatei {} abgelehnt: {reason}",
                sanitize(path)
            ));
            format!(
                "{} fehlt, ist nicht lesbar oder keine Linux-Programmdatei für diese VM \
                 (Programm, AppImage, .deb, .zip, .tar.gz, .tar.xz).",
                sanitize(path)
            )
        })?;
        app_request::create(config, name, title, path, info).map_err(|e| format!("{e:#}"))
    })();
    match checked {
        Ok(request) => (
            true,
            format!(
                "{} wartet auf Freigabe am Rechner: im Menü des Tray-Symbols oder mit `hearth \
                 app approve {}`.",
                request.title, request.id
            ),
            Some(request.id),
        ),
        Err(error) => (false, format!("Nicht angefragt: {error}"), None),
    }
}

/// Control characters and the invisible ones that hide or reorder text.
fn invisible_or_control(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}'
        )
}

/// Switches `names` off in every project of this computer; returns the
/// projects and what went off there.
fn switch_off_everywhere(
    config: &ConfigDir,
    names: &[String],
) -> anyhow::Result<Vec<(ProjectName, Vec<String>)>> {
    let mut switched = Vec::new();
    for name in config.project_names()? {
        let Ok(project) = config.project(&name) else {
            continue;
        };
        let off: Vec<String> = names
            .iter()
            .filter(|app| project.apps.contains(app))
            .cloned()
            .collect();
        if !off.is_empty() {
            config.set_apps(&name, &[], &off)?;
            switched.push((name, off));
        }
    }
    Ok(switched)
}

fn show_file(path: &Path, info: app_file::FileInfo) {
    println!("  {}{}", tr("Datei: ", "File:  "), path.display());
    println!(
        "  {}{}, {:.1} MB",
        tr("Art:   ", "Kind:  "),
        info.kind.label(),
        info.size as f64 / 1024.0 / 1024.0
    );
}

fn add(
    config: &ConfigDir,
    path: &Path,
    name: Option<String>,
    title: Option<String>,
) -> anyhow::Result<bool> {
    let name = name.unwrap_or_else(|| app_file::suggested_name(path));
    let title = title.unwrap_or_else(|| name.clone());
    if let Err(error) = file_app_name_free(config, &name, lang::current()) {
        bail!(error);
    }
    let app = app_file::add(config, &name, &title, path)?;
    println!(
        "{}",
        tr!(
            "{} ist jetzt die App {} (Kopie in {}).",
            "{} is now the app {} (copy in {}).",
            app.title,
            app.name,
            config.app_files_dir().join(&app.name).display()
        )
    );
    println!(
        "{}",
        tr!(
            "Einschalten mit `hearth app on {}` oder im Browser; der nächste Start installiert sie.",
            "Switch it on with `hearth app on {}` or in the browser; the next start installs it.",
            app.name
        )
    );
    Ok(true)
}

fn approve(config: &ConfigDir, id: &str, yes: bool) -> anyhow::Result<bool> {
    let Some(request) = app_request::load(config, id) else {
        bail!(tr!(
            "Keine wartende Programmdatei mit der ID {id} (hearth app list zeigt alle).",
            "No waiting program file with the ID {id} (hearth app list shows all)."
        ));
    };
    let path = PathBuf::from(&request.path);
    println!(
        "{}",
        tr!(
            "Der Browser möchte diese Datei als App {} („{}“) in die VM geben:",
            "The browser wants to put this file into the VM as app {} (“{}”):",
            request.name,
            request.title
        )
    );
    let info = app_file::inspect(&path).map_err(anyhow::Error::msg)?;
    show_file(&path, info);
    println!(
        "{}",
        tr(
            "Agents in Projekten, die die App einschalten, können die Datei lesen und ausführen. \
             Nur freigeben, wenn du sie selbst angefragt hast.",
            "Agents in projects that switch the app on can read and run the file. Only approve \
             it if you asked for it yourself.",
        )
    );
    if !yes {
        if !(std::io::stdin().is_terminal() && std::io::stdout().is_terminal()) {
            bail!(tr(
                "Die Freigabe braucht ein Terminal, in dem du bestätigst.",
                "The approval needs a terminal in which you confirm."
            ));
        }
        if !project::confirm(tr("Freigeben?", "Approve?"), false)? {
            app_request::remove(config, &request.id);
            println!("{}", tr("Abgelehnt.", "Refused."));
            return Ok(false);
        }
    }
    file_app_name_free(config, &request.name, lang::current()).map_err(anyhow::Error::msg)?;
    let app = app_file::add(config, &request.name, &request.title, &path)?;
    app_request::remove(config, &request.id);
    println!(
        "{}",
        tr!(
            "Freigegeben: {} ist die App {}. Einschalten im Browser oder mit `hearth app on {}`.",
            "Approved: {} is the app {}. Switch it on in the browser or with `hearth app on {}`.",
            app.title,
            app.name,
            app.name
        )
    );
    Ok(true)
}

fn deny(config: &ConfigDir, id: &str) -> anyhow::Result<bool> {
    let Some(request) = app_request::load(config, id) else {
        bail!(tr!(
            "Keine wartende Programmdatei mit der ID {id}.",
            "No waiting program file with the ID {id}."
        ));
    };
    app_request::remove(config, &request.id);
    println!(
        "{}",
        tr!(
            "Abgelehnt: {} ({}).",
            "Refused: {} ({}).",
            request.title,
            request.path
        )
    );
    Ok(true)
}

fn forget(config: &ConfigDir, names: &[String]) -> anyhow::Result<bool> {
    for name in names {
        app_file::forget(config, name)?;
        println!(
            "{}",
            tr!(
                "{name}: Rezept und Kopie gelöscht.",
                "{name}: recipe and copy deleted."
            )
        );
    }
    for (project, off) in switch_off_everywhere(config, names)? {
        println!(
            "{}",
            tr!(
                "{project}: ausgeschaltet: {}",
                "{project}: switched off: {}",
                off.join(", ")
            )
        );
    }
    println!(
        "{}",
        tr!(
            "Was schon in der VM installiert ist, löscht `hearth app remove {}`.",
            "What is installed in the VM already is deleted by `hearth app remove {}`.",
            names.join(" ")
        )
    );
    Ok(true)
}

/// Switches a project's apps as the browser asks: only for a project the
/// computer offers to web chats, only apps it knows. Returns whether it
/// worked and what to tell the user (German, the browser translates on its
/// own).
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

/// Takes over the account's templates as the server sent them: the ones
/// this hearth can read become the new copy, and templates that are gone
/// are switched off in every project, whose next start would fail on an
/// unknown app otherwise. Returns what changed, for the service's log;
/// `None` if nothing.
pub(crate) fn apply_templates(
    config: &ConfigDir,
    raw: Vec<serde_json::Value>,
) -> anyhow::Result<Option<String>> {
    let offered = raw.len();
    let templates: Vec<AppTemplate> = raw
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect();
    let before = config.templates();
    if before == templates && config.templates_file().exists() {
        return Ok(None);
    }
    config.save_templates(&templates)?;
    let mut message = tr!(
        "Eigene Programme vom Server: {}",
        "Custom programs from the server: {}",
        templates.len()
    );
    if templates.len() < offered {
        message.push_str(&tr!(
            " ({} kann dieses hearth nicht lesen, bitte aktualisieren)",
            " ({} this hearth cannot read, please update)",
            offered - templates.len()
        ));
    }
    let gone: Vec<String> = before
        .into_iter()
        .map(|template| template.name)
        .filter(|name| matches!(config.app(name), Err(Error::AppUnknown(_))))
        .collect();
    for (project, off) in switch_off_everywhere(config, &gone)? {
        message.push_str(&tr!(
            "; in {project} ausgeschaltet: {}",
            "; switched off in {project}: {}",
            off.join(", ")
        ));
    }
    Ok(Some(message))
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
            println!(
                "{}",
                tr!("{name}: nicht installiert.", "{name}: not installed.")
            );
            continue;
        }
        for install in installs {
            match apps::remove(&provider, install) {
                Ok(()) => println!(
                    "{}",
                    tr!(
                        "{name} {}: gelöscht.",
                        "{name} {}: deleted.",
                        install.version
                    )
                ),
                Err(error) => {
                    all_gone = false;
                    println!(
                        "{}",
                        tr!(
                            "{name} {}: bleibt, weil eine Instanz sie noch einhängt (auch \
                             gestoppte). Erst in den Projekten ausschalten (`hearth app off \
                             {name}`) und sie einmal starten. {error}",
                            "{name} {}: stays, because an instance still mounts it (stopped \
                             ones too). First switch it off in the projects (`hearth app off \
                             {name}`) and start them once. {error}",
                            install.version
                        )
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
        // A newer recipe of hearth does not reinstall by itself (downloads,
        // minutes); `hearth app list` points it out. A template the user
        // changed in the browser does: that is what the change was for.
        let install = match inventory.latest(&app.name) {
            // If the changed one fails, the old install keeps the project
            // usable; the failed step tells why, and the next start retries.
            Some(install)
                if (app.source == AppSource::Server || app.file.is_some())
                    && install.outdated(&app) =>
            {
                install_one(config, provider, project, &app, &ready, progress)
                    .unwrap_or_else(|_| install.clone())
            }
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
        &tr!(in progress.lang();
            "{} installieren (einmalig, für alle Projekte)",
            "Installing {} (once, for all projects)",
            app.title
        ),
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
        Some(project) => println!(
            "{}",
            tr!("Apps für Projekt {}:", "Apps for project {}:", project.name)
        ),
        None => println!(
            "{}",
            tr(
                "App-Katalog (kein Projekt in diesem Ordner):",
                "App catalog (no project in this folder):",
            )
        ),
    }
    let mut outdated = false;
    for app in &catalog {
        let state = match &project {
            Some(project) if project.apps.contains(&app.name) => tr("an", "on").to_owned(),
            Some(_) if active.iter().any(|a| a.name == app.name) => {
                let by: Vec<&str> = active
                    .iter()
                    .filter(|a| a.requires.contains(&app.name))
                    .map(|a| a.title.as_str())
                    .collect();
                tr!("an (für {})", "on (for {})", by.join(", "))
            }
            Some(_) => tr("aus", "off").to_owned(),
            None => String::new(),
        };
        let origin = match app.source {
            AppSource::Builtin => "",
            AppSource::Local => tr("(eigene Datei)", "(own file)"),
            AppSource::Server => tr("(aus dem Browser)", "(from the browser)"),
        };
        let about = format!(
            "{} {origin}",
            app.description.as_deref().unwrap_or_default()
        );
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
            about.trim()
        );
    }
    match &inventory {
        None => println!(
            "{}",
            tr(
                "(Die VM läuft nicht; installierte Versionen zeigt die Liste, wenn sie läuft.)",
                "(The VM is not running; the list shows installed versions when it runs.)",
            )
        ),
        Some(_) => println!(
            "{}",
            tr(
                "(Versionen: installiert in der VM, – nicht installiert)",
                "(Versions: installed in the VM, – not installed)",
            )
        ),
    }
    if outdated {
        println!(
            "{}",
            tr(
                "* Das Rezept ist seither neuer; `hearth app update <name>` installiert neu.",
                "* The recipe is newer since; `hearth app update <name>` installs it again.",
            )
        );
    }
    let pending = app_request::list(config);
    if !pending.is_empty() {
        println!();
        println!(
            "{}",
            tr(
                "Vom Browser angefragt, wartet auf Freigabe:",
                "Asked for by the browser, waiting for approval:",
            )
        );
        for request in pending {
            println!(
                "  {}  {} ({}, {})  →  hearth app approve {}",
                request.id, request.title, request.kind, request.path, request.id
            );
        }
    }
    if project.is_some() {
        println!();
        println!(
            "{}",
            tr(
                "Einschalten mit `hearth app on <name>`; der nächste Start (hearth, Web-Chat) \
                 installiert, was fehlt, einmal für alle Projekte.",
                "Switch on with `hearth app on <name>`; the next start (hearth, web chat) \
                 installs what is missing, once for all projects.",
            )
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
        "{}",
        tr!(
            "Projekt {}: {}",
            "Project {}: {}",
            project.name,
            if titles.is_empty() {
                tr("keine Apps", "no apps").to_owned()
            } else {
                titles.join(", ")
            }
        )
    );
    if !on {
        for name in names {
            if let Some(app) = active.iter().find(|app| app.name == *name) {
                let by: Vec<&str> = active
                    .iter()
                    .filter(|other| other.requires.contains(name))
                    .map(|other| other.title.as_str())
                    .collect();
                println!(
                    "{}",
                    tr!(
                        "{} bleibt, weil {} es braucht.",
                        "{} stays, because {} needs it.",
                        app.title,
                        by.join(", ")
                    )
                );
            }
        }
    }
    println!(
        "{}",
        tr(
            "Gilt ab dem nächsten Start der Instanz (hearth, hearth shell, Web-Chat); eine \
             laufende wird dabei neu angelegt, Arbeitskopie und Home bleiben.",
            "Applies from the instance's next start (hearth, hearth shell, web chat); a \
             running one is created anew then, working copy and home stay.",
        )
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
        bail!(tr!(
            "Projekt {} hat keine Apps eingeschaltet (`hearth app on <name>`)",
            "Project {} has no apps switched on (`hearth app on <name>`)",
            project.name
        ));
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
                println!(
                    "{}",
                    tr!(
                        "{}: {} ist schon installiert.",
                        "{}: {} is installed already.",
                        app.title,
                        install.version
                    )
                );
                install.clone()
            }
            _ => {
                let install = install_one(config, &provider, &project, &app, &ready, &progress)?;
                println!(
                    "{}",
                    tr!(
                        "{}: {} installiert.",
                        "{}: {} installed.",
                        app.title,
                        install.version
                    )
                );
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
        assert_eq!(rust.source, Some(AppSource::Builtin));
    }

    #[test]
    fn templates_from_the_server_join_the_catalog_and_leave_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("demo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let config = ConfigDir::new(dir.path().join("home"));
        let project = config.create_project(&repo).unwrap();
        // As after `hearth init`.
        let on = hearth_core::config::RemoteChange {
            enabled: Some(true),
            ..Default::default()
        };
        config.set_remote(&project.name, on).unwrap();
        let template = |name: &str| {
            serde_json::json!({
                "name": name, "title": name, "kind": "apt", "packages": [name],
                "script": "", "hosts": [], "path": [], "requires": []
            })
        };
        let newer = serde_json::json!({ "name": "x", "title": "X", "kind": "cargo" });

        let message = apply_templates(&config, vec![template("tree"), template("ncdu"), newer])
            .unwrap()
            .unwrap();
        assert!(
            message.contains("2") && message.contains("1 kann"),
            "{message}"
        );
        // The same templates again change nothing.
        let again = vec![template("tree"), template("ncdu"), serde_json::json!({})];
        assert_eq!(apply_templates(&config, again).unwrap(), None);

        let offered = crate::connect::offered(&config);
        let (ok, message) = apply_from_browser(
            &config,
            &offered,
            &project.name.to_string(),
            &["tree".into(), "ncdu".into()],
        );
        assert!(ok, "{message}");
        let report = report(&config, &offered, None);
        let tree = report
            .catalog
            .iter()
            .find(|app| app.name == "tree")
            .unwrap();
        assert_eq!(tree.source, Some(AppSource::Server));
        assert_eq!(tree.description.as_deref(), Some("apt: tree"));

        // Deleted in the browser: switched off, so the project still starts.
        let message = apply_templates(&config, vec![template("ncdu")])
            .unwrap()
            .unwrap();
        assert!(message.contains("ausgeschaltet: tree"), "{message}");
        let apps = config.project(&project.name).unwrap().apps;
        assert_eq!(apps, ["ncdu"]);
        config
            .project_apps(&config.project(&project.name).unwrap())
            .unwrap();
    }
}
