//! The `harness` command.
//!
//! Without arguments it starts the default harness in the project of the
//! current folder, setting folder and VM up as needed. The subcommands
//! manage projects, their network and apps, images, the VM service and the
//! pairing with the server (docs/ROADMAP.md, Phase 2).

mod app;
mod app_request;
mod connect;
mod daemon;
mod doctor;
mod idle;
mod login;
mod merge_request;
mod net;
mod progress;
mod project;
mod review;
mod run;
mod setup;
mod status;
mod takeover;
mod terminal;
mod uploads;
mod vm;
mod vmd;

use std::process::ExitCode;

use anyhow::Context as _;
use clap::{Parser, Subcommand, ValueEnum};
use harness_core::Provider;
use harness_core::config::ConfigDir;
use harness_core::image::{BASE_IMAGE, BuildOptions, GATEWAY_IMAGE, IMAGES, ImageDefinition};
use harness_core::lang::tr;
use harness_core::tr;

// Help texts follow the user's language (harness_core::lang), so they are
// attributes rather than doc comments.
#[derive(Parser)]
#[command(
    name = "harness",
    version,
    about = tr(
        "Coding-Agents in isolierten Containern ausführen",
        "Run coding agents in isolated containers"
    )
)]
struct Cli {
    #[arg(long, global = true, help = tr(
        "Name der Podman-Machine (Standard: `machine` aus config.toml)",
        "Name of the Podman machine (default: `machine` from config.toml)"
    ))]
    machine: Option<String>,

    // Without a command: the default harness in this folder's project (like
    // `harness run`; creates repository and project after asking).
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = tr(
        "Prüft Konfiguration, Machine, SSH-Verbindung, VM-Dienst und Images",
        "Checks configuration, machine, SSH connection, VM service and images"
    ))]
    Doctor,
    #[command(about = tr(
        "Richtet harness auf diesem Rechner ein: Hyper-V, git, Podman, VM-Dienst, Images, Server, Autostart",
        "Sets harness up on this computer: Hyper-V, git, Podman, VM service, images, server, autostart"
    ))]
    Setup {
        #[arg(long, help = tr(
            "Server, mit dem dieser Rechner gekoppelt wird (sonst fragt setup)",
            "Server to pair this computer with (otherwise setup asks)"
        ))]
        server: Option<String>,
        #[arg(long, help = tr(
            "Allen Schritten zustimmen, ohne zu fragen (Downloads, Administratorrechte)",
            "Agree to every step without asking (downloads, administrator rights)"
        ))]
        yes: bool,
    },
    #[command(about = tr("Images verwalten", "Manage images"))]
    Image {
        #[command(subcommand)]
        command: ImageCommand,
    },
    #[command(about = tr(
        "Projekt des aktuellen Git-Repositorys anzeigen oder verwalten",
        "Show or manage the project of the current Git repository"
    ))]
    Project {
        #[command(subcommand)]
        command: Option<ProjectCommand>,
    },
    #[command(about = tr(
        "Startet eine Harness (Standard: pi) in der Instanz des Projekts",
        "Starts a harness (default: pi) in the project's instance"
    ))]
    Run {
        #[arg(help = tr(
            "Harness, z. B. pi (Standard: aus der Projektdatei)",
            "Harness, e.g. pi (default: from the project file)"
        ))]
        harness: Option<String>,
        #[arg(last = true, help = tr(
            "Weitere Argumente für die Harness, nach `--`",
            "More arguments for the harness, after `--`"
        ))]
        args: Vec<String>,
    },
    #[command(visible_alias = "ssh", about = tr(
        "Öffnet eine Shell in der Instanz des Projekts (/workspace, mit micro und nano), auch als `harness ssh <projekt>`",
        "Opens a shell in the project's instance (/workspace, with micro and nano), also as `harness ssh <project>`"
    ))]
    Shell {
        #[arg(help = tr(
            "Projektname (Standard: Projekt des aktuellen Ordners)",
            "Project name (default: project of the current folder)"
        ))]
        project: Option<String>,
    },
    #[command(about = tr(
        "Holt die Commits des Agents als Branch agent/<projekt> ins Repository",
        "Fetches the agent's commits into the repository as branch agent/<project>"
    ))]
    Sync {
        #[arg(long, help = tr(
            "Auch einen umgeschriebenen Agent-Branch übernehmen",
            "Take over a rewritten agent branch as well"
        ))]
        force: bool,
    },
    #[command(about = tr(
        "Prüft die Änderungen des Agents und führt agent/<projekt> in den aktuellen Branch zusammen",
        "Checks the agent's changes and merges agent/<project> into the current branch"
    ))]
    Merge {
        #[arg(long, help = tr(
            "Auch bei schweren Funden der Prüfung übernehmen (nach eigener Prüfung)",
            "Take over even with severe findings of the review (after checking yourself)"
        ))]
        ignore_findings: bool,
        #[arg(long, value_name = "ID", conflicts_with = "ignore_findings", help = tr(
            "Gibt einen Merge frei, den ein Web-Chat wegen schwerer Funde angehalten hat (ID aus dem Chat)",
            "Approves a merge a web chat held for severe findings (ID from the chat)"
        ))]
        approve: Option<String>,
    },
    #[command(about = tr(
        "Prüft, was `harness merge` übernehmen würde, ohne es zu übernehmen",
        "Checks what `harness merge` would take over, without taking it over"
    ))]
    Review,
    #[command(about = tr(
        "Stoppt Instanz und Gateway des Projekts",
        "Stops the project's instance and gateway"
    ))]
    Stop,
    #[command(about = tr(
        "Zeigt, was für das Projekt läuft",
        "Shows what runs for the project"
    ))]
    Status,
    #[command(about = tr(
        "Koppelt diesen Rechner mit dem harness-Server (LLM-Zugang mit deinen Keys)",
        "Pairs this computer with the harness server (LLM access with your keys)"
    ))]
    Login {
        #[arg(help = tr(
            "Server-Adresse (Standard: aus config.toml, sonst http://localhost:8080)",
            "Server address (default: from config.toml, else http://localhost:8080)"
        ))]
        url: Option<String>,
        #[arg(long, help = tr(
            "Den Browser nicht öffnen, nur Code und Adresse anzeigen",
            "Do not open the browser, only show code and address"
        ))]
        no_browser: bool,
    },
    #[command(about = tr(
        "Entfernt die Kopplung mit dem Server",
        "Removes the pairing with the server"
    ))]
    Logout,
    #[command(about = tr(
        "Macht den aktuellen Ordner zum Projekt (mit Git, falls nötig) und bietet ihn den Web-Chats deines Kontos an",
        "Makes the current folder a project (with Git if needed) and offers it to the web chats of your account"
    ))]
    Init {
        #[arg(help = tr(
            "Ordner des Projekts (Standard: der aktuelle)",
            "Folder of the project (default: the current one)"
        ))]
        path: Option<std::path::PathBuf>,
        #[arg(help = tr(
            "Name der Umgebung in der Web-App (Standard: Ordnername); wird zu Kleinbuchstaben, Ziffern und '-'",
            "Name of the environment in the web app (default: folder name); becomes lowercase letters, digits and '-'"
        ))]
        name: Option<String>,
    },
    #[command(about = tr(
        "Hintergrunddienst für Web-Chats: Status, Start, Stopp, Autostart",
        "Background service for web chats: status, start, stop, autostart"
    ))]
    Daemon {
        #[command(subcommand)]
        command: Option<daemon::DaemonCommand>,
    },
    #[command(about = tr(
        "Verbindung für Web-Chats in diesem Terminal statt im Hintergrunddienst (zum Zuschauen, bis Strg+C)",
        "Connection for web chats in this terminal instead of the background service (to watch, until Ctrl+C)"
    ))]
    Connect,
    #[command(about = tr(
        "VM-Dienst: startet die VM ohne Admin-Fenster (einmalig: harness vmd install)",
        "VM service: starts the VM without an admin window (once: harness vmd install)"
    ))]
    Vmd {
        #[command(subcommand)]
        command: Option<vmd::VmdCommand>,
    },
    #[command(about = tr(
        "Netzwerk des Projekts: Voreinstellung, Freigaben, Gateway, Nachfragen",
        "The project's network: preset, permissions, gateway, questions"
    ))]
    Net {
        #[arg(long, global = true, help = tr(
            "Projektname (Standard: Projekt des aktuellen Git-Repositorys)",
            "Project name (default: project of the current Git repository)"
        ))]
        project: Option<String>,
        #[command(subcommand)]
        command: Option<net::NetCommand>,
    },
    #[command(about = tr(
        "Programme für den Agent (App-Katalog): anzeigen, pro Projekt ein- und ausschalten, installieren",
        "Programs for the agent (app catalog): show, switch on and off per project, install"
    ))]
    App {
        #[arg(long, global = true, help = tr(
            "Projektname (Standard: Projekt des aktuellen Git-Repositorys)",
            "Project name (default: project of the current Git repository)"
        ))]
        project: Option<String>,
        #[command(subcommand)]
        command: Option<app::AppCommand>,
    },
}

