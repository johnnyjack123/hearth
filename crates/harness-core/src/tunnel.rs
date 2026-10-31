//! The device tunnel to the harness server (Phase 4): `harness connect`
//! keeps a WebSocket open, the server sends prompts of linked web chats, and
//! the device answers with harness-independent agent events.
//!
//! Mirrors `packages/shared/src/agent.ts`; the server validates everything
//! the device sends against those schemas, the device checks everything the
//! server asks for against its own configuration.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::config::AppSource;

/// Protocol version of this harness. The server greets with its own; the
/// device speaks the lower of both and only sends what that version knows.
pub const TUNNEL_VERSION: u32 = 9;

/// Version that brought VM activity, the idle time and `vm.stop`.
pub const VERSION_VM_CONTROL: u32 = 2;

/// Version that brought taking over the agent's work (`session.takeover`,
/// the `takeover` event) and autosync.
pub const VERSION_TAKEOVER: u32 = 3;

/// Version that brought pi's settings from the chat: the thinking level
/// with each prompt, summarizing on request (`session.compact`) and the
/// `compaction` event for pi's summaries.
pub const VERSION_PI_SETTINGS: u32 = 4;

/// Version that brought the app catalog in the browser: the device reports
/// its apps ([`DeviceMessage::Apps`]), the server switches them per project
/// ([`ServerMessage::AppsSet`]).
pub const VERSION_APPS: u32 = 5;

/// Version that brought the account's own programs: the server sends its
/// templates ([`ServerMessage::AppsTemplates`]), and each app in the report
/// says where it comes from.
pub const VERSION_TEMPLATES: u32 = 6;

/// Version that brought program files from the computer: the browser asks
/// for one ([`ServerMessage::AppsFile`]), the device answers with the id of
/// the request the user approves on the computer, and reports the requests
/// still waiting.
pub const VERSION_APP_FILES: u32 = 7;

/// Version that brought the `workspace` event: after each reply and
/// takeover, what of the agent's work the user's folder lacks.
pub const VERSION_WORKSPACE: u32 = 8;

/// Version that brought files in prompts: the device downloads them from
/// the server and copies them into the instance.
pub const VERSION_FILES: u32 = 9;

/// Longest summary of pi a `compaction` event carries; the server accepts
/// 100,000 characters.
pub const MAX_SUMMARY: usize = 60_000;

/// Most findings a takeover report carries; the server accepts 100.
pub const MAX_REPORTED_FINDINGS: usize = 100;

/// Idle minutes until the VM stops, until the server says otherwise
/// (`DEFAULT_IDLE_MINUTES` in `@harness/shared`).
pub const DEFAULT_IDLE_MINUTES: u32 = 30;

/// Longest idle time the server may set (`MAX_IDLE_MINUTES`).
pub const MAX_IDLE_MINUTES: u32 = 24 * 60;

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
    /// Being stopped (protocol 2).
    Stopping,
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

/// Progress of a summary of the harness, see [`AgentEvent::Compaction`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CompactionState {
    /// Being written.
    Running,
    /// Written.
    Done,
    /// Failed or aborted.
    Failed,
}

/// Why the harness summarized its context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CompactionTrigger {
    /// The context ran full.
    Auto,
    /// The user asked (in the chat or in pi).
    Manual,
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
    /// Taking over the agent's work: where it stands, what the check found
    /// and how it ended; the same report updates in place (protocol 3).
    #[serde(rename = "takeover")]
    Takeover {
        /// The report so far.
        report: TakeoverReport,
    },
    /// The harness summarized its context: `running`, then `done` or
    /// `failed` (protocol 4; older servers get a notice instead).
    #[serde(rename = "compaction")]
    Compaction {
        /// How far it is.
        state: CompactionState,
        /// Because the context ran full, or because the user asked.
        trigger: CompactionTrigger,
        /// The summary, truncated to [`MAX_SUMMARY`].
        #[serde(skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
        /// Tokens of the context before.
        #[serde(rename = "tokensBefore", skip_serializing_if = "Option::is_none")]
        tokens_before: Option<u64>,
        /// Estimated tokens after.
        #[serde(rename = "tokensAfter", skip_serializing_if = "Option::is_none")]
        tokens_after: Option<u64>,
        /// Prompt tokens of the call that wrote the summary.
        #[serde(rename = "inputTokens", skip_serializing_if = "Option::is_none")]
        input_tokens: Option<u64>,
        /// Completion tokens of that call.
        #[serde(rename = "outputTokens", skip_serializing_if = "Option::is_none")]
        output_tokens: Option<u64>,
        /// Cost of writing the summary in US dollars.
        #[serde(skip_serializing_if = "Option::is_none")]
        cost: Option<f64>,
        /// Why it failed, German.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// What of the agent's work the user's folder does not have yet, after
    /// a reply or a takeover (protocol 8).
    #[serde(rename = "workspace")]
    Workspace {
        /// Commits the user's checked-out branch lacks.
        commits: u32,
        /// Files changed but not committed in the agent's clone.
        changes: u32,
    },
    /// The agent has nothing left to do for this prompt; ends the reply.
    #[serde(rename = "done")]
    Done {
        /// Whether it was interrupted.
        aborted: bool,
    },
}

