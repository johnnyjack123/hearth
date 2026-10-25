//! `harness run`, `shell`, `sync`, `merge`, `stop` and `status`: the agent's
//! container for the project of the current repository.

use std::collections::BTreeMap;
use std::env;
use std::io::IsTerminal as _;

use anyhow::bail;
use harness_core::briefing::briefing;
use harness_core::config::{ConfigDir, GuardKind, HarnessConfig, LlmWiring, Project};
use harness_core::gateway::{self, GatewayState};
use harness_core::instance::{self, InstanceState};
use harness_core::pi;
use harness_core::podman::PodmanProvider;
use harness_core::provider::ExecSpec;
use harness_core::sync::{
    self, HostRepo, ImportOutcome, MergeOutcome, PullOutcome, WORKSPACE, WorkspaceState, sanitize,
};

use crate::progress::{Progress, Terminal, step};
use crate::terminal::{self, Approvals};
use crate::{login, project};

/// Gateway, instance and the clone of the repository, ready for an agent.
pub(crate) struct Prepared {
    pub(crate) provider: PodmanProvider,
    pub(crate) project: Project,
    pub(crate) harnesses: Vec<HarnessConfig>,
}

impl Prepared {
    pub(crate) fn instance(&self) -> String {
        self.project.name.instance()
    }
}

/// Starts gateway and instance and brings the repository into the container.
///
/// The instance always mounts the volumes of the project's default harness
/// and of `extra`; otherwise `harness shell` next to a running `harness run`
/// would recreate the container under it.
fn prepare(config: &ConfigDir, machine: &str, extra: Option<&str>) -> anyhow::Result<Prepared> {
    let project = project::resolve_or_offer(config)?;
    prepare_project(config, machine, project, extra, &Terminal::default())
}