#[derive(Subcommand)]
enum ImageCommand {
    #[command(about = tr(
        "Baut die Images in der VM (Standard: alle)",
        "Builds the images in the VM (default: all)"
    ))]
    Build {
        #[arg(help = tr("Nur dieses Image", "Only this image"))]
        image: Option<ImageName>,
        #[arg(long, help = tr(
            "Neuere Versionen der Basis-Images holen",
            "Fetch newer versions of the base images"
        ))]
        pull: bool,
        #[arg(long, help = tr("Layer-Cache ignorieren", "Ignore the layer cache"))]
        no_cache: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ImageName {
    #[value(help = tr("Base-Image der Instanzen", "Base image of the instances"))]
    Base,
    #[value(help = tr("Netzwerk-Gateway", "Network gateway"))]
    Gateway,
}

#[derive(Subcommand)]
enum ProjectCommand {
    #[command(about = tr(
        "Zeigt die wirksamen Einstellungen des Projekts (Standard)",
        "Shows the project's effective settings (default)"
    ))]
    Show,
    #[command(about = tr(
        "Legt das aktuelle Git-Repository als Projekt an",
        "Adds the current Git repository as a project"
    ))]
    Add,
    #[command(about = tr(
        "Listet alle Projekte mit Ordner und Freigabe für Web-Chats",
        "Lists all projects with folder and availability for web chats"
    ))]
    List,
    #[command(about = tr(
        "Entfernt ein Projekt: Instanz, Gateway und (ohne --keep-data) die Daten des Agents in der VM; der Ordner bleibt",
        "Removes a project: instance, gateway and (without --keep-data) the agent's data in the VM; the folder stays"
    ))]
    Remove {
        #[arg(help = tr(
            "Projektname (Standard: Projekt des aktuellen Git-Repositorys)",
            "Project name (default: project of the current Git repository)"
        ))]
        name: Option<String>,
        #[arg(long, help = tr(
            "Arbeitskopie und Home des Agents in der VM behalten",
            "Keep the agent's working copy and home in the VM"
        ))]
        keep_data: bool,
        #[arg(long, help = tr("Ohne Nachfrage", "Without asking"))]
        yes: bool,
    },
    #[command(about = tr(
        "Für Web-Chats freigeben (harness connect) oder die Freigabe zurücknehmen",
        "Offer to web chats (harness connect) or withdraw the offer"
    ))]
    Remote {
        #[arg(help = tr(
            "on oder off; ohne Angabe nur anzeigen",
            "on or off; without, only show"
        ))]
        state: Option<Switch>,
        #[arg(long, conflicts_with = "confirm", help = tr(
            "Neue Chats ohne Bestätigung an diesem Rechner zulassen",
            "Allow new chats without confirmation on this computer"
        ))]
        no_confirm: bool,
        #[arg(long, help = tr(
            "Neue Chats wieder an diesem Rechner bestätigen (Standard)",
            "Confirm new chats on this computer again (default)"
        ))]
        confirm: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Switch {
    #[value(help = tr("Einschalten", "Switch on"))]
    On,
    #[value(help = tr("Ausschalten", "Switch off"))]
    Off,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = run(cli);
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("{} {error:#}", tr("Fehler:", "Error:"));
            ExitCode::FAILURE
        }
    }
}

