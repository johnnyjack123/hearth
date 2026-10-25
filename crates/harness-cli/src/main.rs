//! The `harness` command.
//!
//! Without arguments it starts the default harness in the project of the
//! current folder, setting folder and VM up as needed. The subcommands
//! manage projects, their network, images, the VM service and the pairing
//! with the server (docs/ROADMAP.md, Phase 2).

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
mod vm;
mod vmd;

use std::process::ExitCode;

use anyhow::Context as _;
use clap::{Parser, Subcommand, ValueEnum};
use harness_core::Provider;
use harness_core::config::ConfigDir;
use harness_core::image::{BASE_IMAGE, BuildOptions, GATEWAY_IMAGE, IMAGES, ImageDefinition};

#[derive(Parser)]
#[command(
    name = "harness",
    version,
    about = "Coding-Agents in isolierten Containern ausführen"
)]
struct Cli {
    /// Name der Podman-Machine (Standard: `machine` aus config.toml)
    #[arg(long, global = true)]
    machine: Option<String>,

    /// Ohne Befehl: die Standard-Harness im Projekt dieses Ordners starten
    /// (wie `harness run`; legt Repository und Projekt nach Rückfrage an)
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Prüft Konfiguration, Machine, SSH-Verbindung, VM-Dienst und Images
    Doctor,
    /// Richtet harness auf diesem Rechner ein: Hyper-V, git, Podman, VM-Dienst, Images, Server, Autostart
    Setup {
        /// Server, mit dem dieser Rechner gekoppelt wird (sonst fragt setup)
        #[arg(long)]
        server: Option<String>,
        /// Allen Schritten zustimmen, ohne zu fragen (Downloads, Administratorrechte)
        #[arg(long)]
        yes: bool,
    },
    /// Images verwalten
    Image {
        #[command(subcommand)]
        command: ImageCommand,
    },
    /// Projekt des aktuellen Git-Repositorys anzeigen oder verwalten
    Project {
        #[command(subcommand)]
        command: Option<ProjectCommand>,
    },
    /// Startet eine Harness (Standard: pi) in der Instanz des Projekts
    Run {
        /// Harness, z. B. pi (Standard: aus der Projektdatei)
        harness: Option<String>,
        /// Weitere Argumente für die Harness, nach `--`
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Öffnet eine Shell in der Instanz des Projekts (/workspace, mit micro und nano), auch als `harness ssh <projekt>`
    #[command(visible_alias = "ssh")]
    Shell {
        /// Projektname (Standard: Projekt des aktuellen Ordners)
        project: Option<String>,
    },
    /// Holt die Commits des Agents als Branch agent/<projekt> ins Repository
    Sync {
        /// Auch einen umgeschriebenen Agent-Branch übernehmen
        #[arg(long)]
        force: bool,
    },
    /// Prüft die Änderungen des Agents und führt agent/<projekt> in den aktuellen Branch zusammen
    Merge {
        /// Auch bei schweren Funden der Prüfung übernehmen (nach eigener Prüfung)
        #[arg(long)]
        ignore_findings: bool,
        /// Gibt einen Merge frei, den ein Web-Chat wegen schwerer Funde angehalten hat (ID aus dem Chat)
        #[arg(long, value_name = "ID", conflicts_with = "ignore_findings")]
        approve: Option<String>,
    },
    /// Prüft, was `harness merge` übernehmen würde, ohne es zu übernehmen
    Review,
    /// Stoppt Instanz und Gateway des Projekts
    Stop,
    /// Zeigt, was für das Projekt läuft
    Status,
    /// Koppelt diesen Rechner mit dem harness-Server (LLM-Zugang mit deinen Keys)
    Login {
        /// Server-Adresse (Standard: aus config.toml, sonst http://localhost:8080)
        url: Option<String>,
        /// Den Browser nicht öffnen, nur Code und Adresse anzeigen
        #[arg(long)]
        no_browser: bool,
    },
    /// Entfernt die Kopplung mit dem Server
    Logout,
    /// Macht den aktuellen Ordner zum Projekt (mit Git, falls nötig) und bietet ihn den Web-Chats deines Kontos an
    Init {
        /// Ordner des Projekts (Standard: der aktuelle)
        path: Option<std::path::PathBuf>,
        /// Name der Umgebung in der Web-App (Standard: Ordnername); wird zu Kleinbuchstaben, Ziffern und '-'
        name: Option<String>,
    },
    /// Hintergrunddienst für Web-Chats: Status, Start, Stopp, Autostart
    Daemon {
        #[command(subcommand)]
        command: Option<daemon::DaemonCommand>,
    },
    /// Verbindung für Web-Chats in diesem Terminal statt im Hintergrunddienst (zum Zuschauen, bis Strg+C)
    Connect,
    /// VM-Dienst: startet die VM ohne Admin-Fenster (einmalig: harness vmd install)
    Vmd {
        #[command(subcommand)]
        command: Option<vmd::VmdCommand>,
    },
    /// Netzwerk des Projekts: Voreinstellung, Freigaben, Gateway, Nachfragen
    Net {
        /// Projektname (Standard: Projekt des aktuellen Git-Repositorys)
        #[arg(long, global = true)]
        project: Option<String>,
        #[command(subcommand)]
        command: Option<net::NetCommand>,
    },
}

#[derive(Subcommand)]
enum ImageCommand {
    /// Baut die Images in der VM (Standard: alle)
    Build {
        /// Nur dieses Image
        image: Option<ImageName>,
        /// Neuere Versionen der Basis-Images holen
        #[arg(long)]
        pull: bool,
        /// Layer-Cache ignorieren
        #[arg(long)]
        no_cache: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ImageName {
    /// Base-Image der Instanzen
    Base,
    /// Netzwerk-Gateway
    Gateway,
}

#[derive(Subcommand)]
enum ProjectCommand {
    /// Zeigt die wirksamen Einstellungen des Projekts (Standard)
    Show,
    /// Legt das aktuelle Git-Repository als Projekt an
    Add,
    /// Listet alle Projekte mit Ordner und Freigabe für Web-Chats
    List,
    /// Entfernt ein Projekt: Instanz, Gateway und (ohne --keep-data) die Daten des Agents in der VM; der Ordner bleibt
    Remove {
        /// Projektname (Standard: Projekt des aktuellen Git-Repositorys)
        name: Option<String>,
        /// Arbeitskopie und Home des Agents in der VM behalten
        #[arg(long)]
        keep_data: bool,
        /// Ohne Nachfrage
        #[arg(long)]
        yes: bool,
    },
    /// Für Web-Chats freigeben (harness connect) oder die Freigabe zurücknehmen
    Remote {
        /// on oder off; ohne Angabe nur anzeigen
        state: Option<Switch>,
        /// Neue Chats ohne Bestätigung an diesem Rechner zulassen
        #[arg(long, conflicts_with = "confirm")]
        no_confirm: bool,
        /// Neue Chats wieder an diesem Rechner bestätigen (Standard)
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Switch {
    /// Einschalten
    On,
    /// Ausschalten
    Off,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = run(cli);
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("Fehler: {error:#}");
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
    provider
        .ping()
        .context("VM prüfen (harness doctor zeigt Details)")?;
    for image in images {
        println!("Baue {} …", image.tag);
        image.build(&provider, options)?;
        println!("{} gebaut.", image.tag);
    }
    Ok(())
}
