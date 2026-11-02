//! Reading the header of a git bundle (gitformat-bundle(5)).
//!
//! Done by hand instead of `git bundle verify` so the host can tell "missing
//! prerequisites" apart from other failures without parsing localized git
//! messages. Bundles written in the container are untrusted, hence the size
//! limit and strict validation.

use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use crate::lang::tr;
use crate::{Error, Result, tr};

/// Upper bound for the header (signature, capabilities, prerequisites, refs).
const MAX_HEADER_BYTES: u64 = 4 * 1024 * 1024;

/// What a bundle needs and what it provides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleHeader {
    /// Commits the receiving repository must already have.
    pub prerequisites: Vec<String>,
    /// `(object id, ref name)` pairs the bundle contains.
    pub refs: Vec<(String, String)>,
}

impl BundleHeader {
    /// Reads and validates the header of the bundle at `path`.
    pub fn read(path: &Path) -> Result<Self> {
        let file = File::open(path)
            .map_err(|e| Error::io(tr!("{} öffnen", "opening {}", path.display()), e))?;
        Self::parse(BufReader::new(file.take(MAX_HEADER_BYTES))).map_err(|reason| {
            Error::Sync(tr!(
                "Bundle {} ist ungültig: {reason}",
                "Bundle {} is invalid: {reason}",
                path.display()
            ))
        })
    }

    /// Object id of `refs/heads/<branch>` in the bundle.
    pub fn branch(&self, branch: &str) -> Option<&str> {
        let wanted = format!("refs/heads/{branch}");
        self.refs
            .iter()
            .find(|(_, name)| *name == wanted)
            .map(|(oid, _)| oid.as_str())
    }

    fn parse(mut reader: impl BufRead) -> Result<Self, String> {
        let mut line = String::new();
        let mut next_line = |line: &mut String| -> Result<bool, String> {
            line.clear();
            let read = reader
                .read_line(line)
                .map_err(|e| tr!("Lesefehler: {e}", "read error: {e}"))?;
            if read == 0 {
                return Ok(false);
            }
            if !line.ends_with('\n') {
                return Err(tr(
                    "Kopf unvollständig oder zu groß",
                    "header incomplete or too large",
                )
                .to_owned());
            }
            line.pop();
            Ok(true)
        };

        if !next_line(&mut line)? {
            return Err(tr("leere Datei", "empty file").to_owned());
        }
        let v3 = match line.as_str() {
            "# v2 git bundle" => false,
            "# v3 git bundle" => true,
            _ => return Err(tr("keine Git-Bundle-Signatur", "no git bundle signature").to_owned()),
        };

        let mut header = BundleHeader {
            prerequisites: Vec::new(),
            refs: Vec::new(),
        };
        loop {
            if !next_line(&mut line)? {
                return Err(tr(
                    "Kopf endet ohne Leerzeile",
                    "header ends without an empty line",
                )
                .to_owned());
            }
            if line.is_empty() {
                break;
            }
            if let Some(capability) = line.strip_prefix('@') {
                // Only object-format and filter are defined; an unknown one
                // means a format we cannot judge.
                if !v3
                    || !(capability.starts_with("object-format=")
                        || capability.starts_with("filter="))
                {
                    return Err(tr!(
                        "unbekannte Fähigkeit @{capability}",
                        "unknown capability @{capability}"
                    ));
                }
                continue;
            }
            if let Some(rest) = line.strip_prefix('-') {
                let oid = rest.split(' ').next().unwrap_or_default();
                header.prerequisites.push(valid_oid(oid)?.to_owned());
                continue;
            }
            let (oid, name) = line
                .split_once(' ')
                .ok_or_else(|| tr!("ungültige Zeile {line:?}", "invalid line {line:?}"))?;
            if name.is_empty() || name.chars().any(|c| c.is_control() || c == ' ') {
                return Err(tr!(
                    "ungültiger Ref-Name {name:?}",
                    "invalid ref name {name:?}"
                ));
            }
            header
                .refs
                .push((valid_oid(oid)?.to_owned(), name.to_owned()));
        }
        Ok(header)
    }
}

fn valid_oid(oid: &str) -> Result<&str, String> {
    let hex = oid
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
    if hex && (oid.len() == 40 || oid.len() == 64) {
        Ok(oid)
    } else {
        Err(tr!(
            "ungültige Objekt-ID {oid:?}",
            "invalid object ID {oid:?}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "1111111111111111111111111111111111111111";
    const B: &str = "2222222222222222222222222222222222222222";

    fn parse(text: &str) -> Result<BundleHeader, String> {
        BundleHeader::parse(text.as_bytes())
    }

    #[test]
    fn parses_v2_and_v3() {
        let v2 = format!("# v2 git bundle\n-{A} subject line\n{B} refs/heads/agent/x\n\nPACK");
        let header = parse(&v2).unwrap();
        assert_eq!(header.prerequisites, [A]);
        assert_eq!(header.branch("agent/x"), Some(B));
        assert_eq!(header.branch("main"), None);

        let v3 = format!("# v3 git bundle\n@object-format=sha1\n{B} refs/heads/main\n\n");
        let header = parse(&v3).unwrap();
        assert!(header.prerequisites.is_empty());
        assert_eq!(header.branch("main"), Some(B));
    }

    #[test]
    fn rejects_garbage() {
        for bad in [
            String::new(),
            "not a bundle\n\n".to_owned(),
            format!("# v2 git bundle\n{B} refs/heads/main\n"),
            "# v2 git bundle\nxyz refs/heads/main\n\n".to_owned(),
            format!(
                "# v2 git bundle\n{} refs/heads/main\n\n",
                "ABCDEF0123".repeat(4)
            ),
            format!("# v2 git bundle\n{B} refs/heads/a b\n\n"),
            format!("# v2 git bundle\n@object-format=sha1\n{B} refs/heads/main\n\n"),
            format!("# v3 git bundle\n@evil\n{B} refs/heads/main\n\n"),
            format!("# v2 git bundle\n{B}"),
        ] {
            assert!(parse(&bad).is_err(), "{bad:?} accepted");
        }
    }
}
