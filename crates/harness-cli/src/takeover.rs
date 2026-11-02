//! Taking over an agent's work into the user's folder (ROADMAP Phase 3,
//! "Fortschritt sichern"): commit what the agent left uncommitted in the
//! container, bring the agent branch home, check it, and merge it into the
//! checked-out branch when the check finds nothing severe. Severe findings
//! wait for `harness merge --approve <id>` in a terminal on this computer:
//! a browser alone never lets them through. After a merge the agent's branch
//! continues from the merged state, in the container and on the host.
//!
//! The report goes to the web chat and stays German, as the web app
//! translates on its own; `harness merge --approve` follows the user's
//! language.

use std::io::IsTerminal as _;
use std::sync::Mutex;

use anyhow::{Context as _, bail};
use harness_core::config::ConfigDir;
use harness_core::instance::{self, InstanceState};
use harness_core::lang::{self, Lang, tr};
use harness_core::podman::PodmanProvider;
use harness_core::project::ProjectName;
use harness_core::review::{Finding, Severity};
use harness_core::sync::{
    self, CommitOutcome, HostRepo, MergeOutcome, PullOutcome, RefreshOutcome, sanitize,
};
use harness_core::tr;
use harness_core::tunnel::{
    MAX_NOTICE, MAX_REPORTED_FINDINGS, ReportedFinding, TakeoverReport, TakeoverState, truncate,
};

use crate::progress::Progress;
use crate::{merge_request, project, review};

/// One takeover at a time: they share host repositories and containers.
static RUNNING: Mutex<()> = Mutex::new(());

/// Uncommitted files harness commits at most; more is likely build output
/// missing from `.gitignore`.
const MAX_UNCOMMITTED: u32 = 2_000;

/// Message of the commit of what the agent left uncommitted.
const COMMIT_MESSAGE: &str = "harness: offene Änderungen des Agents gesichert";

/// Conflicting files listed at most.
const MAX_CONFLICTS: usize = 50;

/// Notes a report carries at most (server limit: 20).
const MAX_NOTES: usize = 10;

/// Takes the agent's work of project `name` over and returns the final
/// report; every step is passed to `update` first, so a chat can show it.
pub fn takeover(
    config: &ConfigDir,
    machine: &str,
    name: &ProjectName,
    auto: bool,
    progress: &dyn Progress,
    update: &mut dyn FnMut(&TakeoverReport),
) -> TakeoverReport {
    let _turn = RUNNING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut report = TakeoverReport::running(auto, "Umgebung bereitstellen");
    update(&report);
    if let Err(error) = run(config, machine, name, progress, &mut report, update) {
        report.state = TakeoverState::Failed;
        report.reason = Some(truncate(&format!("{error:#}"), MAX_NOTICE));
    }
    report.step = None;
    within_limits(&mut report);
    update(&report);
    report
}

/// Keeps a report inside what the server accepts; it would otherwise drop
/// the connection over a long note.
fn within_limits(report: &mut TakeoverReport) {
    report.notes = report
        .notes
        .iter()
        .take(MAX_NOTES)
        .map(|note| truncate(note, 1_000))
        .collect();
    report.branch = report.branch.as_deref().map(|b| truncate(b, 200));
    for file in &mut report.conflicts {
        *file = truncate(file, 500);
    }
}

