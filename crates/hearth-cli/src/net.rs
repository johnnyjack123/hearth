//! `hearth net`: a project's network rules, its gateway, and answering the
//! gateway's questions.

use std::collections::VecDeque;
use std::io::{self, BufRead, Write};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, bail};
use clap::{Subcommand, ValueEnum};
use hearth_core::config::{ConfigDir, DomainList, DomainPattern, NetworkPreset, Project};
use hearth_core::gateway::{
    self, Event, GatewayControl, GatewayState, Host, LogEntry, Mode, Reason, Reply, Scope, Status,
    Watch,
};
use hearth_core::lang::tr;
use hearth_core::podman::PodmanProvider;
use hearth_core::tr;

use crate::{login, project};

/// Minutes behind answer `m`.
const TEMPORARY_MINUTES: u64 = 15;

// Help texts follow the user's language (hearth_core::lang), so they are
// attributes rather than doc comments.
#[derive(Subcommand)]
pub enum NetCommand {
    #[command(about = tr(
        "Zeigt Regeln, Gateway, Freigaben und die letzten Verbindungen (Standard)",
        "Shows rules, gateway, permissions and the latest connections (default)"
    ))]
    Status,
    #[command(about = tr(
        "Zeigt Verbindungen live und fragt bei nicht freigegebenen Domains nach",
        "Shows connections live and asks about domains that are not allowed"
    ))]
    Watch,
    #[command(about = tr(
        "Erlaubt eine Domain, bis das Gateway stoppt (oder kürzer bzw. dauerhaft)",
        "Allows a domain until the gateway stops (or shorter, or for good)"
    ))]
    Allow {
        #[arg(help = tr(
            "z. B. example.com oder *.example.com",
            "e.g. example.com or *.example.com"
        ))]
        domain: String,
        #[arg(long, conflicts_with = "always", help = tr(
            "Nur für so viele Minuten",
            "Only for this many minutes"
        ))]
        minutes: Option<u64>,
        #[arg(long, help = tr(
            "Dauerhaft in die allow-Liste des Projekts eintragen",
            "Add it to the project's allow list for good"
        ))]
        always: bool,
    },
    #[command(about = tr(
        "Sperrt eine Domain, bis das Gateway stoppt (oder dauerhaft)",
        "Blocks a domain until the gateway stops (or for good)"
    ))]
    Deny {
        #[arg(help = tr(
            "z. B. example.com oder *.example.com",
            "e.g. example.com or *.example.com"
        ))]
        domain: String,
        #[arg(long, help = tr(
            "Dauerhaft in die deny-Liste des Projekts eintragen",
            "Add it to the project's deny list for good"
        ))]
        always: bool,
    },
    #[command(about = tr(
        "Setzt die Voreinstellung des Projekts",
        "Sets the project's preset"
    ))]
    Preset {
        #[arg(help = tr(
            "off: nur LLM-APIs · development: Paketquellen und Git-Hoster · open: ganzes Internet · custom: nur die allow-Liste",
            "off: only LLM APIs · development: package registries and Git hosts · open: the whole internet · custom: only the allow list"
        ))]
        preset: PresetArg,
    },
    #[command(about = tr(
        "Startet das Gateway des Projekts und wendet die Regeln an",
        "Starts the project's gateway and applies the rules"
    ))]
    Start,
    #[command(about = tr(
        "Stoppt das Gateway; vorübergehende Freigaben verfallen",
        "Stops the gateway; temporary permissions expire"
    ))]
    Stop,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum PresetArg {
    #[value(help = tr("Nur die LLM-APIs", "Only the LLM APIs"))]
    Off,
    #[value(help = tr(
        "Paketquellen, Git-Hoster, LLM-APIs und die allow-Liste",
        "Package registries, Git hosts, LLM APIs and the allow list"
    ))]
    Development,
    #[value(help = tr(
        "Gesamtes öffentliches Internet (Host, LAN und VM bleiben gesperrt)",
        "The whole public internet (host, LAN and VM stay blocked)"
    ))]
    Open,
    #[value(help = tr(
        "Nur die LLM-APIs und die allow-Liste",
        "Only the LLM APIs and the allow list"
    ))]
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

/// Runs a `hearth net` subcommand.
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
            let provider = crate::vm::provider(machine)?;
            let (llm, llm_note) = login::gateway_llm(config, &provider)?;
            gateway::ensure_running(&provider, &project, llm.as_ref())?;
            println!(
                "{}",
                tr!(
                    "Gateway {} läuft, Regeln von {} angewendet.",
                    "Gateway {} is running, rules of {} applied.",
                    project.name.gateway(),
                    project.name
                )
            );
            print_rules(&project);
            row("LLM:", llm_note);
            Ok(true)
        }
        NetCommand::Stop => {
            let provider = PodmanProvider::for_machine(machine)?;
            gateway::stop(&provider, &project.name)?;
            println!(
                "{}",
                tr!(
                    "Gateway {} gestoppt.",
                    "Gateway {} stopped.",
                    project.name.gateway()
                )
            );
            Ok(true)
        }
    }
}

