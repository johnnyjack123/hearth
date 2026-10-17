//! `harness review` and the check in front of `harness merge` (ROADMAP
//! Phase 2c): fixed rules, then new dependencies against OSV.

use std::io::IsTerminal as _;

use anyhow::bail;
use harness_core::config::ConfigDir;
use harness_core::osv;
use harness_core::review::{Finding, Severity, dependencies, review};
use harness_core::sync::{HostRepo, sanitize};

use crate::project;

/// Patch read for the check at most; larger changes are checked in part
/// and say so.
const MAX_PATCH_BYTES: usize = 32 << 20;

/// What the check found for the agent branch.
pub struct Checked {
    /// Findings, most severe first.
    pub findings: Vec<Finding>,
}

impl Checked {
    fn has_high(&self) -> bool {
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

/// Checks what merging `branch` would bring in and prints the findings.
/// `None` if the branch brings nothing in.
pub fn check(config: &ConfigDir, repo: &HostRepo, branch: &str) -> anyhow::Result<Option<Checked>> {
    if !repo.branch_exists(branch)? {
        return Ok(None);
    }
    let incoming = repo.incoming(branch, MAX_PATCH_BYTES)?;
    if incoming.files.is_empty() {
        return Ok(None);
    }
    let mut findings = review(&incoming);
    let mut checks = "feste Regeln";
    let new_dependencies = dependencies(&incoming);
    if config.global()?.review.osv && !new_dependencies.is_empty() {
        match osv::malicious(&new_dependencies) {
            Ok(found) => {
                findings.extend(found);
                findings.sort_by(|a, b| {
                    (a.severity, &a.path, a.line).cmp(&(b.severity, &b.path, b.line))
                });
                checks = "feste Regeln, OSV";
            }
            // The rules still stand; only the lookup is missing.
            Err(error) => println!("OSV-Abgleich übersprungen: {error}"),
        }
    }
    println!(
        "Prüfung von {branch}: {} Datei(en) geändert, {} Fund(e) ({checks})",
        incoming.files.len(),
        findings.len()
    );
    for finding in &findings {
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
    if findings.is_empty() {
        println!("  Keine Auffälligkeiten. Das ersetzt keinen Blick in den Diff:");
    } else {
        println!("  Alles ansehen:");
    }
    println!("  git diff HEAD...{branch}");
    Ok(Some(Checked { findings }))
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
