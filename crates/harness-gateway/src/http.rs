//! Just enough HTTP/1.1 for a forward proxy: request heads of `CONNECT` and
//! absolute-form requests, the rewritten head sent upstream, and the
//! gateway's own error responses.

use std::io::{self, BufRead, Read, Write};

use crate::pattern::Host;

/// Upper bound for a request head: generous for real clients, small enough
/// that an agent cannot make the gateway buffer much.
pub const MAX_HEAD_BYTES: usize = 32 * 1024;

const MAX_HEADERS: usize = 100;

/// Headers that only concern the client-proxy hop (RFC 9110, 7.6.1).
/// `Transfer-Encoding` stays: the body is passed through unchanged and needs
/// its framing.
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "upgrade",
];

/// A parsed request head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestHead {
    /// Method, e.g. `CONNECT` or `GET`.
    pub method: String,
    /// Where the client wants to go.
    pub target: Target,
    /// Header fields in order, names as sent.
    pub headers: Vec<(String, String)>,
}

/// Destination of a proxy request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// `CONNECT host:port`: a byte tunnel, usually TLS.
    Tunnel {
        /// Target host.
        host: Host,
        /// Target port.
        port: u16,
    },
    /// `GET http://host:port/path`: plain HTTP, forwarded.
    Forward {
        /// Target host.
        host: Host,
        /// Target port.
        port: u16,
        /// Path and query, starting with `/`.
        path: String,
    },
}

impl Target {
    /// Host and port.
    pub fn host_port(&self) -> (&Host, u16) {
        match self {
            Target::Tunnel { host, port } | Target::Forward { host, port, .. } => (host, *port),
        }
    }
}

/// Why a request head could not be read.
#[derive(Debug, PartialEq, Eq)]
pub enum HeadError {
    /// The client closed the connection before sending anything.
    Closed,
    /// Reading failed (timeout, reset).
    Io(io::ErrorKind),
    /// Head larger than [`MAX_HEAD_BYTES`] or too many header fields.
    TooLarge,
    /// Not a proxy request we understand; German text for the error page.
    Bad(String),
}

/// Reads and parses a request head. Bytes after the head stay in `reader`.
pub fn read_head(reader: &mut impl BufRead) -> Result<RequestHead, HeadError> {
    let mut total = 0;
    let mut lines: Vec<String> = Vec::new();
    loop {
        let mut line = Vec::new();
        let limit = (MAX_HEAD_BYTES - total) as u64;
        let read = reader
            .by_ref()
            .take(limit)
            .read_until(b'\n', &mut line)
            .map_err(|e| HeadError::Io(e.kind()))?;
        total += read;
        if read == 0 && total == 0 {
            return Err(HeadError::Closed);
        }
        if !line.ends_with(b"\n") {
            return Err(if total >= MAX_HEAD_BYTES {
                HeadError::TooLarge
            } else {
                HeadError::Bad("Anfrage unvollständig".to_owned())
            });
        }
        line.pop();
        if line.ends_with(b"\r") {
            line.pop();
        }
        if line.is_empty() {
            // Empty lines before the request line are allowed (RFC 9112, 2.2).
            if lines.is_empty() {
                continue;
            }
            break;
        }
        let line = String::from_utf8(line)
            .map_err(|_| HeadError::Bad("Anfrage ist kein gültiges UTF-8".to_owned()))?;
        lines.push(line);
        if lines.len() > MAX_HEADERS + 1 {
            return Err(HeadError::TooLarge);
        }
    }
    parse_head(&lines)
}

