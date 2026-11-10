//! The virtual host `llm.hearth`: how harnesses in instances reach their
//! model (ARCHITECTURE.md §6.4).
//!
//! Harnesses are configured with `http://llm.hearth/…` and a dummy key. The
//! gateway forwards these requests to the LLM endpoint of the hearth server
//! and adds the device token itself. No credential ever enters an instance,
//! so even in mode open a hijacked agent has no key to leak.

use std::fmt;

use crate::pattern::Host;
use crate::pem;

/// Name harnesses use for the LLM; only the gateway knows it.
pub const LLM_HOST: &str = "llm.hearth";

/// Headers a client might use to send its own credentials; they are dropped
/// so only the device token reaches the server. The approved host of
/// `web_fetch` counts as one: only the gateway may set it.
pub const CREDENTIAL_HEADERS: &[&str] = &[
    "authorization",
    "x-api-key",
    "api-key",
    "x-goog-api-key",
    "cookie",
    APPROVED_HOST_HEADER,
    PROJECT_HEADER,
    // Their names before 0.2.0 (llm-harness): an older server still reads
    // them, so an agent must not send them either.
    "x-harness-approved-host",
    "x-harness-project",
];

/// Path of `web_fetch` below `llm.hearth`: the server loads the page named
/// in [`URL_HEADER`] once its host passed the project's rules here. Paths
/// starting with `/_` belong to hearth; credential ids never do.
pub const WEB_FETCH_PATH: &str = "/_web/fetch";

/// Path of `web_search` below `llm.hearth`. It names no target: the server
/// asks the search provider the admin set up, and only while a reply of
/// this computer runs in research mode, so the gateway passes it on.
pub const WEB_SEARCH_PATH: &str = "/_web/search";

/// Path below the server's prefix where the gateway itself asks whether a
/// page is a hit of a research reply running on this computer. Agents
/// cannot use it: only [`WEB_FETCH_PATH`], [`WEB_SEARCH_PATH`],
/// [`MEMORY_PATH`] and [`DOCUMENTS_PATH`] pass from them.
pub const KNOWN_PATH: &str = "/_web/known";

/// Path of the memory tools below `llm.hearth`. Like a search it names no
/// target: the server keeps the memories of the computer's owner.
pub const MEMORY_PATH: &str = "/_memory";

/// Path of `read_document` below `llm.hearth`. It names no target either:
/// the server reads the documents of the chats running on this computer.
pub const DOCUMENTS_PATH: &str = "/_documents";

/// Header with the URL an agent wants `web_fetch` to load.
pub const URL_HEADER: &str = "x-hearth-url";

/// Header with the host the gateway allowed for `web_fetch`; the server
/// loads nothing else.
pub const APPROVED_HOST_HEADER: &str = "x-hearth-approved-host";

/// Header with the gateway's project on every request to the server: an
/// agent's documents, searches and research hits belong to its project,
/// not to every project on the computer. Only the gateway sets it.
pub const PROJECT_HEADER: &str = "x-hearth-project";

/// Whether `name` can be a project name on the wire: what hearth allows
/// for projects (lower-case letters, digits, `-`, `_`, `.`), at most 64.
pub fn valid_project(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_.".contains(&b))
}

/// Longest URL `web_fetch` takes, as on the server.
const MAX_URL_LENGTH: usize = 2048;

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

/// Where `llm.hearth` forwards to, and with which token.
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
        let (host, port) =
            parse_authority(authority, scheme.default_port()).map_err(|e| invalid(&e))?;
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

    /// The upstream path for a request to `llm.hearth`, or `None` if the
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

/// Host and port of `host`, `host:port`, `[v6]` or `[v6]:port`.
fn parse_authority(authority: &str, default_port: u16) -> Result<(Host, u16), String> {
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
    let (host, port) = split.ok_or("ungültiger Host")?;
    let port = match port {
        None => default_port,
        Some(port) => port
            .parse::<u16>()
            .ok()
            .filter(|p| *p > 0 && port.bytes().all(|b| b.is_ascii_digit()))
            .ok_or("ungültiger Port")?,
    };
    Ok((Host::parse(host)?, port))
}

