//! The `harness` command.
//!
//! Without arguments it starts the default harness in the project of the
//! current folder, setting folder and VM up as needed. The subcommands
//! manage projects, their network, images, the VM service and the pairing
//! with the server (docs/ROADMAP.md, Phase 2).

mod doctor;
mod login;
mod net;
mod project;
mod run;
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
    /// Öffnet eine Shell in der Instanz des Projekts
    Shell,
    /// Holt die Commits des Agents als Branch agent/<projekt> ins Repository
    Sync {
        /// Auch einen umgeschriebenen Agent-Branch übernehmen
        #[arg(long)]
        force: bool,
    },
    /// Führt agent/<projekt> in den aktuellen Branch zusammen
    Merge,
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
    /// Listet alle Projekte
    List,
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
    let Some(command) = cli.command else {
        let machine = machine_name(&config, cli.machine)?;
        return run::run(&config, &machine, None, Vec::new());
    };
    match command {
        Command::Doctor => doctor::run(&config, cli.machine.as_deref()),
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
        },
        Command::Run { harness, args } => {
            let machine = machine_name(&config, cli.machine)?;
            run::run(&config, &machine, harness.as_deref(), args)
        }
        Command::Shell => run::shell(&config, &machine_name(&config, cli.machine)?),
        Command::Sync { force } => run::sync(&config, &machine_name(&config, cli.machine)?, force),
        Command::Merge => run::merge(&config),
        Command::Stop => run::stop(&config, &machine_name(&config, cli.machine)?),
        Command::Status => run::status(&config, &machine_name(&config, cli.machine)?),
        Command::Login { url, no_browser } => login::login(&config, url, !no_browser),
        Command::Logout => login::logout(&config),
        Command::Vmd { command } => vmd::run(&machine_name(&config, cli.machine)?, command),
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
