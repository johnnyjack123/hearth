//! Control protocol between the host and a running gateway.
//!
//! The host runs `harness-gateway ctl` inside the gateway container, which
//! relays lines to the gateway's Unix socket. One request per line, words
//! separated by single spaces. Every request is answered by zero or more data
//! lines and a final `ok` or `error <message>`. A connection that sent
//! `watch` also receives `event …` lines at any time, including between a
//! request and its answer.
//!
//! All fields are validated tokens (hosts, patterns, numbers, fixed words,
//! base64), so a line never needs quoting. Hosts originate from agent requests; they
//! pass [`Host::parse`] on both ends, which also keeps terminal escapes out of
//! the CLI.

use std::fmt;
use std::str::FromStr;

use crate::llm::LlmUpstream;
use crate::pattern::{DomainPattern, Host};
use crate::policy::{Policy, Reason};

/// Longest accepted line; a policy with a few hundred domains fits easily.
pub const MAX_LINE: usize = 64 * 1024;

/// A control connection that sends nothing for this long is closed. Watchers
/// therefore ping regularly; a watcher whose `podman exec` hangs after the
/// CLI died must not keep requests waiting for answers that never come.
pub const WATCH_IDLE_SECS: u64 = 45;

/// How long an approval lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Only the requests waiting for this decision.
    Once,
    /// The given number of seconds.
    For(u64),
    /// Until the gateway stops.
    Session,
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scope::Once => f.write_str("once"),
            Scope::For(seconds) => write!(f, "{seconds}"),
            Scope::Session => f.write_str("session"),
        }
    }
}

impl FromStr for Scope {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        match text {
            "once" => Ok(Scope::Once),
            "session" => Ok(Scope::Session),
            _ => match text.parse::<u64>() {
                Ok(seconds) if seconds > 0 => Ok(Scope::For(seconds)),
                _ => Err(format!(
                    "Dauer {text:?}: erwartet once, session oder Sekunden"
                )),
            },
        }
    }
}

/// A request from the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Replace the project's rules: `policy <policy>`.
    Policy(Policy),
    /// Allow a domain for a while: `allow <pattern> <seconds|session>`.
    Allow {
        /// Domain or `*.` pattern.
        pattern: DomainPattern,
        /// Duration; [`Scope::Once`] is only valid for `decide`.
        scope: Scope,
    },
    /// Block a domain until the gateway stops: `deny <pattern>`.
    Deny(DomainPattern),
    /// Take back what `allow` granted for a domain: `revoke <pattern>`.
    /// The host does this once an installer is done with its hosts.
    Revoke(DomainPattern),
    /// Answer a pending question: `decide <id> allow <scope>` or
    /// `decide <id> deny`.
    Decide {
        /// Id from the `pending` event.
        id: u64,
        /// `Some` allows for the given scope, `None` refuses.
        allow: Option<Scope>,
    },
    /// Set or clear where `llm.harness` forwards to: `llm <url> <token>`,
    /// `llm <url> <token> <ca>` (root certificates as comma-separated
    /// base64 DER) or `llm none`. Carries the device token, so the host
    /// sends it through stdin, never as an argument.
    Llm(Option<LlmUpstream>),
    /// Name the project the gateway belongs to: `project <name>`. Requests
    /// to the server carry it, see [`crate::llm::PROJECT_HEADER`].
    Project(String),
    /// Receive events on this connection and answer questions. While at
    /// least one connection watches, requests to unknown hosts wait for an
    /// answer instead of failing.
    Watch,
    /// Current rules, approvals, questions and the recent log.
    Status,
    /// Keep-alive; watchers must send something at least every
    /// [`WATCH_IDLE_SECS`] seconds.
    Ping,
    /// End the connection after the answer.
    Bye,
}

impl fmt::Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Request::Policy(policy) => write!(f, "policy {policy}"),
            Request::Allow { pattern, scope } => write!(f, "allow {pattern} {scope}"),
            Request::Deny(pattern) => write!(f, "deny {pattern}"),
            Request::Revoke(pattern) => write!(f, "revoke {pattern}"),
            Request::Decide {
                id,
                allow: Some(scope),
            } => write!(f, "decide {id} allow {scope}"),
            Request::Decide { id, allow: None } => write!(f, "decide {id} deny"),
            Request::Llm(Some(upstream)) => write!(f, "llm {upstream}"),
            Request::Llm(None) => f.write_str("llm none"),
            Request::Project(name) => write!(f, "project {name}"),
            Request::Watch => f.write_str("watch"),
            Request::Status => f.write_str("status"),
            Request::Ping => f.write_str("ping"),
            Request::Bye => f.write_str("bye"),
        }
    }
}