fn parse_head(lines: &[String]) -> Result<RequestHead, HeadError> {
    let bad = |message: &str| HeadError::Bad(message.to_owned());
    let request_line = &lines[0];
    if request_line.chars().any(char::is_control) {
        return Err(bad("Steuerzeichen in der Anfragezeile"));
    }
    let parts: Vec<&str> = request_line.split(' ').collect();
    let [method, target, version] = parts[..] else {
        return Err(bad("Anfragezeile hat nicht die Form METHODE ZIEL VERSION"));
    };
    if !matches!(version, "HTTP/1.1" | "HTTP/1.0") {
        return Err(bad("nur HTTP/1.0 und HTTP/1.1"));
    }
    if method.is_empty() || method.len() > 20 || !method.bytes().all(is_token_byte) {
        return Err(bad("ungültige Methode"));
    }

    let mut headers = Vec::with_capacity(lines.len() - 1);
    for line in &lines[1..] {
        if line.starts_with([' ', '\t']) {
            return Err(bad("gefaltete Header-Zeilen werden nicht unterstützt"));
        }
        let (name, value) = line.split_once(':').ok_or_else(|| bad("Header ohne ':'"))?;
        if name.is_empty() || !name.bytes().all(is_token_byte) {
            return Err(bad("ungültiger Header-Name"));
        }
        let value = value.trim_matches([' ', '\t']);
        if value.chars().any(|c| c.is_control() && c != '\t') {
            return Err(bad("Steuerzeichen in einem Header"));
        }
        headers.push((name.to_owned(), value.to_owned()));
    }

    let target = if method == "CONNECT" {
        let (host, port) = parse_authority(target, None)?;
        Target::Tunnel { host, port }
    } else if let Some(rest) = strip_prefix_ignore_case(target, "http://") {
        let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let (host, port) = parse_authority(&rest[..end], Some(80))?;
        let path = rest[end..].split('#').next().unwrap_or_default();
        let path = if path.starts_with('/') {
            path.to_owned()
        } else {
            format!("/{path}")
        };
        Target::Forward { host, port, path }
    } else if strip_prefix_ignore_case(target, "https://").is_some() {
        return Err(bad("https-Ziele bitte per CONNECT anfragen"));
    } else {
        return Err(bad(
            "Das ist das harness-Gateway, ein HTTP-Proxy. Erwartet CONNECT oder eine absolute http-URL",
        ));
    };
    Ok(RequestHead {
        method: method.to_owned(),
        target,
        headers,
    })
}

/// `host:port`, `[v6]:port`; without a port only if `default_port` is set.
fn parse_authority(text: &str, default_port: Option<u16>) -> Result<(Host, u16), HeadError> {
    let bad = |message: &str| HeadError::Bad(message.to_owned());
    if text.contains('@') {
        return Err(bad("Zugangsdaten in der URL werden nicht unterstützt"));
    }
    let (host, port) = if text.starts_with('[') {
        let end = text
            .find(']')
            .ok_or_else(|| bad("ungültige IPv6-Adresse"))?;
        let (host, rest) = text.split_at(end + 1);
        match rest.strip_prefix(':') {
            Some(port) => (host, Some(port)),
            None if rest.is_empty() => (host, None),
            None => return Err(bad("ungültiges Ziel")),
        }
    } else {
        match text.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (text, None),
        }
    };
    let port = match port {
        // Digits only: `parse` would also take a leading `+`.
        Some(text) => match text.parse::<u16>() {
            Ok(port) if port > 0 && text.bytes().all(|b| b.is_ascii_digit()) => port,
            _ => return Err(bad("ungültiger Port")),
        },
        None => default_port.ok_or_else(|| bad("Port fehlt"))?,
    };
    let host = Host::parse(host).map_err(HeadError::Bad)?;
    Ok((host, port))
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

fn is_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

