//! The device tunnel to the harness server (Phase 4): `harness connect`
//! keeps a WebSocket open, the server sends prompts of linked web chats, and
//! the device answers with harness-independent agent events.
//!
//! Mirrors `packages/shared/src/agent.ts`; the server validates everything
//! the device sends against those schemas, the device checks everything the
//! server asks for against its own configuration.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Protocol version; must match the server's `TUNNEL_VERSION`.
pub const TUNNEL_VERSION: u32 = 1;

/// Path of the tunnel on the server.
pub const TUNNEL_PATH: &str = "/api/device/tunnel";

/// Longest tool output sent, in bytes. The server accepts 64 000 characters;
/// bytes are never fewer than UTF-16 units, so this always fits.
pub const MAX_TOOL_OUTPUT: usize = 60_000;

/// Longest notice or error text, in bytes (server limit: 4 000 characters).
pub const MAX_NOTICE: usize = 3_900;

/// A project offered to web chats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteProject {
    /// Project name.
    pub name: String,
    /// Harnesses with a machine interface (adapter).
    pub harnesses: Vec<String>,
}

/// Severity of a [`AgentEvent::Notice`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NoticeLevel {
    /// Progress and hints.
    Info,
    /// Something the user should look at.
    Warning,
    /// Something failed.
    Error,
}

/// State of a step in [`AgentEvent::Progress`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProgressState {
    /// Under way.
    Running,
    /// Finished.
    Done,
    /// Failed; the reply usually ends with an error.
    Failed,
}

/// State of the device's VM, as the web app shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VmStatus {
    /// Running and reachable.
    Running,
    /// Not running; starts when an agent needs it.
    Stopped,
    /// Being started.
    Starting,
    /// Not known (no VM service).
    Unknown,
}

/// State of a gateway question in [`AgentEvent::Approval`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalState {
    /// Waits for an answer.
    Pending,
    /// Answered (here, in the terminal or by timeout).
    Resolved,
}

/// What an agent does, independent of the harness.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
    /// A model call (with its tool calls) starts.
    #[serde(rename = "turn.start")]
    TurnStart,
    /// A model call (with its tool calls) ended.
    #[serde(rename = "turn.end")]
    TurnEnd,
    /// A text block starts.
    #[serde(rename = "text.start")]
    TextStart {
        /// Block id, unique within the reply.
        id: String,
    },
    /// Text to append to a block.
    #[serde(rename = "text.delta")]
    TextDelta {
        /// Block id.
        id: String,
        /// The text.
        delta: String,
    },
    /// A text block ended.
    #[serde(rename = "text.end")]
    TextEnd {
        /// Block id.
        id: String,
    },
    /// A reasoning block starts.
    #[serde(rename = "reasoning.start")]
    ReasoningStart {
        /// Block id.
        id: String,
    },
    /// Reasoning text to append.
    #[serde(rename = "reasoning.delta")]
    ReasoningDelta {
        /// Block id.
        id: String,
        /// The text.
        delta: String,
    },
    /// A reasoning block ended.
    #[serde(rename = "reasoning.end")]
    ReasoningEnd {
        /// Block id.
        id: String,
    },
    /// A tool call starts.
    #[serde(rename = "tool.start")]
    ToolStart {
        /// Call id.
        id: String,
        /// Tool name, e.g. `bash`.
        name: String,
        /// Arguments.
        input: Map<String, Value>,
    },
    /// Output so far of a running tool call.
    #[serde(rename = "tool.update")]
    ToolUpdate {
        /// Call id.
        id: String,
        /// Output, truncated to [`MAX_TOOL_OUTPUT`].
        output: String,
    },
    /// A tool call ended.
    #[serde(rename = "tool.end")]
    ToolEnd {
        /// Call id.
        id: String,
        /// Output, truncated to [`MAX_TOOL_OUTPUT`].
        output: String,
        /// Whether the tool failed.
        #[serde(rename = "isError")]
        is_error: bool,
    },
    /// A step on the way to an answer (environment, VM, agent), so the user
    /// sees that something happens; the same id updates the step.
    #[serde(rename = "progress")]
    Progress {
        /// Step id, unique within the reply.
        id: String,
        /// What happens, in German.
        label: String,
        /// Its state.
        state: ProgressState,
        /// Duration or reason, if any.
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    /// A message of harness or the harness itself.
    #[serde(rename = "notice")]
    Notice {
        /// Severity.
        level: NoticeLevel,
        /// German text, truncated to [`MAX_NOTICE`].
        text: String,
    },
    /// A question of the project's gateway.
    #[serde(rename = "approval")]
    Approval {
        /// Question id.
        id: u64,
        /// Host the agent wants to reach.
        host: String,
        /// Port.
        port: u16,
        /// Open or answered.
        state: ApprovalState,
    },
    /// Tokens of one model call.
    #[serde(rename = "usage")]
    Usage {
        /// Input tokens, cached ones included.
        #[serde(rename = "inputTokens")]
        input_tokens: u64,
        /// Output tokens.
        #[serde(rename = "outputTokens")]
        output_tokens: u64,
        /// Input tokens read from the provider's cache.
        #[serde(rename = "cachedInputTokens", skip_serializing_if = "Option::is_none")]
        cached_input_tokens: Option<u64>,
        /// Cost in US dollars, as the harness computes it.
        #[serde(skip_serializing_if = "Option::is_none")]
        cost: Option<f64>,
        /// Context window of the model, if known.
        #[serde(rename = "contextWindow", skip_serializing_if = "Option::is_none")]
        context_window: Option<u64>,
    },
    /// The reply failed; ends it.
    #[serde(rename = "error")]
    Error {
        /// German text, truncated to [`MAX_NOTICE`].
        message: String,
    },
    /// The agent has nothing left to do for this prompt; ends the reply.
    #[serde(rename = "done")]
    Done {
        /// Whether it was interrupted.
        aborted: bool,
    },
}

