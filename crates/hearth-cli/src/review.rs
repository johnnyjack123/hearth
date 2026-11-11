//! `hearth review` and the check in front of every merge (ROADMAP Phase
//! 2c): fixed rules, new dependencies against OSV, then Jev's opinion
//! through the paired server. The same check runs before the background
//! service takes over an agent's work for a chat. Jev only adds findings:
//! the diff comes from the agent and may try to talk it into "harmless".

use std::io::IsTerminal as _;

use anyhow::bail;
use hearth_core::config::ConfigDir;
use hearth_core::lang::{self, Lang, tr};
use hearth_core::osv;
use hearth_core::review::{Finding, Severity, dependencies, review};
use hearth_core::server::{DiffFile, MAX_ASSESSED_CHARS, MAX_ASSESSED_FILES};
use hearth_core::sync::{HostRepo, Incoming, sanitize};
use hearth_core::tr;

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
    /// Which checks ran, e.g. "feste Regeln".
    pub checks: Vec<&'static str>,
    /// Checks that could not run, and why.
    pub notes: Vec<String>,
}

impl Checked {
    /// Whether a finding blocks a merge without the user's explicit consent.
    pub fn has_high(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::High)
    }

    /// Whether a finding should be looked at before a merge nobody at this
    /// computer started: medium ones (build scripts, CI, editor settings,
    /// instructions for agents) run on the host just as well.
    pub fn has_medium(&self) -> bool {
        self.findings.iter().any(|f| f.severity == Severity::Medium)
    }
}

fn label(severity: Severity) -> &'static str {
    match severity {
        Severity::High => tr("✗ hoch   ", "✗ high   "),
        Severity::Medium => tr("! mittel ", "! medium "),
        Severity::Low => tr("· niedrig", "· low    "),
    }
}

/// Checks what merging `branch` would bring in. `None` if the branch brings
/// nothing in. Checks and notes are in `lang`: a chat's takeover report
/// stays German, the terminal follows the user's language.
pub fn inspect(
    config: &ConfigDir,
    repo: &HostRepo,
    branch: &str,
    lang: Lang,
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
        checks: vec![lang.pick("feste Regeln", "fixed rules")],
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
            Err(error) => checked.notes.push(tr!(in lang;
                "OSV-Abgleich übersprungen: {error}",
                "OSV lookup skipped: {error}"
            )),
        }
    }
    if settings.jev {
        jev(config, &incoming, &mut checked, lang);
    }
    checked
        .findings
        .sort_by(|a, b| (a.severity, &a.path, a.line).cmp(&(b.severity, &b.path, b.line)));
    Ok(Some(checked))
}

/// Characters of one part Jev reads, with room for UTF-16 surrogates on
/// the server's side.
const PART_CHARS: usize = MAX_ASSESSED_CHARS * 9 / 10;

/// Parts of one file at most; a minified bundle would take every request.
const MAX_PARTS_PER_FILE: usize = 10;

/// Requests to Jev per check at most (60 parts each, a few tenths of a
/// cent each).
const MAX_REQUESTS: usize = 5;

/// The added lines of a file in parts Jev reads, split between lines where
/// possible; at most [`MAX_PARTS_PER_FILE`]. Returns the parts and whether
/// some of the file was left out.
fn added_parts(lines: &[(u32, String)]) -> (Vec<String>, bool) {
    let mut parts: Vec<String> = Vec::new();
    let mut part = String::new();
    let mut chars = 0;
    for (_, line) in lines {
        let mut rest: Vec<char> = line.chars().chain(['\n']).collect();
        while !rest.is_empty() {
            if chars + rest.len() <= PART_CHARS {
                part.extend(&rest);
                chars += rest.len();
                rest.clear();
            } else if chars > 0 {
                parts.push(std::mem::take(&mut part));
                chars = 0;
            } else {
                // One line longer than a part: cut it.
                part.extend(&rest[..PART_CHARS]);
                parts.push(std::mem::take(&mut part));
                rest.drain(..PART_CHARS);
            }
        }
    }
    if chars > 0 {
        parts.push(part);
    }
    let cut = parts.len() > MAX_PARTS_PER_FILE;
    parts.truncate(MAX_PARTS_PER_FILE);
    (parts, cut)
}

/// Why Jev gave no scores, for the report.
fn reason_note(reason: Option<&str>, lang: Lang) -> String {
    match reason {
        Some("disabled") => lang.pick(
            "Jev übersprungen: für diesen Rechner in der Web-App abgeschaltet \
             (Einstellungen → Geräte).",
            "Jev skipped: switched off for this computer in the web app \
             (Settings → Devices).",
        ),
        Some("no_helpers") => lang.pick(
            "Jev übersprungen: kein OpenRouter-Zugang im Konto (oder Hilfsmodelle vom Admin \
             abgeschaltet).",
            "Jev skipped: no OpenRouter access in the account (or helper models switched off \
             by the admin).",
        ),
        _ => lang.pick(
            "Jev hat nicht geantwortet; die übrigen Prüfungen gelten.",
            "Jev did not answer; the other checks apply.",
        ),
    }
    .to_owned()
}

