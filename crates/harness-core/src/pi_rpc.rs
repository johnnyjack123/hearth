//! pi in RPC mode (`pi --mode rpc`) as the agent of a web chat: commands
//! for pi's stdin and the translation of its stdout records into
//! [`AgentEvent`]s. See pi's `docs/rpc.md` and `docs/json.md`.
//!
//! Everything pi prints comes from the container and is untrusted: records
//! are parsed leniently, unknown ones are skipped, and texts are truncated
//! before they leave the device.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

use crate::tunnel::{AgentEvent, MAX_TOOL_OUTPUT, NoticeLevel, truncate};

/// Arguments that give each chat its own pi session, continued across
/// restarts of `harness connect`: pi creates the session on first use.
pub fn session_args(session: &str, title: Option<&str>) -> Vec<String> {
    let mut args = vec!["--session-id".to_owned(), session.to_owned()];
    if let Some(title) = title.map(str::trim).filter(|t| !t.is_empty()) {
        args.extend(["--name".to_owned(), title.chars().take(80).collect()]);
    }
    args
}

/// `prompt`; while pi is still working the message waits for the end.
pub fn prompt(id: &str, message: &str, busy: bool) -> String {
    let mut command = json!({ "id": id, "type": "prompt", "message": message });
    if busy {
        command["streamingBehavior"] = json!("followUp");
    }
    command.to_string()
}

/// `abort`: interrupts the current run.
pub fn abort() -> String {
    json!({ "type": "abort" }).to_string()
}

/// `set_model`.
pub fn set_model(id: &str, provider: &str, model: &str) -> String {
    json!({ "id": id, "type": "set_model", "provider": provider, "modelId": model }).to_string()
}

/// `get_state`, answered with the session's message count among others.
pub fn get_state(id: &str) -> String {
    json!({ "id": id, "type": "get_state" }).to_string()
}

/// The first prompt of a new pi session in a chat that already has
/// messages: the earlier conversation as context, then the message.
pub fn with_history(history: &str, message: &str) -> String {
    format!(
        "Bisheriger Verlauf dieses Chats, bevor du dazukamst (nur zur Information):\n\
         ---\n{history}\n---\n\nNeue Nachricht:\n{message}"
    )
}

/// A `response` record of pi.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    /// Command id, if the command had one.
    pub id: Option<String>,
    /// Command name, e.g. `prompt`.
    pub command: String,
    /// Whether pi accepted it.
    pub success: bool,
    /// pi's error text.
    pub error: Option<String>,
    /// Result data.
    pub data: Value,
}

/// What a record of pi leads to.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// An event for the chat.
    Event(AgentEvent),
    /// A line to write back to pi's stdin.
    Reply(String),
    /// The answer to a command.
    Response(Response),
}

/// Updates of a running tool are sent at most this often; `bash` can
/// report every line.
const UPDATE_INTERVAL: Duration = Duration::from_millis(300);

