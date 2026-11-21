//! `hearth project host` (switch the direct mode) and `hearth host` (answer
//! its questions in a terminal, read its record).

use std::io::{self, BufRead as _, IsTerminal as _, Write as _};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::bail;
use clap::Subcommand;
use hearth_core::config::{ConfigDir, HostChange, HostConfig, HostWrites};
use hearth_core::host::{Decision, Request, audit, inbox};
use hearth_core::lang::tr;
use hearth_core::sync::sanitize;
use hearth_core::tr;

/// `hearth host …`.
#[derive(Subcommand)]
pub enum HostCommand {
    #[command(about = tr(
        "Offene Freigaben anzeigen (Standard)",
        "Show waiting approvals (default)"
    ))]
    List,
    #[command(about = tr(
        "Eine Freigabe erteilen",
        "Grant an approval"
    ))]
    Approve {
        #[arg(help = tr("ID der Freigabe", "ID of the approval"))]
        id: String,
        #[arg(long, help = tr(
            "Für den ganzen Chat: denselben Befehl bzw. denselben Ordner nicht mehr fragen",
            "For the whole chat: do not ask again for the same command or folder"
        ))]
        chat: bool,
    },
    #[command(about = tr("Eine Freigabe ablehnen", "Deny an approval"))]
    Deny {
        #[arg(help = tr("ID der Freigabe", "ID of the approval"))]
        id: String,
    },
    #[command(about = tr(
        "Wartet auf Freigaben und fragt sie hier nacheinander ab (bis Strg+C)",
        "Waits for approvals and asks for them here one by one (until Ctrl+C)"
    ))]
    Watch,
    #[command(about = tr(
        "Die letzten Einträge im Protokoll eines Projekts",
        "The last entries of a project's record"
    ))]
    Log {
        #[arg(help = tr("Projektname", "Project name"))]
        project: String,
        #[arg(long, default_value_t = 30, help = tr("Anzahl", "Count"))]
        count: usize,
    },
}

/// `hearth project host [on|off] …` for the project of the current folder.
pub fn project_host(config: &ConfigDir, change: HostChange) -> anyhow::Result<bool> {
    let repo = crate::project::current_repo()?;
    let Some(project) = config.find_project(&repo)? else {
        bail!(tr!(
            "{} ist noch kein Projekt. `hearth init` legt es an.",
            "{} is not a project yet. `hearth init` creates it.",
            repo.display()
        ));
    };
    let host = if change == HostChange::default() {
        config.project_config(&project.name)?.host
    } else {
        let host = config.set_host(&project.name, change)?;
        crate::login::announce_projects(config);
        host
    };
    let remote = config.project_config(&project.name)?.remote;
    println!(
        "{}",
        describe(&project.name.to_string(), &host, remote.enabled)
    );
    Ok(true)
}

/// What the direct mode of a project does, for people.
pub fn describe(name: &str, host: &HostConfig, offered: bool) -> String {
    if !host.enabled {
        return tr!(
            "{name}: Direktmodus aus. Web-Chats nutzen die Sandbox. Einschalten: `hearth project host on`.",
            "{name}: direct mode off. Web chats use the sandbox. Switch it on: `hearth project host on`."
        );
    }
    let mut lines = vec![tr!(
        "{name}: Direktmodus an. Der Agent arbeitet ohne VM im Projektordner; jeder Befehl und jeder Zugriff außerhalb des Ordners braucht deine Freigabe.",
        "{name}: direct mode on. The agent works in the project folder without a VM; every command and every access outside the folder needs your approval."
    )];
    lines.push(match host.writes {
        HostWrites::Free => tr(
            "Schreiben im Ordner ohne Rückfrage (außer Git-Hooks, Editor- und Agent-Einstellungen).",
            "Writing in the folder without asking (except git hooks, editor and agent settings).",
        )
        .to_owned(),
        HostWrites::Ask => tr("Jedes Schreiben fragt.", "Every write asks.").to_owned(),
    });
    lines.push(if host.browser_approvals {
        tr(
            "Freigaben gehen auch im Web-Chat. Wer den Server kontrolliert, kann damit Befehle hier ausführen.",
            "Approvals also work in the web chat. Whoever controls the server can run commands here with that.",
        )
        .to_owned()
    } else {
        tr(
            "Freigaben nur an diesem Rechner: Desktop-App, Benachrichtigung oder `hearth host`.",
            "Approvals on this computer only: desktop app, notification or `hearth host`.",
        )
        .to_owned()
    });
    if !offered {
        lines.push(
            tr(
                "Noch nicht für Web-Chats freigegeben: `hearth project remote on`.",
                "Not offered to web chats yet: `hearth project remote on`.",
            )
            .to_owned(),
        );
    }
    lines.join("\n")
}