fn status(machine: &str, project: &Project) -> anyhow::Result<bool> {
    println!(
        "{}",
        tr!(
            "Netzwerk von Projekt {}",
            "Network of project {}",
            project.name
        )
    );
    print_rules(project);
    let provider = PodmanProvider::for_machine(machine)?;
    let state = match gateway::state(&provider, &project.name) {
        Ok(state) => state,
        Err(error) => {
            row(
                "Gateway:",
                tr!("nicht erreichbar ({error})", "unreachable ({error})"),
            );
            return Ok(false);
        }
    };
    match state {
        GatewayState::Missing => row(
            "Gateway:",
            tr(
                "noch nicht angelegt (hearth net start)",
                "not created yet (hearth net start)",
            ),
        ),
        GatewayState::Stopped => row(
            "Gateway:",
            tr("gestoppt (hearth net start)", "stopped (hearth net start)"),
        ),
        GatewayState::Running => {
            row(
                "Gateway:",
                tr!("läuft ({})", "running ({})", project.name.gateway()),
            );
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

/// "an" or "aus" for a switch, in the user's language.
fn on_off(on: bool) -> &'static str {
    if on { tr("an", "on") } else { tr("aus", "off") }
}

fn print_rules(project: &Project) {
    let policy = gateway::policy(project);
    let ask = on_off(project.ask);
    row(
        tr("Voreinstellung:", "Preset:"),
        tr!("{} (nachfragen: {ask})", "{} (ask: {ask})", project.network),
    );
    if policy.mode == Mode::Open {
        row(
            tr("Erlaubt:", "Allowed:"),
            tr(
                "gesamtes öffentliches Internet",
                "the whole public internet",
            ),
        );
        row(
            "",
            tr(
                "Achtung: Der Agent kann alles, was er im Container sieht, nach außen schicken.",
                "Caution: the agent can send everything it sees in the container to the outside.",
            ),
        );
    } else {
        row(tr("Erlaubt:", "Allowed:"), join(&policy.allow));
    }
    if !policy.deny.is_empty() {
        row(tr("Gesperrt:", "Blocked:"), join(&policy.deny));
    }
    row(
        tr("Immer gesperrt:", "Always blocked:"),
        tr(
            "Windows-Host, lokales Netz, Tailnet, VM",
            "Windows host, local network, tailnet, VM",
        ),
    );
}

fn print_runtime(project: &Project, status: &Status) {
    if status.policy != gateway::policy(project) {
        println!(
            "{}",
            tr(
                "  ! Das Gateway hat andere Regeln als die Konfiguration → hearth net start wendet sie an",
                "  ! The gateway has other rules than the configuration → hearth net start applies them",
            )
        );
    }
    match &status.llm {
        Some(url) => row("LLM:", format!("{} → {url}", gateway::LLM_HOST)),
        None => row(
            "LLM:",
            tr(
                "nicht eingerichtet (hearth login, dann hearth net start)",
                "not set up (hearth login, then hearth net start)",
            ),
        ),
    }
    if !status.grants.is_empty() {
        let grants: Vec<String> = status
            .grants
            .iter()
            .map(|g| match g.remaining_secs {
                Some(seconds) => tr!("{} (noch {})", "{} ({} left)", g.pattern, duration(seconds)),
                None => tr!("{} (bis zum Stopp)", "{} (until stopped)", g.pattern),
            })
            .collect();
        row(tr("Freigaben:", "Permissions:"), grants.join(", "));
    }
    if !status.denied.is_empty() {
        row(
            tr("Abgelehnt:", "Refused:"),
            tr!(
                "{} (bis zum Stopp)",
                "{} (until stopped)",
                join(&status.denied)
            ),
        );
    }
    for (id, host, port) in &status.pending {
        row(
            tr("Offene Frage:", "Open question:"),
            format!("#{id} {host}:{port} → hearth net watch"),
        );
    }
    if status.watchers > 0 {
        row(
            tr("Beobachter:", "Watchers:"),
            format!("{} (hearth net watch)", status.watchers),
        );
    }
    if !status.log.is_empty() {
        println!();
        println!("{}", tr("Letzte Verbindungen:", "Latest connections:"));
        let now = unix_ms();
        for entry in status.log.iter().rev().take(20).rev() {
            let age = duration(now.saturating_sub(entry.time_ms) / 1000);
            // Padded as a whole, since the word order differs by language.
            let age = tr!("vor {age}", "{age} ago");
            println!("  {age:<12} {}", log_line(entry));
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
        bail!(tr!(
            "{pattern} steht in der deny-Liste von {} und bleibt gesperrt. Erst dort entfernen: {}",
            "{pattern} is in the deny list of {} and stays blocked. Remove it there first: {}",
            project.name,
            project.file.display()
        ));
    }
    if always {
        let (updated, changed) =
            config.add_network_domain(&project.name, DomainList::Allow, &pattern)?;
        if changed {
            println!(
                "{}",
                tr!(
                    "{pattern} dauerhaft erlaubt ({}).",
                    "{pattern} allowed for good ({}).",
                    updated.file.display()
                )
            );
        } else {
            println!(
                "{}",
                tr!(
                    "{pattern} steht schon in der allow-Liste.",
                    "{pattern} is already in the allow list."
                )
            );
        }
        apply_and_report(machine, &updated);
        if updated.network == NetworkPreset::Off {
            println!(
                "{}",
                tr(
                    "Hinweis: Bei preset = \"off\" gilt die allow-Liste nicht.",
                    "Note: with preset = \"off\", the allow list does not apply.",
                )
            );
        }
        return Ok(true);
    }
    let provider = crate::vm::provider(machine)?;
    require_running(&provider, project)?;
    let scope = match minutes {
        Some(0) => bail!(tr(
            "--minutes muss mindestens 1 sein",
            "--minutes must be at least 1"
        )),
        Some(minutes) => Scope::For(minutes * 60),
        None => Scope::Session,
    };
    GatewayControl::new(&provider, &project.name).allow(&pattern, scope)?;
    match minutes {
        Some(minutes) => println!(
            "{}",
            tr!(
                "{pattern} für {minutes} Minuten erlaubt.",
                "{pattern} allowed for {minutes} minutes."
            )
        ),
        None => println!(
            "{}",
            tr!(
                "{pattern} erlaubt, bis das Gateway stoppt (dauerhaft: --always).",
                "{pattern} allowed until the gateway stops (for good: --always)."
            )
        ),
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
            println!(
                "{}",
                tr!(
                    "{pattern} dauerhaft gesperrt ({}).",
                    "{pattern} blocked for good ({}).",
                    updated.file.display()
                )
            );
        } else {
            println!(
                "{}",
                tr!(
                    "{pattern} steht schon in der deny-Liste.",
                    "{pattern} is already in the deny list."
                )
            );
        }
        apply_and_report(machine, &updated);
        return Ok(true);
    }
    let provider = crate::vm::provider(machine)?;
    require_running(&provider, project)?;
    GatewayControl::new(&provider, &project.name).deny(&pattern)?;
    println!(
        "{}",
        tr!(
            "{pattern} gesperrt, bis das Gateway stoppt (dauerhaft: --always).",
            "{pattern} blocked until the gateway stops (for good: --always)."
        )
    );
    Ok(true)
}

fn set_preset(
    config: &ConfigDir,
    machine: &str,
    project: &Project,
    preset: NetworkPreset,
) -> anyhow::Result<bool> {
    let updated = config.set_network_preset(&project.name, preset)?;
    println!(
        "{}",
        tr!(
            "Voreinstellung von {} ist jetzt {preset}.",
            "The preset of {} is now {preset}.",
            updated.name
        )
    );
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
        Ok(true) => println!(
            "{}",
            tr(
                "Im laufenden Gateway angewendet.",
                "Applied in the running gateway."
            )
        ),
        Ok(false) => println!(
            "{}",
            tr(
                "Gilt ab dem nächsten Start des Gateways.",
                "Applies from the gateway's next start."
            )
        ),
        Err(error) => println!(
            "{}",
            tr!(
                "Gateway nicht erreichbar ({error:#}). Gilt ab dem nächsten Start.",
                "Gateway unreachable ({error:#}). Applies from the next start."
            )
        ),
    }
}

fn require_running(provider: &PodmanProvider, project: &Project) -> anyhow::Result<()> {
    if gateway::state(provider, &project.name)? != GatewayState::Running {
        bail!(tr!(
            "Gateway {} läuft nicht (hearth net start). Dauerhafte Änderungen gehen mit --always",
            "Gateway {} is not running (hearth net start). Permanent changes work with --always",
            project.name.gateway()
        ));
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
pub const WHO_ASKS: WhoAsks = WhoAsks;

/// The text of [`WHO_ASKS`], in the user's language when it is shown.
pub struct WhoAsks;

impl std::fmt::Display for WhoAsks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(tr(
            "Das kann der Agent sein, die Harness selbst oder ein Programm, das der Agent \
             gestartet hat.",
            "This can be the agent, the harness itself or a program the agent started.",
        ))
    }
}

/// The answers, as shown after a question. The keys follow the language;
/// [`decide`] takes those of both.
pub fn answers() -> String {
    tr!(
        "[e] einmal  [m] {TEMPORARY_MINUTES} Minuten  [s] bis zum Stopp  [i] immer  [n] nein",
        "[o] once  [m] {TEMPORARY_MINUTES} minutes  [s] until stopped  [a] always  [n] no"
    )
}

/// The decision behind an answer key, `None` for keys that are no answer.
/// Answer `i` (`a` in English) also writes the host into the project's
/// allow list and returns what happened as a note. The web chat answers
/// with the German keys.
pub fn decide(
    config: &ConfigDir,
    project: &Project,
    question: &Question,
    key: &str,
) -> Option<(Option<Scope>, Option<String>)> {
    Some(match key {
        "e" | "o" => (Some(Scope::Once), None),
        "m" => (Some(Scope::For(TEMPORARY_MINUTES * 60)), None),
        "s" => (Some(Scope::Session), None),
        "i" | "a" => (
            Some(Scope::Session),
            Some(remember(config, project, &question.host)),
        ),
        "n" => (None, None),
        _ => return None,
    })
}

fn watch(config: &ConfigDir, machine: &str, project: &Project) -> anyhow::Result<bool> {
    let provider = crate::vm::provider(machine)?;
    require_running(&provider, project)?;
    let mut watch = GatewayControl::new(&provider, &project.name)
        .watch()
        .context(tr("Verbindung zum Gateway", "Connection to the gateway"))?;

    println!(
        "{}",
        tr!(
            "Gateway {} · Voreinstellung {} · nachfragen: {}",
            "Gateway {} · preset {} · ask: {}",
            project.name.gateway(),
            project.network,
            on_off(project.ask)
        )
    );
    if !project.ask {
        println!(
            "{}",
            tr!(
                "Nachfragen ist aus (ask = false in {}), hier wird nur mitgeschrieben.",
                "Asking is off (ask = false in {}), this only logs.",
                project.file.display()
            )
        );
    }
    println!(
        "{}",
        tr(
            "Beenden: q und Enter (oder Strg+C).",
            "Quit: q and Enter (or Ctrl+C)."
        )
    );

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
                            (true, _) => println!("  → {}", tr("erlaubt", "allowed")),
                            (false, Reason::DeniedByUser) => {
                                println!("  → {}", tr("abgelehnt", "refused"))
                            }
                            // Timed out, or answered by a rule change elsewhere.
                            (false, reason) => println!(
                                "  → {}",
                                tr!(
                                    "{} blockiert ({})",
                                    "{} blocked ({})",
                                    question.host,
                                    describe(reason)
                                )
                            ),
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
                    println!(
                        "{}",
                        tr(
                            "(Keine Eingabe mehr möglich, Fragen bleiben unbeantwortet.)",
                            "(No more input possible, questions stay unanswered.)"
                        )
                    );
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
        "{}",
        tr!(
            "? Ein Programm in der Sandbox möchte {}:{} erreichen.",
            "? A program in the sandbox wants to reach {}:{}.",
            question.host,
            question.port
        )
    );
    println!("  {WHO_ASKS}");
    print!("  {} > ", answers());
    let _ = io::stdout().flush();
}

/// Answer "immer": the host goes into the project's allow list.
fn remember(config: &ConfigDir, project: &Project, host: &Host) -> String {
    let Some(pattern) = host.name().and_then(|n| n.parse::<DomainPattern>().ok()) else {
        return tr!(
            "{host} ist keine Domain, die sich dauerhaft eintragen lässt; erlaubt bis zum Stopp.",
            "{host} is not a domain that can be added permanently; allowed until stopped."
        );
    };
    match config.add_network_domain(&project.name, DomainList::Allow, &pattern) {
        Ok((updated, _)) => tr!(
            "{pattern} steht jetzt in der allow-Liste ({}).",
            "{pattern} is in the allow list now ({}).",
            updated.file.display()
        ),
        Err(error) => tr!(
            "Konnte {pattern} nicht eintragen: {error}",
            "Could not add {pattern}: {error}"
        ),
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
        describe(entry.reason)
    )
}

/// Why the gateway let a connection through or not; the gateway has both
/// languages.
fn describe(reason: Reason) -> &'static str {
    tr(reason.describe(), reason.describe_en())
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
