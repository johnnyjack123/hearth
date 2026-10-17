//! The virtual host `llm.harness`: how harnesses in instances reach their
//! model (ARCHITECTURE.md §6.4).
//!
//! Harnesses are configured with `http://llm.harness/…` and a dummy key. The
//! gateway forwards these requests to the LLM endpoint of the harness server
//! and adds the device token itself. No credential ever enters an instance,
//! so even in mode open a hijacked agent has no key to leak.

use std::fmt;

use crate::pattern::Host;
use crate::pem;

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

/// How the gateway talks to the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// Plain HTTP, e.g. to a server on the user's machine.
    Http,
    /// HTTP over TLS, e.g. to a server behind a reverse proxy. Needs a
    /// gateway built with the `tls` feature (the image always is).
    Https,
}

impl Scheme {
    /// `http` or `https`.
    pub fn as_str(self) -> &'static str {
        match self {
            Scheme::Http => "http",
            Scheme::Https => "https",
        }
    }

    /// Port used when the URL names none.
    pub fn default_port(self) -> u16 {
        match self {
            Scheme::Http => 80,
            Scheme::Https => 443,
        }
    }
}

/// Where `llm.harness` forwards to, and with which token.
#[derive(Clone, PartialEq, Eq)]
pub struct LlmUpstream {
    scheme: Scheme,
    host: Host,
    port: u16,
    prefix: String,
    token: String,
    ca: Vec<Vec<u8>>,
}