/// Where taking over the agent's work stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TakeoverState {
    /// Under way; `step` says what happens.
    Running,
    /// Merged into the checked-out branch.
    Merged,
    /// Nothing new to take over.
    UpToDate,
    /// Not merged: severe findings wait for the user's approval in the
    /// terminal (`request`).
    Held,
    /// Not merged: the merge would conflict.
    Conflict,
    /// Not merged: the host repository is not ready (uncommitted changes,
    /// detached HEAD …); `reason` says why.
    Refused,
    /// Something failed; `reason` says what.
    Failed,
}

/// A finding of the merge check, as the chat shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ReportedFinding {
    /// `high`, `medium` or `low`.
    pub severity: &'static str,
    /// Rule id, e.g. `install-script`, `osv-malware`, `jev`.
    pub rule: String,
    /// File in the repository; empty for findings about the whole change.
    pub path: String,
    /// Line in the new file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// What and why, in German.
    pub message: String,
    /// The line in question, shortened.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

/// Report of taking over the agent's work into the user's folder.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TakeoverReport {
    /// Where it stands.
    pub state: TakeoverState,
    /// What happens right now (German), while running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// Started by autosync after a reply rather than by the user.
    pub auto: bool,
    /// Files the agent had left uncommitted and harness committed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub committed: Option<u32>,
    /// Commits brought into the user's branch, or waiting for approval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commits: Option<u32>,
    /// Files the merge changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<u32>,
    /// The user's branch on the host.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Checks that ran (German), e.g. "feste Regeln", "OSV".
    pub checks: Vec<String>,
    /// Findings, most severe first, at most [`MAX_REPORTED_FINDINGS`].
    pub findings: Vec<ReportedFinding>,
    /// Id for `harness merge --approve <id>` when held.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request: Option<String>,
    /// Files that would conflict.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
    /// Hints along the way (German), e.g. a check that could not run.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Why it was refused or failed (German).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl TakeoverReport {
    /// A report that just started.
    pub fn running(auto: bool, step: &str) -> Self {
        TakeoverReport {
            state: TakeoverState::Running,
            step: Some(step.to_owned()),
            auto,
            committed: None,
            commits: None,
            files: None,
            branch: None,
            checks: Vec::new(),
            findings: Vec::new(),
            request: None,
            conflicts: Vec::new(),
            notes: Vec::new(),
            reason: None,
        }
    }
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

