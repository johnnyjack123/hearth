//! Merge check: new dependencies against OSV (osv.dev), the open database
//! that also lists known malicious packages.
//!
//! Only malware becomes a finding: an `MAL-` id (OpenSSF), CWE-506
//! "embedded malicious code", or "malware"/"malicious code" in the summary.
//! Not "malicious" alone: vulnerabilities use it for the attacker's side
//! ("…when accessing a malicious website", vitest's GHSA-9crc-q9x8-hgqq),
//! which once called vitest 5.0.5 a malicious package.
//! Ordinary vulnerabilities depend on the version, which a manifest line
//! like `^1.2.0` does not pin; popular packages would always show some.
//! The query sends package names to api.osv.dev, which is why
//! `[review] osv` in `config.toml` can turn it off.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Value, json};
use ureq::Agent;

use crate::lang::tr;
use crate::review::{Dependency, Finding, Severity, excerpt};
use crate::{Error, Result, tr};

const QUERY_BATCH: &str = "https://api.osv.dev/v1/querybatch";
const VULN: &str = "https://api.osv.dev/v1/vulns/";

/// At most this many packages are looked up per merge.
const MAX_PACKAGES: usize = 200;

/// At most this many advisories are fetched in full per merge.
const MAX_ADVISORIES: usize = 100;

const TIMEOUT: Duration = Duration::from_secs(15);

fn agent() -> Agent {
    Agent::new_with_config(
        Agent::config_builder()
            .tls_config(crate::server::tls_config())
            .timeout_global(Some(TIMEOUT))
            .build(),
    )
}

fn failed(what: &str, error: impl std::fmt::Display) -> Error {
    Error::Unsupported(format!("OSV ({what}): {error}"))
}

/// Whether an OSV advisory describes malware rather than a bug.
pub fn is_malware(advisory: &Value) -> bool {
    let id = advisory["id"].as_str().unwrap_or_default();
    let cwe_506 = advisory["database_specific"]["cwe_ids"]
        .as_array()
        .is_some_and(|ids| ids.iter().any(|id| id == "CWE-506"));
    let summary = advisory["summary"]
        .as_str()
        .unwrap_or_default()
        .to_lowercase();
    id.starts_with("MAL-")
        || cwe_506
        || summary.contains("malware")
        || summary.contains("malicious code")
}

/// Findings for dependencies OSV knows as malicious.
pub fn malicious(dependencies: &[Dependency]) -> Result<Vec<Finding>> {
    // One lookup per package, even if several lines declare it.
    let mut packages: BTreeMap<(&str, &str), &Dependency> = BTreeMap::new();
    for dependency in dependencies {
        packages
            .entry((dependency.ecosystem, dependency.name.as_str()))
            .or_insert(dependency);
    }
    let packages: Vec<&Dependency> = packages.into_values().take(MAX_PACKAGES).collect();
    if packages.is_empty() {
        return Ok(Vec::new());
    }
    let agent = agent();
    let queries: Vec<Value> = packages
        .iter()
        .map(|d| json!({ "package": { "name": d.name, "ecosystem": d.ecosystem } }))
        .collect();
    let answer: Value = agent
        .post(QUERY_BATCH)
        .send_json(json!({ "queries": queries }))
        .map_err(|e| failed(tr("Abfrage", "query"), e))?
        .body_mut()
        .read_json()
        .map_err(|e| failed(tr("Antwort", "answer"), e))?;
    let results = answer["results"].as_array().cloned().unwrap_or_default();

    let mut findings = Vec::new();
    let mut fetched = 0;
    for (dependency, result) in packages.iter().zip(results) {
        let ids: Vec<String> = result["vulns"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v["id"].as_str().map(str::to_owned))
            .collect();
        let mut malware = Vec::new();
        for id in ids {
            if fetched >= MAX_ADVISORIES {
                break;
            }
            fetched += 1;
            // Ids come from OSV; only plain ones are put into the URL.
            if !id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
            {
                continue;
            }
            let advisory: Value = agent
                .get(&format!("{VULN}{id}"))
                .call()
                .map_err(|e| failed(&id, e))?
                .body_mut()
                .read_json()
                .map_err(|e| failed(&id, e))?;
            if is_malware(&advisory) {
                malware.push(id);
            }
        }
        if !malware.is_empty() {
            findings.push(Finding {
                severity: Severity::High,
                rule: "osv-malware",
                path: dependency.path.clone(),
                line: Some(dependency.line),
                message: tr!(
                    "„{}“ ist als Schadpaket bekannt ({})",
                    "“{}” is known as a malicious package ({})",
                    dependency.name,
                    malware.join(", ")
                ),
                excerpt: Some(excerpt(&dependency.text)),
            });
        }
    }
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malware_markers() {
        assert!(is_malware(&json!({ "id": "MAL-2024-1234" })));
        assert!(is_malware(&json!({
            "id": "GHSA-c2m4-w5hm-vqjw",
            "summary": "crossenv is malware",
            "database_specific": { "cwe_ids": ["CWE-506"] }
        })));
        assert!(!is_malware(&json!({
            "id": "GHSA-29mw-wpgm-hmr9",
            "summary": "Prototype Pollution in lodash",
            "database_specific": { "cwe_ids": ["CWE-1321"] }
        })));
        assert!(is_malware(&json!({
            "id": "MAL-2022-1",
            "summary": "Malicious code in rustdecimal (crates.io)"
        })));
        // A vulnerability that mentions an attacker's malicious website.
        assert!(!is_malware(&json!({
            "id": "GHSA-9crc-q9x8-hgqq",
            "summary": "Vitest allows Remote Code Execution when accessing a malicious website",
            "database_specific": { "cwe_ids": ["CWE-1385"] }
        })));
    }

    #[test]
    #[ignore = "needs internet access to api.osv.dev"]
    fn finds_a_known_malicious_package() {
        let dependency = |name: &str| Dependency {
            ecosystem: "npm",
            name: name.into(),
            path: "package.json".into(),
            line: 3,
            text: format!("\"{name}\": \"^1.0.0\""),
        };
        let findings = malicious(&[dependency("crossenv"), dependency("react")]).unwrap();
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].message.contains("crossenv"));
        assert!(findings[0].message.contains("GHSA-c2m4-w5hm-vqjw"));
    }
}
