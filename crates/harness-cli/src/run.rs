//! `harness run`, `shell`, `sync`, `merge`, `stop` and `status`: the agent's
//! container for the project of the current repository.

use std::collections::BTreeMap;
use std::env;
use std::io::IsTerminal as _;
use std::time::Instant;

use anyhow::bail;
use harness_core::Provider;
use harness_core::config::{ConfigDir, HarnessConfig, LlmWiring, Project};
use harness_core::gateway::{self, GatewayState};
use harness_core::instance::{self, InstanceState};
use harness_core::pi;
use harness_core::podman::PodmanProvider;
use harness_core::provider::ExecSpec;
use harness_core::sync::{
    self, HostRepo, ImportOutcome, MergeOutcome, PullOutcome, WORKSPACE, WorkspaceState, sanitize,
};

use crate::{login, project};

/// Gateway, instance and the clone of the repository, ready for an agent.
struct Prepared {
    provider: PodmanProvider,
    project: Project,
    harnesses: Vec<HarnessConfig>,
}

impl Prepared {
    fn instance(&self) -> String {
        self.project.name.instance()
    }
}

/// Prints `label …` and finishes the line with the duration once `work` is done.
fn step<T>(label: &str, work: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
    print!("{label} … ");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let started = Instant::now();
    let result = work();
    match &result {
        Ok(_) => println!("✓ ({:.1} s)", started.elapsed().as_secs_f32()),
        Err(_) => println!("✗"),
    }
    result
}

/// Starts gateway and instance and brings the repository into the container.
///
/// The instance always mounts the volumes of the project's default harness
/// and of `extra`; otherwise `harness shell` next to a running `harness run`
/// would recreate the container under it.
fn prepare(config: &ConfigDir, machine: &str, extra: Option<&str>) -> anyhow::Result<Prepared> {
    let project = project::resolve(config, None)?;
    let mut harnesses = vec![config.harness(&project.harness)?];
    if let Some(name) = extra.filter(|name| *name != project.harness) {
        harnesses.push(config.harness(name)?);
    }
    let provider = PodmanProvider::for_machine(machine)?;
    let (llm, _) = login::gateway_llm(config, &provider)?;
    step(&format!("Gateway {}", project.name.gateway()), || {
        Ok(gateway::ensure_running(&provider, &project, llm.as_ref())?)
    })?;
    let refs: Vec<&HarnessConfig> = harnesses.iter().collect();
    let created = step(&format!("Instanz {}", project.name.instance()), || {
        Ok(instance::ensure_running(&provider, &project, &refs)?)
    })?;
    if created {
        println!("  (neu angelegt; Arbeitskopie und Home liegen in Volumes und bleiben erhalten)");
    }
    let repo = HostRepo::open(&project.path)?;
    let state = step("Arbeitskopie", || {
        Ok(sync::push_to_workspace(
            &provider,
            &project.name.instance(),
            &repo,
            &project.name,
        )?)
    })?;
    match state {
        WorkspaceState::Cloned { branch } => {
            println!("  geklont nach {WORKSPACE}, Branch {}", sanitize(&branch))
        }
        WorkspaceState::Fetched { branch } => println!(
            "  Stand des Hosts als host/* geholt, Branch {}",
            sanitize(&branch)
        ),
    }
    Ok(Prepared {
        provider,
        project,
        harnesses,
    })
}

fn terminal_env() -> BTreeMap<String, String> {
    // Windows terminals set no TERM; the TUI needs one to draw.
    let term = env::var("TERM").unwrap_or_else(|_| "xterm-256color".to_owned());
    [
        ("TERM".to_owned(), term),
        ("COLORTERM".to_owned(), "truecolor".to_owned()),
    ]
    .into()
}

/// Runs a command in the instance attached to this terminal. Without a
/// terminal, stdin is only passed on if `stdin` is set: `podman exec` does
/// not reliably forward its end, so a program reading it to the end (pi in
/// print mode) would wait forever.
fn attach(prepared: &Prepared, command: Vec<String>, stdin: bool) -> anyhow::Result<i32> {
    let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    Ok(prepared.provider.exec(&ExecSpec {
        container: prepared.instance(),
        command,
        workdir: Some(WORKSPACE.to_owned()),
        env: terminal_env(),
        interactive: tty || stdin,
        tty,
    })?)
}