impl DeviceMessage {
    /// The message as a server speaking `version` understands it, `None` if
    /// it should not get it: a server before protocol 4 knows no
    /// `compaction` event and would drop the connection, so it gets a notice
    /// instead; one before protocol 8 simply does not hear about the
    /// workspace.
    pub fn for_version(self, version: u32) -> Option<DeviceMessage> {
        Some(match self {
            DeviceMessage::SessionEvent {
                event: AgentEvent::Workspace { .. },
                ..
            } if version < VERSION_WORKSPACE => return None,
            // Older servers know neither `source` (6) nor `pending` (7).
            DeviceMessage::Apps { mut report } if version < VERSION_APP_FILES => {
                report.pending.clear();
                if version < VERSION_TEMPLATES {
                    for app in &mut report.catalog {
                        app.source = None;
                    }
                }
                DeviceMessage::Apps { report }
            }
            DeviceMessage::AppsResult {
                request,
                ok,
                message,
                approval: Some(_),
            } if version < VERSION_APP_FILES => DeviceMessage::AppsResult {
                request,
                ok,
                message,
                approval: None,
            },
            DeviceMessage::SessionEvent {
                session,
                event: AgentEvent::Compaction { state, error, .. },
            } if version < VERSION_PI_SETTINGS => {
                let event = match state {
                    CompactionState::Running => AgentEvent::notice(
                        NoticeLevel::Info,
                        "pi fasst den bisherigen Verlauf zusammen …",
                    ),
                    CompactionState::Done => {
                        AgentEvent::notice(NoticeLevel::Info, "pi hat den Verlauf zusammengefasst.")
                    }
                    CompactionState::Failed => AgentEvent::notice(
                        NoticeLevel::Warning,
                        format!(
                            "pi konnte den Verlauf nicht zusammenfassen: {}",
                            error.as_deref().unwrap_or("unbekannter Fehler")
                        ),
                    ),
                };
                DeviceMessage::SessionEvent { session, event }
            }
            other => other,
        })
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
        /// What keeps it busy; only from protocol 2 on.
        #[serde(flatten)]
        activity: Option<VmActivity>,
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
    /// Answer to [`ServerMessage::VmStop`] (protocol 2).
    #[serde(rename = "vm.result")]
    VmResult {
        /// The request's id.
        request: String,
        /// Whether the VM is off now.
        ok: bool,
        /// What happened, in German.
        message: String,
    },
    /// The app catalog and which apps the offered projects switched on
    /// (protocol 5); sent after the hello and whenever it changes.
    #[serde(rename = "apps")]
    Apps {
        /// The report.
        report: AppsReport,
    },
    /// Answer to [`ServerMessage::AppsSet`] (protocol 5) and
    /// [`ServerMessage::AppsFile`] (protocol 7), after the new
    /// [`DeviceMessage::Apps`].
    #[serde(rename = "apps.result")]
    AppsResult {
        /// The request's id.
        request: String,
        /// Whether it worked: apps switched, or a file waits for approval.
        ok: bool,
        /// What happened, in German.
        message: String,
        /// Id of the file request to approve on the computer (protocol 7).
        #[serde(skip_serializing_if = "Option::is_none")]
        approval: Option<String>,
    },
}

/// The apps of a device, for the browser (protocol 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppsReport {
    /// Every app the device knows: the catalog and its own recipes.
    pub catalog: Vec<AppInfo>,
    /// Whether `version` reflects the VM; it is only known once the VM ran.
    pub versions: bool,
    /// Apps switched on per offered project, without the ones they require.
    pub projects: Vec<ProjectApps>,
    /// Program files the browser asked for, waiting for the user's
    /// approval on the computer (protocol 7).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<PendingFile>,
}

/// A program file waiting for approval on the computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PendingFile {
    /// Id for `harness app approve`.
    pub id: String,
    /// App name it becomes.
    pub name: String,
    /// Name for people.
    pub title: String,
    /// The file on the computer.
    pub path: String,
    /// What it is, in German.
    pub kind: String,
}

/// One app of an [`AppsReport`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppInfo {
    /// App name.
    pub name: String,
    /// Name for people.
    pub title: String,
    /// One line for listings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Apps it requires.
    pub requires: Vec<String>,
    /// Version installed in the VM; `None` if not installed or unknown.
    pub version: Option<String>,
    /// Whether its recipe changed since it was installed.
    pub outdated: bool,
    /// Where it comes from (protocol 6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<AppSource>,
}

/// Apps a project switched on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectApps {
    /// Project name.
    pub name: String,
    /// `[apps] enabled` of the project.
    pub enabled: Vec<String>,
}

/// Use of the VM, sent with [`DeviceMessage::Status`] from protocol 2 on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct VmActivity {
    /// Agent replies running.
    pub replies: u32,
    /// harness commands in a terminal that use the VM.
    pub terminals: u32,
    /// Seconds until the idle VM is stopped; `None` while something uses
    /// it, while it is off, or with idle time 0.
    #[serde(rename = "stopIn")]
    pub stop_in: Option<u64>,
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
    /// Take the agent's work over after the reply if the check finds
    /// nothing severe (protocol 3; older servers do not send it).
    #[serde(default)]
    pub autosync: bool,
    /// pi's thinking level chosen in the chat (`off` … `max`); none for
    /// pi's own setting (protocol 4).
    #[serde(default)]
    pub thinking: Option<String>,
    /// Files attached to the message (protocol 9).
    #[serde(default)]
    pub files: Vec<PromptFile>,
}