/// The head sent to the origin server for a plain HTTP request:
/// origin-form target, `Host` from the URL (RFC 9112, 3.2.2), hop-by-hop
/// headers removed and `Connection: close`, so every client connection
/// carries exactly one request to the host that was checked.
pub fn forward_head(head: &RequestHead) -> Option<String> {
    let Target::Forward { host, port, path } = &head.target else {
        return None;
    };
    let mut listed: Vec<String> = Vec::new();
    for (name, value) in &head.headers {
        if name.eq_ignore_ascii_case("connection") {
            listed.extend(value.split(',').map(|t| t.trim().to_ascii_lowercase()));
        }
    }
    let mut out = format!("{} {path} HTTP/1.1\r\n", head.method);
    if *port == 80 {
        out.push_str(&format!("Host: {host}\r\n"));
    } else {
        out.push_str(&format!("Host: {host}:{port}\r\n"));
    }
    for (name, value) in &head.headers {
        let lower = name.to_ascii_lowercase();
        if lower == "host" || HOP_BY_HOP.contains(&lower.as_str()) || listed.contains(&lower) {
            continue;
        }
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str("Connection: close\r\n\r\n");
    Some(out)
}

/// Writes a complete response with a plain-text body and closes nothing;
/// the caller drops the connection afterwards.
pub fn write_response(
    out: &mut impl Write,
    status: u16,
    extra_headers: &[(&str, &str)],
    body: &str,
) -> io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n",
        status_text(status),
        body.len()
    );
    for (name, value) in extra_headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    out.write_all(head.as_bytes())?;
    out.write_all(body.as_bytes())?;
    out.flush()
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        431 => "Request Header Fields Too Large",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<RequestHead, HeadError> {
        read_head(&mut text.as_bytes())
    }

    fn host(text: &str) -> Host {
        Host::parse(text).unwrap()
    }

    #[test]
    fn connect_requests() {
        let head =
            parse("CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n").unwrap();
        assert_eq!(
            head.target,
            Target::Tunnel {
                host: host("example.com"),
                port: 443
            }
        );
        let head = parse("\r\nCONNECT [2001:db8::1]:8443 HTTP/1.1\n\n").unwrap();
        assert_eq!(head.target.host_port(), (&host("[2001:db8::1]"), 8443));
        // Bytes after the head stay in the reader (early TLS data).
        let mut input: &[u8] = b"CONNECT a.example:443 HTTP/1.1\r\n\r\n\x16\x03\x01";
        read_head(&mut input).unwrap();
        assert_eq!(input, b"\x16\x03\x01");
    }

    #[test]
    fn forward_requests() {
        let head = parse(
            "GET http://Example.com/a/b?c=d#frag HTTP/1.1\r\nHost: evil.example\r\nAccept: */*\r\n\
             Proxy-Connection: keep-alive\r\nConnection: keep-alive, X-Secret\r\nX-Secret: 1\r\n\
             Transfer-Encoding: chunked\r\n\r\n",
        )
        .unwrap();
        assert_eq!(
            head.target,
            Target::Forward {
                host: host("example.com"),
                port: 80,
                path: "/a/b?c=d".into()
            }
        );
        assert_eq!(
            forward_head(&head).unwrap(),
            "GET /a/b?c=d HTTP/1.1\r\nHost: example.com\r\nAccept: */*\r\n\
             Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
        );

        let head = parse("POST HTTP://example.com:8080?x HTTP/1.0\r\n\r\n").unwrap();
        assert_eq!(
            head.target,
            Target::Forward {
                host: host("example.com"),
                port: 8080,
                path: "/?x".into()
            }
        );
        assert!(
            forward_head(&head)
                .unwrap()
                .contains("Host: example.com:8080\r\n")
        );
    }

    #[test]
    fn rejects_bad_requests() {
        assert_eq!(parse(""), Err(HeadError::Closed));
        for bad in [
            "GET / HTTP/1.1\r\n\r\n",
            "GET https://example.com/ HTTP/1.1\r\n\r\n",
            "CONNECT example.com HTTP/1.1\r\n\r\n",
            "CONNECT example.com:0 HTTP/1.1\r\n\r\n",
            "CONNECT example.com:+443 HTTP/1.1\r\n\r\n",
            "CONNECT example.com:99999 HTTP/1.1\r\n\r\n",
            "CONNECT user@example.com:443 HTTP/1.1\r\n\r\n",
            "CONNECT 127.1:443 HTTP/1.1\r\n\r\n",
            "CONNECT example.com:443 HTTP/2\r\n\r\n",
            "CONNECT  example.com:443 HTTP/1.1\r\n\r\n",
            "GET http://example.com/ HTTP/1.1\r\n folded\r\n\r\n",
            "GET http://example.com/ HTTP/1.1\r\nBad Name: x\r\n\r\n",
            "GET http://example.com/ HTTP/1.1\r\n",
            "G\u{7f}T http://example.com/ HTTP/1.1\r\n\r\n",
        ] {
            assert!(
                matches!(parse(bad), Err(HeadError::Bad(_))),
                "{bad:?} accepted"
            );
        }
        let huge = format!(
            "GET http://example.com/ HTTP/1.1\r\nX: {}\r\n\r\n",
            "a".repeat(MAX_HEAD_BYTES)
        );
        assert_eq!(parse(&huge), Err(HeadError::TooLarge));
        let many = format!(
            "GET http://example.com/ HTTP/1.1\r\n{}\r\n",
            "X: y\r\n".repeat(MAX_HEADERS + 1)
        );
        assert_eq!(parse(&many), Err(HeadError::TooLarge));
    }

    #[test]
    fn responses() {
        let mut out = Vec::new();
        write_response(
            &mut out,
            403,
            &[("X-Harness-Gateway", "not-allowed")],
            "nein",
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("HTTP/1.1 403 Forbidden\r\n"));
        assert!(text.contains("Content-Length: 4\r\n"));
        assert!(text.contains("X-Harness-Gateway: not-allowed\r\n"));
        assert!(text.ends_with("\r\n\r\nnein"));
    }
}