fn approvals_hint(project: &Project) {
    if project.ask {
        println!(
            "Rückfragen des Gateways (nicht freigegebene Domains) beantwortest du mit \
             `harness net watch` in einem zweiten Fenster."
        );
    }
}

/// `harness run [HARNESS] [-- ARGS]`.
pub fn run(
    config: &ConfigDir,
    machine: &str,
    name: Option<&str>,
    args: Vec<String>,
) -> anyhow::Result<bool> {
    let prepared = prepare(config, machine, name)?;
    let harness = prepared
        .harnesses
        .iter()
        .find(|h| name.is_none_or(|n| h.name == n))
        .expect("prepare loads the requested harness");

    let installed =
        instance::harness_installed(&prepared.provider, &prepared.project.name, harness)?;
    if !installed {
        println!("Installiere {} in das Tools-Volume …", harness.name);
        instance::install_harness(&prepared.provider, &prepared.project, harness)?;
    }
    if harness.llm == Some(LlmWiring::Pi) {
        wire_pi(config, &prepared)?;
    }
    approvals_hint(&prepared.project);
    println!();

    let mut command = harness.command.clone();
    command.extend(args);
    let code = attach(&prepared, command, false)?;
    println!();
    println!(
        "{} beendet. Commits des Agents holst du mit `harness sync`, übernehmen mit `harness merge`.",
        harness.name
    );
    Ok(code == 0)
}

/// Points pi at `llm.harness` when this computer is paired; otherwise
/// removes that wiring again, so pi uses its own logins.
fn wire_pi(config: &ConfigDir, prepared: &Prepared) -> anyhow::Result<()> {
    let Some((client, token)) = login::paired(config)? else {
        if pi::unwire(&prepared.provider, &prepared.instance())? {
            println!(
                "pi: nicht mehr gekoppelt, die Anbieter über llm.harness sind aus {} entfernt.",
                pi::MODELS_FILE
            );
        }
        println!(
            "pi: nicht mit einem harness-Server gekoppelt, pi nutzt seine eigene Anmeldung (/login)."
        );
        return Ok(());
    };
    let credentials = client.credentials(&token)?;
    let models = pi::models_json(&credentials);
    let own = pi::wire(&prepared.provider, &prepared.instance(), &models)?;
    let wired: Vec<&str> = models
        .providers
        .iter()
        .filter(|id| !own.contains(id))
        .map(String::as_str)
        .collect();
    if wired.is_empty() {
        println!(
            "pi: In der Web-App von {} ist noch kein passender Anbieter hinterlegt.",
            client.url()
        );
    } else {
        println!(
            "pi: Anbieter über {}: {} (Modell wählen mit /model)",
            client.url(),
            wired.join(", ")
        );
    }
    for id in &own {
        println!(
            "  {id}: dein eigener Eintrag in {} hat Vorrang",
            pi::MODELS_FILE
        );
    }
    for note in &models.skipped {
        println!("  übersprungen: {note}");
    }
    Ok(())
}

/// `harness shell`: a login shell in the instance.
pub fn shell(config: &ConfigDir, machine: &str) -> anyhow::Result<bool> {
    let prepared = prepare(config, machine, None)?;
    approvals_hint(&prepared.project);
    println!();
    let code = attach(&prepared, vec!["bash".into(), "-l".into()], true)?;
    Ok(code == 0)
}

fn running_instance(
    config: &ConfigDir,
    machine: &str,
) -> anyhow::Result<(PodmanProvider, Project)> {
    let project = project::resolve(config, None)?;
    let provider = PodmanProvider::for_machine(machine)?;
    if instance::state(&provider, &project.name)? != InstanceState::Running {
        bail!(
            "Instanz {} läuft nicht. `harness run` oder `harness shell` startet sie",
            project.name.instance()
        );
    }
    Ok((provider, project))
}

