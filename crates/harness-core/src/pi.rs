//! Connecting pi to `llm.harness`: its `~/.pi/agent/models.json` lists the
//! account's providers with the gateway as base URL and a dummy key.
//!
//! Providers pi knows itself keep its model catalog (context windows,
//! reasoning, prices); only their base URL changes. The path part of pi's
//! own default stays, so the request paths match what the server's
//! pass-through expects for that protocol (`apps/server/src/routes/llm.ts`).

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::gateway::LLM_HOST;
use crate::provider::{ExecOutput, ExecSpec, Provider};
use crate::server::DeviceCredential;
use crate::{Error, Result};

/// Placeholder key; the gateway replaces it with the device token.
pub const DUMMY_KEY: &str = "harness";

/// pi's file for custom providers and base URL overrides.
pub const MODELS_FILE: &str = "/home/agent/.pi/agent/models.json";

/// Result of [`models_json`].
#[derive(Debug, Clone, PartialEq)]
pub struct PiModels {
    /// Contents of `models.json`.
    pub json: Value,
    /// pi provider ids, in the order of the credentials.
    pub providers: Vec<String>,
    /// Credentials that could not be wired up, with the reason (German).
    pub skipped: Vec<String>,
}

/// pi's built-in provider for a credential, with the path of pi's default
/// base URL, or `None` if pi has no matching provider.
fn builtin(credential: &DeviceCredential) -> Option<(&'static str, &'static str)> {
    let host = credential
        .base_url
        .as_deref()
        .and_then(|url| url.split("://").nth(1))
        .and_then(|rest| rest.split(['/', ':']).next())
        .unwrap_or_default();
    match credential.kind.as_str() {
        "openai" => Some(("openai", "")),
        "anthropic" => Some(("anthropic", "")),
        // pi's default is …/v1beta and the server strips that version.
        "google" => Some(("google", "/v1beta")),
        "openrouter" => Some(("openrouter", "")),
        "openai-compatible" if host == "api.deepseek.com" => Some(("deepseek", "")),
        _ => None,
    }
}

