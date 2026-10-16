//! `harness net`: a project's network rules, its gateway, and answering the
//! gateway's questions.

use std::collections::VecDeque;
use std::io::{self, BufRead, Write};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, bail};
use clap::{Subcommand, ValueEnum};
use harness_core::config::{ConfigDir, DomainList, DomainPattern, NetworkPreset, Project};
use harness_core::gateway::{
    self, Event, GatewayControl, GatewayState, Host, LogEntry, Mode, Reason, Reply, Scope, Status,
    Watch,
};
use harness_core::podman::PodmanProvider;

use crate::{login, project};

/// Minutes behind answer `m`.
const TEMPORARY_MINUTES: u64 = 15;

#[derive(Subcommand)]
pub enum NetCommand {
    /// Zeigt Regeln, Gateway, Freigaben und die letzten Verbindungen (Standard)
    Status,
    /// Zeigt Verbindungen live und fragt bei nicht freigegebenen Domains nach
    Watch,
    /// Erlaubt eine Domain, bis das Gateway stoppt (oder kürzer bzw. dauerhaft)
    Allow {
        /// z. B. example.com oder *.example.com
        domain: String,
        /// Nur für so viele Minuten
        #[arg(long, conflicts_with = "always")]
        minutes: Option<u64>,
        /// Dauerhaft in die allow-Liste des Projekts eintragen
        #[arg(long)]
        always: bool,
    },
    /// Sperrt eine Domain, bis das Gateway stoppt (oder dauerhaft)
    Deny {
        /// z. B. example.com oder *.example.com
        domain: String,
        /// Dauerhaft in die deny-Liste des Projekts eintragen
        #[arg(long)]
        always: bool,
    },
    /// Setzt die Voreinstellung des Projekts
    Preset {
        /// off: nur LLM-APIs · development: Paketquellen und Git-Hoster ·
        /// open: ganzes Internet · custom: nur die allow-Liste
        preset: PresetArg,
    },
    /// Startet das Gateway des Projekts und wendet die Regeln an
    Start,
    /// Stoppt das Gateway; vorübergehende Freigaben verfallen
    Stop,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum PresetArg {
    /// Nur die LLM-APIs
    Off,
    /// Paketquellen, Git-Hoster, LLM-APIs und die allow-Liste
    Development,
    /// Gesamtes öffentliches Internet (Host, LAN und VM bleiben gesperrt)
    Open,
    /// Nur die LLM-APIs und die allow-Liste
    Custom,
}

impl From<PresetArg> for NetworkPreset {
    fn from(preset: PresetArg) -> Self {
        match preset {
            PresetArg::Off => NetworkPreset::Off,
            PresetArg::Development => NetworkPreset::Development,
            PresetArg::Open => NetworkPreset::Open,
            PresetArg::Custom => NetworkPreset::Custom,
        }
    }
}

/// Runs a `harness net` subcommand.
pub fn run(
    config: &ConfigDir,
    machine: &str,
    project_name: Option<&str>,
    command: Option<NetCommand>,
) -> anyhow::Result<bool> {
    let project = project::resolve(config, project_name)?;
    match command.unwrap_or(NetCommand::Status) {
        NetCommand::Status => status(machine, &project),
        NetCommand::Watch => watch(config, machine, &project),
        NetCommand::Allow {
            domain,
            minutes,
            always,
        } => allow(config, machine, &project, &domain, minutes, always),
        NetCommand::Deny { domain, always } => deny(config, machine, &project, &domain, always),
        NetCommand::Preset { preset } => set_preset(config, machine, &project, preset.into()),
        NetCommand::Start => {
            let provider = PodmanProvider::for_machine(machine)?;
            let (llm, llm_note) = login::gateway_llm(config, &provider)?;
            gateway::ensure_running(&provider, &project, llm.as_ref())?;
            println!(
                "Gateway {} läuft, Regeln von {} angewendet.",
                project.name.gateway(),
                project.name
            );
            print_rules(&project);
            row("LLM:", llm_note);
            Ok(true)
        }
        NetCommand::Stop => {
            let provider = PodmanProvider::for_machine(machine)?;
            gateway::stop(&provider, &project.name)?;
            println!("Gateway {} gestoppt.", project.name.gateway());
            Ok(true)
        }
    }
}

fn status(machine: &str, project: &Project) -> anyhow::Result<bool> {
    println!("Netzwerk von Projekt {}", project.name);
    print_rules(project);
    let provider = PodmanProvider::for_machine(machine)?;
    let state = match gateway::state(&provider, &project.name) {
        Ok(state) => state,
        Err(error) => {
            row("Gateway:", format!("nicht erreichbar ({error})"));
            return Ok(false);
        }
    };
    match state {
        GatewayState::Missing => row("Gateway:", "noch nicht angelegt (harness net start)"),
        GatewayState::Stopped => row("Gateway:", "gestoppt (harness net start)"),
        GatewayState::Running => {
            row("Gateway:", format!("läuft ({})", project.name.gateway()));
            let status = GatewayControl::new(&provider, &project.name).status()?;
            print_runtime(project, &status);
        }
    }
    Ok(true)
}

/// One aligned `label value` line of the overviews.
fn row(label: &str, value: impl std::fmt::Display) {
    println!("  {label:<16}{value}");
}

fn print_rules(project: &Project) {
    let policy = gateway::policy(project);
    let ask = if project.ask { "an" } else { "aus" };
    row(
        "Voreinstellung:",
        format!("{} (nachfragen: {ask})", project.network),
    );
    if policy.mode == Mode::Open {
        row("Erlaubt:", "gesamtes öffentliches Internet");
        row(
            "",
            "Achtung: Der Agent kann alles, was er im Container sieht, nach außen schicken.",
        );
    } else {
        row("Erlaubt:", join(&policy.allow));
    }
    if !policy.deny.is_empty() {
        row("Gesperrt:", join(&policy.deny));
    }
    row("Immer gesperrt:", "Windows-Host, lokales Netz, Tailnet, VM");
}

fn print_runtime(project: &Project, status: &Status) {
    if status.policy != gateway::policy(project) {
        println!(
            "  ! Das Gateway hat andere Regeln als die Konfiguration → harness net start wendet sie an"
        );
    }
    match &status.llm {
        Some(url) => row("LLM:", format!("{} → {url}", gateway::LLM_HOST)),
        None => row(
            "LLM:",
            "nicht eingerichtet (harness login, dann harness net start)",
        ),
    }
    if !status.grants.is_empty() {
        let grants: Vec<String> = status
            .grants
            .iter()
            .map(|g| match g.remaining_secs {
                Some(seconds) => format!("{} (noch {})", g.pattern, duration(seconds)),
                None => format!("{} (bis zum Stopp)", g.pattern),
            })
            .collect();
        row("Freigaben:", grants.join(", "));
    }
    if !status.denied.is_empty() {
        row(
            "Abgelehnt:",
            format!("{} (bis zum Stopp)", join(&status.denied)),
        );
    }
    for (id, host, port) in &status.pending {
        row(
            "Offene Frage:",
            format!("#{id} {host}:{port} → harness net watch"),
        );
    }
    if status.watchers > 0 {
        row(
            "Beobachter:",
            format!("{} (harness net watch)", status.watchers),
        );
    }
    if !status.log.is_empty() {
        println!();
        println!("Letzte Verbindungen:");
        let now = unix_ms();
        for entry in status.log.iter().rev().take(20).rev() {
            let age = duration(now.saturating_sub(entry.time_ms) / 1000);
            println!("  vor {age:<8} {}", log_line(entry));
        }
    }
}

fn allow(
    config: &ConfigDir,
    machine: &str,
    project: &Project,
    domain: &str,
    minutes: Option<u64>,
    always: bool,
) -> anyhow::Result<bool> {
    let pattern = parse_domain(domain)?;
    if project
        .deny
        .iter()
        .any(|d| *d == pattern || d.matches(domain))
    {
        bail!(
            "{pattern} steht in der deny-Liste von {} und bleibt gesperrt. Erst dort entfernen: {}",
            project.name,
            project.file.display()
        );
    }
    if always {
        let (updated, changed) =
            config.add_network_domain(&project.name, DomainList::Allow, &pattern)?;
        if changed {
            println!("{pattern} dauerhaft erlaubt ({}).", updated.file.display());
        } else {
            println!("{pattern} steht schon in der allow-Liste.");
        }
        apply_and_report(machine, &updated);
        if updated.network == NetworkPreset::Off {
            println!("Hinweis: Bei preset = \"off\" gilt die allow-Liste nicht.");
        }
        return Ok(true);
    }
    let provider = PodmanProvider::for_machine(machine)?;
    require_running(&provider, project)?;
    let scope = match minutes {
        Some(0) => bail!("--minutes muss mindestens 1 sein"),
        Some(minutes) => Scope::For(minutes * 60),
        None => Scope::Session,
    };
    GatewayControl::new(&provider, &project.name).allow(&pattern, scope)?;
    match minutes {
        Some(minutes) => println!("{pattern} für {minutes} Minuten erlaubt."),
        None => println!("{pattern} erlaubt, bis das Gateway stoppt (dauerhaft: --always)."),
    }
    Ok(true)
}

fn deny(
    config: &ConfigDir,
    machine: &str,
    project: &Project,
    domain: &str,
    always: bool,
) -> anyhow::Result<bool> {
    let pattern = parse_domain(domain)?;
    if always {
        let (updated, changed) =
            config.add_network_domain(&project.name, DomainList::Deny, &pattern)?;
        if changed {
            println!("{pattern} dauerhaft gesperrt ({}).", updated.file.display());
        } else {
            println!("{pattern} steht schon in der deny-Liste.");
        }
        apply_and_report(machine, &updated);
        return Ok(true);
    }
    let provider = PodmanProvider::for_machine(machine)?;
    require_running(&provider, project)?;
    GatewayControl::new(&provider, &project.name).deny(&pattern)?;
    println!("{pattern} gesperrt, bis das Gateway stoppt (dauerhaft: --always).");
    Ok(true)
}

fn set_preset(
    config: &ConfigDir,
    machine: &str,
    project: &Project,
    preset: NetworkPreset,
) -> anyhow::Result<bool> {
    let updated = config.set_network_preset(&project.name, preset)?;
    println!("Voreinstellung von {} ist jetzt {preset}.", updated.name);
    print_rules(&updated);
    apply_and_report(machine, &updated);
    Ok(true)
}

/// Applies changed rules to a running gateway. A stopped gateway or an
/// unreachable VM is not an error: the file is the source of truth and the
/// next start applies it.
fn apply_and_report(machine: &str, project: &Project) {
    let applied = PodmanProvider::for_machine(machine)
        .map_err(anyhow::Error::from)
        .and_then(|provider| {
            if gateway::state(&provider, &project.name)? != GatewayState::Running {
                return Ok(false);
            }
            GatewayControl::new(&provider, &project.name).apply(&gateway::policy(project))?;
            Ok(true)
        });
    match applied {
        Ok(true) => println!("Im laufenden Gateway angewendet."),
        Ok(false) => println!("Gilt ab dem nächsten Start des Gateways."),
        Err(error) => println!("Gateway nicht erreichbar ({error:#}). Gilt ab dem nächsten Start."),
    }
}

fn require_running(provider: &PodmanProvider, project: &Project) -> anyhow::Result<()> {
    if gateway::state(provider, &project.name)? != GatewayState::Running {
        bail!(
            "Gateway {} läuft nicht (harness net start). Dauerhafte Änderungen gehen mit --always",
            project.name.gateway()
        );
    }
    Ok(())
}

fn parse_domain(domain: &str) -> anyhow::Result<DomainPattern> {
    domain.parse::<DomainPattern>().map_err(anyhow::Error::msg)
}

/// A question of the gateway shown to the user.
pub struct Question {
    /// Id for the answer.
    pub id: u64,
    /// The host the agent wants to reach.
    pub host: Host,
    /// Its port.
    pub port: u16,
}

/// Under a question: the gateway only sees a connection from the container,
/// not which program in it made it.
pub const WHO_ASKS: &str =
    "Das kann der Agent sein, die Harness selbst oder ein Programm, das der Agent gestartet hat.";

/// The answers, as shown after a question.
pub fn answers() -> String {
    format!("[e] einmal  [m] {TEMPORARY_MINUTES} Minuten  [s] bis zum Stopp  [i] immer  [n] nein")
}

/// The decision behind an answer key, `None` for keys that are no answer.
/// Answer `i` also writes the host into the project's allow list and
/// returns what happened as a note.
pub fn decide(
    config: &ConfigDir,
    project: &Project,
    question: &Question,
    key: &str,
) -> Option<(Option<Scope>, Option<String>)> {
    Some(match key {
        "e" => (Some(Scope::Once), None),
        "m" => (Some(Scope::For(TEMPORARY_MINUTES * 60)), None),
        "s" => (Some(Scope::Session), None),
        "i" => (
            Some(Scope::Session),
            Some(remember(config, project, &question.host)),
        ),
        "n" => (None, None),
        _ => return None,
    })
}

fn watch(config: &ConfigDir, machine: &str, project: &Project) -> anyhow::Result<bool> {
    let provider = PodmanProvider::for_machine(machine)?;
    require_running(&provider, project)?;
    let mut watch = GatewayControl::new(&provider, &project.name)
        .watch()
        .context("Verbindung zum Gateway")?;

    println!(
        "Gateway {} · Voreinstellung {} · nachfragen: {}",
        project.name.gateway(),
        project.network,
        if project.ask { "an" } else { "aus" }
    );
    if !project.ask {
        println!(
            "Nachfragen ist aus (ask = false in {}), hier wird nur mitgeschrieben.",
            project.file.display()
        );
    }
    println!("Beenden: q und Enter (oder Strg+C).");

    let input = spawn_input();
    let mut questions: VecDeque<Question> = VecDeque::new();
    let mut asking: Option<u64> = None;
    let mut held: Vec<String> = Vec::new();
    let mut last_ping = Instant::now();
    let mut input_open = true;

    loop {
        if last_ping.elapsed() >= Watch::PING_INTERVAL {
            watch.ping()?;
            last_ping = Instant::now();
        }
        match watch.next(Duration::from_millis(200))? {
            Some(Reply::Event(Event::Pending { id, host, port })) => {
                questions.push_back(Question { id, host, port });
            }
            Some(Reply::Event(Event::Resolved {
                id,
                allowed,
                reason,
            })) => {
                if let Some(index) = questions.iter().position(|q| q.id == id) {
                    let question = questions.remove(index).expect("index from position");
                    if asking == Some(id) {
                        asking = None;
                        println!();
                        match (allowed, reason) {
                            (true, _) => println!("  → erlaubt"),
                            (false, Reason::DeniedByUser) => println!("  → abgelehnt"),
                            // Timed out, or answered by a rule change elsewhere.
                            (false, reason) => {
                                println!("  → {} blockiert ({})", question.host, reason.describe())
                            }
                        }
                        flush_held(&mut held);
                    }
                }
            }
            Some(Reply::Event(Event::Log(entry))) => {
                let line = log_line(&entry);
                if asking.is_some() {
                    held.push(line);
                } else {
                    println!("{line}");
                }
            }
            Some(Reply::Error(message)) => eprintln!("Gateway: {message}"),
            Some(Reply::Ok | Reply::Data(_)) | None => {}
        }

        if asking.is_none()
            && input_open
            && let Some(question) = questions.front()
        {
            ask(question);
            asking = Some(question.id);
        }

        let line = match input.try_recv() {
            Ok(line) => line,
            Err(TryRecvError::Empty) => continue,
            Err(TryRecvError::Disconnected) => {
                if input_open {
                    input_open = false;
                    println!("(Keine Eingabe mehr möglich, Fragen bleiben unbeantwortet.)");
                }
                continue;
            }
        };
        let answer = line.trim().to_lowercase();
        if answer == "q" {
            return Ok(true);
        }
        let Some(id) = asking else {
            continue;
        };
        let question = questions
            .iter()
            .find(|q| q.id == id)
            .expect("asked question is queued");
        let Some((decision, note)) = decide(config, project, question, &answer) else {
            ask(question);
            continue;
        };
        if let Some(note) = note {
            println!("  {note}");
        }
        watch.decide(id, decision)?;
        // The `resolved` event removes the question and prints the outcome.
    }
}

fn ask(question: &Question) {
    println!();
    println!(
        "? Ein Programm in der Sandbox möchte {}:{} erreichen.",
        question.host, question.port
    );
    println!("  {WHO_ASKS}");
    print!("  {} > ", answers());
    let _ = io::stdout().flush();
}

/// Answer "immer": the host goes into the project's allow list.
fn remember(config: &ConfigDir, project: &Project, host: &Host) -> String {
    let Some(pattern) = host.name().and_then(|n| n.parse::<DomainPattern>().ok()) else {
        return format!(
            "{host} ist keine Domain, die sich dauerhaft eintragen lässt; erlaubt bis zum Stopp."
        );
    };
    match config.add_network_domain(&project.name, DomainList::Allow, &pattern) {
        Ok((updated, _)) => format!(
            "{pattern} steht jetzt in der allow-Liste ({}).",
            updated.file.display()
        ),
        Err(error) => format!("Konnte {pattern} nicht eintragen: {error}"),
    }
}

fn flush_held(held: &mut Vec<String>) {
    for line in held.drain(..) {
        println!("{line}");
    }
}

/// Lines from stdin, read on their own thread so the event loop never blocks.
fn spawn_input() -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

fn log_line(entry: &LogEntry) -> String {
    format!(
        "{} {}:{} · {}",
        if entry.allowed { "✓" } else { "✗" },
        entry.host,
        entry.port,
        entry.reason.describe()
    )
}

fn join(patterns: &[DomainPattern]) -> String {
    if patterns.is_empty() {
        "–".to_owned()
    } else {
        patterns
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn duration(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds} s"),
        60..3600 => format!("{} min", seconds / 60),
        _ => format!("{} h", seconds / 3600),
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}
