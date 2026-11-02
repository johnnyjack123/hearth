//! `harness doctor`: one line per check, from configuration to the images.

use std::fmt::Display;

use harness_core::Provider;
use harness_core::config::{ConfigDir, GlobalConfig};
use harness_core::image::{BASE_IMAGE, GATEWAY_IMAGE, ImageStatus};
use harness_core::lang::tr;
use harness_core::podman::{MachineConfig, PodmanProvider, find_podman};
use harness_core::server::{ServerCa, ServerClient, load_token};
use harness_core::tr;

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
        report.warn(tr(
            "Nicht mit einem harness-Server gekoppelt → harness login",
            "Not paired with a harness server → harness login",
        ));
        return;
    };
    let token = match load_token(url) {
        Ok(Some(token)) => token,
        Ok(None) => {
            report.fail(tr!(
                "Kein Geräte-Token für {url} gespeichert → harness login",
                "No device token stored for {url} → harness login"
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
        Ok((me, count)) => report.ok(tr!(
            "Server {url}: angemeldet als {} (Gerät „{}“), {count} Anbieter",
            "Server {url}: signed in as {} (device “{}”), {count} provider(s)",
            me.user.email,
            me.device.name
        )),
        Err(error) => report.fail(error),
    }
}

/// Checks `~/.harness`; falls back to the defaults so the VM checks still run
/// when the configuration is broken.
fn check_config(config: &ConfigDir, report: &mut Report) -> GlobalConfig {
    let root = config.root();
    if !root.exists() {
        report.warn(tr!(
            "Konfiguration {} fehlt noch, es gelten die Standards (harness project add legt sie an)",
            "Configuration {} does not exist yet, the defaults apply (harness project add creates it)",
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
                report.ok(tr!(
                    "Konfiguration {}: {valid} Projekt(e)",
                    "Configuration {}: {valid} project(s)",
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
            report.ok(tr!(
                "Machine „{}“: {}",
                "Machine “{}”: {}",
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
        report.ok(tr(
            "Keine Host-Ordner in der VM freigegeben",
            "No host folders shared into the VM",
        ));
    }
    for mount in &machine.mounts {
        // Machines from before the copies share the old exchange folder;
        // nothing uses it any more, but it is not dangerous either.
        let legacy = mount.target == "/mnt/exchange";
        let message = tr!(
            "Freigabe {} → {}: Die VM sieht diese Dateien, harness braucht keine Freigaben{}",
            "Share {} → {}: the VM sees these files, harness needs no shares{}",
            mount.source.display(),
            mount.target,
            if legacy {
                tr(
                    " (alter Austauschordner, fällt mit `harness vmd install` weg)",
                    " (old exchange folder, goes away with `harness vmd install`)",
                )
            } else {
                tr(
                    ". Machine ohne Freigaben neu anlegen",
                    ". Create the machine again without shares",
                )
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
        Ok(answer) => report.ok(tr!(
            "VM-Dienst läuft, VM: {:?}",
            "VM service running, VM: {:?}",
            answer.state
        )),
        Err(_) => report.warn(tr(
            "VM-Dienst nicht eingerichtet: `harness vmd install` startet die VM künftig ohne \
             Admin-Fenster",
            "VM service not set up: `harness vmd install` starts the VM without an admin window \
             from then on",
        )),
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
                report.hint(tr(
                    "VM starten: harness vmd start",
                    "Start the VM: harness vmd start",
                ));
            } else {
                report.hint(tr!(
                    "VM in einer Admin-PowerShell starten: $env:CONTAINERS_MACHINE_PROVIDER='hyperv'; \
                     & \"C:\\Program Files\\RedHat\\Podman\\podman.exe\" machine start {machine_name}",
                    "Start the VM in an admin PowerShell: $env:CONTAINERS_MACHINE_PROVIDER='hyperv'; \
                     & \"C:\\Program Files\\RedHat\\Podman\\podman.exe\" machine start {machine_name}"
                ));
            }
            return Ok(());
        }
    };
    let line = tr!(
        "VM erreichbar über SSH {}@127.0.0.1:{}: {} {} ({}), Podman {}, RAM {} MiB, frei {} MiB",
        "VM reachable over SSH {}@127.0.0.1:{}: {} {} ({}), Podman {}, RAM {} MiB, free {} MiB",
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
        report.fail(tr!(
            "{line}, aber nicht rootless",
            "{line}, but not rootless"
        ));
    }

    for (label, image) in [
        (tr("Base-Image", "Base image"), BASE_IMAGE),
        (tr("Gateway-Image", "Gateway image"), GATEWAY_IMAGE),
    ] {
        match image.status(&provider) {
            Ok(ImageStatus::Current) => report.ok(tr!(
                "{label} {} aktuell{}",
                "{label} {} up to date{}",
                image.tag,
                if image.prebuilt() {
                    tr(
                        " (Programm liegt harness bei)",
                        " (program included with harness)",
                    )
                } else {
                    ""
                }
            )),
            Ok(ImageStatus::Outdated) => report.warn(tr!(
                "{label} {} ist veraltet → harness image build {}",
                "{label} {} is outdated → harness image build {}",
                image.tag,
                image.name
            )),
            Ok(ImageStatus::Missing) => report.fail(tr!(
                "{label} {} fehlt → harness image build {}",
                "{label} {} is missing → harness image build {}",
                image.tag,
                image.name
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