fn slug(label: &str) -> String {
    let mut slug = String::new();
    for c in label.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

/// Builds `models.json` for the account's credentials.
pub fn models_json(credentials: &[DeviceCredential]) -> PiModels {
    let mut providers: BTreeMap<String, Value> = BTreeMap::new();
    let mut order = Vec::new();
    let mut skipped = Vec::new();
    for credential in credentials {
        let base = format!("http://{LLM_HOST}/{}", credential.id);
        if let Some((id, path)) = builtin(credential).filter(|(id, _)| !providers.contains_key(*id))
        {
            providers.insert(
                id.to_owned(),
                json!({ "baseUrl": format!("{base}{path}"), "apiKey": DUMMY_KEY }),
            );
            order.push(id.to_owned());
            continue;
        }
        let openai_style = matches!(credential.kind.as_str(), "ollama" | "openai-compatible");
        let models = credential.models.as_deref().unwrap_or_default();
        if !openai_style {
            skipped.push(format!(
                "„{}“ ({}): wird für pi noch nicht über den Server angebunden",
                credential.label, credential.kind
            ));
            continue;
        }
        if models.is_empty() {
            skipped.push(format!(
                "„{}“: der Server konnte keine Modelle abfragen",
                credential.label
            ));
            continue;
        }
        let stem = match slug(&credential.label) {
            s if s.is_empty() => credential.kind.clone(),
            s => s,
        };
        let mut id = format!("harness-{stem}");
        let mut n = 2;
        while providers.contains_key(&id) {
            id = format!("harness-{stem}-{n}");
            n += 1;
        }
        let models: Vec<Value> = models
            .iter()
            .map(|m| json!({ "id": m.id, "name": m.name }))
            .collect();
        providers.insert(
            id.clone(),
            json!({
                "baseUrl": base,
                "api": "openai-completions",
                "apiKey": DUMMY_KEY,
                "models": models,
            }),
        );
        order.push(id);
    }
    PiModels {
        json: json!({ "providers": providers }),
        providers: order,
        skipped,
    }
}

/// Larger files are not read; the volume belongs to the agent, which could
/// make the file arbitrarily large.
const MAX_MODELS_BYTES: usize = 1 << 20;

/// `models.json` as found in an instance.
#[derive(Debug, Clone, PartialEq)]
pub enum Existing {
    /// No file.
    Missing,
    /// Not a JSON object or too large.
    Invalid,
    /// The file's object.
    Json(Map<String, Value>),
}

/// Whether a provider entry is one harness wrote: the gateway as base URL
/// and the placeholder key. Nobody configures that by hand, so everything
/// else in the file is the user's and stays.
fn is_ours(entry: &Value) -> bool {
    let prefix = format!("http://{LLM_HOST}/");
    entry.get("apiKey").and_then(Value::as_str) == Some(DUMMY_KEY)
        && entry
            .get("baseUrl")
            .and_then(Value::as_str)
            .is_some_and(|url| url.starts_with(&prefix))
}

/// `file` without harness's provider entries; `None` if nothing else is left.
pub fn strip(file: &Map<String, Value>) -> Option<Map<String, Value>> {
    let mut file = file.clone();
    if let Some(Value::Object(providers)) = file.get_mut("providers") {
        providers.retain(|_, entry| !is_ours(entry));
        if providers.is_empty() {
            file.remove("providers");
        }
    }
    (!file.is_empty()).then_some(file)
}

/// `existing` with harness's entries replaced by `models`. An entry of the
/// user's with the same id wins; those ids are returned.
pub fn merge(existing: &Existing, models: &PiModels) -> (Value, Vec<String>) {
    let mut file = match existing {
        Existing::Json(file) => strip(file).unwrap_or_default(),
        Existing::Missing | Existing::Invalid => Map::new(),
    };
    let providers = file
        .entry("providers")
        .or_insert_with(|| Value::Object(Map::new()));
    if !providers.is_object() {
        // pi could not read such a file anyway.
        *providers = Value::Object(Map::new());
    }
    let providers = providers.as_object_mut().expect("made an object above");
    let mut own = Vec::new();
    for (id, entry) in models.json["providers"].as_object().into_iter().flatten() {
        if providers.contains_key(id) {
            own.push(id.clone());
        } else {
            providers.insert(id.clone(), entry.clone());
        }
    }
    (Value::Object(file), own)
}

fn run_script(
    provider: &dyn Provider,
    container: &str,
    script: &str,
    args: &[&str],
) -> Result<ExecOutput> {
    let mut command: Vec<String> = vec!["sh".into(), "-c".into(), script.into(), "sh".into()];
    command.extend(args.iter().map(|&a| a.to_owned()));
    provider.exec_output(&ExecSpec {
        container: container.to_owned(),
        command,
        ..ExecSpec::default()
    })
}

fn failed(action: &str, output: &ExecOutput) -> Error {
    Error::Unsupported(format!(
        "{MODELS_FILE} {action} fehlgeschlagen: {}",
        crate::sync::sanitize(output.stderr.trim())
    ))
}

/// Reads `models.json` in a running instance.
pub fn read_models(provider: &dyn Provider, container: &str) -> Result<Existing> {
    let limit = (MAX_MODELS_BYTES + 1).to_string();
    let output = run_script(
        provider,
        container,
        r#"[ -f "$1" ] || exit 3; head -c "$2" "$1""#,
        &[MODELS_FILE, &limit],
    )?;
    Ok(match output.code {
        0 if output.stdout.len() > MAX_MODELS_BYTES => Existing::Invalid,
        0 => match serde_json::from_str(&output.stdout) {
            Ok(Value::Object(file)) => Existing::Json(file),
            _ => Existing::Invalid,
        },
        3 => Existing::Missing,
        _ => return Err(failed("lesen", &output)),
    })
}

/// Writes `models.json` in a running instance; an unreadable old file is
/// kept as `models.json.bak`. The contents are no secret (the key is a
/// placeholder), so they travel as an argument: `podman exec` does not
/// reliably forward stdin's end.
pub fn write_models(
    provider: &dyn Provider,
    container: &str,
    file: &Value,
    backup: bool,
) -> Result<()> {
    let text = serde_json::to_string_pretty(file)
        .map_err(|e| Error::Unsupported(format!("models.json: {e}")))?;
    let script = r#"set -e
mkdir -p "$(dirname "$1")"
if [ "$3" = backup ]; then cp -f "$1" "$1.bak"; fi
printf '%s\n' "$2" > "$1.partial"
mv -f "$1.partial" "$1""#;
    let mode = if backup { "backup" } else { "" };
    let output = run_script(provider, container, script, &[MODELS_FILE, &text, mode])?;
    if output.code == 0 {
        Ok(())
    } else {
        Err(failed("schreiben", &output))
    }
}

/// Points pi at `llm.harness`: replaces harness's entries in `models.json`
/// and keeps the user's. Returns the provider ids where the user's own
/// entry wins.
pub fn wire(provider: &dyn Provider, container: &str, models: &PiModels) -> Result<Vec<String>> {
    let existing = read_models(provider, container)?;
    let (file, own) = merge(&existing, models);
    write_models(provider, container, &file, existing == Existing::Invalid)?;
    Ok(own)
}

/// Removes harness's entries from `models.json` after `harness logout`, so
/// pi uses its own logins again; deletes the file if nothing else is in it.
/// Returns whether there was anything to remove.
pub fn unwire(provider: &dyn Provider, container: &str) -> Result<bool> {
    let Existing::Json(file) = read_models(provider, container)? else {
        return Ok(false);
    };
    let wired = file
        .get("providers")
        .and_then(Value::as_object)
        .is_some_and(|providers| providers.values().any(is_ours));
    if !wired {
        return Ok(false);
    }
    match strip(&file) {
        Some(rest) => write_models(provider, container, &Value::Object(rest), false).map(|()| true),
        None => {
            let output = run_script(provider, container, r#"rm -f "$1""#, &[MODELS_FILE])?;
            if output.code == 0 {
                Ok(true)
            } else {
                Err(failed("löschen", &output))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::ModelInfo;

    fn credential(id: &str, kind: &str, label: &str, base_url: Option<&str>) -> DeviceCredential {
        DeviceCredential {
            id: id.into(),
            kind: kind.into(),
            label: label.into(),
            base_url: base_url.map(Into::into),
            models: None,
        }
    }

    #[test]
    fn builtin_providers_only_change_their_base_url() {
        let models = models_json(&[
            credential(
                "c1",
                "openai-compatible",
                "DeepSeek",
                Some("https://api.deepseek.com"),
            ),
            credential("c2", "anthropic", "Claude", None),
            credential("c3", "google", "Gemini", None),
            credential("c4", "anthropic", "Second", None),
        ]);
        assert_eq!(models.providers, ["deepseek", "anthropic", "google"]);
        let providers = &models.json["providers"];
        assert_eq!(
            providers["deepseek"],
            json!({ "baseUrl": "http://llm.harness/c1", "apiKey": "harness" })
        );
        assert_eq!(
            providers["google"]["baseUrl"],
            "http://llm.harness/c3/v1beta"
        );
        assert!(providers["deepseek"].get("models").is_none());
        // A second Anthropic key has no models list to become its own provider.
        assert_eq!(models.skipped.len(), 1);
        assert!(models.skipped[0].contains("Second"));
    }

    #[test]
    fn own_endpoints_become_providers_with_the_servers_models() {
        let mut ollama = credential(
            "c5",
            "ollama",
            "Mein Ollama!",
            Some("http://localhost:11434/v1"),
        );
        ollama.models = Some(vec![ModelInfo {
            id: "qwen2.5-coder:7b".into(),
            name: "Qwen".into(),
        }]);
        let mut empty = credential("c6", "openai-compatible", "Leer", Some("http://x/v1"));
        empty.models = Some(vec![]);
        let models = models_json(&[
            ollama.clone(),
            ollama,
            empty,
            credential("c7", "mistral", "Mistral", None),
        ]);
        assert_eq!(
            models.providers,
            ["harness-mein-ollama", "harness-mein-ollama-2"]
        );
        assert_eq!(
            models.json["providers"]["harness-mein-ollama"],
            json!({
                "baseUrl": "http://llm.harness/c5",
                "api": "openai-completions",
                "apiKey": "harness",
                "models": [{ "id": "qwen2.5-coder:7b", "name": "Qwen" }],
            })
        );
        assert_eq!(models.skipped.len(), 2);
    }

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn the_users_own_entries_stay() {
        let models = models_json(&[
            credential(
                "c1",
                "openai-compatible",
                "DeepSeek",
                Some("https://api.deepseek.com"),
            ),
            credential("c2", "anthropic", "Claude", None),
        ]);
        let users = json!({
            // Same id as one of harness's: the user's entry wins.
            "anthropic": { "baseUrl": "https://proxy.example.com", "apiKey": "own" },
            "local": { "baseUrl": "http://localhost:1234/v1", "api": "openai-completions" },
        });
        // Left over from an earlier login with a credential that is gone.
        let stale = json!({ "baseUrl": "http://llm.harness/old", "apiKey": "harness" });
        let mut providers = object(users.clone());
        providers.insert("openrouter".into(), stale);
        let existing = object(json!({ "providers": providers, "other": 1 }));

        let (file, own) = merge(&Existing::Json(existing.clone()), &models);
        assert_eq!(own, ["anthropic"]);
        assert_eq!(
            file,
            json!({
                "providers": {
                    "anthropic": users["anthropic"],
                    "local": users["local"],
                    "deepseek": { "baseUrl": "http://llm.harness/c1", "apiKey": "harness" },
                },
                "other": 1,
            })
        );

        // Logging out removes exactly harness's entries ...
        assert_eq!(
            strip(file.as_object().unwrap()),
            Some(object(json!({ "providers": users, "other": 1 })))
        );
        // ... and the file disappears if nothing else was in it.
        let (file, _) = merge(&Existing::Missing, &models);
        assert_eq!(file, models.json);
        assert_eq!(strip(file.as_object().unwrap()), None);
        let (file, _) = merge(&Existing::Invalid, &models);
        assert_eq!(file, models.json);
    }
}