/// Asks Jev through the paired server and adds findings for changes it
/// finds dangerous. Without pairing, OpenRouter key or answer, the other
/// checks stand alone and a note says so. Long files go in parts and many
/// files in several requests; a danger found in any part counts for its
/// file. What exceeds the limits is named, so nobody takes it as checked.
fn jev(config: &ConfigDir, incoming: &Incoming, checked: &mut Checked, lang: Lang) {
    let mut parts: Vec<(String, DiffFile)> = Vec::new();
    let mut cut_files: Vec<String> = Vec::new();
    for file in incoming
        .files
        .iter()
        .filter(|file| file.status != 'D' && !file.binary && !file.added.is_empty())
    {
        let (texts, cut) = added_parts(&file.added);
        if cut {
            cut_files.push(sanitize(&file.path));
        }
        let count = texts.len();
        for (index, added) in texts.into_iter().enumerate() {
            let label = if count > 1 {
                format!("{} (Teil {}/{count})", file.path, index + 1)
            } else {
                file.path.clone()
            };
            let status = match file.status {
                'A' => "A",
                'T' => "T",
                _ => "M",
            };
            parts.push((
                file.path.clone(),
                DiffFile {
                    path: label,
                    status: status.to_owned(),
                    added,
                },
            ));
        }
    }
    if parts.is_empty() {
        return;
    }
    let total_parts = parts.len();
    parts.truncate(MAX_ASSESSED_FILES * MAX_REQUESTS);
    let left_out = total_parts - parts.len();
    let (client, token) = match crate::login::paired(config) {
        Ok(Some(paired)) => paired,
        Ok(None) => {
            checked.notes.push(
                lang.pick(
                    "Jev übersprungen: Rechner nicht gekoppelt (`hearth login`).",
                    "Jev skipped: computer not paired (`hearth login`).",
                )
                .to_owned(),
            );
            return;
        }
        Err(error) => {
            checked.notes.push(tr!(in lang;
                "Jev übersprungen: {error:#}",
                "Jev skipped: {error:#}"
            ));
            return;
        }
    };
    // The highest danger per file over its parts, and what Jev cost.
    let mut dangers: Vec<(String, f64)> = Vec::new();
    let mut cost = Some(0.0);
    let mut requests = 0;
    let mut unanswered = 0;
    for batch in parts.chunks(MAX_ASSESSED_FILES) {
        let files: Vec<DiffFile> = batch.iter().map(|(_, part)| part.clone()).collect();
        let assessment = match client.assess_diff(&token, &files) {
            Ok(assessment) => assessment,
            Err(error) => {
                if requests == 0 {
                    checked.notes.push(tr!(in lang;
                        "Jev übersprungen: {error}",
                        "Jev skipped: {error}"
                    ));
                    return;
                }
                unanswered += batch.len();
                continue;
            }
        };
        requests += 1;
        cost = cost.zip(assessment.cost).map(|(a, b)| a + b);
        let Some(answer) = assessment.files else {
            if requests == 1 {
                checked
                    .notes
                    .push(reason_note(assessment.reason.as_deref(), lang));
                return;
            }
            unanswered += batch.len();
            continue;
        };
        for danger in answer {
            // The server answers with the labels it got.
            let Some((path, _)) = batch.iter().find(|(_, part)| part.path == danger.path) else {
                continue;
            };
            match dangers.iter_mut().find(|(known, _)| known == path) {
                Some((_, worst)) => *worst = worst.max(danger.danger),
                None => dangers.push((path.clone(), danger.danger)),
            }
        }
    }
    checked.checks.push("Jev");
    if !cut_files.is_empty() || left_out + unanswered > 0 {
        let mut gaps = Vec::new();
        if !cut_files.is_empty() {
            gaps.push(tr!(in lang;
                "von {} nur die ersten {} Zeichen",
                "only the first {1} characters of {0}",
                cut_files.join(", "),
                PART_CHARS * MAX_PARTS_PER_FILE
            ));
        }
        if left_out + unanswered > 0 {
            gaps.push(tr!(in lang;
                "{} Teil(e) gar nicht",
                "{} part(s) not at all",
                left_out + unanswered
            ));
        }
        checked.notes.push(tr!(in lang;
            "Jev hat nicht alles gelesen: {}. Dafür gelten nur die festen Regeln und OSV.",
            "Jev did not read everything: {}. For that, only the fixed rules and OSV apply.",
            gaps.join(", ")
        ));
    }
    let mut worst: Option<(f64, String)> = None;
    for (path, danger) in &dangers {
        let path = sanitize(path);
        let danger = *danger;
        let percent = (danger.clamp(0.0, 1.0) * 100.0).round();
        let severity = if danger >= JEV_HIGH {
            Some((Severity::High, lang.pick("gefährlich", "dangerous")))
        } else if danger >= JEV_MEDIUM {
            Some((Severity::Medium, lang.pick("verdächtig", "suspicious")))
        } else {
            None
        };
        if let Some((severity, word)) = severity {
            checked.findings.push(Finding {
                severity,
                rule: "jev",
                path: path.clone(),
                line: None,
                message: tr!(in lang;
                    "Jev hält diese Änderung für {word} ({percent} %)",
                    "Jev considers this change {word} ({percent} %)"
                ),
                excerpt: None,
            });
        }
        if worst.as_ref().is_none_or(|(d, _)| danger > *d) {
            worst = Some((danger, path));
        }
    }
    // Jev's scores stay visible even when none is a finding.
    if let Some((danger, path)) = worst {
        let cost = cost
            .map(|c| tr!(in lang; ", {:.3} Cent", ", {:.3} cents", c * 100.0))
            .unwrap_or_default();
        let asked = if requests > 1 {
            tr!(in lang; ", {requests} Anfragen", ", {requests} requests")
        } else {
            String::new()
        };
        checked.notes.push(tr!(in lang;
            "Jev: höchste Gefahr {} % in {path} ({} Datei(en){asked}{cost}).",
            "Jev: highest danger {} % in {path} ({} file(s){asked}{cost}).",
            (danger.clamp(0.0, 1.0) * 100.0).round(),
            dangers.len()
        ));
    }
}