fn run(
    config: &ConfigDir,
    machine: &str,
    name: &ProjectName,
    progress: &dyn Progress,
    report: &mut TakeoverReport,
    update: &mut dyn FnMut(&TakeoverReport),
) -> anyhow::Result<()> {
    let mut step = |report: &mut TakeoverReport, label: &str| {
        report.step = Some(label.to_owned());
        update(report);
    };
    let project = config.project(name)?;
    let repo = HostRepo::open(&project.path)?;
    let branch = name.agent_branch();
    let container = name.instance();
    // The work lives in the container's volumes; it has to run to hand it over.
    let mut provider = crate::vm::provider_with(machine, progress)?;
    if instance::state(&provider, name)? != InstanceState::Running {
        provider =
            crate::run::prepare_project(config, machine, project.clone(), None, progress)?.provider;
    }

    step(report, "Offene Änderungen im Container committen");
    match sync::commit_workspace(&provider, &container, name, COMMIT_MESSAGE, MAX_UNCOMMITTED)? {
        CommitOutcome::Committed { files } => report.committed = Some(files),
        CommitOutcome::Clean | CommitOutcome::NoWorkspace => {}
        CommitOutcome::OtherBranch { branch: other } => report.notes.push(format!(
            "Im Container ist {other} ausgecheckt; übernommen wird nur {branch}."
        )),
        CommitOutcome::TooMany { files } => report.notes.push(format!(
            "{files} geänderte Dateien im Container, zu viele zum Committen. Fehlt etwas in \
             der .gitignore?"
        )),
    }

    step(report, "Commits des Agents holen");
    match sync::pull_from_workspace(&provider, &container, &repo, name, false)? {
        PullOutcome::NoWorkspace | PullOutcome::NoBranch => {
            report.state = TakeoverState::UpToDate;
            return Ok(());
        }
        PullOutcome::NothingNew | PullOutcome::Imported(_) => {}
    }
    let Some(checked_commit) = repo.branch_commit(&branch)? else {
        report.state = TakeoverState::UpToDate;
        return Ok(());
    };

    step(report, "Änderungen prüfen");
    let Some(checked) = review::inspect(config, &repo, &branch, Lang::De)? else {
        report.state = TakeoverState::UpToDate;
        return Ok(());
    };
    report.branch = repo.current_branch()?.map(|b| sanitize(&b));
    report.files = Some(u32::try_from(checked.files).unwrap_or(u32::MAX));
    report.commits = Some(repo.commits_ahead(&branch)?);
    report.checks = checked.checks.iter().map(|c| (*c).to_owned()).collect();
    report.notes.extend(checked.notes.iter().cloned());
    report.findings = reported(&checked.findings);
    if checked.findings.len() > MAX_REPORTED_FINDINGS {
        report.notes.push(format!(
            "Nur die ersten {MAX_REPORTED_FINDINGS} von {} Funden; alle zeigt `harness review`.",
            checked.findings.len()
        ));
    }
    if checked.has_high() {
        let id = merge_request::create(config, name, &branch, &checked_commit)?;
        report.request = Some(id);
        report.state = TakeoverState::Held;
        return Ok(());
    }

    step(report, "In den ausgecheckten Branch übernehmen");
    match repo.merge(&branch) {
        Ok(MergeOutcome::UpToDate) => report.state = TakeoverState::UpToDate,
        Ok(MergeOutcome::FastForward { commits } | MergeOutcome::Merged { commits }) => {
            report.state = TakeoverState::Merged;
            report.commits = Some(commits);
            step(
                report,
                "Arbeitskopie im Container auf den neuen Stand bringen",
            );
            if let Some(note) =
                continue_from_merge(&provider, &repo, name, &checked_commit, Lang::De)
            {
                report.notes.push(note);
            }
        }
        Ok(MergeOutcome::Conflict { files }) => {
            report.state = TakeoverState::Conflict;
            report.conflicts = files
                .iter()
                .take(MAX_CONFLICTS)
                .map(|f| sanitize(f))
                .collect();
        }
        // The host repository is not ready; HostRepo says why in German.
        Err(harness_core::Error::Sync(reason)) => {
            report.state = TakeoverState::Refused;
            report.reason = Some(truncate(&reason, MAX_NOTICE));
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

/// Findings for the chat, most severe first, shortened to what the server
/// accepts.
fn reported(findings: &[Finding]) -> Vec<ReportedFinding> {
    findings
        .iter()
        .take(MAX_REPORTED_FINDINGS)
        .map(|finding| ReportedFinding {
            severity: match finding.severity {
                Severity::High => "high",
                Severity::Medium => "medium",
                Severity::Low => "low",
            },
            rule: finding.rule.to_owned(),
            path: truncate(&sanitize(&finding.path), 400),
            line: finding.line,
            message: truncate(&finding.message, 900),
            excerpt: finding.excerpt.as_deref().map(|e| truncate(e, 400)),
        })
        .collect()
}

/// After a merge: the container's agent branch moves to the merged state,
/// and the host's agent branch follows only if that worked, so both stay
/// on one line of history. Returns a note in `lang` if the container kept
/// its state.
fn continue_from_merge(
    provider: &PodmanProvider,
    repo: &HostRepo,
    name: &ProjectName,
    checked_commit: &str,
    lang: Lang,
) -> Option<String> {
    let head = match repo.head_commit() {
        Ok(Some(head)) => head,
        Ok(None) => return None,
        Err(error) => {
            return Some(tr!(in lang;
                "Neuer Stand nicht lesbar: {error}",
                "New state not readable: {error}"
            ));
        }
    };
    let branch = name.agent_branch();
    match sync::refresh_workspace(
        provider,
        &name.instance(),
        repo,
        name,
        checked_commit,
        &head,
    ) {
        Ok(RefreshOutcome::Refreshed) => match repo.set_branch(&branch, &head, checked_commit) {
            Ok(()) => None,
            Err(error) => Some(tr!(in lang;
                "{branch} auf dem Host nicht nachgezogen: {error}",
                "{branch} not moved along on the host: {error}"
            )),
        },
        Ok(RefreshOutcome::Moved) => Some(tr!(in lang;
            "Der Agent hat inzwischen weitergearbeitet; {branch} im Container bleibt, wie er ist.",
            "The agent has kept working in the meantime; {branch} in the container stays as it is."
        )),
        Ok(RefreshOutcome::OtherBranch) => Some(tr!(in lang;
            "Im Container ist nicht {branch} ausgecheckt; dort bleibt alles, wie es ist.",
            "{branch} is not checked out in the container; everything there stays as it is."
        )),
        Ok(RefreshOutcome::NoWorkspace) => None,
        Err(error) => Some(tr!(in lang;
            "Arbeitskopie im Container nicht aufgefrischt: {error}",
            "Working copy in the container not refreshed: {error}"
        )),
    }
}

/// `harness merge --approve <id>`: merges what a chat asked for despite
/// severe findings, after the user saw them here and agreed.
pub fn approve(config: &ConfigDir, machine: &str, id: &str) -> anyhow::Result<bool> {
    let Some(request) = merge_request::load(config, id)? else {
        bail!(tr!(
            "Keine offene Freigabe mit der ID {}. Im Chat erneut „Fortschritt sichern“.",
            "No open approval with the ID {}. Use “Save progress” in the chat again.",
            sanitize(id)
        ));
    };
    let name = ProjectName::new(&request.project)?;
    let project = config.project(&name)?;
    let repo = HostRepo::open(&project.path)?;
    if repo.branch_commit(&request.branch)?.as_deref() != Some(request.commit.as_str()) {
        merge_request::remove(config, &request.id);
        bail!(tr!(
            "{} hat sich seit dem Antrag geändert. Im Chat erneut „Fortschritt sichern“, dann \
             wird der neue Stand geprüft.",
            "{} has changed since the request. Use “Save progress” in the chat again, then the \
             new state is checked.",
            request.branch
        ));
    }
    println!(
        "{}",
        tr!(
            "Freigabe {} für Projekt {} ({})",
            "Approval {} for project {} ({})",
            request.id,
            name,
            project.path.display()
        )
    );
    let Some(_) = review::check(config, &repo, &request.branch)? else {
        println!(
            "{}",
            tr("Nichts mehr zu übernehmen.", "Nothing left to take over.")
        );
        merge_request::remove(config, &request.id);
        return Ok(true);
    };
    println!();
    if !(std::io::stdin().is_terminal() && std::io::stdout().is_terminal()) {
        bail!(tr(
            "Die Freigabe braucht ein Terminal, in dem du bestätigst.",
            "The approval needs a terminal in which you confirm."
        ));
    }
    let target = repo.current_branch()?.context(tr(
        "HEAD ist kein Branch (detached). Bitte einen Branch auschecken",
        "HEAD is not on a branch (detached). Please check out a branch",
    ))?;
    if !project::confirm(
        &tr!(
            "Trotzdem in {} übernehmen?",
            "Take over into {} anyway?",
            sanitize(&target)
        ),
        false,
    )? {
        println!(
            "{}",
            tr(
                "Nicht übernommen. Die Freigabe bleibt offen.",
                "Not taken over. The approval stays open.",
            )
        );
        return Ok(false);
    }
    let merged = crate::run::print_merge(&repo, &request.branch)?;
    merge_request::remove(config, &request.id);
    if !merged {
        return Ok(false);
    }
    let provider = PodmanProvider::for_machine(machine)?;
    let running = harness_core::vmd::ssh_answers(
        provider.machine().ssh_port,
        std::time::Duration::from_secs(3),
    ) && instance::state(&provider, &name)? == InstanceState::Running;
    if running {
        match continue_from_merge(&provider, &repo, &name, &request.commit, lang::current()) {
            Some(note) => println!("{note}"),
            None => println!(
                "{}",
                tr(
                    "Der Agent arbeitet ab jetzt auf dem übernommenen Stand weiter.",
                    "From now on, the agent continues from the state taken over.",
                )
            ),
        }
    }
    Ok(true)
}
