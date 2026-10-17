//! `harness doctor`: one line per check, from configuration to the images.

use std::fmt::Display;

use harness_core::Provider;
use harness_core::config::{ConfigDir, GlobalConfig};
use harness_core::image::{BASE_IMAGE, GATEWAY_IMAGE, ImageStatus};
use harness_core::podman::{MachineConfig, PodmanProvider, find_podman};
use harness_core::server::{ServerCa, ServerClient, load_token};

/// Runs all checks; returns whether all passed.
pub fn run(config: &ConfigDir, machine_flag: Option<&str>) -> anyhow::Result<bool> {
    let mut report = Report::default();
    let global = check_config(config, &mut report);
    let machine_name = machine_flag.unwrap_or(&global.machine);
    check_vm(machine_name, &mut report)?;
    check_server(&global, &mut report);
    Ok(report.passed())
}

/// Pairing with the harness server; missing pairing is only a warning,
/// because harnesses can also log in to their provider themselves.
fn check_server(global: &GlobalConfig, report: &mut Report) {
    let Some(url) = &global.server.url else {
        report.warn("Nicht mit einem harness-Server gekoppelt → harness login");
        return;
    };
    let token = match load_token(url) {
        Ok(Some(token)) => token,
        Ok(None) => {
            report.fail(format!(
                "Kein Geräte-Token für {url} gespeichert → harness login"
            ));
            return;
        }
        Err(error) => {
            report.fail(error);
            return;
        }
    };
    let checked = ServerCa::from_config(&global.server).and_then(|ca| {
        let client = ServerClient::new(url, ca.as_ref())?;
        let me = client.device(&token)?;
        let credentials = client.credentials(&token)?;
        Ok((me, credentials.len()))
    });
    match checked {
        Ok((me, count)) => report.ok(format!(
            "Server {url}: angemeldet als {} (Gerät „{}“), {count} Anbieter",
            me.user.email, me.device.name
        )),
        Err(error) => report.fail(error),
    }
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

    if machine.mounts.is_empty() {
        report.ok("Keine Host-Ordner in der VM freigegeben");
    }
    for mount in &machine.mounts {
        // Machines from before the copies share the old exchange folder;
        // nothing uses it any more, but it is not dangerous either.
        let legacy = mount.target == "/mnt/exchange";
        let message = format!(
            "Freigabe {} → {}: Die VM sieht diese Dateien, harness braucht keine Freigaben{}",
            mount.source.display(),
            mount.target,
            if legacy {
                " (alter Austauschordner, fällt mit `harness vmd install` weg)"
            } else {
                ". Machine ohne Freigaben neu anlegen"
            }
        );
        if legacy {
            report.warn(message);
        } else {
            report.fail(message);
        }
    }

    let service = harness_core::vmd::request(harness_core::vmd::Request::Status);
    match &service {
        Ok(answer) => report.ok(format!("VM-Dienst läuft, VM: {:?}", answer.state)),
        Err(_) => report.warn(
            "VM-Dienst nicht eingerichtet: `harness vmd install` startet die VM künftig ohne \
             Admin-Fenster",
        ),
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
            if service.is_ok() {
                report.hint("VM starten: harness vmd start");
            } else {
                report.hint(format!(
                    "VM in einer Admin-PowerShell starten: $env:CONTAINERS_MACHINE_PROVIDER='hyperv'; \
                     & \"C:\\Program Files\\RedHat\\Podman\\podman.exe\" machine start {machine_name}"
                ));
            }
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
