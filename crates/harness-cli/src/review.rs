//! `harness review` and the check in front of every merge (ROADMAP Phase
//! 2c): fixed rules, new dependencies against OSV, then Jev's opinion
//! through the paired server. The same check runs before the background
//! service takes over an agent's work for a chat. Jev only adds findings:
//! the diff comes from the agent and may try to talk it into "harmless".

use std::io::IsTerminal as _;

use anyhow::bail;
use harness_core::config::ConfigDir;
use harness_core::osv;
use harness_core::review::{Finding, Severity, dependencies, review};
use harness_core::server::{DiffFile, MAX_ASSESSED_CHARS, MAX_ASSESSED_FILES};
use harness_core::sync::{HostRepo, Incoming, sanitize};

use crate::project;

/// Patch read for the check at most; larger changes are checked in part
/// and say so.
const MAX_PATCH_BYTES: usize = 32 << 20;

/// Danger from which Jev's opinion holds a merge for the user's consent.
const JEV_HIGH: f64 = 0.75;

/// Danger from which Jev's opinion is shown as worth a look.
const JEV_MEDIUM: f64 = 0.5;

/// What the check found for the agent branch.
pub struct Checked {
    /// Findings, most severe first.
    pub findings: Vec<Finding>,
    /// Files the merge would change.
    pub files: usize,
    /// Which checks ran, in German, e.g. "feste Regeln".
    pub checks: Vec<&'static str>,
    /// Checks that could not run, and why (German).
    pub notes: Vec<String>,
}

impl Checked {
    /// Whether a finding blocks a merge without the user's explicit consent.
    pub fn has_high(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::High)
    }
}

fn label(severity: Severity) -> &'static str {
    match severity {
        Severity::High => "✗ hoch   ",
        Severity::Medium => "! mittel ",
        Severity::Low => "· niedrig",
    }
}

/// Checks what merging `branch` would bring in. `None` if the branch brings
/// nothing in.
pub fn inspect(
    config: &ConfigDir,
    repo: &HostRepo,
    branch: &str,
) -> anyhow::Result<Option<Checked>> {
    if !repo.branch_exists(branch)? {
        return Ok(None);
    }
    let incoming = repo.incoming(branch, MAX_PATCH_BYTES)?;
    if incoming.files.is_empty() {
        return Ok(None);
    }
    let mut checked = Checked {
        findings: review(&incoming),
        files: incoming.files.len(),
        checks: vec!["feste Regeln"],
        notes: Vec::new(),
    };
    let settings = config.global()?.review;
    let new_dependencies = dependencies(&incoming);
    if settings.osv && !new_dependencies.is_empty() {
        match osv::malicious(&new_dependencies) {
            Ok(found) => {
                checked.findings.extend(found);
                checked.checks.push("OSV");
            }
            // The rules still stand; only the lookup is missing.
            Err(error) => checked
                .notes
                .push(format!("OSV-Abgleich übersprungen: {error}")),
        }
    }
    if settings.jev {
        jev(config, &incoming, &mut checked);
    }
    checked
        .findings
        .sort_by(|a, b| (a.severity, &a.path, a.line).cmp(&(b.severity, &b.path, b.line)));
    Ok(Some(checked))
}

/// The added lines of a file as Jev reads them, cut to what the server
/// accepts (characters, with room for UTF-16 surrogates on its side).
fn added_text(lines: &[(u32, String)]) -> String {
    let mut text = String::new();
    for (_, line) in lines {
        text.push_str(line);
        text.push('\n');
    }
    text.chars().take(MAX_ASSESSED_CHARS * 9 / 10).collect()
}