/// Host and port of a URL an agent wants `web_fetch` to load. The server
/// parses the URL again (WHATWG) and loads it only if it finds the same
/// host, so this parser is strict instead of clever: lowercase `http://` or
/// `https://`, printable ASCII only (a browser-style parser drops tabs and
/// reads `\` as `/`), no user info. Agents send `new URL(…).href`, which
/// always passes.
pub fn web_fetch_target(url: &str) -> Result<(Host, u16), String> {
    let invalid = |reason: &str| format!("web_fetch-Adresse: {reason}");
    if url.len() > MAX_URL_LENGTH {
        return Err(invalid("zu lang"));
    }
    if !url.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(invalid("Leer-, Steuer- oder Nicht-ASCII-Zeichen"));
    }
    let (default_port, rest) = if let Some(rest) = url.strip_prefix("http://") {
        (80, rest)
    } else if let Some(rest) = url.strip_prefix("https://") {
        (443, rest)
    } else {
        return Err(invalid("erwartet http:// oder https://"));
    };
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    if authority.contains(['@', '\\']) {
        return Err(invalid("Zugangsdaten oder \\ im Host"));
    }
    parse_authority(authority, default_port).map_err(|e| invalid(&e))
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
        let default_port = LlmUpstream::new("http://hearth.example", TOKEN).unwrap();
        assert_eq!(default_port.port(), 80);
        assert_eq!(default_port.url(), "http://hearth.example");
        let v6 = LlmUpstream::new("http://[fd00::1]:8080/x", TOKEN).unwrap();
        assert_eq!(v6.url(), "http://[fd00::1]:8080/x");
        assert_eq!(
            LlmUpstream::new("http://[fd00::1]", TOKEN).unwrap().port(),
            80
        );

        let tls = LlmUpstream::new("https://hearth.example/api/llm/proxy", TOKEN).unwrap();
        assert_eq!(tls.scheme(), Scheme::Https);
        assert_eq!(tls.port(), 443);
        assert_eq!(tls.host_header(), "hearth.example");
        assert_eq!(tls.url(), "https://hearth.example/api/llm/proxy");
        let tls_port = LlmUpstream::new("https://hearth.example:8443", TOKEN).unwrap();
        assert_eq!(tls_port.host_header(), "hearth.example:8443");
        // Port 80 is no default for https.
        let odd = LlmUpstream::new("https://hearth.example:80", TOKEN).unwrap();
        assert_eq!(odd.url(), "https://hearth.example:80");

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
        let tls = LlmUpstream::new("https://hearth.example", TOKEN).unwrap();
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

        let plain = LlmUpstream::new("http://hearth.example", TOKEN).unwrap();
        assert!(plain.with_ca(vec![fake_der(1)]).is_err(), "CA for http");
        assert!(tls.clone().with_ca(vec![]).is_err());
        assert!(tls.clone().with_ca(vec![b"garbage".to_vec()]).is_err());
        for bad in ["", "!!!!", "MCA=,", "AAAA"] {
            assert!(
                LlmUpstream::from_wire("https://hearth.example", TOKEN, Some(bad)).is_err(),
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
    fn web_fetch_targets() {
        let target = |url: &str| web_fetch_target(url).map(|(host, port)| format!("{host} {port}"));
        assert_eq!(
            target("https://example.com/a?b#c").unwrap(),
            "example.com 443"
        );
        assert_eq!(target("http://Example.COM.").unwrap(), "example.com 80");
        assert_eq!(
            target("http://example.com:8080?x").unwrap(),
            "example.com 8080"
        );
        assert_eq!(
            target("https://xn--bcher-kva.de/").unwrap(),
            "xn--bcher-kva.de 443"
        );
        assert_eq!(
            target("https://93.184.215.14/").unwrap(),
            "93.184.215.14 443"
        );
        assert_eq!(
            target("https://[2606:4700::1]:443/").unwrap(),
            "[2606:4700::1] 443"
        );
        // Backslashes outside the host do not change it.
        assert_eq!(
            target("https://example.com/a\\b").unwrap(),
            "example.com 443"
        );
        for bad in [
            "HTTPS://example.com/",
            "ftp://example.com/",
            "example.com",
            "https://user@example.com/",
            "https://evil.test\\@example.com/",
            "https://evil.test\\.example.com/",
            "https://exa mple.com/",
            "https://exa	mple.com/",
            "https://bücher.de/",
            "https://127.1/",
            "https://0x7f000001/",
            "https://[::1/",
            "https://example.com:0/",
            "https://example.com:99999/",
            "https://example.com:+80/",
            "https:///path",
        ] {
            assert!(web_fetch_target(bad).is_err(), "{bad:?} accepted");
        }
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL_LENGTH));
        assert!(web_fetch_target(&long).is_err());
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
