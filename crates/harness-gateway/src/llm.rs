//! The virtual host `llm.harness`: how harnesses in instances reach their
//! model (ARCHITECTURE.md §6.4).
//!
//! Harnesses are configured with `http://llm.harness/…` and a dummy key. The
//! gateway forwards these requests to the LLM endpoint of the harness server
//! and adds the device token itself. No credential ever enters an instance,
//! so even in mode open a hijacked agent has no key to leak.

use std::fmt;

use crate::pattern::Host;

/// Name harnesses use for the LLM; only the gateway knows it.
pub const LLM_HOST: &str = "llm.harness";

/// Headers a client might use to send its own credentials; they are dropped
/// so only the device token reaches the server.
pub const CREDENTIAL_HEADERS: &[&str] = &[
    "authorization",
    "x-api-key",
    "api-key",
    "x-goog-api-key",
    "cookie",
];

/// Where `llm.harness` forwards to, and with which token.
#[derive(Clone, PartialEq, Eq)]
pub struct LlmUpstream {
    host: Host,
    port: u16,
    prefix: String,
    token: String,
}

impl LlmUpstream {
    /// `url` is `http://host[:port][/prefix]` as reachable from the gateway;
    /// `token` the device token.
    ///
    /// Only plain HTTP for now: the server runs on the user's machine and is
    /// reached through the VM's host address. A remote server needs TLS in
    /// the gateway first.
    pub fn new(url: &str, token: &str) -> Result<LlmUpstream, String> {
        let invalid = |reason: &str| format!("LLM-Adresse {url:?}: {reason}");
        let rest = url
            .strip_prefix("http://")
            .ok_or_else(|| invalid("nur http:// wird unterstützt"))?;
        let (authority, prefix) = match rest.find('/') {
            Some(index) => rest.split_at(index),
            None => (rest, ""),
        };
        // `[v6]:port`, `host:port` or just the host.
        let split = match authority.strip_prefix('[') {
            Some(_) => authority.find(']').and_then(|end| {
                let (host, rest) = authority.split_at(end + 1);
                match rest.strip_prefix(':') {
                    Some(port) => Some((host, Some(port))),
                    None if rest.is_empty() => Some((host, None)),
                    None => None,
                }
            }),
            None => Some(match authority.split_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (authority, None),
            }),
        };
        let (host, port) = split.ok_or_else(|| invalid("ungültiger Host"))?;
        let port = match port {
            None => 80,
            Some(port) => port
                .parse::<u16>()
                .ok()
                .filter(|p| *p > 0 && port.bytes().all(|b| b.is_ascii_digit()))
                .ok_or_else(|| invalid("ungültiger Port"))?,
        };
        let host = Host::parse(host).map_err(|e| invalid(&e))?;
        let prefix = prefix.trim_end_matches('/');
        if !safe_path(prefix) && !prefix.is_empty() {
            return Err(invalid("ungültiger Pfad"));
        }
        let token_ok = (16..=512).contains(&token.len())
            && token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._~+/=-".contains(&b));
        if !token_ok {
            return Err("ungültiges Geräte-Token".to_owned());
        }
        Ok(LlmUpstream {
            host,
            port,
            prefix: prefix.to_owned(),
            token: token.to_owned(),
        })
    }

    /// The upstream as URL, without the token.
    pub fn url(&self) -> String {
        format!(
            "http://{}{}",
            crate::http::host_header(&self.host, self.port),
            self.prefix
        )
    }

    /// Upstream host.
    pub fn host(&self) -> &Host {
        &self.host
    }

    /// Upstream port.
    pub fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    /// The upstream path for a request to `llm.harness`, or `None` if the
    /// path could climb out of the prefix (`..`, encoded characters).
    pub fn path(&self, request: &str) -> Option<String> {
        let (path, query) = match request.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (request, None),
        };
        if !safe_path(path) {
            return None;
        }
        Some(match query {
            Some(query) => format!("{}{path}?{query}", self.prefix),
            None => format!("{}{path}", self.prefix),
        })
    }
}

/// Absolute path of plain segments: no `..` or `.`, no empty segments, no
/// percent-encoding (which could hide either), only unreserved characters
/// plus `:` (Google's `model:streamGenerateContent`).
fn safe_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix('/') else {
        return false;
    };
    rest.split('/').all(|segment| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && segment
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~:@".contains(&b))
    })
}

/// Never prints the token.
impl fmt::Debug for LlmUpstream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LlmUpstream")
            .field("url", &self.url())
            .field("token", &"…")
            .finish()
    }
}

/// Wire form `<url> <token>`; carries the secret, so it is only ever sent
/// through stdin of `ctl`, never as an argument.
impl fmt::Display for LlmUpstream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.url(), self.token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "abcdefghijklmnop_0123456789";

    #[test]
    fn upstreams() {
        let up = LlmUpstream::new("http://192.168.127.254:8080/api/llm/", TOKEN).unwrap();
        assert_eq!(up.url(), "http://192.168.127.254:8080/api/llm");
        assert_eq!(up.port(), 8080);
        let default_port = LlmUpstream::new("http://harness.example", TOKEN).unwrap();
        assert_eq!(default_port.port(), 80);
        assert_eq!(default_port.url(), "http://harness.example");
        let v6 = LlmUpstream::new("http://[fd00::1]:8080/x", TOKEN).unwrap();
        assert_eq!(v6.url(), "http://[fd00::1]:8080/x");
        assert_eq!(
            LlmUpstream::new("http://[fd00::1]", TOKEN).unwrap().port(),
            80
        );

        for bad in [
            "https://example.com",
            "http://",
            "http://a b",
            "http://example.com:0",
            "http://example.com:x/",
            "http://[fd00::1]x",
            "http://example.com/../x",
            "http://example.com/a%2e",
        ] {
            assert!(LlmUpstream::new(bad, TOKEN).is_err(), "{bad:?} accepted");
        }
        assert!(LlmUpstream::new("http://example.com", "short").is_err());
        assert!(LlmUpstream::new("http://example.com", &format!("{TOKEN} x")).is_err());
    }

    #[test]
    fn paths_stay_below_the_prefix() {
        let up = LlmUpstream::new("http://127.0.0.1:8080/api/llm", TOKEN).unwrap();
        assert_eq!(
            up.path("/c1/v1/chat/completions").as_deref(),
            Some("/api/llm/c1/v1/chat/completions")
        );
        assert_eq!(
            up.path("/c1/v1beta/models/gemini:streamGenerateContent?alt=sse")
                .as_deref(),
            Some("/api/llm/c1/v1beta/models/gemini:streamGenerateContent?alt=sse")
        );
        for bad in [
            "/../api/conversations",
            "/c1/../../api",
            "/c1/%2e%2e/x",
            "//evil",
            "/c1/./x",
            "relative",
            "/a\\b",
        ] {
            assert_eq!(up.path(bad), None, "{bad:?} accepted");
        }
    }

    #[test]
    fn token_stays_out_of_debug() {
        let up = LlmUpstream::new("http://127.0.0.1:8080", TOKEN).unwrap();
        assert!(!format!("{up:?}").contains(TOKEN));
        assert!(up.to_string().ends_with(TOKEN));
    }
}