impl LlmUpstream {
    /// `url` is `http(s)://host[:port][/prefix]` as reachable from the
    /// gateway; `token` the device token.
    ///
    /// `https://` is accepted even without the `tls` feature, because the
    /// host builds upstreams with this type too. A gateway without TLS
    /// refuses them when they arrive.
    pub fn new(url: &str, token: &str) -> Result<LlmUpstream, String> {
        let invalid = |reason: &str| format!("LLM-Adresse {url:?}: {reason}");
        let (scheme, rest) = if let Some(rest) = url.strip_prefix("http://") {
            (Scheme::Http, rest)
        } else if let Some(rest) = url.strip_prefix("https://") {
            (Scheme::Https, rest)
        } else {
            return Err(invalid("erwartet http:// oder https://"));
        };
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
            None => scheme.default_port(),
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
            scheme,
            host,
            port,
            prefix: prefix.to_owned(),
            token: token.to_owned(),
            ca: Vec::new(),
        })
    }

    /// Trusts only these root certificates (DER) for the server instead of
    /// the public CAs, e.g. the root of Caddy's `tls internal`. Only for
    /// `https://`.
    pub fn with_ca(mut self, certificates: Vec<Vec<u8>>) -> Result<LlmUpstream, String> {
        if self.scheme != Scheme::Https {
            return Err("Eine eigene CA gibt es nur für https://-Server".to_owned());
        }
        pem::check_certificates(&certificates).map_err(|e| format!("CA: {e}"))?;
        self.ca = certificates;
        Ok(self)
    }

    /// Parses the wire form written by [`fmt::Display`]: the URL, the token
    /// and optionally the root certificates as comma-separated base64 DER.
    pub fn from_wire(url: &str, token: &str, ca: Option<&str>) -> Result<LlmUpstream, String> {
        let upstream = LlmUpstream::new(url, token)?;
        let Some(ca) = ca else {
            return Ok(upstream);
        };
        let certificates = ca
            .split(',')
            .map(|field| pem::base64_decode(field).ok_or("CA: ungültiges Base64"))
            .collect::<Result<Vec<_>, _>>()?;
        upstream.with_ca(certificates)
    }

    /// The upstream as URL, without the token.
    pub fn url(&self) -> String {
        format!(
            "{}://{}{}",
            self.scheme.as_str(),
            self.host_header(),
            self.prefix
        )
    }

    /// `Host` header of upstream requests: with the port only if it is not
    /// the scheme's default, as a reverse proxy matching on names expects.
    pub fn host_header(&self) -> String {
        if self.port == self.scheme.default_port() {
            self.host.to_string()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// HTTP or HTTPS.
    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    /// Upstream host. For `https://` also the name the certificate must be
    /// valid for; an IP literal must be among its IP addresses.
    pub fn host(&self) -> &Host {
        &self.host
    }

    /// Upstream port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Root certificates (DER) trusted instead of the public CAs; empty
    /// means the public CAs.
    pub fn ca(&self) -> &[Vec<u8>] {
        &self.ca
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
            .field("ca", &self.ca.len())
            .field("token", &"…")
            .finish()
    }
}

/// Wire form `<url> <token> [<ca>]`, `<ca>` being the root certificates as
/// comma-separated base64 DER; none of the fields contains a space or a
/// comma. Carries the secret, so it is only ever sent through stdin of
/// `ctl`, never as an argument.
impl fmt::Display for LlmUpstream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.url(), self.token)?;
        if !self.ca.is_empty() {
            let encoded: Vec<String> = self.ca.iter().map(|der| pem::base64_encode(der)).collect();
            write!(f, " {}", encoded.join(","))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "abcdefghijklmnop_0123456789";

    /// Passes the rough DER check; real certificates only matter to TLS.
    fn fake_der(fill: u8) -> Vec<u8> {
        let mut der = vec![0x30, 0x20];
        der.extend([fill; 0x20]);
        der
    }

    #[test]
    fn upstreams() {
        let up = LlmUpstream::new("http://192.168.127.254:8080/api/llm/", TOKEN).unwrap();
        assert_eq!(up.url(), "http://192.168.127.254:8080/api/llm");
        assert_eq!(up.port(), 8080);
        assert_eq!(up.scheme(), Scheme::Http);
        let default_port = LlmUpstream::new("http://harness.example", TOKEN).unwrap();
        assert_eq!(default_port.port(), 80);
        assert_eq!(default_port.url(), "http://harness.example");
        let v6 = LlmUpstream::new("http://[fd00::1]:8080/x", TOKEN).unwrap();
        assert_eq!(v6.url(), "http://[fd00::1]:8080/x");
        assert_eq!(
            LlmUpstream::new("http://[fd00::1]", TOKEN).unwrap().port(),
            80
        );

        let tls = LlmUpstream::new("https://harness.example/api/llm/proxy", TOKEN).unwrap();
        assert_eq!(tls.scheme(), Scheme::Https);
        assert_eq!(tls.port(), 443);
        assert_eq!(tls.host_header(), "harness.example");
        assert_eq!(tls.url(), "https://harness.example/api/llm/proxy");
        let tls_port = LlmUpstream::new("https://harness.example:8443", TOKEN).unwrap();
        assert_eq!(tls_port.host_header(), "harness.example:8443");
        // Port 80 is no default for https.
        let odd = LlmUpstream::new("https://harness.example:80", TOKEN).unwrap();
        assert_eq!(odd.url(), "https://harness.example:80");

        for bad in [
            "ftp://example.com",
            "HTTPS://example.com",
            "http://",
            "https://",
            "http://a b",
            "http://example.com:0",
            "http://example.com:x/",
            "http://[fd00::1]x",
            "http://example.com/../x",
            "http://example.com/a%2e",
            "https://user@example.com",
        ] {
            assert!(LlmUpstream::new(bad, TOKEN).is_err(), "{bad:?} accepted");
        }
        assert!(LlmUpstream::new("http://example.com", "short").is_err());
        assert!(LlmUpstream::new("http://example.com", &format!("{TOKEN} x")).is_err());
    }

    #[test]
    fn custom_ca() {
        let tls = LlmUpstream::new("https://harness.example", TOKEN).unwrap();
        let with_ca = tls.clone().with_ca(vec![fake_der(1), fake_der(2)]).unwrap();
        assert_eq!(with_ca.ca().len(), 2);
        assert_ne!(with_ca, tls);

        // Wire form round trip, with and without CA.
        for upstream in [&tls, &with_ca] {
            let wire = upstream.to_string();
            let words: Vec<&str> = wire.split(' ').collect();
            let parsed = LlmUpstream::from_wire(words[0], words[1], words.get(2).copied()).unwrap();
            assert_eq!(&parsed, upstream, "{wire}");
        }
        assert_eq!(tls.to_string().split(' ').count(), 2);

        let plain = LlmUpstream::new("http://harness.example", TOKEN).unwrap();
        assert!(plain.with_ca(vec![fake_der(1)]).is_err(), "CA for http");
        assert!(tls.clone().with_ca(vec![]).is_err());
        assert!(tls.clone().with_ca(vec![b"garbage".to_vec()]).is_err());
        for bad in ["", "!!!!", "MCA=,", "AAAA"] {
            assert!(
                LlmUpstream::from_wire("https://harness.example", TOKEN, Some(bad)).is_err(),
                "{bad:?} accepted"
            );
        }
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
        let up = LlmUpstream::new("https://127.0.0.1:8080", TOKEN)
            .unwrap()
            .with_ca(vec![fake_der(3)])
            .unwrap();
        let debug = format!("{up:?}");
        assert!(!debug.contains(TOKEN), "{debug}");
        assert!(debug.contains("ca: 1"), "{debug}");
        assert!(up.to_string().contains(TOKEN));
    }
}