impl AgentEvent {
    /// A notice with its text truncated to what the server accepts.
    pub fn notice(level: NoticeLevel, text: impl AsRef<str>) -> Self {
        AgentEvent::Notice {
            level,
            text: truncate(text.as_ref(), MAX_NOTICE),
        }
    }

    /// An error with its text truncated to what the server accepts.
    pub fn error(message: impl AsRef<str>) -> Self {
        AgentEvent::Error {
            message: truncate(message.as_ref(), MAX_NOTICE),
        }
    }
}

/// Messages to the server.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum DeviceMessage {
    /// First message after connecting.
    #[serde(rename = "hello")]
    Hello {
        /// [`TUNNEL_VERSION`].
        version: u32,
        /// Projects offered to web chats.
        projects: Vec<RemoteProject>,
    },
    /// The offered projects changed.
    #[serde(rename = "projects")]
    Projects {
        /// Projects offered to web chats.
        projects: Vec<RemoteProject>,
    },
    /// State of the computer's environment.
    #[serde(rename = "status")]
    Status {
        /// The VM.
        vm: VmStatus,
    },
    /// An event of a session's current reply.
    #[serde(rename = "session.event")]
    SessionEvent {
        /// Session id.
        session: String,
        /// The event.
        event: AgentEvent,
    },
    /// The session ended on the device (harness exited, refused, failed).
    #[serde(rename = "session.closed")]
    SessionClosed {
        /// Session id.
        session: String,
        /// Why, in German, if not a normal end.
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

/// The chat behind a session.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConversationRef {
    /// Conversation id.
    pub id: String,
    /// Its title, if it has one yet.
    pub title: Option<String>,
}

/// Model chosen in the chat.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    /// Credential on the server.
    pub credential_id: String,
    /// Provider's model id.
    pub model_id: String,
}

/// A user message for a session.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Prompt {
    /// Session id (the conversation id).
    pub session: String,
    /// Project to work in.
    pub project: String,
    /// Harness to run.
    pub harness: String,
    /// The chat.
    pub conversation: ConversationRef,
    /// The message.
    pub text: String,
    /// Earlier messages as text, for a new agent session.
    pub history: Option<String>,
    /// Model chosen in the chat.
    pub model: Option<ModelRef>,
}

/// The user's answer to a gateway question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// Only the waiting requests.
    Once,
    /// A few minutes.
    Minutes,
    /// Until the gateway stops.
    Session,
    /// Until the gateway stops, and into the project's allow list.
    Always,
    /// Refuse.
    Deny,
}

