//! The rules a project applies to its gateway, and why a connection was
//! allowed or blocked.

use std::fmt;
use std::str::FromStr;

use crate::pattern::DomainPattern;

/// How hosts that are not on the allow list are treated.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Mode {
    /// Only the allow list and approvals.
    #[default]
    Allowlist,
    /// Every public host. The deny list still applies.
    Open,
}

impl Mode {
    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Allowlist => "allowlist",
            Mode::Open => "open",
        }
    }
}

/// Rules of one project. The host derives them from the project
/// configuration; the gateway only applies them.
///
/// The default blocks everything: that is what a freshly started gateway
/// enforces until the host has sent the project's rules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policy {
    /// Allow list only, or the whole public internet.
    pub mode: Mode,
    /// Hold requests to unknown hosts and ask the user, if someone watches.
    pub ask: bool,
    /// Allowed domains.
    pub allow: Vec<DomainPattern>,
    /// Blocked domains; they win over everything else.
    pub deny: Vec<DomainPattern>,
}

/// Wire form: `<mode> <ask|noask> <allow> <deny>`, lists comma-separated or
/// `-` when empty. Patterns contain neither spaces nor commas.
impl fmt::Display for Policy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let list = |patterns: &[DomainPattern]| {
            if patterns.is_empty() {
                "-".to_owned()
            } else {
                patterns
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            }
        };
        write!(
            f,
            "{} {} {} {}",
            self.mode.as_str(),
            if self.ask { "ask" } else { "noask" },
            list(&self.allow),
            list(&self.deny)
        )
    }
}

impl FromStr for Policy {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let words: Vec<&str> = text.split(' ').collect();
        let [mode, ask, allow, deny] = words[..] else {
            return Err(format!("Regeln {text:?}: erwartet 4 Felder"));
        };
        let mode = match mode {
            "allowlist" => Mode::Allowlist,
            "open" => Mode::Open,
            other => return Err(format!("unbekannter Modus {other:?}")),
        };
        let ask = match ask {
            "ask" => true,
            "noask" => false,
            other => return Err(format!("erwartet ask oder noask, nicht {other:?}")),
        };
        let list = |field: &str| -> Result<Vec<DomainPattern>, String> {
            if field == "-" {
                Ok(Vec::new())
            } else {
                field.split(',').map(str::parse).collect()
            }
        };
        Ok(Policy {
            mode,
            ask,
            allow: list(allow)?,
            deny: list(deny)?,
        })
    }
}

/// Why a connection was allowed or blocked. Shown in the log, sent over the
/// control protocol and named in the gateway's error pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Reason {
    /// On the allow list.
    Allowlist,
    /// Mode open.
    Open,
    /// Covered by an approval given earlier.
    Grant,
    /// The user approved this request.
    Approved,
    /// On the project's deny list.
    DenyList,
    /// The user refused this host.
    DeniedByUser,
    /// Not allowed, and nobody could be asked.
    NotAllowed,
    /// An IP literal outside mode open; rules only name domains.
    IpLiteral,
    /// The name resolves only to non-public addresses (host, LAN, VM).
    PrivateAddress,
    /// The user did not answer in time.
    Unanswered,
    /// The name does not resolve.
    ResolveFailed,
    /// No address accepted the connection.
    ConnectFailed,
    /// `llm.harness`, forwarded to the harness server.
    Llm,
    /// `llm.harness`, but the host has not set up LLM access.
    NoLlm,
}

impl Reason {
    const ALL: [Reason; 14] = [
        Reason::Llm,
        Reason::NoLlm,
        Reason::Allowlist,
        Reason::Open,
        Reason::Grant,
        Reason::Approved,
        Reason::DenyList,
        Reason::DeniedByUser,
        Reason::NotAllowed,
        Reason::IpLiteral,
        Reason::PrivateAddress,
        Reason::Unanswered,
        Reason::ResolveFailed,
        Reason::ConnectFailed,
    ];