/// Checks what merging `branch` would bring in and prints the findings.
/// `None` if the branch brings nothing in.
pub fn check(config: &ConfigDir, repo: &HostRepo, branch: &str) -> anyhow::Result<Option<Checked>> {
    let Some(checked) = inspect(config, repo, branch, lang::current())? else {
        return Ok(None);
    };
    for note in &checked.notes {
        println!("{note}");
    }
    println!(
        "{}",
        tr!(
            "Prüfung von {branch}: {} Datei(en) geändert, {} Fund(e) ({})",
            "Check of {branch}: {} file(s) changed, {} finding(s) ({})",
            checked.files,
            checked.findings.len(),
            checked.checks.join(", ")
        )
    );
    print_findings(&checked.findings);
    if checked.findings.is_empty() {
        println!(
            "{}",
            tr(
                "  Keine Auffälligkeiten. Das ersetzt keinen Blick in den Diff:",
                "  Nothing stands out. That does not replace a look at the diff:",
            )
        );
    } else {
        println!("{}", tr("  Alles ansehen:", "  See everything:"));
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

/// `hearth review`.
pub fn run(config: &ConfigDir) -> anyhow::Result<bool> {
    let project = project::resolve(config, None)?;
    let repo = HostRepo::open(&project.path)?;
    let branch = project.name.agent_branch();
    match check(config, &repo, &branch)? {
        None => {
            println!(
                "{}",
                tr!(
                    "Nichts zu prüfen: {branch} bringt nichts Neues (erst `hearth sync`?).",
                    "Nothing to check: {branch} brings nothing new (`hearth sync` first?)."
                )
            );
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
        bail!(tr(
            "Schwere Funde; nicht übernommen. Nach eigener Prüfung: hearth merge --ignore-findings",
            "Severe findings; not taken over. After checking yourself: hearth merge --ignore-findings"
        ));
    }
    if project::confirm(
        tr(
            "Schwere Funde. Trotzdem übernehmen?",
            "Severe findings. Take over anyway?",
        ),
        false,
    )? {
        Ok(true)
    } else {
        println!(
            "{}",
            tr!(
                "Nicht übernommen. Der Branch {branch} bleibt, dein Arbeitsbaum ist unverändert.",
                "Not taken over. The branch {branch} stays, your working tree is unchanged."
            )
        );
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(texts: &[String]) -> Vec<(u32, String)> {
        texts
            .iter()
            .enumerate()
            .map(|(n, text)| (n as u32 + 1, text.clone()))
            .collect()
    }

    #[test]
    fn long_files_go_to_jev_in_parts_between_lines() {
        let short = lines(&["fn main() {}".to_owned()]);
        assert_eq!(
            added_parts(&short),
            (vec!["fn main() {}\n".to_owned()], false)
        );

        // Lines of 1000 characters: each part ends at a line end.
        let line = "x".repeat(999);
        let long = lines(&vec![line.clone(); 25]);
        let (parts, cut) = added_parts(&long);
        assert!(!cut);
        assert_eq!(
            parts.len(),
            25_000_usize.div_ceil(PART_CHARS - PART_CHARS % 1000)
        );
        assert!(parts.iter().all(|part| part.chars().count() <= PART_CHARS));
        assert!(parts.iter().all(|part| part.ends_with('\n')));
        assert_eq!(parts.concat().len(), 25_000);

        // One endless line (minified) is cut, and a huge file is capped.
        let minified = lines(&["y".repeat(PART_CHARS * 12)]);
        let (parts, cut) = added_parts(&minified);
        assert!(cut);
        assert_eq!(parts.len(), MAX_PARTS_PER_FILE);
        assert!(parts.iter().all(|part| part.chars().count() == PART_CHARS));
    }
}