impl FromStr for Request {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let (command, rest, words): (&str, &str, Vec<&str>) = match text.split_once(' ') {
            Some((command, rest)) => (command, rest, rest.split(' ').collect()),
            None => (text, "", Vec::new()),
        };
        let arity = |n: usize| {
            if words.len() == n {
                Ok(())
            } else {
                Err(format!("{command}: erwartet {n} Argument(e)"))
            }
        };
        match command {
            "policy" => Ok(Request::Policy(rest.parse()?)),
            "allow" => {
                arity(2)?;
                Ok(Request::Allow {
                    pattern: words[0].parse()?,
                    scope: words[1].parse()?,
                })
            }
            "deny" => {
                arity(1)?;
                Ok(Request::Deny(words[0].parse()?))
            }
            "revoke" => {
                arity(1)?;
                Ok(Request::Revoke(words[0].parse()?))
            }
            "decide" => {
                let id = words
                    .first()
                    .and_then(|w| w.parse().ok())
                    .ok_or("decide: erwartet eine Id")?;
                match words[1..] {
                    ["allow", scope] => Ok(Request::Decide {
                        id,
                        allow: Some(scope.parse()?),
                    }),
                    ["deny"] => Ok(Request::Decide { id, allow: None }),
                    _ => Err("decide: erwartet allow <dauer> oder deny".to_owned()),
                }
            }
            "llm" => match words[..] {
                ["none"] => Ok(Request::Llm(None)),
                [url, token] => Ok(Request::Llm(Some(LlmUpstream::from_wire(
                    url, token, None,
                )?))),
                [url, token, ca] => Ok(Request::Llm(Some(LlmUpstream::from_wire(
                    url,
                    token,
                    Some(ca),
                )?))),
                _ => Err("llm: erwartet <url> <token> [<ca>] oder none".to_owned()),
            },
            "project" => {
                arity(1)?;
                if !crate::llm::valid_project(words[0]) {
                    return Err(format!("project: ungültiger Name {:?}", words[0]));
                }
                Ok(Request::Project(words[0].to_owned()))
            }
            "watch" | "status" | "ping" | "bye" => {
                arity(0)?;
                Ok(match command {
                    "watch" => Request::Watch,
                    "status" => Request::Status,
                    "ping" => Request::Ping,
                    _ => Request::Bye,
                })
            }
            other => Err(format!("unbekannter Befehl {other:?}")),
        }
    }
}

/// One connection attempt as recorded in the gateway's log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// Unix time in milliseconds.
    pub time_ms: u64,
    /// Whether the connection was let through.
    pub allowed: bool,
    /// Target host.
    pub host: Host,
    /// Target port.
    pub port: u16,
    /// Why.
    pub reason: Reason,
}

/// Wire form: `<ms> <allowed|blocked> <host> <port> <reason>`.
impl fmt::Display for LogEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {} {} {}",
            self.time_ms,
            verdict(self.allowed),
            self.host,
            self.port,
            self.reason
        )
    }
}

impl FromStr for LogEntry {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let words: Vec<&str> = text.split(' ').collect();
        let [time, allowed, host, port, reason] = words[..] else {
            return Err(format!("Protokollzeile {text:?}: erwartet 5 Felder"));
        };
        Ok(LogEntry {
            time_ms: number(time)?,
            allowed: parse_verdict(allowed)?,
            host: Host::parse(host)?,
            port: number(port)?,
            reason: reason.parse()?,
        })
    }
}

/// Something that happened, pushed to watchers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A request waits for a decision: `event pending <id> <host> <port>`.
    Pending {
        /// Id for `decide`.
        id: u64,
        /// Requested host.
        host: Host,
        /// Requested port.
        port: u16,
    },
    /// A question was answered or timed out:
    /// `event resolved <id> <allowed|blocked> <reason>`.
    Resolved {
        /// Id of the question.
        id: u64,
        /// Outcome.
        allowed: bool,
        /// Why.
        reason: Reason,
    },
    /// A connection attempt: `event log <entry>`.
    Log(LogEntry),
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::Pending { id, host, port } => write!(f, "event pending {id} {host} {port}"),
            Event::Resolved {
                id,
                allowed,
                reason,
            } => write!(f, "event resolved {id} {} {reason}", verdict(*allowed)),
            Event::Log(entry) => write!(f, "event log {entry}"),
        }
    }
}

impl FromStr for Event {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let rest = text
            .strip_prefix("event ")
            .ok_or_else(|| format!("kein Ereignis: {text:?}"))?;
        let (kind, rest) = rest.split_once(' ').unwrap_or((rest, ""));
        let words: Vec<&str> = rest.split(' ').collect();
        match (kind, &words[..]) {
            ("pending", [id, host, port]) => Ok(Event::Pending {
                id: number(id)?,
                host: Host::parse(host)?,
                port: number(port)?,
            }),
            ("resolved", [id, allowed, reason]) => Ok(Event::Resolved {
                id: number(id)?,
                allowed: parse_verdict(allowed)?,
                reason: reason.parse()?,
            }),
            ("log", _) => Ok(Event::Log(rest.parse()?)),
            _ => Err(format!("unbekanntes Ereignis {text:?}")),
        }
    }
}