/// `hearth host …`.
pub fn run(config: &ConfigDir, command: Option<HostCommand>) -> anyhow::Result<bool> {
    match command.unwrap_or(HostCommand::List) {
        HostCommand::List => {
            let pending = inbox::read_pending(config.root());
            if pending.is_empty() {
                println!(
                    "{}",
                    tr("Keine offenen Freigaben.", "No waiting approvals.")
                );
            }
            for item in &pending {
                print_pending(item);
            }
            Ok(true)
        }
        HostCommand::Approve { id, chat } => answer(
            config,
            &id,
            if chat { Decision::Chat } else { Decision::Once },
        ),
        HostCommand::Deny { id } => answer(config, &id, Decision::Deny),
        HostCommand::Watch => watch(config),
        HostCommand::Log { project, count } => {
            for entry in audit::recent(config.root(), &project, count) {
                println!(
                    "{}  {:<6} {:<8} {:<9} {}{}",
                    clock(entry.at / 1000),
                    entry.tool,
                    entry.decision,
                    entry.by,
                    sanitize(&entry.target),
                    entry
                        .exit
                        .map(|code| format!("  [exit {code}]"))
                        .unwrap_or_default()
                );
            }
            Ok(true)
        }
    }
}

fn answer(config: &ConfigDir, id: &str, decision: Decision) -> anyhow::Result<bool> {
    let id = id.trim().to_uppercase();
    if !inbox::read_pending(config.root())
        .iter()
        .any(|item| item.id == id)
    {
        bail!(tr!(
            "Keine offene Freigabe {id}. `hearth host` zeigt die offenen.",
            "No waiting approval {id}. `hearth host` shows the waiting ones."
        ));
    }
    inbox::submit(config.root(), &id, decision)?;
    println!(
        "{}",
        match decision {
            Decision::Deny => tr!("{id} abgelehnt.", "{id} denied."),
            Decision::Chat => tr!(
                "{id} für diesen Chat freigegeben.",
                "{id} approved for this chat."
            ),
            Decision::Once => tr!("{id} freigegeben.", "{id} approved."),
        }
    );
    Ok(true)
}

fn print_pending(item: &inbox::Pending) {
    let chat = item.title.as_deref().unwrap_or(&item.chat);
    println!(
        "[{}] {} · {}",
        item.id,
        sanitize(&item.project),
        sanitize(chat)
    );
    println!("  {}", sanitize(&item.summary));
    if let Request::Command {
        command,
        shell,
        cwd,
    } = &item.request
    {
        for line in command.lines() {
            println!("    {}", sanitize(line));
        }
        println!("  {}", tr!("({shell} in {cwd})", "({shell} in {cwd})"));
    }
    println!("  {}", sanitize(&item.why));
}

/// Asks for each new question here until Ctrl+C.
fn watch(config: &ConfigDir) -> anyhow::Result<bool> {
    if !io::stdin().is_terminal() {
        bail!(tr(
            "`hearth host watch` braucht ein Terminal.",
            "`hearth host watch` needs a terminal."
        ));
    }
    println!(
        "{}",
        tr(
            "Warte auf Freigaben des Direktmodus (Beenden: Strg+C) …",
            "Waiting for approvals of the direct mode (quit: Ctrl+C) …"
        )
    );
    let mut asked = std::collections::HashSet::new();
    loop {
        let pending = inbox::read_pending(config.root());
        let Some(item) = pending.into_iter().find(|item| !asked.contains(&item.id)) else {
            thread::sleep(Duration::from_millis(300));
            continue;
        };
        asked.insert(item.id.clone());
        println!();
        print_pending(&item);
        let choices = if item.chat_wide {
            tr(
                "[e]inmal, für den [c]hat, [a]blehnen? ",
                "[o]nce, for the [c]hat, [d]eny? ",
            )
        } else {
            tr("[e]inmal, [a]blehnen? ", "[o]nce, [d]eny? ")
        };
        print!("{choices}");
        io::stdout().flush()?;
        let mut line = String::new();
        if io::stdin().lock().read_line(&mut line)? == 0 {
            return Ok(true);
        }
        let decision = match line.trim().to_lowercase().as_str() {
            "e" | "o" | "j" | "y" | "einmal" | "once" => Decision::Once,
            "c" | "chat" if item.chat_wide => Decision::Chat,
            _ => Decision::Deny,
        };
        // Answered meanwhile elsewhere (app, chat): nothing to do.
        if !inbox::read_pending(config.root())
            .iter()
            .any(|waiting| waiting.id == item.id)
        {
            println!("{}", tr("Schon beantwortet.", "Already answered."));
            continue;
        }
        inbox::submit(config.root(), &item.id, decision)?;
        println!(
            "{}",
            match decision {
                Decision::Deny => tr("Abgelehnt.", "Denied."),
                Decision::Chat => tr("Für diesen Chat freigegeben.", "Approved for this chat."),
                Decision::Once => tr("Freigegeben.", "Approved."),
            }
        );
    }
}

/// `HH:MM:SS` in UTC, without a date library.
fn clock(secs: u64) -> String {
    let secs = if secs == 0 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |time| time.as_secs())
    } else {
        secs
    };
    let rest = secs % 86_400;
    format!(
        "{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}