/// A file attached to a message of a linked chat, as the server describes
/// it; the bytes come from `/api/device/files/<id>`. Name and kind come
/// from the server, which checked the contents; the device cleans the
/// name once more before it becomes a path.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptFile {
    /// File id (UUID).
    pub id: String,
    /// Name as uploaded.
    pub name: String,
    /// Media type the server determined.
    pub media_type: String,
    /// `image`, `document` or `text`.
    pub kind: String,
    /// Size in bytes.
    pub size: u64,
    /// Characters of text the model reads, for documents and text files.
    #[serde(default)]
    pub chars: Option<u64>,
    /// Set when that text was cut.
    #[serde(default)]
    pub truncated: Option<bool>,
}

/// A request to summarize the agent's context of a session's chat.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CompactRequest {
    /// Session id (the conversation id).
    pub session: String,
    /// Project of the chat.
    pub project: String,
    /// Harness of the chat.
    pub harness: String,
    /// The chat.
    pub conversation: ConversationRef,
}

impl CompactRequest {
    /// A prompt without a message, to start the chat's session for the
    /// summary when none runs.
    pub fn as_prompt(&self) -> Prompt {
        Prompt {
            session: self.session.clone(),
            project: self.project.clone(),
            harness: self.harness.clone(),
            conversation: self.conversation.clone(),
            text: String::new(),
            history: None,
            model: None,
            autosync: false,
            thinking: None,
            files: Vec::new(),
        }
    }
}

