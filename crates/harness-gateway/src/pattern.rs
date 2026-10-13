//! Host names of requests and the domain patterns rules are written in.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

/// A domain in an allow or deny list: `example.com` or `*.example.com`
/// (subdomains only). Stored lowercase.
///
/// IP literals are rejected on purpose: rules name domains, and the gateway
/// decides about addresses itself after resolving names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DomainPattern {
    wildcard: bool,
    domain: String,
}

impl DomainPattern {
    /// Whether `host` is covered by this pattern. Case and a trailing dot
    /// are ignored.
    pub fn matches(&self, host: &str) -> bool {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if self.wildcard {
            host.strip_suffix(&self.domain)
                .is_some_and(|rest| rest.len() > 1 && rest.ends_with('.'))
        } else {
            host == self.domain
        }
    }

    /// Whether this is a `*.` pattern.
    pub fn is_wildcard(&self) -> bool {
        self.wildcard
    }
}

impl FromStr for DomainPattern {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let lower = text.trim().to_ascii_lowercase();
        let (wildcard, domain) = match lower.strip_prefix("*.") {
            Some(rest) => (true, rest.to_owned()),
            None => (false, lower),
        };
        let labels: Vec<&str> = domain.split('.').collect();
        let label_ok = |l: &&str| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        };
        let tld_ok = labels
            .last()
            .is_some_and(|tld| tld.chars().any(|c| c.is_ascii_alphabetic()));
        if labels.len() < 2 || !labels.iter().all(label_ok) || !tld_ok || domain.len() > 253 {
            return Err(format!(
                "ungültige Domain {text:?}, erwartet z. B. \"api.example.com\" oder \"*.example.com\""
            ));
        }
        Ok(DomainPattern { wildcard, domain })
    }
}

impl fmt::Display for DomainPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.wildcard {
            write!(f, "*.{}", self.domain)
        } else {
            f.write_str(&self.domain)
        }
    }
}

/// Target of a request: a host name or an IP literal.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Host {
    /// Lowercase host name without trailing dot.
    Name(String),
    /// IP literal.
    Ip(IpAddr),
}

impl Host {
    /// Parses the host part of a request target; IPv6 literals in brackets.
    ///
    /// Names are restricted to letters, digits, `-` and `_`, and the last
    /// label must start with a letter. That rules out the legacy numeric
    /// forms the system resolver would turn into addresses (`127.1`,
    /// `0x7f000001`, `2130706433`), so an address always arrives as
    /// [`Host::Ip`] and never hides behind something that looks like a name.
    pub fn parse(text: &str) -> Result<Host, String> {
        let invalid = || format!("ungültiger Host {text:?}");
        if let Some(inner) = text.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
            return inner
                .parse::<Ipv6Addr>()
                .map(|ip| Host::Ip(IpAddr::V6(ip)))
                .map_err(|_| invalid());
        }
        if let Ok(ip) = text.parse::<Ipv4Addr>() {
            return Ok(Host::Ip(IpAddr::V4(ip)));
        }
        let name = text.strip_suffix('.').unwrap_or(text).to_ascii_lowercase();
        let labels: Vec<&str> = name.split('.').collect();
        let label_ok = |l: &&str| {
            !l.is_empty()
                && l.len() <= 63
                && l.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        };
        let last_ok = labels
            .last()
            .is_some_and(|l| l.starts_with(|c: char| c.is_ascii_alphabetic()));
        if name.len() > 253 || !labels.iter().all(label_ok) || !last_ok {
            return Err(invalid());
        }
        Ok(Host::Name(name))
    }

    /// The name, or `None` for an IP literal.
    pub fn name(&self) -> Option<&str> {
        match self {
            Host::Name(name) => Some(name),
            Host::Ip(_) => None,
        }
    }
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Host::Name(name) => f.write_str(name),
            Host::Ip(IpAddr::V4(ip)) => write!(f, "{ip}"),
            Host::Ip(IpAddr::V6(ip)) => write!(f, "[{ip}]"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_patterns() {
        let exact: DomainPattern = "API.DeepSeek.com".parse().unwrap();
        assert_eq!(exact.to_string(), "api.deepseek.com");
        assert!(exact.matches("api.deepseek.com"));
        assert!(exact.matches("API.deepseek.com."));
        assert!(!exact.matches("x.api.deepseek.com"));
        assert!(!exact.is_wildcard());

        let wildcard: DomainPattern = "*.githubusercontent.com".parse().unwrap();
        assert!(wildcard.matches("objects.githubusercontent.com"));
        assert!(wildcard.matches("a.b.githubusercontent.com"));
        assert!(!wildcard.matches("githubusercontent.com"));
        assert!(!wildcard.matches("evilgithubusercontent.com"));
        assert!(wildcard.is_wildcard());

        for bad in [
            "",
            "localhost",
            "1.2.3.4",
            "*.com.",
            "a..b",
            "-a.com",
            "a.com:443",
            "https://a.com",
            "*",
            "a.*.com",
            "ex ample.com",
        ] {
            assert!(bad.parse::<DomainPattern>().is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn hosts() {
        assert_eq!(
            Host::parse("Example.COM.").unwrap(),
            Host::Name("example.com".into())
        );
        assert_eq!(
            Host::parse("gateway").unwrap(),
            Host::Name("gateway".into())
        );
        assert_eq!(
            Host::parse("_srv.example.com").unwrap().to_string(),
            "_srv.example.com"
        );
        assert_eq!(
            Host::parse("10.0.0.1").unwrap(),
            Host::Ip("10.0.0.1".parse().unwrap())
        );
        let v6 = Host::parse("[::1]").unwrap();
        assert_eq!(v6, Host::Ip("::1".parse().unwrap()));
        assert_eq!(v6.to_string(), "[::1]");
        assert_eq!(v6.name(), None);

        // Legacy numeric forms the resolver would accept as addresses.
        for bad in [
            "",
            "127.1",
            "0x7f000001",
            "2130706433",
            "0177.0.0.1",
            "a..b",
            "a b",
            "::1",
            "[127.0.0.1]",
            "exa\u{0}mple.com",
            "ex%61mple.com",
        ] {
            assert!(Host::parse(bad).is_err(), "{bad:?} accepted");
        }
    }
}
