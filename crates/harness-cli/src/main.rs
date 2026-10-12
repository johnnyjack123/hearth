//! The `harness` command.
//!
//! So far only the commands needed to bring up the sandbox base: `doctor` and
//! `image build`. `run`, `shell`, `sync` and friends follow in later Phase 2
//! packages (docs/ROADMAP.md).

use std::process::ExitCode;

use anyhow::Context as _;
use clap::{Parser, Subcommand};
use harness_core::Provider;
use harness_core::image::{BASE_IMAGE, BuildOptions, ImageStatus};
use harness_core::podman::{DEFAULT_MACHINE, MachineConfig, PodmanProvider, find_podman};

#[derive(Parser)]
#[command(
    name = "harness",
    version,
    about = "Coding-Agents in isolierten Containern ausführen"
)]
struct Cli {
    /// Name der Podman-Machine
    #[arg(long, global = true, default_value = DEFAULT_MACHINE)]
    machine: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Prüft Machine, SSH-Verbindung, Austauschordner und Base-Image
    Doctor,
    /// Images verwalten
    Image {
        #[command(subcommand)]
        command: ImageCommand,
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

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Doctor => doctor(&cli.machine),
        Command::Image {
            command: ImageCommand::Build { pull, no_cache },
        } => build_base_image(&cli.machine, BuildOptions { pull, no_cache }).map(|()| true),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("Fehler: {error:#}");
            ExitCode::FAILURE
        }
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

/// Prints one line per check; returns whether all checks passed.
fn doctor(machine_name: &str) -> anyhow::Result<bool> {
    let mut report = Report::default();

    let machine = match MachineConfig::load(machine_name) {
        Ok(machine) => {
            report.ok(format!(
                "Machine „{}“: {}",
                machine.name,
                machine.path.display()
            ));
            machine
        }
        Err(error) => {
            report.fail(error);
            return Ok(report.passed());
        }
    };

    match machine.exchange() {
        Some(exchange) if exchange.host_dir().is_dir() => report.ok(format!(
            "Austauschordner: {} → {}",
            exchange.host_dir().display(),
            exchange.guest_dir()
        )),
        Some(exchange) => report.fail(format!(
            "Austauschordner {} fehlt auf dem Host",
            exchange.host_dir().display()
        )),
        None => report.fail(format!(
            "Machine hat keinen beschreibbaren Austauschordner unter {}",
            harness_core::podman::EXCHANGE_GUEST_DIR
        )),
    }
    for mount in machine.unexpected_mounts() {
        report.fail(format!(
            "Zusätzliche Freigabe {} → {}: Die VM sieht diese Dateien. Im Isoliert-Modus \
             soll nur der Austauschordner freigegeben sein (Machine neu anlegen)",
            mount.source.display(),
            mount.target
        ));
    }

    let podman = match find_podman() {
        Ok(path) => path,
        Err(error) => {
            report.fail(error);
            return Ok(report.passed());
        }
    };
    let state_dir = harness_core::paths::local_data_dir()?.join("podman");
    let provider = match PodmanProvider::new(machine, podman, &state_dir) {
        Ok(provider) => provider,
        Err(error) => {
            report.fail(error);
            return Ok(report.passed());
        }
    };

    let info = match provider.ping() {
        Ok(info) => info,
        Err(error) => {
            report.fail(error);
            report.hint(
                "VM in einer Admin-PowerShell starten: $env:CONTAINERS_MACHINE_PROVIDER='hyperv'; \
                 & \"C:\\Program Files\\RedHat\\Podman\\podman.exe\" machine start harness",
            );
            return Ok(report.passed());
        }
    };
    let line = format!(
        "VM erreichbar über SSH {}@127.0.0.1:{}: {} {} ({}), Podman {}, RAM {} MiB, frei {} MiB",
        provider.machine().ssh_user,
        provider.machine().ssh_port,
        info.distribution,
        info.distribution_version,
        info.arch,
        info.engine_version,
        info.mem_total / (1024 * 1024),
        info.mem_free / (1024 * 1024),
    );
    if info.rootless {
        report.ok(line);
    } else {
        report.fail(format!("{line}, aber nicht rootless"));
    }

    match BASE_IMAGE.status(&provider) {
        Ok(ImageStatus::Current) => report.ok(format!("Base-Image {} aktuell", BASE_IMAGE.tag)),
        Ok(ImageStatus::Outdated) => report.warn(format!(
            "Base-Image {} ist veraltet → harness image build",
            BASE_IMAGE.tag
        )),
        Ok(ImageStatus::Missing) => report.fail(format!(
            "Base-Image {} fehlt → harness image build",
            BASE_IMAGE.tag
        )),
        Err(error) => report.fail(error),
    }

    Ok(report.passed())
}

#[derive(Default)]
struct Report {
    failures: usize,
}

impl Report {
    fn ok(&mut self, message: impl std::fmt::Display) {
        println!("  ✓ {message}");
    }

    fn warn(&mut self, message: impl std::fmt::Display) {
        println!("  ! {message}");
    }

    fn fail(&mut self, message: impl std::fmt::Display) {
        self.failures += 1;
        println!("  ✗ {message}");
    }

    fn hint(&mut self, message: impl std::fmt::Display) {
        println!("    {message}");
    }

    fn passed(&self) -> bool {
        self.failures == 0
    }
}