/// One line received from the gateway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// The request succeeded.
    Ok,
    /// The request failed; the message is German UI text.
    Error(String),
    /// An event (watchers only).
    Event(Event),
    /// A data line belonging to the answer of the current request.
    Data(String),
}

impl Reply {
    /// Classifies a received line. Malformed events become [`Reply::Error`].
    pub fn parse(line: &str) -> Reply {
        if line == "ok" {
            Reply::Ok
        } else if let Some(message) = line.strip_prefix("error ") {
            Reply::Error(message.to_owned())
        } else if line.starts_with("event ") {
            match line.parse() {
                Ok(event) => Reply::Event(event),
                Err(message) => Reply::Error(message),
            }
        } else {
            Reply::Data(line.to_owned())
        }
    }
}

/// An approval as reported by `status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantInfo {
    /// Allowed domain or pattern.
    pub pattern: DomainPattern,
    /// Seconds left, `None` until the gateway stops.
    pub remaining_secs: Option<u64>,
}

/// Answer to `status`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    /// Rules in force.
    pub policy: Policy,
    /// Approvals given at runtime.
    pub grants: Vec<GrantInfo>,
    /// Hosts the user refused at runtime.
    pub denied: Vec<DomainPattern>,
    /// Open questions: id, host, port.
    pub pending: Vec<(u64, Host, u16)>,
    /// Number of connected watchers.
    pub watchers: usize,
    /// Where `llm.harness` forwards to (without the token), if set.
    pub llm: Option<String>,
    /// Recent connection attempts, oldest first.
    pub log: Vec<LogEntry>,
}

impl Status {
    /// Data lines of the `status` answer (without the final `ok`).
    pub fn to_lines(&self) -> Vec<String> {
        let mut lines = vec![format!("policy {}", self.policy)];
        if let Some(url) = &self.llm {
            lines.push(format!("llm {url}"));
        }
        for grant in &self.grants {
            match grant.remaining_secs {
                Some(seconds) => lines.push(format!("grant {} {seconds}", grant.pattern)),
                None => lines.push(format!("grant {} session", grant.pattern)),
            }
        }
        lines.extend(self.denied.iter().map(|p| format!("denied {p}")));
        lines.extend(
            self.pending
                .iter()
                .map(|(id, host, port)| format!("pending {id} {host} {port}")),
        );
        lines.push(format!("watchers {}", self.watchers));
        lines.extend(self.log.iter().map(|entry| format!("log {entry}")));
        lines
    }

    /// Parses the data lines of a `status` answer.
    pub fn from_lines<S: AsRef<str>>(lines: &[S]) -> Result<Status, String> {
        let mut status = Status::default();
        for line in lines {
            let line = line.as_ref();
            let (kind, rest) = line.split_once(' ').unwrap_or((line, ""));
            let words: Vec<&str> = rest.split(' ').collect();
            match (kind, &words[..]) {
                ("policy", _) => status.policy = rest.parse()?,
                ("grant", [pattern, remaining]) => status.grants.push(GrantInfo {
                    pattern: pattern.parse()?,
                    remaining_secs: match *remaining {
                        "session" => None,
                        seconds => Some(number(seconds)?),
                    },
                }),
                ("denied", [pattern]) => status.denied.push(pattern.parse()?),
                ("pending", [id, host, port]) => {
                    status
                        .pending
                        .push((number(id)?, Host::parse(host)?, number(port)?))
                }
                ("watchers", [count]) => status.watchers = number(count)?,
                // Re-validated with a placeholder token; only the URL is kept.
                ("llm", [url]) => {
                    status.llm = Some(LlmUpstream::new(url, "status-placeholder-token")?.url())
                }
                ("log", _) => status.log.push(rest.parse()?),
                _ => return Err(format!("unerwartete Statuszeile {line:?}")),
            }
        }
        Ok(status)
    }
}

fn verdict(allowed: bool) -> &'static str {
    if allowed { "allowed" } else { "blocked" }
}

fn parse_verdict(text: &str) -> Result<bool, String> {
    match text {
        "allowed" => Ok(true),
        "blocked" => Ok(false),
        other => Err(format!("erwartet allowed oder blocked, nicht {other:?}")),
    }
}

