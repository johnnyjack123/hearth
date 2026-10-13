//! `harness doctor`: one line per check, from configuration to the images.

use std::fmt::Display;

use harness_core::Provider;
use harness_core::config::{ConfigDir, GlobalConfig};
use harness_core::image::{BASE_IMAGE, GATEWAY_IMAGE, ImageStatus};
use harness_core::podman::{EXCHANGE_GUEST_DIR, MachineConfig, PodmanProvider, find_podman};

/// Runs all checks; returns whether all passed.
pub fn run(config: &ConfigDir, machine_flag: Option<&str>) -> anyhow::Result<bool> {
    let mut report = Report::default();
    let global = check_config(config, &mut report);
    let machine_name = machine_flag.unwrap_or(&global.machine);
    check_vm(machine_name, &mut report)?;
    Ok(report.passed())
}

/// Checks `~/.harness`; falls back to the defaults so the VM checks still run
/// when the configuration is broken.
fn check_config(config: &ConfigDir, report: &mut Report) -> GlobalConfig {
    let root = config.root();
    if !root.exists() {
        report.warn(format!(
            "Konfiguration {} fehlt noch, es gelten die Standards (harness project add legt sie an)",
            root.display()
        ));
        return GlobalConfig::default();
    }
    let global = match config.global() {
        Ok(global) => global,
        Err(error) => {
            report.fail(error);
            GlobalConfig::default()
        }
    };
    match config.project_names() {
        Ok(names) => {
            let mut valid = 0;
            for name in &names {
                match config.project(name) {
                    Ok(_) => valid += 1,
                    Err(error) => report.fail(error),
                }
            }
            if valid == names.len() {
                report.ok(format!(
                    "Konfiguration {}: {valid} Projekt(e)",
                    root.display()
                ));
            }
        }
        Err(error) => report.fail(error),
    }
    if let Err(error) = config.harness(&global.defaults.harness) {
        report.fail(error);
    }
    global
}

fn check_vm(machine_name: &str, report: &mut Report) -> anyhow::Result<()> {
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
            return Ok(());
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
            "Machine hat keinen beschreibbaren Austauschordner unter {EXCHANGE_GUEST_DIR}"
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
            return Ok(());
        }
    };
    let state_dir = harness_core::paths::local_data_dir()?.join("podman");
    let provider = match PodmanProvider::new(machine, podman, &state_dir) {
        Ok(provider) => provider,
        Err(error) => {
            report.fail(error);
            return Ok(());
        }
    };

    let info = match provider.ping() {
        Ok(info) => info,
        Err(error) => {
            report.fail(error);
            report.hint(format!(
                "VM in einer Admin-PowerShell starten: $env:CONTAINERS_MACHINE_PROVIDER='hyperv'; \
                 & \"C:\\Program Files\\RedHat\\Podman\\podman.exe\" machine start {machine_name}"
            ));
            return Ok(());
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

    for (label, image) in [("Base-Image", BASE_IMAGE), ("Gateway-Image", GATEWAY_IMAGE)] {
        match image.status(&provider) {
            Ok(ImageStatus::Current) => report.ok(format!("{label} {} aktuell", image.tag)),
            Ok(ImageStatus::Outdated) => report.warn(format!(
                "{label} {} ist veraltet → harness image build {}",
                image.tag, image.name
            )),
            Ok(ImageStatus::Missing) => report.fail(format!(
                "{label} {} fehlt → harness image build {}",
                image.tag, image.name
            )),
            Err(error) => report.fail(error),
        }
    }
    Ok(())
}

#[derive(Default)]
struct Report {
    failures: usize,
}

impl Report {
    fn ok(&mut self, message: impl Display) {
        println!("  ✓ {message}");
    }

    fn warn(&mut self, message: impl Display) {
        println!("  ! {message}");
    }

    fn fail(&mut self, message: impl Display) {
        self.failures += 1;
        println!("  ✗ {message}");
    }

    fn hint(&mut self, message: impl Display) {
        println!("    {message}");
    }

    fn passed(&self) -> bool {
        self.failures == 0
    }
}