/// Asks Jev through the paired server and adds findings for changes it
/// finds dangerous. Without pairing, OpenRouter key or answer, the other
/// checks stand alone and a note says so.
fn jev(config: &ConfigDir, incoming: &Incoming, checked: &mut Checked) {
    let files: Vec<DiffFile> = incoming
        .files
        .iter()
        .filter(|file| file.status != 'D' && !file.binary && !file.added.is_empty())
        .take(MAX_ASSESSED_FILES)
        .map(|file| DiffFile {
            path: file.path.clone(),
            status: match file.status {
                'A' => "A",
                'T' => "T",
                _ => "M",
            }
            .to_owned(),
            added: added_text(&file.added),
        })
        .collect();
    if files.is_empty() {
        return;
    }
    let (client, token) = match crate::login::paired(config) {
        Ok(Some(paired)) => paired,
        Ok(None) => {
            checked
                .notes
                .push("Jev übersprungen: Rechner nicht gekoppelt (`harness login`).".to_owned());
            return;
        }
        Err(error) => {
            checked.notes.push(format!("Jev übersprungen: {error:#}"));
            return;
        }
    };
    let assessment = match client.assess_diff(&token, &files) {
        Ok(assessment) => assessment,
        Err(error) => {
            checked.notes.push(format!("Jev übersprungen: {error}"));
            return;
        }
    };
    let Some(dangers) = assessment.files else {
        checked.notes.push(
            match assessment.reason.as_deref() {
                Some("no_helpers") => {
                    "Jev übersprungen: kein OpenRouter-Zugang im Konto (oder Hilfsmodelle vom Admin \
                     abgeschaltet)."
                }
                _ => "Jev hat nicht geantwortet; die übrigen Prüfungen gelten.",
            }
            .to_owned(),
        );
        return;
    };
    checked.checks.push("Jev");
    let mut worst: Option<(f64, String)> = None;
    for danger in &dangers {
        let path = sanitize(&danger.path);
        let percent = (danger.danger.clamp(0.0, 1.0) * 100.0).round();
        let severity = if danger.danger >= JEV_HIGH {
            Some((Severity::High, "gefährlich"))
        } else if danger.danger >= JEV_MEDIUM {
            Some((Severity::Medium, "verdächtig"))
        } else {
            None
        };
        if let Some((severity, word)) = severity {
            checked.findings.push(Finding {
                severity,
                rule: "jev",
                path: path.clone(),
                line: None,
                message: format!("Jev hält diese Änderung für {word} ({percent} %)"),
                excerpt: None,
            });
        }
        if worst.as_ref().is_none_or(|(d, _)| danger.danger > *d) {
            worst = Some((danger.danger, path));
        }
    }
    // Jev's scores stay visible even when none is a finding.
    if let Some((danger, path)) = worst {
        let cost = assessment
            .cost
            .map(|c| format!(", {:.3} Cent", c * 100.0))
            .unwrap_or_default();
        checked.notes.push(format!(
            "Jev: höchste Gefahr {} % in {path} ({} Datei(en){cost}).",
            (danger.clamp(0.0, 1.0) * 100.0).round(),
            dangers.len()
        ));
    }
}

/// Checks what merging `branch` would bring in and prints the findings.
/// `None` if the branch brings nothing in.
pub fn check(config: &ConfigDir, repo: &HostRepo, branch: &str) -> anyhow::Result<Option<Checked>> {
    let Some(checked) = inspect(config, repo, branch)? else {
        return Ok(None);
    };
    for note in &checked.notes {
        println!("{note}");
    }
    println!(
        "Prüfung von {branch}: {} Datei(en) geändert, {} Fund(e) ({})",
        checked.files,
        checked.findings.len(),
        checked.checks.join(", ")
    );
    print_findings(&checked.findings);
    if checked.findings.is_empty() {
        println!("  Keine Auffälligkeiten. Das ersetzt keinen Blick in den Diff:");
    } else {
        println!("  Alles ansehen:");
    }
    println!("  git diff HEAD...{branch}");
    Ok(Some(checked))
}

/// Prints findings, one per line, with the line in question below.
pub fn print_findings(findings: &[Finding]) {
    for finding in findings {
        let place = match finding.line {
            Some(line) => format!("{}:{line}", sanitize(&finding.path)),
            None if finding.path.is_empty() => String::new(),
            None => sanitize(&finding.path),
        };
        println!(
            "  {}  {place}  {}",
            label(finding.severity),
            finding.message
        );
        if let Some(excerpt) = &finding.excerpt {
            println!("             {excerpt}");
        }
    }
}

/// `harness review`.
pub fn run(config: &ConfigDir) -> anyhow::Result<bool> {
    let project = project::resolve(config, None)?;
    let repo = HostRepo::open(&project.path)?;
    let branch = project.name.agent_branch();
    match check(config, &repo, &branch)? {
        None => {
            println!("Nichts zu prüfen: {branch} bringt nichts Neues (erst `harness sync`?).");
            Ok(true)
        }
        Some(checked) => Ok(!checked.has_high()),
    }
}

/// Before a merge: findings are shown; severe ones must be confirmed on a
/// terminal, and without one need `--ignore-findings`. Returns whether the
/// merge may go ahead.
pub fn approve(
    config: &ConfigDir,
    repo: &HostRepo,
    branch: &str,
    ignore_findings: bool,
) -> anyhow::Result<bool> {
    let Some(checked) = check(config, repo, branch)? else {
        return Ok(true);
    };
    println!();
    if !checked.has_high() || ignore_findings {
        return Ok(true);
    }
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    if !interactive {
        bail!(
            "Schwere Funde; nicht übernommen. Nach eigener Prüfung: harness merge --ignore-findings"
        );
    }
    if project::confirm("Schwere Funde. Trotzdem übernehmen?", false)? {
        Ok(true)
    } else {
        println!("Nicht übernommen. Der Branch {branch} bleibt, dein Arbeitsbaum ist unverändert.");
        Ok(false)
    }
}