/// Returns whether the command succeeded; hard errors propagate.
fn run(cli: Cli) -> anyhow::Result<bool> {
    let config = ConfigDir::open_default()?;
    // The background service stops an idle VM; not while a command uses it.
    // The service itself counts its own sessions.
    let _in_use = match &cli.command {
        None
        | Some(
            Command::Run { .. }
            | Command::Shell { .. }
            | Command::Sync { .. }
            | Command::Stop
            | Command::Status
            | Command::Image { .. }
            | Command::Setup { .. }
            | Command::Net { .. }
            | Command::App {
                command:
                    Some(
                        app::AppCommand::Install { .. }
                        | app::AppCommand::Update { .. }
                        | app::AppCommand::Remove { .. },
                    ),
                ..
            }
            | Command::Project {
                command: Some(ProjectCommand::Remove { .. }),
            },
        ) => vm::mark_in_use(&machine_name(&config, cli.machine.clone())?),
        Some(_) => None,
    };
    let Some(command) = cli.command else {
        let machine = machine_name(&config, cli.machine)?;
        return run::run(&config, &machine, None, Vec::new());
    };
    match command {
        Command::Doctor => doctor::run(&config, cli.machine.as_deref()),
        Command::Setup { server, yes } => setup::run(
            &config,
            &machine_name(&config, cli.machine)?,
            setup::Options { server, yes },
        ),
        Command::Image {
            command:
                ImageCommand::Build {
                    image,
                    pull,
                    no_cache,
                },
        } => {
            let machine = machine_name(&config, cli.machine)?;
            let images: &[ImageDefinition] = match image {
                None => IMAGES,
                Some(ImageName::Base) => &[BASE_IMAGE],
                Some(ImageName::Gateway) => &[GATEWAY_IMAGE],
            };
            build_images(&machine, images, BuildOptions { pull, no_cache })?;
            Ok(true)
        }
        Command::Project { command } => match command.unwrap_or(ProjectCommand::Show) {
            ProjectCommand::Show => project::show(&config),
            ProjectCommand::Add => project::add(&config),
            ProjectCommand::List => project::list(&config),
            ProjectCommand::Remove {
                name,
                keep_data,
                yes,
            } => project::remove(
                &config,
                &machine_name(&config, cli.machine)?,
                name.as_deref(),
                keep_data,
                yes,
            ),
            ProjectCommand::Remote {
                state,
                no_confirm,
                confirm,
            } => project::remote(
                &config,
                state.map(|s| matches!(s, Switch::On)),
                (no_confirm || confirm).then_some(confirm),
            ),
        },
        Command::Run { harness, args } => {
            let machine = machine_name(&config, cli.machine)?;
            run::run(&config, &machine, harness.as_deref(), args)
        }
        Command::Shell { project } => run::shell(
            &config,
            &machine_name(&config, cli.machine)?,
            project.as_deref(),
        ),
        Command::Sync { force } => run::sync(&config, &machine_name(&config, cli.machine)?, force),
        Command::Merge {
            approve: Some(id), ..
        } => takeover::approve(&config, &machine_name(&config, cli.machine)?, &id),
        Command::Merge {
            ignore_findings, ..
        } => run::merge(&config, ignore_findings),
        Command::Review => review::run(&config),
        Command::Stop => run::stop(&config, &machine_name(&config, cli.machine)?),
        Command::Status => run::status(&config, &machine_name(&config, cli.machine)?),
        Command::Login { url, no_browser } => login::login(&config, url, !no_browser),
        Command::Logout => login::logout(&config),
        Command::Connect => connect::connect(&config, &machine_name(&config, cli.machine)?),
        Command::Init { path, name } => project::init(&config, path.as_deref(), name.as_deref()),
        Command::Daemon { command } => {
            daemon::run(&config, &machine_name(&config, cli.machine)?, command)
        }
        Command::Vmd { command } => {
            vmd::run(&config, &machine_name(&config, cli.machine)?, command)
        }
        Command::Net { project, command } => {
            let machine = machine_name(&config, cli.machine)?;
            net::run(&config, &machine, project.as_deref(), command)
        }
        Command::App { project, command } => {
            let machine = machine_name(&config, cli.machine)?;
            app::run(&config, &machine, project.as_deref(), command)
        }
    }
}

/// `--machine` if given, else the machine from `config.toml`.
fn machine_name(config: &ConfigDir, flag: Option<String>) -> anyhow::Result<String> {
    match flag {
        Some(machine) => Ok(machine),
        None => Ok(config.global()?.machine),
    }
}

fn build_images(
    machine: &str,
    images: &[ImageDefinition],
    options: BuildOptions,
) -> anyhow::Result<()> {
    let provider = vm::provider(machine)?;
    provider.ping().context(tr(
        "VM prüfen (harness doctor zeigt Details)",
        "Checking the VM (harness doctor shows details)",
    ))?;
    for image in images {
        let tag = image.tag;
        if image.prebuilt() {
            println!(
                "{}",
                tr!(
                    "Baue {tag} (Programm liegt bei, nichts zu kompilieren) …",
                    "Building {tag} (program included, nothing to compile) …"
                )
            );
        } else {
            println!("{}", tr!("Baue {tag} …", "Building {tag} …"));
        }
        image.build(&provider, options)?;
        println!("{}", tr!("{tag} gebaut.", "{tag} built."));
    }
    Ok(())
}