/// [`prepare`] for a given project (`harness connect` gets it by name),
/// reporting its steps to `progress`.
pub(crate) fn prepare_project(
    config: &ConfigDir,
    machine: &str,
    project: Project,
    extra: Option<&str>,
    progress: &dyn Progress,
) -> anyhow::Result<Prepared> {
    let mut harnesses = vec![config.harness(&project.harness)?];
    if let Some(name) = extra.filter(|name| *name != project.harness) {
        harnesses.push(config.harness(name)?);
    }
    let provider = crate::vm::provider_with(machine, progress)?;
    let (llm, _) = login::gateway_llm(config, &provider)?;
    step(
        progress,
        "gateway",
        &format!("Gateway {}", project.name.gateway()),
        || Ok(gateway::ensure_running(&provider, &project, llm.as_ref())?),
    )?;
    let refs: Vec<&HarnessConfig> = harnesses.iter().collect();
    let created = step(
        progress,
        "instance",
        &format!("Instanz {}", project.name.instance()),
        || Ok(instance::ensure_running(&provider, &project, &refs)?),
    )?;
    if created {
        println!("  (neu angelegt; Arbeitskopie und Home liegen in Volumes und bleiben erhalten)");
    }
    let repo = HostRepo::open(&project.path)?;
    let state = step(progress, "workspace", "Arbeitskopie", || {
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

/// Runs a command in the instance attached to this terminal; the gateway's
/// questions appear there too where [`terminal::attach`] can show them.
/// Without a terminal, stdin is only passed on if `stdin` is set: `podman
/// exec` does not reliably forward its end, so a program reading it to the
/// end (pi in print mode) would wait forever.
fn attach(
    config: &ConfigDir,
    prepared: &Prepared,
    command: Vec<String>,
    env: &BTreeMap<String, String>,
    stdin: bool,
) -> anyhow::Result<i32> {
    let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let mut all_env = terminal_env();
    all_env.extend(env.clone());
    let spec = ExecSpec {
        container: prepared.instance(),
        command,
        workdir: Some(WORKSPACE.to_owned()),
        env: all_env,
        interactive: tty || stdin,
        tty,
    };
    if prepared.project.ask && !terminal::shows_questions(&spec) {
        println!(
            "Rückfragen des Gateways (nicht freigegebene Domains) beantwortest du mit \
             `harness net watch` in einem zweiten Fenster."
        );
    }
    println!();
    terminal::attach(
        &prepared.provider,
        &spec,
        Some(Approvals {
            config,
            project: &prepared.project,
        }),
    )
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

    let mut command = ready_harness(
        config,
        &prepared,
        harness,
        harness.command.clone(),
        &Terminal::default(),
    )?
    .command;
    command.extend(args);
    let code = attach(config, &prepared, command, &harness.env, false)?;
    println!();
    println!(
        "{} beendet. Commits des Agents holst du mit `harness sync`, übernehmen mit `harness merge`.",
        harness.name
    );
    Ok(code == 0)
}

/// A harness installed and wired up in a prepared instance.
pub(crate) struct ReadyHarness {
    /// `base` plus the guard and the briefing for the system prompt.
    pub(crate) command: Vec<String>,
    /// pi's providers, if pi was pointed at the server.
    pub(crate) models: Option<pi::PiModels>,
}

/// Installs `harness` if needed, points it at the server and adds guard and
/// briefing to `base` (its terminal or its machine command).
pub(crate) fn ready_harness(
    config: &ConfigDir,
    prepared: &Prepared,
    harness: &HarnessConfig,
    base: Vec<String>,
    progress: &dyn Progress,
) -> anyhow::Result<ReadyHarness> {
    let installed =
        instance::harness_installed(&prepared.provider, &prepared.project.name, harness)?;
    if !installed {
        step(
            progress,
            "install",
            &format!("{} installieren (einmalig)", harness.name),
            || {
                Ok(instance::install_harness(
                    &prepared.provider,
                    &prepared.project,
                    harness,
                )?)
            },
        )?;
    }
    let models = if harness.llm == Some(LlmWiring::Pi) {
        wire_pi(config, prepared)?
    } else {
        None
    };
    let mut command = base;
    if harness.guard == Some(GuardKind::Pi) {
        // A warning, not a stop: pi without the guard still beats no pi.
        match pi::Extension::Guard.ensure(&prepared.provider, &prepared.project) {
            Ok(path) => command.extend(["-e".to_owned(), path]),
            Err(error) => println!("Schutz gegen Prompt-Injection nicht geladen: {error}"),
        }
    }
    // web_fetch goes through the server like the model does, so only pi
    // wired to a server gets it.
    if models.is_some() {
        match pi::Extension::Web.ensure(&prepared.provider, &prepared.project) {
            Ok(path) => command.extend(["-e".to_owned(), path]),
            Err(error) => println!("pi: web_fetch nicht geladen: {error}"),
        }
    }
    if let Some(flag) = &harness.prompt_flag {
        command.push(flag.clone());
        command.push(briefing(&prepared.project));
    }
    Ok(ReadyHarness { command, models })
}

/// Points pi at `llm.harness` when this computer is paired; otherwise
/// removes that wiring again, so pi uses its own logins. Returns the
/// providers it wired.
fn wire_pi(config: &ConfigDir, prepared: &Prepared) -> anyhow::Result<Option<pi::PiModels>> {
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
        return Ok(None);
    };
    // Without the server pi still starts: the shell, its own logins and the
    // wiring from the last run keep working, and model requests succeed as
    // soon as the server is back.
    let credentials = match client.credentials(&token) {
        Ok(credentials) => credentials,
        Err(error) => {
            println!(
                "pi: {error}. pi startet trotzdem; Anfragen ans Modell klappen erst, wenn der \
                 harness-Server läuft."
            );
            return Ok(None);
        }
    };
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
    Ok(Some(models))
}

/// `harness shell`: a login shell in the instance.
pub fn shell(config: &ConfigDir, machine: &str) -> anyhow::Result<bool> {
    let prepared = prepare(config, machine, None)?;
    let shell = vec!["bash".into(), "-l".into()];
    let code = attach(config, &prepared, shell, &BTreeMap::new(), true)?;
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
    report_leftovers(&provider, &project)?;
    Ok(true)
}

/// Points out what `sync` cannot carry: it brings commits on the agent
/// branch only, and an agent may well leave its work uncommitted.
fn report_leftovers(provider: &PodmanProvider, project: &Project) -> anyhow::Result<()> {
    let status = sync::workspace_status(provider, &project.name.instance())?;
    let branch = project.name.agent_branch();
    if !status.branch.is_empty() && status.branch != branch {
        println!(
            "Im Container ist Branch {} ausgecheckt, `harness sync` holt nur {branch}.",
            status.branch
        );
    }
    if !status.changes.is_empty() {
        println!("Noch nicht committet im Container (sync holt nur Commits):");
        for change in &status.changes {
            println!("  {change}");
        }
        if status.more {
            println!("  … und weitere");
        }
        println!(
            "Bitte den Agent, das zu committen, oder committe selbst in `harness shell`. \
             Danach erneut `harness sync`."
        );
    }
    Ok(())
}

/// `harness merge`: merges `agent/<project>` into the current branch.
pub fn merge(config: &ConfigDir, ignore_findings: bool) -> anyhow::Result<bool> {
    let project = project::resolve(config, None)?;
    let repo = HostRepo::open(&project.path)?;
    let branch = project.name.agent_branch();
    if !crate::review::approve(config, &repo, &branch, ignore_findings)? {
        return Ok(false);
    }
    print_merge(&repo, &branch)?;
    Ok(true)
}

/// Merges `branch` into the current branch and says what happened. Returns
/// whether commits came in; a conflict is no error, it leaves everything
/// as it was.
pub(crate) fn print_merge(repo: &HostRepo, branch: &str) -> anyhow::Result<bool> {
    match repo.merge(branch)? {
        MergeOutcome::UpToDate => {
            println!("Alles von {branch} ist schon im aktuellen Branch.");
            Ok(false)
        }
        MergeOutcome::FastForward { commits } => {
            println!("{commits} Commit(s) von {branch} per Fast-Forward übernommen.");
            Ok(true)
        }
        MergeOutcome::Merged { commits } => {
            println!("{commits} Commit(s) von {branch} mit Merge-Commit übernommen.");
            Ok(true)
        }
        MergeOutcome::Conflict { files } => {
            println!("Nicht übernommen, der Merge hätte Konflikte in:");
            for file in files {
                println!("  {}", sanitize(&file));
            }
            println!(
                "Dein Arbeitsbaum ist unverändert. Lös den Konflikt z. B. mit `git merge {branch}`."
            );
            Ok(false)
        }
    }
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