/// Messages from the server.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    /// Sent once after connecting.
    #[serde(rename = "welcome")]
    Welcome {
        /// The server's protocol version.
        version: u32,
    },
    /// A message of a linked chat.
    #[serde(rename = "session.prompt")]
    Prompt(Prompt),
    /// Interrupt the session's reply.
    #[serde(rename = "session.cancel")]
    Cancel {
        /// Session id.
        session: String,
    },
    /// End the session (chat unlinked or deleted).
    #[serde(rename = "session.close")]
    Close {
        /// Session id.
        session: String,
    },
    /// Answer to a gateway question.
    #[serde(rename = "approval.decide")]
    Decide {
        /// Session id.
        session: String,
        /// Question id.
        id: u64,
        /// The answer.
        decision: Decision,
    },
    /// A message type of a newer server; ignored.
    #[serde(other)]
    Unknown,
}

/// `text` cut to at most `max` bytes on a character boundary, with a note
/// when something was cut.
pub fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    const NOTE: &str = "\n… (gekürzt)";
    let mut end = max.saturating_sub(NOTE.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{NOTE}", &text[..end])
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn events_serialize_like_the_server_expects() {
        let tool = AgentEvent::ToolEnd {
            id: "c1".into(),
            output: "ok".into(),
            is_error: false,
        };
        assert_eq!(
            serde_json::to_value(&tool).unwrap(),
            json!({ "type": "tool.end", "id": "c1", "output": "ok", "isError": false })
        );
        let usage = AgentEvent::Usage {
            input_tokens: 1,
            output_tokens: 2,
            cached_input_tokens: None,
            cost: Some(0.5),
            context_window: Some(1000),
        };
        assert_eq!(
            serde_json::to_value(&usage).unwrap(),
            json!({ "type": "usage", "inputTokens": 1, "outputTokens": 2, "cost": 0.5, "contextWindow": 1000 })
        );
        let closed = DeviceMessage::SessionClosed {
            session: "s".into(),
            reason: None,
        };
        assert_eq!(
            serde_json::to_value(&closed).unwrap(),
            json!({ "type": "session.closed", "session": "s" })
        );
        let progress = AgentEvent::Progress {
            id: "vm".into(),
            label: "VM starten".into(),
            state: ProgressState::Done,
            detail: None,
        };
        assert_eq!(
            serde_json::to_value(&progress).unwrap(),
            json!({ "type": "progress", "id": "vm", "label": "VM starten", "state": "done" })
        );
        let status = DeviceMessage::Status {
            vm: VmStatus::Stopped,
        };
        assert_eq!(
            serde_json::to_value(&status).unwrap(),
            json!({ "type": "status", "vm": "stopped" })
        );
        let approval = AgentEvent::Approval {
            id: 3,
            host: "example.com".into(),
            port: 443,
            state: ApprovalState::Pending,
        };
        assert_eq!(serde_json::to_value(&approval).unwrap()["state"], "pending");
    }

    #[test]
    fn server_messages_parse_and_unknown_ones_are_ignored() {
        let prompt: ServerMessage = serde_json::from_value(json!({
            "type": "session.prompt",
            "session": "c1",
            "project": "demo",
            "harness": "pi",
            "conversation": { "id": "c1", "title": null },
            "text": "Hallo",
            "history": null,
            "model": { "credentialId": "k1", "modelId": "deepseek-chat" }
        }))
        .unwrap();
        let ServerMessage::Prompt(prompt) = prompt else {
            panic!("not a prompt")
        };
        assert_eq!(prompt.model.unwrap().model_id, "deepseek-chat");
        let decide: ServerMessage = serde_json::from_value(
            json!({ "type": "approval.decide", "session": "c1", "id": 4, "decision": "always" }),
        )
        .unwrap();
        assert!(matches!(
            decide,
            ServerMessage::Decide {
                decision: Decision::Always,
                ..
            }
        ));
        let newer: ServerMessage =
            serde_json::from_value(json!({ "type": "files.list", "session": "c1" })).unwrap();
        assert_eq!(newer, ServerMessage::Unknown);
    }

    #[test]
    fn truncation_keeps_characters_whole() {
        assert_eq!(truncate("kurz", 10), "kurz");
        let long = "ä".repeat(100);
        let cut = truncate(&long, 50);
        assert!(cut.len() <= 50);
        assert!(cut.ends_with("(gekürzt)"));
    }
}
