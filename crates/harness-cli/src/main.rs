//! The `harness` command.
//!
//! So far the commands needed to bring up the sandbox base and manage
//! projects: `doctor`, `image build` and `project`. `run`, `shell`, `sync` and
//! friends follow in later Phase 2 packages (docs/ROADMAP.md).

mod doctor;
mod project;

use std::process::ExitCode;

use anyhow::Context as _;
use clap::{Parser, Subcommand};
use harness_core::Provider;
use harness_core::config::ConfigDir;
use harness_core::image::{BASE_IMAGE, BuildOptions};
use harness_core::podman::PodmanProvider;

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

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Prüft Konfiguration, Machine, SSH-Verbindung, Austauschordner und Base-Image
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
}

#[derive(Subcommand)]
enum ImageCommand {
    /// Baut das Base-Image in der VM
    Build {
        /// Neuere Versionen der Basis-Images holen
        #[arg(long)]
        pull: bool,
        /// Layer-Cache ignorieren
        #[arg(long)]
        no_cache: bool,
    },
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
    match cli.command {
        Command::Doctor => doctor::run(&config, cli.machine.as_deref()),
        Command::Image {
            command: ImageCommand::Build { pull, no_cache },
        } => {
            let machine = machine_name(&config, cli.machine)?;
            build_base_image(&machine, BuildOptions { pull, no_cache })?;
            Ok(true)
        }
        Command::Project { command } => match command.unwrap_or(ProjectCommand::Show) {
            ProjectCommand::Show => project::show(&config),
            ProjectCommand::Add => project::add(&config),
            ProjectCommand::List => project::list(&config),
        },
    }
}

/// `--machine` if given, else the machine from `config.toml`.
fn machine_name(config: &ConfigDir, flag: Option<String>) -> anyhow::Result<String> {
    match flag {
        Some(machine) => Ok(machine),
        None => Ok(config.global()?.machine),
    }
}

fn build_base_image(machine: &str, options: BuildOptions) -> anyhow::Result<()> {
    let provider = PodmanProvider::for_machine(machine)?;
    provider
        .ping()
        .context("VM prüfen (harness doctor zeigt Details)")?;
    BASE_IMAGE.build(&provider, options)?;
    println!("Base-Image {} gebaut.", BASE_IMAGE.tag);
    Ok(())
}
