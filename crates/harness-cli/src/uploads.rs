//! Files attached in a linked chat (protocol 9): downloaded from the server
//! with the device token and copied into the instance, where the agent reads
//! them. They go to `~/uploads` in the home volume, not into `/workspace`:
//! the user's uploads are no part of the project and must not land in a
//! commit of the agent.

use std::collections::HashSet;
use std::fs;

use anyhow::Context;
use harness_core::Provider;
use harness_core::provider::ExecSpec;
use harness_core::server::ServerClient;
use harness_core::tunnel::PromptFile;

/// Where the files go in the instance.
pub(crate) const UPLOADS_DIR: &str = "/home/agent/uploads";

/// A file name usable as one path component in the instance: letters,
/// digits and a few signs, never a dot or dash in front, at most 120
/// characters. The server cleaned it already; this side does not rely on
/// that.
pub(crate) fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " ._-()+".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let name: String = cleaned
        .trim()
        .trim_start_matches(['.', '-'])
        .chars()
        .take(120)
        .collect();
    if name.trim().is_empty() {
        "datei".to_owned()
    } else {
        name.trim_end().to_owned()
    }
}

/// `name`, or `name (2)` etc. if a file of this message already has it.
fn unique(name: String, used: &mut HashSet<String>) -> String {
    let mut candidate = name.clone();
    let mut count = 1;
    while !used.insert(candidate.clone()) {
        count += 1;
        candidate = match name.rsplit_once('.') {
            Some((stem, extension)) if !stem.is_empty() => format!("{stem} ({count}).{extension}"),
            _ => format!("{name} ({count})"),
        };
    }
    candidate
}

/// The short name of a file the server's `read_document` takes: the end of
/// its UUID, as `documentRef` in `apps/server/src/files/read.ts`.
fn document_ref(id: &str) -> &str {
    id.get(id.len().saturating_sub(8)..).unwrap_or(id)
}

fn size(bytes: u64) -> String {
    if bytes < 1024 * 1024 {
        format!("{} KB", bytes.div_ceil(1024))
    } else {
        format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
    }
}

/// Downloads `files` and puts them into `container` under [`UPLOADS_DIR`];
/// documents also as `<name>.md`, the text the server made of them. Returns
/// the note that goes with the message, so the agent knows where they are.
pub(crate) fn place(
    client: &ServerClient,
    token: &str,
    provider: &dyn Provider,
    container: &str,
    files: &[PromptFile],
) -> anyhow::Result<String> {
    let made = provider.exec_output(&ExecSpec {
        container: container.to_owned(),
        command: vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "PATH=/usr/bin:/bin mkdir -p \"$1\"".to_owned(),
            "harness-uploads".to_owned(),
            UPLOADS_DIR.to_owned(),
        ],
        ..ExecSpec::default()
    })?;
    if made.code != 0 {
        anyhow::bail!(
            "Ordner {UPLOADS_DIR} nicht angelegt: {}",
            made.stderr.trim()
        );
    }
    let work = tempfile::tempdir().context("temporärer Ordner")?;
    let mut used = HashSet::new();
    let mut lines = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let name = unique(safe_name(&file.name), &mut used);
        let local = work.path().join(index.to_string());
        let bytes = client.device_file(token, &file.id, false)?;
        fs::write(&local, &bytes).context("Datei zwischenspeichern")?;
        provider.copy_into(container, &local, &format!("{UPLOADS_DIR}/{name}"))?;
        let what = format!("{}, {}", file.media_type, size(file.size));
        // The server keeps the text and its embeddings; `read_document`
        // (pi/documents.ts) searches it there by this id.
        let search = format!(
            "; search it with read_document, file \"{}\"",
            document_ref(&file.id)
        );
        lines.push(match file.kind.as_str() {
            "document" => {
                let text = client.device_file(token, &file.id, true)?;
                let markdown = work.path().join(format!("{index}.md"));
                fs::write(&markdown, &text).context("Text zwischenspeichern")?;
                provider.copy_into(container, &markdown, &format!("{UPLOADS_DIR}/{name}.md"))?;
                format!(
                    "- ~/uploads/{name} ({what}); its text as Markdown: ~/uploads/{name}.md{search}"
                )
            }
            "image" => format!("- ~/uploads/{name} ({what}); look at it with the read tool"),
            _ => format!("- ~/uploads/{name} ({what}){search}"),
        });
    }
    Ok(format!(
        "[The user attached {} file(s), saved outside the project:\n{}]",
        files.len(),
        lines.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_stay_one_plain_path_component() {
        assert_eq!(safe_name("Bericht 2026.pdf"), "Bericht 2026.pdf");
        assert_eq!(safe_name("../../.ssh/id_rsa"), "_.._.ssh_id_rsa");
        assert_eq!(safe_name("-rf"), "rf");
        assert_eq!(safe_name("a,b:c\u{202E}.txt"), "a_b_c_.txt");
        assert_eq!(safe_name("..."), "datei");
        assert_eq!(safe_name("Größe äöü.md"), "Größe äöü.md");
        assert_eq!(safe_name(&"x".repeat(300)).chars().count(), 120);
    }

    #[test]
    fn documents_are_named_by_the_end_of_their_id() {
        assert_eq!(
            document_ref("0193f0aa-1b2c-7d3e-8f40-5a6b7c8d9e0f"),
            "7c8d9e0f"
        );
        assert_eq!(document_ref("kurz"), "kurz");
    }

    #[test]
    fn equal_names_get_a_number() {
        let mut used = HashSet::new();
        assert_eq!(unique("a.pdf".to_owned(), &mut used), "a.pdf");
        assert_eq!(unique("a.pdf".to_owned(), &mut used), "a (2).pdf");
        assert_eq!(unique("a.pdf".to_owned(), &mut used), "a (3).pdf");
        assert_eq!(unique("README".to_owned(), &mut used), "README");
        assert_eq!(unique("README".to_owned(), &mut used), "README (2)");
    }
}