fn number<T: FromStr>(text: &str) -> Result<T, String> {
    text.parse()
        .map_err(|_| format!("keine gültige Zahl: {text:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::Mode;

    fn round_trip<T>(value: T)
    where
        T: fmt::Display + FromStr<Err = String> + PartialEq + fmt::Debug,
    {
        let wire = value.to_string();
        assert_eq!(wire.parse::<T>().unwrap(), value, "{wire}");
    }

    #[test]
    fn requests() {
        let pattern: DomainPattern = "example.com".parse().unwrap();
        for request in [
            Request::Policy(Policy {
                mode: Mode::Open,
                ask: false,
                allow: vec![],
                deny: vec![pattern.clone()],
            }),
            Request::Allow {
                pattern: pattern.clone(),
                scope: Scope::For(900),
            },
            Request::Allow {
                pattern: "*.example.com".parse().unwrap(),
                scope: Scope::Session,
            },
            Request::Deny(pattern),
            Request::Decide {
                id: 7,
                allow: Some(Scope::Once),
            },
            Request::Decide { id: 8, allow: None },
            Request::Llm(Some(
                LlmUpstream::new(
                    "http://192.168.127.254:8080/api/llm",
                    "abcdefghijklmnop0123",
                )
                .unwrap(),
            )),
            Request::Llm(Some(
                LlmUpstream::new("https://harness.example/api/llm", "abcdefghijklmnop0123")
                    .unwrap(),
            )),
            Request::Llm(Some(
                LlmUpstream::new("https://harness.example/api/llm", "abcdefghijklmnop0123")
                    .unwrap()
                    .with_ca(vec![
                        [0x30, 0x01, 0x05].to_vec(),
                        [0x30, 0x02, 1, 2].to_vec(),
                    ])
                    .unwrap(),
            )),
            Request::Llm(None),
            Request::Project("bildbearbeitung".to_owned()),
            Request::Revoke("registry.npmjs.org".parse().unwrap()),
            Request::Watch,
            Request::Status,
            Request::Ping,
            Request::Bye,
        ] {
            round_trip(request);
        }
        assert_eq!(
            "decide 3 allow 60".parse::<Request>().unwrap(),
            Request::Decide {
                id: 3,
                allow: Some(Scope::For(60))
            }
        );
        for bad in [
            "",
            "allow example.com",
            "allow example.com forever",
            "allow example.com 0",
            "allow 1.2.3.4 session",
            "deny",
            "decide x deny",
            "decide 1 maybe",
            "status now",
            "llm",
            "llm http://example.com",
            "llm ftp://example.com abcdefghijklmnop0123",
            "llm http://example.com abcdefghijklmnop0123 MAMFAAA=",
            "llm https://example.com abcdefghijklmnop0123 not-base64",
            "llm https://example.com abcdefghijklmnop0123 MAEF x",
            "watch ",
            "rm -rf /",
        ] {
            assert!(bad.parse::<Request>().is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn events_and_replies() {
        let host = Host::parse("example.com").unwrap();
        for event in [
            Event::Pending {
                id: 1,
                host: host.clone(),
                port: 443,
            },
            Event::Resolved {
                id: 1,
                allowed: false,
                reason: Reason::Unanswered,
            },
            Event::Log(LogEntry {
                time_ms: 1_760_000_000_000,
                allowed: true,
                host: Host::parse("[2001:4860::1]").unwrap(),
                port: 80,
                reason: Reason::Open,
            }),
        ] {
            let wire = event.to_string();
            assert_eq!(Reply::parse(&wire), Reply::Event(event));
        }
        assert_eq!(Reply::parse("ok"), Reply::Ok);
        assert_eq!(
            Reply::parse("error kaputt und so"),
            Reply::Error("kaputt und so".into())
        );
        assert_eq!(Reply::parse("watchers 1"), Reply::Data("watchers 1".into()));
        // Escape sequences in a host never get through.
        assert!(matches!(
            Reply::parse("event pending 1 \u{1b}[2J 443"),
            Reply::Error(_)
        ));
    }

    #[test]
    fn status_round_trip() {
        let status = Status {
            policy: Policy {
                mode: Mode::Allowlist,
                ask: true,
                allow: vec!["github.com".parse().unwrap()],
                deny: vec![],
            },
            grants: vec![
                GrantInfo {
                    pattern: "example.com".parse().unwrap(),
                    remaining_secs: Some(42),
                },
                GrantInfo {
                    pattern: "*.example.org".parse().unwrap(),
                    remaining_secs: None,
                },
            ],
            denied: vec!["evil.example".parse().unwrap()],
            pending: vec![(3, Host::parse("docs.rs").unwrap(), 443)],
            watchers: 2,
            llm: Some("https://harness.example:8443/api/llm".into()),
            log: vec![LogEntry {
                time_ms: 5,
                allowed: false,
                host: Host::parse("1.2.3.4").unwrap(),
                port: 443,
                reason: Reason::IpLiteral,
            }],
        };
        assert_eq!(Status::from_lines(&status.to_lines()).unwrap(), status);
        assert!(Status::from_lines(&["grant example.com"]).is_err());
    }
}
