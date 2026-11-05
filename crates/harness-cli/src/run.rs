//! `harness run`, `shell`, `sync`, `merge`, `stop` and `status`: the agent's
//! container for the project of the current repository.

use std::collections::BTreeMap;
use std::env;
use std::io::IsTerminal as _;

use anyhow::bail;
use harness_core::apps::ReadyApp;
use harness_core::briefing::briefing;
use harness_core::config::{AppConfig, ConfigDir, GuardKind, HarnessConfig, LlmWiring, Project};
use harness_core::gateway::{self, GatewayState};
use harness_core::instance::{self, InstanceState};
use harness_core::lang::tr;
use harness_core::pi;
use harness_core::podman::PodmanProvider;
use harness_core::provider::ExecSpec;
use harness_core::sync::{
    self, HostRepo, ImportOutcome, MergeOutcome, PullOutcome, WORKSPACE, WorkspaceState, sanitize,
};
use harness_core::tr;

use crate::progress::{Progress, Terminal, step};
use crate::terminal::{self, Approvals};
use crate::{login, project};

/// Gateway, instance and the clone of the repository, ready for an agent.
pub(crate) struct Prepared {
    pub(crate) provider: PodmanProvider,
    pub(crate) project: Project,
    pub(crate) harnesses: Vec<HarnessConfig>,
    /// The project's apps, mounted in the instance.
    pub(crate) apps: Vec<ReadyApp>,
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
    let prepared = prepare_project(config, machine, project, extra, &Terminal::default())?;
    // The agent gets commits only; say so before it starts without these.
    if let Ok(changed) = HostRepo::open(&prepared.project.path).and_then(|repo| repo.uncommitted())
        && !changed.is_empty()
    {
        println!(
            "{}",
            tr!(
                "  Achtung: {} uncommittete Datei(en) im Projektordner ({}). Der Agent sieht nur \
                 Commits und arbeitet ohne sie; harness merge übernimmt erst, wenn sie committet \
                 oder gestasht sind.",
                "  Note: {} uncommitted file(s) in the project folder ({}). The agent only sees \
                 commits and works without them; harness merge takes its work over once they are \
                 committed or stashed.",
                changed.len(),
                sync::file_list(&changed, 5)
            )
        );
    }
    Ok(prepared)
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
    let apps = crate::app::ready(config, &provider, &project, progress)?;
    let refs: Vec<&HarnessConfig> = harnesses.iter().collect();
    let created = step(
        progress,
        "instance",
        &tr!(in progress.lang();
            "Instanz {}",
            "Instance {}",
            project.name.instance()
        ),
        || Ok(instance::ensure_running(&provider, &project, &refs, &apps)?),
    )?;
    if created {
        println!(
            "{}",
            tr(
                "  (neu angelegt; Arbeitskopie und Home liegen in Volumes und bleiben erhalten)",
                "  (newly created; working copy and home live in volumes and are kept)",
            )
        );
    }
    let repo = HostRepo::open(&project.path)?;
    let label = progress.lang().pick("Arbeitskopie", "Working copy");
    let state = step(progress, "workspace", label, || {
        Ok(sync::push_to_workspace(
            &provider,
            &project.name.instance(),
            &repo,
            &project.name,
        )?)
    })?;
    match state {
        WorkspaceState::Cloned { branch } => println!(
            "{}",
            tr!(
                "  geklont nach {WORKSPACE}, Branch {}",
                "  cloned to {WORKSPACE}, branch {}",
                sanitize(&branch)
            )
        ),
        WorkspaceState::Fetched { branch } => println!(
            "{}",
            tr!(
                "  Stand des Hosts als host/* geholt, Branch {}",
                "  fetched the host's state as host/*, branch {}",
                sanitize(&branch)
            )
        ),
    }
    Ok(Prepared {
        provider,
        project,
        harnesses,
        apps,
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
            "{}",
            tr(
                "Rückfragen des Gateways (nicht freigegebene Domains) beantwortest du mit \
                 `harness net watch` in einem zweiten Fenster.",
                "You answer the gateway's questions (domains that are not allowed) with \
                 `harness net watch` in a second window.",
            )
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
        false,
        &[],
    )?
    .command;
    command.extend(args);
    let code = attach(config, &prepared, command, &harness.env, false)?;
    println!();
    println!(
        "{}",
        tr!(
            "{} beendet. Commits des Agents holst du mit `harness sync`, übernehmen mit `harness merge`.",
            "{} ended. Fetch the agent's commits with `harness sync`, take them over with `harness merge`.",
            harness.name
        )
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
/// briefing to `base` (its terminal or its machine command). `web_chat` adds
/// what only the web chat shows, such as pi's progress list; `extra` are
/// models pi lacks that a web chat chose.
pub(crate) fn ready_harness(
    config: &ConfigDir,
    prepared: &Prepared,
    harness: &HarnessConfig,
    base: Vec<String>,
    progress: &dyn Progress,
    web_chat: bool,
    extra: &[pi::ExtraModel],
) -> anyhow::Result<ReadyHarness> {
    let installed =
        instance::harness_installed(&prepared.provider, &prepared.project.name, harness)?;
    if !installed {
        step(
            progress,
            "install",
            &tr!(in progress.lang();
                "{} installieren (einmalig)",
                "Installing {} (once)",
                harness.name
            ),
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
        wire_pi(config, prepared, extra)?
    } else {
        None
    };
    let mut command = base;
    if harness.guard == Some(GuardKind::Pi) {
        // A warning, not a stop: pi without the guard still beats no pi.
        match pi::Extension::Guard.ensure(&prepared.provider, &prepared.project) {
            Ok(path) => command.extend(["-e".to_owned(), path]),
            Err(error) => println!(
                "{}",
                tr!(
                    "Schutz gegen Prompt-Injection nicht geladen: {error}",
                    "Protection against prompt injection not loaded: {error}"
                )
            ),
        }
    }
    // web_fetch goes through the server like the model does, so only pi
    // wired to a server gets it.
    if models.is_some() {
        match pi::Extension::Web.ensure(&prepared.provider, &prepared.project) {
            Ok(path) => command.extend(["-e".to_owned(), path]),
            Err(error) => println!(
                "{}",
                tr!(
                    "pi: web_fetch nicht geladen: {error}",
                    "pi: web_fetch not loaded: {error}"
                )
            ),
        }
        // The memories live on the server as well.
        match pi::Extension::Memory.ensure(&prepared.provider, &prepared.project) {
            Ok(path) => command.extend(["-e".to_owned(), path]),
            Err(error) => println!(
                "{}",
                tr!(
                    "pi: Gedächtnis nicht geladen: {error}",
                    "pi: memory not loaded: {error}"
                )
            ),
        }
    }
    // Files are attached in the web chat; their text lives on the server.
    if web_chat && models.is_some() {
        match pi::Extension::Documents.ensure(&prepared.provider, &prepared.project) {
            Ok(path) => command.extend(["-e".to_owned(), path]),
            Err(error) => println!(
                "{}",
                tr!(
                    "pi: read_document nicht geladen: {error}",
                    "pi: read_document not loaded: {error}"
                )
            ),
        }
    }
    if web_chat && harness.llm == Some(LlmWiring::Pi) {
        match pi::Extension::Progress.ensure(&prepared.provider, &prepared.project) {
            Ok(path) => command.extend(["-e".to_owned(), path]),
            Err(error) => println!(
                "{}",
                tr!(
                    "pi: Fortschrittsliste nicht geladen: {error}",
                    "pi: progress list not loaded: {error}"
                )
            ),
        }
    }
    if let Some(flag) = &harness.prompt_flag {
        command.push(flag.clone());
        let apps: Vec<AppConfig> = prepared
            .apps
            .iter()
            .map(|ready| ready.app.clone())
            .collect();
        command.push(briefing(&prepared.project, &apps, &config.apps()?));
    }
    Ok(ReadyHarness { command, models })
}

/// Points pi at `llm.harness` when this computer is paired; otherwise
/// removes that wiring again, so pi uses its own logins. Returns the
/// providers it wired.
fn wire_pi(
    config: &ConfigDir,
    prepared: &Prepared,
    extra: &[pi::ExtraModel],
) -> anyhow::Result<Option<pi::PiModels>> {
    let Some((client, token)) = login::paired(config)? else {
        if pi::unwire(&prepared.provider, &prepared.instance())? {
            println!(
                "{}",
                tr!(
                    "pi: nicht mehr gekoppelt, die Anbieter über llm.harness sind aus {} entfernt.",
                    "pi: no longer paired, the providers through llm.harness are removed from {}.",
                    pi::MODELS_FILE
                )
            );
        }
        println!(
            "{}",
            tr(
                "pi: nicht mit einem harness-Server gekoppelt, pi nutzt seine eigene Anmeldung (/login).",
                "pi: not paired with a harness server, pi uses its own login (/login).",
            )
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
                "{}",
                tr!(
                    "pi: {error}. pi startet trotzdem; Anfragen ans Modell klappen erst, wenn der \
                     harness-Server läuft.",
                    "pi: {error}. pi starts anyway; requests to the model only work once the \
                     harness server runs."
                )
            );
            return Ok(None);
        }
    };
    let models = pi::models_json(&credentials, extra);
    let own = pi::wire(&prepared.provider, &prepared.instance(), &models)?;
    let wired: Vec<&str> = models
        .providers
        .iter()
        .filter(|id| !own.contains(id))
        .map(String::as_str)
        .collect();
    if wired.is_empty() {
        println!(
            "{}",
            tr!(
                "pi: In der Web-App von {} ist noch kein passender Anbieter hinterlegt.",
                "pi: The web app of {} has no matching provider yet.",
                client.url()
            )
        );
    } else {
        println!(
            "{}",
            tr!(
                "pi: Anbieter über {}: {} (Modell wählen mit /model)",
                "pi: providers through {}: {} (pick a model with /model)",
                client.url(),
                wired.join(", ")
            )
        );
    }
    for id in &own {
        println!(
            "{}",
            tr!(
                "  {id}: dein eigener Eintrag in {} hat Vorrang",
                "  {id}: your own entry in {} takes precedence",
                pi::MODELS_FILE
            )
        );
    }
    for note in &models.skipped {
        println!("{}", tr!("  übersprungen: {note}", "  skipped: {note}"));
    }
    Ok(Some(models))
}

/// `harness shell`: a login shell in the instance.
pub fn shell(config: &ConfigDir, machine: &str, project: Option<&str>) -> anyhow::Result<bool> {
    let prepared = match project {
        // From anywhere, e.g. `harness ssh shop` to look at the agent's files.
        Some(name) => prepare_project(
            config,
            machine,
            project::resolve(config, Some(name))?,
            None,
            &Terminal::default(),
        )?,
        None => prepare(config, machine, None)?,
    };
    // By path: `~/.local/bin`, first on the PATH, belongs to the agent.
    let shell = vec!["/bin/bash".into(), "-l".into()];
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
        bail!(tr!(
            "Instanz {} läuft nicht. `harness run` oder `harness shell` startet sie",
            "Instance {} is not running. `harness run` or `harness shell` starts it",
            project.name.instance()
        ));
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
        PullOutcome::NoWorkspace => println!(
            "{}",
            tr(
                "Im Container gibt es noch keine Arbeitskopie.",
                "There is no working copy in the container yet.",
            )
        ),
        PullOutcome::NoBranch => println!(
            "{}",
            tr!(
                "Der Agent hat noch nichts auf {branch} committet.",
                "The agent has not committed anything on {branch} yet."
            )
        ),
        PullOutcome::NothingNew => println!(
            "{}",
            tr!(
                "Keine neuen Commits auf {branch}.",
                "No new commits on {branch}."
            )
        ),
        PullOutcome::Imported(ImportOutcome::Unchanged) => println!(
            "{}",
            tr!(
                "{branch} ist schon aktuell.",
                "{branch} is up to date already."
            )
        ),
        PullOutcome::Imported(ImportOutcome::Updated { to, commits, .. }) => {
            let short: String = to.chars().take(9).collect();
            println!(
                "{}",
                tr!(
                    "{branch} steht jetzt auf {short} ({commits} Commit(s), die sonst kein Branch \
                     hat). Übernehmen mit `harness merge`.",
                    "{branch} is now at {short} ({commits} commit(s) no other branch has). Take \
                     them over with `harness merge`."
                )
            );
        }
        PullOutcome::Imported(ImportOutcome::NeedsFullBundle) => {
            println!(
                "{}",
                tr(
                    "Der Host konnte das Bundle nicht einlesen. Bitte erneut versuchen.",
                    "The host could not read the bundle. Please try again.",
                )
            );
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
            "{}",
            tr!(
                "Im Container ist Branch {} ausgecheckt, `harness sync` holt nur {branch}.",
                "Branch {} is checked out in the container, `harness sync` only fetches {branch}.",
                status.branch
            )
        );
    }
    if !status.changes.is_empty() {
        println!(
            "{}",
            tr(
                "Noch nicht committet im Container (sync holt nur Commits):",
                "Not committed yet in the container (sync only fetches commits):",
            )
        );
        for change in &status.changes {
            println!("  {change}");
        }
        if status.more {
            println!("{}", tr("  … und weitere", "  … and more"));
        }
        println!(
            "{}",
            tr(
                "Bitte den Agent, das zu committen, oder committe selbst in `harness shell`. \
                 Danach erneut `harness sync`.",
                "Ask the agent to commit it, or commit yourself in `harness shell`. Then run \
                 `harness sync` again.",
            )
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
            println!(
                "{}",
                tr!(
                    "Alles von {branch} ist schon im aktuellen Branch.",
                    "Everything from {branch} is in the current branch already."
                )
            );
            Ok(false)
        }
        MergeOutcome::FastForward { commits } => {
            println!(
                "{}",
                tr!(
                    "{commits} Commit(s) von {branch} per Fast-Forward übernommen.",
                    "{commits} commit(s) from {branch} taken over by fast-forward."
                )
            );
            Ok(true)
        }
        MergeOutcome::Merged { commits } => {
            println!(
                "{}",
                tr!(
                    "{commits} Commit(s) von {branch} mit Merge-Commit übernommen.",
                    "{commits} commit(s) from {branch} taken over with a merge commit."
                )
            );
            Ok(true)
        }
        MergeOutcome::Conflict { files } => {
            println!(
                "{}",
                tr(
                    "Nicht übernommen, der Merge hätte Konflikte in:",
                    "Not taken over, the merge would have conflicts in:",
                )
            );
            for file in files {
                println!("  {}", sanitize(&file));
            }
            println!(
                "{}",
                tr!(
                    "Dein Arbeitsbaum ist unverändert. Lös den Konflikt z. B. mit `git merge {branch}`.",
                    "Your working tree is unchanged. Resolve the conflict e.g. with `git merge {branch}`."
                )
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
        "{}",
        tr!(
            "{} und {} gestoppt. Arbeitskopie und Home bleiben in den Volumes.",
            "{} and {} stopped. Working copy and home stay in the volumes.",
            project.name.instance(),
            project.name.gateway()
        )
    );
    Ok(true)
}

/// `harness status`: what runs for the project.
pub fn status(config: &ConfigDir, machine: &str) -> anyhow::Result<bool> {
    let project = project::resolve(config, None)?;
    let provider = PodmanProvider::for_machine(machine)?;
    let describe = |running: bool, missing: bool| match (running, missing) {
        (true, _) => tr("läuft", "running"),
        (false, true) => tr("nicht angelegt", "not created"),
        (false, false) => tr("gestoppt", "stopped"),
    };
    let instance_state = instance::state(&provider, &project.name)?;
    let gateway_state = gateway::state(&provider, &project.name)?;
    println!(
        "{}",
        tr!(
            "Projekt {} ({})",
            "Project {} ({})",
            project.name,
            project.path.display()
        )
    );
    println!(
        "  {}{} ({})",
        tr("Instanz:  ", "Instance: "),
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
            tr!(
                "{note}; harnesses nutzen ihre eigene Anmeldung",
                "{note}; harnesses use their own login"
            )
        }
    );
    println!(
        "  Branch:   {} (harness sync, harness merge)",
        project.name.agent_branch()
    );
    Ok(true)
}