    /// Wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Allowlist => "allowlist",
            Reason::Open => "open",
            Reason::Grant => "grant",
            Reason::Approved => "approved",
            Reason::DenyList => "deny-list",
            Reason::DeniedByUser => "denied-by-user",
            Reason::NotAllowed => "not-allowed",
            Reason::IpLiteral => "ip-literal",
            Reason::PrivateAddress => "private-address",
            Reason::Unanswered => "unanswered",
            Reason::ResolveFailed => "resolve-failed",
            Reason::ConnectFailed => "connect-failed",
            Reason::Llm => "llm",
            Reason::NoLlm => "no-llm",
        }
    }

    /// Short German description for the CLI and the error page.
    pub fn describe(self) -> &'static str {
        match self {
            Reason::Allowlist => "freigegeben",
            Reason::Open => "offen",
            Reason::Grant => "vorübergehend freigegeben",
            Reason::Approved => "vom Nutzer erlaubt",
            Reason::DenyList => "steht in der deny-Liste des Projekts",
            Reason::DeniedByUser => "vom Nutzer abgelehnt",
            Reason::NotAllowed => "nicht freigegeben",
            Reason::IpLiteral => "IP-Adressen sind nur bei preset = \"open\" erlaubt",
            Reason::PrivateAddress => "Host, lokales Netz und VM sind immer gesperrt",
            Reason::Unanswered => "keine Antwort vom Nutzer",
            Reason::ResolveFailed => "Name nicht auflösbar",
            Reason::ConnectFailed => "Verbindung fehlgeschlagen",
            Reason::Llm => "LLM über den harness-Server",
            Reason::NoLlm => "kein LLM-Zugang eingerichtet (auf dem Host: harness login)",
        }
    }

    /// The same in English, for agents reading the error page.
    pub fn describe_en(self) -> &'static str {
        match self {
            Reason::Allowlist => "allowed",
            Reason::Open => "open",
            Reason::Grant => "temporarily allowed",
            Reason::Approved => "approved by the user",
            Reason::DenyList => "on the project's deny list",
            Reason::DeniedByUser => "refused by the user",
            Reason::NotAllowed => "not allowed for this project",
            Reason::IpLiteral => "IP addresses are only allowed in open mode",
            Reason::PrivateAddress => "host, local network and VM are always blocked",
            Reason::Unanswered => "the user did not answer",
            Reason::ResolveFailed => "name does not resolve",
            Reason::ConnectFailed => "connection failed",
            Reason::Llm => "LLM via the harness server",
            Reason::NoLlm => "no LLM access set up (the user runs: harness login)",
        }
    }
}

impl FromStr for Reason {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        Reason::ALL
            .into_iter()
            .find(|r| r.as_str() == text)
            .ok_or_else(|| format!("unbekannter Grund {text:?}"))
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_round_trip() {
        let policy = Policy {
            mode: Mode::Allowlist,
            ask: true,
            allow: vec![
                "github.com".parse().unwrap(),
                "*.npmjs.org".parse().unwrap(),
            ],
            deny: vec![],
        };
        let wire = policy.to_string();
        assert_eq!(wire, "allowlist ask github.com,*.npmjs.org -");
        assert_eq!(wire.parse::<Policy>().unwrap(), policy);
        assert_eq!(
            Policy::default().to_string().parse::<Policy>().unwrap(),
            Policy::default()
        );

        for bad in [
            "",
            "open ask - - extra",
            "closed ask - -",
            "open maybe - -",
            "open ask 1.2.3.4 -",
            "open ask a.com,,b.com -",
        ] {
            assert!(bad.parse::<Policy>().is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn reasons_round_trip() {
        for reason in Reason::ALL {
            assert_eq!(reason.as_str().parse::<Reason>().unwrap(), reason);
        }
        assert!("other".parse::<Reason>().is_err());
    }
}