/// `harness sync`: brings the agent's commits home as `agent/<project>`.
pub fn sync(config: &ConfigDir, machine: &str, force: bool) -> anyhow::Result<bool> {
    let (provider, project) = running_instance(config, machine)?;
    let repo = HostRepo::open(&project.path)?;
    let branch = project.name.agent_branch();
    let outcome = sync::pull_from_workspace(
        &provider,
        &project.name.instance(),
        &repo,
        &project.name,
        force,
    )?;
    match outcome {
        PullOutcome::NoWorkspace => println!("Im Container gibt es noch keine Arbeitskopie."),
        PullOutcome::NoBranch => println!("Der Agent hat noch nichts auf {branch} committet."),
        PullOutcome::NothingNew => println!("Keine neuen Commits auf {branch}."),
        PullOutcome::Imported(ImportOutcome::Unchanged) => println!("{branch} ist schon aktuell."),
        PullOutcome::Imported(ImportOutcome::Updated { to, commits, .. }) => {
            let short: String = to.chars().take(9).collect();
            println!(
                "{branch} steht jetzt auf {short} ({commits} Commit(s), die sonst kein Branch hat). \
                 Übernehmen mit `harness merge`."
            );
        }
        PullOutcome::Imported(ImportOutcome::NeedsFullBundle) => {
            println!("Der Host konnte das Bundle nicht einlesen. Bitte erneut versuchen.");
            return Ok(false);
        }
    }
    Ok(true)
}

/// `harness merge`: merges `agent/<project>` into the current branch.
pub fn merge(config: &ConfigDir) -> anyhow::Result<bool> {
    let project = project::resolve(config, None)?;
    let repo = HostRepo::open(&project.path)?;
    let branch = project.name.agent_branch();
    match repo.merge(&branch)? {
        MergeOutcome::UpToDate => println!("Alles von {branch} ist schon im aktuellen Branch."),
        MergeOutcome::FastForward { commits } => {
            println!("{commits} Commit(s) von {branch} per Fast-Forward übernommen.")
        }
        MergeOutcome::Merged { commits } => {
            println!("{commits} Commit(s) von {branch} mit Merge-Commit übernommen.")
        }
        MergeOutcome::Conflict { files } => {
            println!("Nicht übernommen, der Merge hätte Konflikte in:");
            for file in files {
                println!("  {}", sanitize(&file));
            }
            println!(
                "Dein Arbeitsbaum ist unverändert. Lös den Konflikt z. B. mit `git merge {branch}`."
            );
            return Ok(false);
        }
    }
    Ok(true)
}

/// `harness stop`: stops instance and gateway; volumes stay.
pub fn stop(config: &ConfigDir, machine: &str) -> anyhow::Result<bool> {
    let project = project::resolve(config, None)?;
    let provider = PodmanProvider::for_machine(machine)?;
    instance::stop(&provider, &project.name)?;
    gateway::stop(&provider, &project.name)?;
    println!(
        "{} und {} gestoppt. Arbeitskopie und Home bleiben in den Volumes.",
        project.name.instance(),
        project.name.gateway()
    );
    Ok(true)
}

/// `harness status`: what runs for the project.
pub fn status(config: &ConfigDir, machine: &str) -> anyhow::Result<bool> {
    let project = project::resolve(config, None)?;
    let provider = PodmanProvider::for_machine(machine)?;
    let describe = |running: bool, missing: bool| match (running, missing) {
        (true, _) => "läuft",
        (false, true) => "nicht angelegt",
        (false, false) => "gestoppt",
    };
    let instance_state = instance::state(&provider, &project.name)?;
    let gateway_state = gateway::state(&provider, &project.name)?;
    println!("Projekt {} ({})", project.name, project.path.display());
    println!(
        "  Instanz:  {} ({})",
        project.name.instance(),
        describe(
            instance_state == InstanceState::Running,
            instance_state == InstanceState::Missing
        )
    );
    println!(
        "  Gateway:  {} ({})",
        project.name.gateway(),
        describe(
            gateway_state == GatewayState::Running,
            gateway_state == GatewayState::Missing
        )
    );
    let (llm, note) = login::gateway_llm(config, &provider)?;
    println!(
        "  LLM:      {}",
        if llm.is_some() {
            note
        } else {
            format!("{note}; harnesses nutzen ihre eigene Anmeldung")
        }
    );
    println!(
        "  Branch:   {} (harness sync, harness merge)",
        project.name.agent_branch()
    );
    Ok(true)
}