/// A request to take over the agent's work of a session's chat.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TakeoverRequest {
    /// Session id (the conversation id).
    pub session: String,
    /// Project whose work is taken over.
    pub project: String,
    /// Harness of the chat.
    pub harness: String,
    /// The chat.
    pub conversation: ConversationRef,
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
    /// Take over the agent's work into the user's folder (protocol 3).
    #[serde(rename = "session.takeover")]
    Takeover(TakeoverRequest),
    /// Have the harness summarize its context now (protocol 4).
    #[serde(rename = "session.compact")]
    Compact(CompactRequest),
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
    /// Settings of this device from the web app (protocol 2).
    #[serde(rename = "settings")]
    Settings {
        /// Idle minutes until the VM stops; 0: never.
        #[serde(rename = "idleMinutes")]
        idle_minutes: u32,
    },
    /// The user wants the VM off now (protocol 2).
    #[serde(rename = "vm.stop")]
    VmStop {
        /// Id to repeat in [`DeviceMessage::VmResult`].
        request: String,
    },
    /// The user switched a project's apps in the browser (protocol 5).
    #[serde(rename = "apps.set")]
    AppsSet {
        /// Id to repeat in [`DeviceMessage::AppsResult`].
        request: String,
        /// The project.
        project: String,
        /// Apps it should have switched on from now on.
        enabled: Vec<String>,
    },
    /// The browser asks for a program file of this computer (protocol 7).
    /// Nothing happens before the user approves it here.
    #[serde(rename = "apps.file")]
    AppsFile {
        /// Id to repeat in [`DeviceMessage::AppsResult`].
        request: String,
        /// The file on this computer.
        path: String,
        /// App name it should become.
        name: String,
        /// Name for people.
        title: String,
    },
    /// The account's own programs, after the hello and whenever they
    /// change (protocol 6). Each stays a raw value until the device reads
    /// it, so one it cannot read does not cost it the others.
    #[serde(rename = "apps.templates")]
    AppsTemplates {
        /// All templates of the account.
        templates: Vec<Value>,
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
        // pi's summaries: an event for servers of protocol 4, a notice for older ones.
        let done = DeviceMessage::SessionEvent {
            session: "s".into(),
            event: AgentEvent::Compaction {
                state: CompactionState::Done,
                trigger: CompactionTrigger::Manual,
                summary: Some("kurz".into()),
                tokens_before: Some(10),
                tokens_after: Some(2),
                input_tokens: None,
                output_tokens: None,
                cost: None,
                error: None,
            },
        };
        assert_eq!(
            serde_json::to_value(done.clone().for_version(4)).unwrap()["event"],
            json!({ "type": "compaction", "state": "done", "trigger": "manual", "summary": "kurz", "tokensBefore": 10, "tokensAfter": 2 })
        );
        assert_eq!(
            serde_json::to_value(done.for_version(3)).unwrap()["event"],
            json!({ "type": "notice", "level": "info", "text": "pi hat den Verlauf zusammengefasst." })
        );
        // The thinking level and the request to summarize from the server.
        let prompt: ServerMessage = serde_json::from_value(json!({
            "type": "session.prompt", "session": "s", "project": "p", "harness": "pi",
            "conversation": { "id": "s", "title": null }, "text": "hi", "history": null, "model": null,
            "autosync": false, "thinking": "high"
        }))
        .unwrap();
        assert!(
            matches!(prompt, ServerMessage::Prompt(p) if p.thinking.as_deref() == Some("high") && p.files.is_empty())
        );
        // Files attached in the chat (protocol 9).
        let with_files: ServerMessage = serde_json::from_value(json!({
            "type": "session.prompt", "session": "s", "project": "p", "harness": "pi",
            "conversation": { "id": "s", "title": null }, "text": "", "history": null, "model": null,
            "autosync": false, "files": [{
                "id": "01a1928b-95d4-715f-b716-44769d398636", "name": "bericht.pdf",
                "mediaType": "application/pdf", "size": 1234, "kind": "document", "chars": 99
            }]
        }))
        .unwrap();
        let ServerMessage::Prompt(prompt) = with_files else {
            panic!("no prompt");
        };
        assert_eq!(prompt.files[0].media_type, "application/pdf");
        assert_eq!(prompt.files[0].chars, Some(99));
        assert_eq!(prompt.files[0].truncated, None);
        let compact: ServerMessage = serde_json::from_value(json!({
            "type": "session.compact", "session": "s", "project": "p", "harness": "pi",
            "conversation": { "id": "s", "title": null }
        }))
        .unwrap();
        assert!(matches!(compact, ServerMessage::Compact(r) if r.as_prompt().text.is_empty()));

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
            activity: None,
        };
        assert_eq!(
            serde_json::to_value(&status).unwrap(),
            json!({ "type": "status", "vm": "stopped" })
        );
        let busy = DeviceMessage::Status {
            vm: VmStatus::Running,
            activity: Some(VmActivity {
                replies: 1,
                terminals: 0,
                stop_in: None,
            }),
        };
        assert_eq!(
            serde_json::to_value(&busy).unwrap(),
            json!({ "type": "status", "vm": "running", "replies": 1, "terminals": 0, "stopIn": null })
        );
        let result = DeviceMessage::VmResult {
            request: "r1".into(),
            ok: true,
            message: "Die VM ist gestoppt.".into(),
        };
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            json!({ "type": "vm.result", "request": "r1", "ok": true, "message": "Die VM ist gestoppt." })
        );
        let mut report = TakeoverReport::running(true, "Prüfen");
        report.findings.push(ReportedFinding {
            severity: "high",
            rule: "install-script".into(),
            path: "package.json".into(),
            line: Some(3),
            message: "postinstall".into(),
            excerpt: None,
        });
        assert_eq!(
            serde_json::to_value(AgentEvent::Takeover { report }).unwrap(),
            json!({
                "type": "takeover",
                "report": {
                    "state": "running", "step": "Prüfen", "auto": true, "checks": [],
                    "findings": [{ "severity": "high", "rule": "install-script", "path": "package.json", "line": 3, "message": "postinstall" }]
                }
            })
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
        let settings: ServerMessage =
            serde_json::from_value(json!({ "type": "settings", "idleMinutes": 15 })).unwrap();
        assert_eq!(settings, ServerMessage::Settings { idle_minutes: 15 });
        let stop: ServerMessage =
            serde_json::from_value(json!({ "type": "vm.stop", "request": "r1" })).unwrap();
        assert_eq!(
            stop,
            ServerMessage::VmStop {
                request: "r1".into()
            }
        );
        let takeover: ServerMessage = serde_json::from_value(json!({
            "type": "session.takeover",
            "session": "c1",
            "project": "demo",
            "harness": "pi",
            "conversation": { "id": "c1", "title": "Hallo" }
        }))
        .unwrap();
        assert!(matches!(takeover, ServerMessage::Takeover(request) if request.project == "demo"));
        let apps: ServerMessage = serde_json::from_value(json!({
            "type": "apps.set", "request": "r2", "project": "demo", "enabled": ["c", "rust"]
        }))
        .unwrap();
        assert_eq!(
            apps,
            ServerMessage::AppsSet {
                request: "r2".into(),
                project: "demo".into(),
                enabled: vec!["c".into(), "rust".into()],
            }
        );
        let templates: ServerMessage = serde_json::from_value(json!({
            "type": "apps.templates",
            "templates": [{ "name": "tree", "title": "tree", "kind": "apt", "packages": ["tree"] }]
        }))
        .unwrap();
        assert!(
            matches!(templates, ServerMessage::AppsTemplates { templates } if templates.len() == 1)
        );
        let newer: ServerMessage =
            serde_json::from_value(json!({ "type": "files.list", "session": "c1" })).unwrap();
        assert_eq!(newer, ServerMessage::Unknown);
    }

    #[test]
    fn app_reports_serialize_like_the_server_expects() {
        let report = DeviceMessage::Apps {
            report: AppsReport {
                catalog: vec![
                    AppInfo {
                        name: "c".into(),
                        title: "C/C++".into(),
                        description: Some("gcc".into()),
                        requires: vec![],
                        version: Some("14.2.0".into()),
                        outdated: false,
                        source: Some(AppSource::Builtin),
                    },
                    AppInfo {
                        name: "rust".into(),
                        title: "Rust".into(),
                        description: None,
                        requires: vec!["c".into()],
                        version: None,
                        outdated: false,
                        source: Some(AppSource::Server),
                    },
                ],
                versions: true,
                projects: vec![ProjectApps {
                    name: "demo".into(),
                    enabled: vec!["rust".into()],
                }],
                pending: vec![PendingFile {
                    id: "K7QF2M".into(),
                    name: "tool".into(),
                    title: "Tool".into(),
                    path: "C:\\Tools\\tool".into(),
                    kind: "Archiv (zip)".into(),
                }],
            },
        };
        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            json!({ "type": "apps", "report": {
                "catalog": [
                    { "name": "c", "title": "C/C++", "description": "gcc", "requires": [],
                      "version": "14.2.0", "outdated": false, "source": "builtin" },
                    { "name": "rust", "title": "Rust", "requires": ["c"], "version": null,
                      "outdated": false, "source": "server" }
                ],
                "versions": true,
                "projects": [{ "name": "demo", "enabled": ["rust"] }],
                "pending": [{ "id": "K7QF2M", "name": "tool", "title": "Tool",
                              "path": "C:\\Tools\\tool", "kind": "Archiv (zip)" }]
            } })
        );
        // A server before protocol 6 would reject the fields.
        assert_eq!(
            serde_json::to_value(report.for_version(VERSION_APPS)).unwrap(),
            json!({ "type": "apps", "report": {
                "catalog": [
                    { "name": "c", "title": "C/C++", "description": "gcc", "requires": [],
                      "version": "14.2.0", "outdated": false },
                    { "name": "rust", "title": "Rust", "requires": ["c"], "version": null,
                      "outdated": false }
                ],
                "versions": true,
                "projects": [{ "name": "demo", "enabled": ["rust"] }]
            } })
        );
        let result = DeviceMessage::AppsResult {
            request: "r2".into(),
            ok: false,
            message: "App cobol gibt es nicht".into(),
            approval: None,
        };
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            json!({ "type": "apps.result", "request": "r2", "ok": false,
                    "message": "App cobol gibt es nicht" })
        );
        let waiting = DeviceMessage::AppsResult {
            request: "r3".into(),
            ok: true,
            message: "wartet".into(),
            approval: Some("K7QF2M".into()),
        };
        assert_eq!(
            serde_json::to_value(&waiting).unwrap()["approval"],
            json!("K7QF2M")
        );
        assert!(
            serde_json::to_value(waiting.for_version(VERSION_TEMPLATES))
                .unwrap()
                .get("approval")
                .is_none()
        );
        // What the user's folder lacks; servers before protocol 8 never hear of it.
        let workspace = DeviceMessage::SessionEvent {
            session: "c1".into(),
            event: AgentEvent::Workspace {
                commits: 2,
                changes: 1,
            },
        };
        assert_eq!(
            serde_json::to_value(&workspace).unwrap()["event"],
            json!({ "type": "workspace", "commits": 2, "changes": 1 })
        );
        assert!(workspace.clone().for_version(VERSION_WORKSPACE).is_some());
        assert!(workspace.for_version(VERSION_APP_FILES).is_none());
        let file: ServerMessage = serde_json::from_value(json!({
            "type": "apps.file", "request": "r3", "path": "C:\\x\\tool.zip",
            "name": "tool", "title": "Tool"
        }))
        .unwrap();
        assert!(matches!(file, ServerMessage::AppsFile { name, .. } if name == "tool"));
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