/// Turns pi's stdout records into [`Output`]s, one record at a time.
#[derive(Debug, Default)]
pub struct Translator {
    turn: u32,
    last_update: HashMap<String, Instant>,
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .map(|part| match part.get("type").and_then(Value::as_str) {
                Some("text") => part
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                Some("image") => "[Bild]".to_owned(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

impl Translator {
    /// A fresh translator, one per pi process.
    pub fn new() -> Self {
        Self::default()
    }

    fn part_id(&self, prefix: &str, record: &Value) -> String {
        let index = record
            .get("contentIndex")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        format!("{prefix}{}-{index}", self.turn)
    }

    /// The outputs of one stdout line of pi; lines that are not JSON, or
    /// records harness does not use, give none.
    pub fn feed(&mut self, line: &str) -> Vec<Output> {
        let Ok(record) = serde_json::from_str::<Value>(line.trim_end_matches('\r')) else {
            return Vec::new();
        };
        let event = |e| vec![Output::Event(e)];
        match str_field(&record, "type").unwrap_or_default() {
            "turn_start" => {
                self.turn += 1;
                event(AgentEvent::TurnStart)
            }
            "turn_end" => event(AgentEvent::TurnEnd),
            "message_update" => self.message_update(&record),
            "message_end" => Self::message_end(&record),
            "tool_execution_start" => {
                let id = str_field(&record, "toolCallId").unwrap_or_default();
                let name = str_field(&record, "toolName").unwrap_or("tool");
                let input = match record.get("args") {
                    Some(Value::Object(args)) => args.clone(),
                    _ => Map::new(),
                };
                if id.is_empty() {
                    return Vec::new();
                }
                event(AgentEvent::ToolStart {
                    id: id.to_owned(),
                    name: name.chars().take(100).collect(),
                    input,
                })
            }
            "tool_execution_update" => {
                let id = str_field(&record, "toolCallId").unwrap_or_default();
                let now = Instant::now();
                let due = self
                    .last_update
                    .get(id)
                    .is_none_or(|last| now.duration_since(*last) >= UPDATE_INTERVAL);
                let output = record
                    .get("partialResult")
                    .and_then(|r| r.get("content"))
                    .map(text_of)
                    .unwrap_or_default();
                if id.is_empty() || !due || output.is_empty() {
                    return Vec::new();
                }
                self.last_update.insert(id.to_owned(), now);
                event(AgentEvent::ToolUpdate {
                    id: id.to_owned(),
                    output: tail(&output, MAX_TOOL_OUTPUT),
                })
            }
            "tool_execution_end" => {
                let id = str_field(&record, "toolCallId").unwrap_or_default();
                self.last_update.remove(id);
                if id.is_empty() {
                    return Vec::new();
                }
                let output = record
                    .get("result")
                    .and_then(|r| r.get("content"))
                    .map(text_of)
                    .unwrap_or_default();
                event(AgentEvent::ToolEnd {
                    id: id.to_owned(),
                    output: truncate(&output, MAX_TOOL_OUTPUT),
                    is_error: record
                        .get("isError")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
            }
            "auto_retry_start" => event(AgentEvent::notice(
                NoticeLevel::Info,
                format!(
                    "Anfrage ans Modell wird wiederholt (Versuch {} von {}): {}",
                    record.get("attempt").and_then(Value::as_u64).unwrap_or(1),
                    record
                        .get("maxAttempts")
                        .and_then(Value::as_u64)
                        .unwrap_or(1),
                    str_field(&record, "errorMessage").unwrap_or("Fehler")
                ),
            )),
            "auto_retry_end" if record.get("success").and_then(Value::as_bool) == Some(false) => {
                event(AgentEvent::notice(
                    NoticeLevel::Error,
                    format!(
                        "Anfrage ans Modell gescheitert: {}",
                        str_field(&record, "finalError").unwrap_or("Fehler")
                    ),
                ))
            }
            "compaction_start" => event(AgentEvent::notice(
                NoticeLevel::Info,
                "pi fasst den bisherigen Verlauf zusammen …",
            )),
            "extension_error" => event(AgentEvent::notice(
                NoticeLevel::Warning,
                format!(
                    "Fehler in einer pi-Erweiterung: {}",
                    str_field(&record, "error").unwrap_or("unbekannt")
                ),
            )),
            "extension_ui_request" => Self::ui_request(&record),
            "agent_settled" => event(AgentEvent::Done {
                aborted: record
                    .get("aborted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            }),
            "response" => vec![Output::Response(Response {
                id: str_field(&record, "id").map(str::to_owned),
                command: str_field(&record, "command").unwrap_or_default().to_owned(),
                success: record
                    .get("success")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                error: str_field(&record, "error").map(str::to_owned),
                data: record.get("data").cloned().unwrap_or(Value::Null),
            })],
            _ => Vec::new(),
        }
    }

    fn message_update(&self, record: &Value) -> Vec<Output> {
        let Some(update) = record.get("assistantMessageEvent") else {
            return Vec::new();
        };
        let text_id = || self.part_id("t", update);
        let reasoning_id = || self.part_id("r", update);
        let delta = || str_field(update, "delta").unwrap_or_default().to_owned();
        let event = match str_field(update, "type").unwrap_or_default() {
            "text_start" => AgentEvent::TextStart { id: text_id() },
            "text_delta" => AgentEvent::TextDelta {
                id: text_id(),
                delta: delta(),
            },
            "text_end" => AgentEvent::TextEnd { id: text_id() },
            "thinking_start" => AgentEvent::ReasoningStart { id: reasoning_id() },
            "thinking_delta" => AgentEvent::ReasoningDelta {
                id: reasoning_id(),
                delta: delta(),
            },
            "thinking_end" => AgentEvent::ReasoningEnd { id: reasoning_id() },
            // Tool calls are reported once they run (tool_execution_*).
            _ => return Vec::new(),
        };
        vec![Output::Event(event)]
    }

    fn message_end(record: &Value) -> Vec<Output> {
        let Some(message) = record.get("message") else {
            return Vec::new();
        };
        if str_field(message, "role") != Some("assistant") {
            return Vec::new();
        }
        let mut outputs = Vec::new();
        if let Some(usage) = message.get("usage") {
            let tokens = |key| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
            let cached = tokens("cacheRead");
            outputs.push(Output::Event(AgentEvent::Usage {
                input_tokens: tokens("input") + cached + tokens("cacheWrite"),
                output_tokens: tokens("output"),
                cached_input_tokens: (cached > 0).then_some(cached),
                // pi prices each call from its own model catalog.
                cost: usage
                    .get("cost")
                    .and_then(|cost| cost.get("total"))
                    .and_then(Value::as_f64)
                    .filter(|cost| cost.is_finite() && *cost >= 0.0),
                context_window: None,
            }));
        }
        if str_field(message, "stopReason") == Some("error") {
            outputs.push(Output::Event(AgentEvent::notice(
                NoticeLevel::Error,
                format!(
                    "Fehler des Modells: {}",
                    str_field(message, "errorMessage").unwrap_or("unbekannt")
                ),
            )));
        }
        outputs
    }

    fn ui_request(record: &Value) -> Vec<Output> {
        let id = str_field(record, "id").unwrap_or_default();
        match str_field(record, "method").unwrap_or_default() {
            "notify" => {
                let level = match str_field(record, "notifyType") {
                    Some("warning") => NoticeLevel::Warning,
                    Some("error") => NoticeLevel::Error,
                    _ => NoticeLevel::Info,
                };
                vec![Output::Event(AgentEvent::notice(
                    level,
                    str_field(record, "message").unwrap_or_default(),
                ))]
            }
            // pi waits for an answer to dialogs; the web chat cannot show
            // them yet, so they are declined instead of hanging until a
            // timeout.
            "select" | "confirm" | "input" | "editor" => vec![
                Output::Reply(
                    json!({ "type": "extension_ui_response", "id": id, "cancelled": true })
                        .to_string(),
                ),
                Output::Event(AgentEvent::notice(
                    NoticeLevel::Warning,
                    format!(
                        "pi hat eine Rückfrage gestellt, die der Web-Chat noch nicht anzeigen kann, \
                         und sie wurde abgelehnt: {}",
                        str_field(record, "title").unwrap_or_default()
                    ),
                )),
            ],
            _ => Vec::new(),
        }
    }
}

/// The end of `text` within `max` bytes: for a running command the newest
/// output matters.
fn tail(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut start = text.len() - max + 4;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    format!("…\n{}", &text[start..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tunnel::MAX_NOTICE;

    fn events(translator: &mut Translator, lines: &[Value]) -> Vec<AgentEvent> {
        lines
            .iter()
            .flat_map(|line| translator.feed(&line.to_string()))
            .filter_map(|output| match output {
                Output::Event(event) => Some(event),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_run_becomes_text_tool_and_done_events() {
        let mut translator = Translator::new();
        let lines = [
            json!({ "type": "agent_start" }),
            json!({ "type": "turn_start" }),
            json!({ "type": "message_update", "assistantMessageEvent": { "type": "thinking_start", "contentIndex": 0 } }),
            json!({ "type": "message_update", "assistantMessageEvent": { "type": "thinking_delta", "contentIndex": 0, "delta": "hm" } }),
            json!({ "type": "message_update", "assistantMessageEvent": { "type": "text_start", "contentIndex": 1 } }),
            json!({ "type": "message_update", "assistantMessageEvent": { "type": "text_delta", "contentIndex": 1, "delta": "Ich schaue nach." } }),
            json!({ "type": "message_update", "assistantMessageEvent": { "type": "toolcall_start", "contentIndex": 2, "id": "c1", "toolName": "bash" } }),
            json!({ "type": "message_end", "message": { "role": "assistant", "usage": { "input": 100, "output": 20, "cacheRead": 50, "cacheWrite": 0, "cost": { "total": 0.25 } }, "stopReason": "toolUse" } }),
            json!({ "type": "tool_execution_start", "toolCallId": "c1", "toolName": "bash", "args": { "command": "ls" } }),
            json!({ "type": "tool_execution_update", "toolCallId": "c1", "partialResult": { "content": [{ "type": "text", "text": "REA" }] } }),
            json!({ "type": "tool_execution_update", "toolCallId": "c1", "partialResult": { "content": [{ "type": "text", "text": "README" }] } }),
            json!({ "type": "tool_execution_end", "toolCallId": "c1", "result": { "content": [{ "type": "text", "text": "README.md" }] }, "isError": false }),
            json!({ "type": "turn_end" }),
            json!({ "type": "agent_settled", "aborted": false }),
        ];
        let got = events(&mut translator, &lines);
        assert_eq!(
            got,
            vec![
                AgentEvent::TurnStart,
                AgentEvent::ReasoningStart { id: "r1-0".into() },
                AgentEvent::ReasoningDelta {
                    id: "r1-0".into(),
                    delta: "hm".into()
                },
                AgentEvent::TextStart { id: "t1-1".into() },
                AgentEvent::TextDelta {
                    id: "t1-1".into(),
                    delta: "Ich schaue nach.".into()
                },
                AgentEvent::Usage {
                    input_tokens: 150,
                    output_tokens: 20,
                    cached_input_tokens: Some(50),
                    cost: Some(0.25),
                    context_window: None,
                },
                AgentEvent::ToolStart {
                    id: "c1".into(),
                    name: "bash".into(),
                    input: json!({ "command": "ls" }).as_object().unwrap().clone()
                },
                // The second update came too soon after the first.
                AgentEvent::ToolUpdate {
                    id: "c1".into(),
                    output: "REA".into()
                },
                AgentEvent::ToolEnd {
                    id: "c1".into(),
                    output: "README.md".into(),
                    is_error: false
                },
                AgentEvent::TurnEnd,
                AgentEvent::Done { aborted: false },
            ]
        );
        // Block ids stay unique across turns.
        let next = events(
            &mut translator,
            &[
                json!({ "type": "turn_start" }),
                json!({ "type": "message_update", "assistantMessageEvent": { "type": "text_start", "contentIndex": 1 } }),
            ],
        );
        assert_eq!(next[1], AgentEvent::TextStart { id: "t2-1".into() });
    }

    #[test]
    fn dialogs_are_declined_and_notifications_shown() {
        let mut translator = Translator::new();
        let dialog = json!({ "type": "extension_ui_request", "id": "u1", "method": "confirm", "title": "Löschen?" });
        let outputs = translator.feed(&dialog.to_string());
        assert_eq!(
            outputs[0],
            Output::Reply(r#"{"cancelled":true,"id":"u1","type":"extension_ui_response"}"#.into())
        );
        assert!(matches!(
            &outputs[1],
            Output::Event(AgentEvent::Notice { level: NoticeLevel::Warning, text }) if text.contains("Löschen?")
        ));
        let notify = json!({ "type": "extension_ui_request", "id": "u2", "method": "notify", "message": "harness: Möglicher Prompt-Injection-Versuch", "notifyType": "warning" });
        assert_eq!(
            events(&mut translator, &[notify]),
            vec![AgentEvent::Notice {
                level: NoticeLevel::Warning,
                text: "harness: Möglicher Prompt-Injection-Versuch".into()
            }]
        );
        let status = json!({ "type": "extension_ui_request", "id": "u3", "method": "setStatus", "statusKey": "x" });
        assert!(translator.feed(&status.to_string()).is_empty());
    }

    #[test]
    fn responses_errors_and_garbage() {
        let mut translator = Translator::new();
        let response = json!({ "id": "p1", "type": "response", "command": "prompt", "success": false, "error": "No model selected" });
        assert_eq!(
            translator.feed(&response.to_string()),
            vec![Output::Response(Response {
                id: Some("p1".into()),
                command: "prompt".into(),
                success: false,
                error: Some("No model selected".into()),
                data: Value::Null,
            })]
        );
        let failed = json!({ "type": "message_end", "message": { "role": "assistant", "stopReason": "error", "errorMessage": "401 Unauthorized" } });
        assert!(matches!(
            &events(&mut translator, &[failed])[0],
            AgentEvent::Notice { level: NoticeLevel::Error, text } if text.contains("401")
        ));
        assert!(translator.feed("not json").is_empty());
        assert!(translator.feed(r#"{"type":"queue_update"}"#).is_empty());
        let long = json!({ "type": "extension_ui_request", "id": "u", "method": "notify", "message": "x".repeat(10_000) });
        let AgentEvent::Notice { text, .. } = &events(&mut translator, &[long])[0] else {
            panic!("no notice")
        };
        assert!(text.len() <= MAX_NOTICE);
    }

    #[test]
    fn commands_and_session_arguments() {
        assert_eq!(
            prompt("p1", "Hallo", true),
            r#"{"id":"p1","message":"Hallo","streamingBehavior":"followUp","type":"prompt"}"#
        );
        assert_eq!(
            session_args("c1", Some("  Mein Chat ")),
            ["--session-id", "c1", "--name", "Mein Chat"]
        );
        assert_eq!(session_args("c1", Some(" ")), ["--session-id", "c1"]);
        assert!(with_history("User: a", "b").ends_with("Neue Nachricht:\nb"));
        let long_output = "x".repeat(MAX_TOOL_OUTPUT * 2);
        assert!(tail(&long_output, MAX_TOOL_OUTPUT).len() <= MAX_TOOL_OUTPUT);
    }
}
