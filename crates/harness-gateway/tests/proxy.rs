//! The proxy over real TCP connections on loopback. Names ending in `.test`
//! resolve to 127.0.0.1, where the test servers listen; most tests allow
//! loopback, `private_addresses_stay_blocked` uses the real address filter.

use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use harness_gateway::{Gateway, Options, address, proxy};

fn resolve_test(name: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    if name.ends_with(".test") {
        Ok(vec![SocketAddr::from((Ipv4Addr::LOCALHOST, port))])
    } else {
        Err(io::Error::new(io::ErrorKind::NotFound, "unknown test name"))
    }
}

fn start(address_filter: fn(IpAddr) -> bool) -> (Gateway, SocketAddr) {
    let gateway = Gateway::new(Options {
        address_filter,
        resolve: resolve_test,
        ..Options::default()
    });
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let server = gateway.clone();
    thread::spawn(move || proxy::serve(&server, listener));
    (gateway, address)
}

fn allow_loopback(_: IpAddr) -> bool {
    true
}

/// Echoes every connection back to itself.
fn echo_server() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            thread::spawn(move || {
                let _ = io::copy(&mut &stream, &mut &stream);
            });
        }
    });
    port
}

/// Reads the request body announced in `head`. A server that closes with
/// unread data makes Linux send a reset, which can destroy the response the
/// client has not read yet.
fn drain_body(reader: &mut impl Read, head: &[String]) {
    let length = head
        .iter()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<u64>().ok())
                .flatten()
        })
        .unwrap_or(0);
    let _ = io::copy(&mut reader.take(length), &mut io::sink());
}

/// Answers every request with `hello` and reports the request head it saw.
fn http_server() -> (u16, Receiver<Vec<String>>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let head: Vec<String> = reader
                .by_ref()
                .lines()
                .map_while(Result::ok)
                .take_while(|line| !line.is_empty())
                .collect();
            drain_body(&mut reader, &head);
            let _ = tx.send(head);
            let _ = (&stream).write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
            );
        }
    });
    (port, rx)
}

struct Response {
    status: u16,
    headers: HashMap<String, String>,
    reader: BufReader<TcpStream>,
}

impl Response {
    fn body(mut self) -> String {
        let mut body = String::new();
        self.reader.read_to_string(&mut body).unwrap();
        body
    }
}

fn request(proxy: SocketAddr, head: &str) -> Response {
    let stream = TcpStream::connect(proxy).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    (&stream).write_all(head.as_bytes()).unwrap();
    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line).unwrap();
    let status = status_line
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("bad status line {status_line:?}"));
    let mut headers = HashMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(": ").unwrap();
        headers.insert(name.to_ascii_lowercase(), value.to_owned());
    }
    Response {
        status,
        headers,
        reader,
    }
}

fn connect(proxy: SocketAddr, target: &str) -> Response {
    request(
        proxy,
        &format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n"),
    )
}

fn assert_echo(mut response: Response) {
    assert_eq!(response.status, 200);
    response.reader.get_mut().write_all(b"ping\n").unwrap();
    let mut line = String::new();
    response.reader.read_line(&mut line).unwrap();
    assert_eq!(line, "ping\n");
}

/// A control connection, as `harness-gateway ctl` would relay it. Events
/// may arrive before the `ok` of a request; they are queued for `event`.
struct Control {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    events: VecDeque<String>,
}

impl Control {
    fn open(gateway: &Gateway) -> Control {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let writer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        let gateway = gateway.clone();
        thread::spawn(move || {
            let reader = BufReader::new(server.try_clone().unwrap());
            gateway.serve_control(reader, server);
        });
        writer
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        Control {
            reader: BufReader::new(writer.try_clone().unwrap()),
            writer,
            events: VecDeque::new(),
        }
    }

    fn line(&mut self) -> String {
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        line.trim_end().to_owned()
    }

    fn send(&mut self, request: &str) {
        self.writer
            .write_all(format!("{request}\n").as_bytes())
            .unwrap();
        loop {
            let line = self.line();
            if line.starts_with("event ") {
                self.events.push_back(line);
            } else {
                assert_eq!(line, "ok", "answer to {request}");
                return;
            }
        }
    }

    fn event(&mut self) -> String {
        match self.events.pop_front() {
            Some(event) => event,
            None => self.line(),
        }
    }
}

#[test]
fn allowed_hosts_are_tunneled_others_refused() {
    let (gateway, proxy) = start(allow_loopback);
    let echo = echo_server();
    Control::open(&gateway).send("policy allowlist noask allowed.test -");

    assert_echo(connect(proxy, &format!("allowed.test:{echo}")));

    let refused = connect(proxy, &format!("other.test:{echo}"));
    assert_eq!(refused.status, 403);
    assert_eq!(refused.headers["x-harness-gateway"], "not-allowed");
    let body = refused.body();
    assert!(body.contains("harness net allow other.test"), "{body}");
}

#[test]
fn private_addresses_stay_blocked() {
    let (gateway, proxy) = start(address::is_public);
    let echo = echo_server();
    // Even with everything open, loopback stays out of reach: by name, by
    // IPv4 and IPv6 literal.
    Control::open(&gateway).send("policy open noask - -");
    for target in [
        format!("localhost.test:{echo}"),
        format!("127.0.0.1:{echo}"),
        format!("[::1]:{echo}"),
        format!("[::ffff:127.0.0.1]:{echo}"),
    ] {
        let response = connect(proxy, &target);
        assert_eq!(response.status, 403, "{target}");
        assert_eq!(response.headers["x-harness-gateway"], "private-address");
    }
    let response = request(
        proxy,
        &format!("GET http://127.0.0.1:{echo}/ HTTP/1.1\r\n\r\n"),
    );
    assert_eq!(response.status, 403);
}

#[test]
fn plain_http_is_forwarded_with_a_clean_head() {
    let (gateway, proxy) = start(allow_loopback);
    let (port, heads) = http_server();
    Control::open(&gateway).send("policy allowlist noask web.test -");

    let response = request(
        proxy,
        &format!(
            "GET http://web.test:{port}/path?q=1 HTTP/1.1\r\nHost: elsewhere.test\r\n\
             Proxy-Connection: keep-alive\r\nProxy-Authorization: secret\r\nAccept: */*\r\n\r\n"
        ),
    );
    assert_eq!(response.status, 200);
    assert_eq!(response.body(), "hello");
    let head = heads.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(head[0], "GET /path?q=1 HTTP/1.1");
    assert!(head.contains(&format!("Host: web.test:{port}")), "{head:?}");
    assert!(head.contains(&"Accept: */*".to_owned()));
    assert!(head.contains(&"Connection: close".to_owned()));
    assert!(!head.iter().any(|h| h.starts_with("Proxy-")), "{head:?}");
}

#[test]
fn unknown_hosts_wait_for_the_user() {
    let (gateway, proxy) = start(allow_loopback);
    let echo = echo_server();
    let mut control = Control::open(&gateway);
    control.send("policy allowlist ask - -");
    control.send("watch");

    // Allowed once: this request goes through, the next one asks again.
    let target = format!("ask.test:{echo}");
    let waiting = {
        let target = target.clone();
        thread::spawn(move || connect(proxy, &target))
    };
    assert_eq!(control.event(), format!("event pending 1 ask.test {echo}"));
    control.send("decide 1 allow once");
    assert_echo(waiting.join().unwrap());
    let events = [control.event(), control.event()];
    assert!(
        events.contains(&"event resolved 1 allowed approved".to_owned()),
        "{events:?}"
    );
    let logged = format!(" allowed ask.test {echo} approved");
    assert!(
        events
            .iter()
            .any(|e| e.starts_with("event log ") && e.ends_with(&logged)),
        "{events:?}"
    );

    // Refused: blocked now and for the rest of the session, without asking.
    let waiting = {
        let target = target.clone();
        thread::spawn(move || connect(proxy, &target))
    };
    assert_eq!(control.event(), format!("event pending 2 ask.test {echo}"));
    control.send("decide 2 deny");
    let refused = waiting.join().unwrap();
    assert_eq!(refused.status, 403);
    assert_eq!(refused.headers["x-harness-gateway"], "denied-by-user");
    assert_eq!(connect(proxy, &target).status, 403);
}

#[test]
fn leaving_watcher_answers_waiting_requests() {
    let (gateway, proxy) = start(allow_loopback);
    let echo = echo_server();
    let mut control = Control::open(&gateway);
    control.send("policy allowlist ask - -");
    control.send("watch");
    let waiting = thread::spawn(move || connect(proxy, &format!("wait.test:{echo}")));
    assert!(control.event().starts_with("event pending 1 wait.test"));

    let started = Instant::now();
    drop(control);
    let response = waiting.join().unwrap();
    assert_eq!(response.status, 504);
    assert_eq!(response.headers["x-harness-gateway"], "unanswered");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn llm_requests_reach_the_server_with_the_device_token_only() {
    // The real address filter: the configured server on loopback is the one
    // exception, everything else on loopback stays blocked.
    let (gateway, proxy) = start(address::is_public);
    let (port, heads) = http_server();
    let chat = "POST http://llm.harness/c1/v1/chat/completions?x=1 HTTP/1.1\r\n\
                Authorization: Bearer agent-made-this-up\r\nX-Api-Key: also-fake\r\n\
                Content-Type: application/json\r\nContent-Length: 2\r\n\r\n{}";

    let unset = request(proxy, chat);
    assert_eq!(unset.status, 503);
    assert_eq!(unset.headers["x-harness-gateway"], "no-llm");

    let token = "device-token-0123456789abcdef";
    let mut control = Control::open(&gateway);
    control.send(&format!("llm http://127.0.0.1:{port}/api/llm {token}"));
    let response = request(proxy, chat);
    assert_eq!(response.status, 200);
    assert_eq!(response.body(), "hello");
    let head = heads.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(head[0], "POST /api/llm/c1/v1/chat/completions?x=1 HTTP/1.1");
    assert!(
        head.contains(&format!("Host: 127.0.0.1:{port}")),
        "{head:?}"
    );
    assert!(
        head.contains(&format!("Authorization: Bearer {token}")),
        "{head:?}"
    );
    assert!(head.contains(&"Content-Type: application/json".to_owned()));
    assert!(
        !head
            .iter()
            .any(|h| h.contains("fake") || h.contains("made-this-up")),
        "client credentials leaked: {head:?}"
    );

    // The same through a CONNECT tunnel, as undici's proxy agent does it.
    let mut tunnel = connect(proxy, "llm.harness:80");
    assert_eq!(tunnel.status, 200);
    tunnel
        .reader
        .get_mut()
        .write_all(
            b"POST /c2/chat/completions HTTP/1.1\r\nHost: llm.harness\r\nAuthorization: Bearer fake\r\n\
              Content-Length: 2\r\n\r\n{}",
        )
        .unwrap();
    let mut answer = String::new();
    tunnel.reader.read_to_string(&mut answer).unwrap();
    assert!(
        answer.starts_with("HTTP/1.1 200 OK") && answer.ends_with("hello"),
        "{answer}"
    );
    let head = heads.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(head[0], "POST /api/llm/c2/chat/completions HTTP/1.1");
    assert!(
        head.contains(&format!("Authorization: Bearer {token}")),
        "{head:?}"
    );
    assert!(!head.iter().any(|h| h.contains("fake")), "{head:?}");

    // Nothing but paths below the prefix, and no TLS to the virtual host.
    let escape = request(
        proxy,
        "GET http://llm.harness/../api/conversations HTTP/1.1\r\n\r\n",
    );
    assert_eq!(escape.status, 400);
    assert_eq!(connect(proxy, "llm.harness:443").status, 400);
    // The server's address is no general exception.
    let direct = request(
        proxy,
        &format!("GET http://127.0.0.1:{port}/api/llm/x HTTP/1.1\r\n\r\n"),
    );
    assert_eq!(direct.status, 403);

    // The token never shows up in the status.
    control.writer.write_all(b"status\n").unwrap();
    let mut lines = Vec::new();
    loop {
        let line = control.line();
        if line == "ok" {
            break;
        }
        lines.push(line);
    }
    assert!(
        lines.contains(&format!("llm http://127.0.0.1:{port}/api/llm")),
        "{lines:?}"
    );
    assert!(!lines.iter().any(|l| l.contains(token)));
}

#[test]
fn web_fetch_hosts_pass_the_rules_first() {
    let (gateway, proxy) = start(address::is_public);
    let (port, heads) = http_server();
    let token = "device-token-0123456789abcdef";
    let mut control = Control::open(&gateway);
    control.send(&format!("llm http://127.0.0.1:{port}/api/llm {token}"));
    control.send("policy allowlist ask allowed.test -");
    control.send("watch");
    let fetch = |headers: &str| {
        format!(
            "POST http://llm.harness/_web/fetch HTTP/1.1\r\n{headers}\
             Content-Type: application/json\r\nContent-Length: 2\r\n\r\n{{}}"
        )
    };

    // An allowed host: forwarded with the host the gateway checked, never
    // the one the agent claims.
    let response = request(
        proxy,
        &fetch(
            "X-Harness-Url: https://allowed.test/page?q=1\r\n\
             X-Harness-Approved-Host: forged.test\r\n",
        ),
    );
    assert_eq!(response.status, 200);
    let head = heads.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(head[0], "POST /api/llm/_web/fetch HTTP/1.1");
    assert!(
        head.contains(&"X-Harness-Url: https://allowed.test/page?q=1".to_owned()),
        "{head:?}"
    );
    assert!(
        head.contains(&"X-Harness-Approved-Host: allowed.test".to_owned()),
        "{head:?}"
    );
    assert!(!head.iter().any(|h| h.contains("forged")), "{head:?}");
    assert!(
        head.contains(&format!("Authorization: Bearer {token}")),
        "{head:?}"
    );
    let logged = control.event();
    assert!(
        logged.starts_with("event log ") && logged.contains(" allowed allowed.test 443 allowlist"),
        "{logged}"
    );
    assert!(control.event().ends_with(" allowed llm.harness 80 llm"));

    // Any other host asks the user like a direct connection, once the server
    // said the page is no research hit; it hears nothing more of a refused one.
    let waiting =
        thread::spawn(move || request(proxy, &fetch("X-Harness-Url: http://ask.test/secret\r\n")));
    assert_eq!(control.event(), "event pending 1 ask.test 80");
    let known = heads.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(known[0], "POST /api/llm/_web/known HTTP/1.1");
    control.send("decide 1 deny");
    let refused = waiting.join().unwrap();
    assert_eq!(refused.status, 403);
    assert_eq!(refused.headers["x-harness-gateway"], "denied-by-user");
    assert!(heads.recv_timeout(Duration::from_millis(300)).is_err());

    // The URL must be there once and read the same everywhere.
    for headers in [
        "",
        "X-Harness-Url: https://allowed.test/\r\nX-Harness-Url: https://ask.test/\r\n",
        "X-Harness-Url: https://user@allowed.test/\r\n",
        "X-Harness-Url: allowed.test\r\n",
    ] {
        assert_eq!(request(proxy, &fetch(headers)).status, 400, "{headers:?}");
    }
    // Searches name no target; the server decides about them.
    let search = request(
        proxy,
        "POST http://llm.harness/_web/search HTTP/1.1\r\n\
         X-Harness-Approved-Host: forged.test\r\nContent-Length: 0\r\n\r\n",
    );
    assert_eq!(search.status, 200);
    let head = heads.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(head[0], "POST /api/llm/_web/search HTTP/1.1");
    assert!(!head.iter().any(|h| h.contains("forged")), "{head:?}");
    // The memory tools name none either.
    let memory = request(
        proxy,
        "POST http://llm.harness/_memory HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
    );
    assert_eq!(memory.status, 200);
    let head = heads.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(head[0], "POST /api/llm/_memory HTTP/1.1");

    // Other paths of harness, also with a query, do not exist.
    for path in [
        "/_web/other",
        "/_web/fetch?x=1",
        "/_web/search?x=1",
        "/_memory/x",
        "/_x",
    ] {
        let response = request(
            proxy,
            &format!(
                "POST http://llm.harness{path} HTTP/1.1\r\n\
                 X-Harness-Url: https://allowed.test/\r\nContent-Length: 0\r\n\r\n"
            ),
        );
        assert_eq!(response.status, 404, "{path}");
    }
    // A model request cannot smuggle an approved host either.
    let chat = request(
        proxy,
        "POST http://llm.harness/c1/chat/completions HTTP/1.1\r\n\
         X-Harness-Approved-Host: forged.test\r\nContent-Length: 0\r\n\r\n",
    );
    assert_eq!(chat.status, 200);
    let head = heads.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(!head.iter().any(|h| h.contains("forged")), "{head:?}");
}

/// A harness server that knows one page as a research hit (`_web/known`
/// answers 204 for it, 404 otherwise) and answers everything else with 200.
fn research_server(hit: &'static str) -> (u16, Receiver<Vec<String>>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let head: Vec<String> = reader
                .by_ref()
                .lines()
                .map_while(Result::ok)
                .take_while(|line| !line.is_empty())
                .collect();
            let length = head
                .iter()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let answer: &[u8] = if head[0].contains("/_web/known") {
                if String::from_utf8_lossy(&body).contains(hit) {
                    b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n"
                } else {
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                }
            } else {
                b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello"
            };
            let _ = tx.send(head);
            let _ = (&stream).write_all(answer);
        }
    });
    (port, rx)
}

#[test]
fn research_hits_load_without_asking() {
    let (gateway, proxy) = start(address::is_public);
    let (port, heads) = research_server("https://found.test/article?id=7");
    let mut control = Control::open(&gateway);
    control.send(&format!(
        "llm http://127.0.0.1:{port}/api/llm token-0123456789abcdef"
    ));
    control.send("policy allowlist ask - blocked.test");
    control.send("watch");
    let fetch = |url: &str| {
        format!(
            "POST http://llm.harness/_web/fetch HTTP/1.1\r\nX-Harness-Url: {url}\r\n\
             Content-Type: application/json\r\nContent-Length: 2\r\n\r\n{{}}"
        )
    };

    // A page a search found: the server says so, no question, and the
    // fetch goes out with the checked host.
    let response = request(proxy, &fetch("https://found.test/article?id=7"));
    assert_eq!(response.status, 200);
    assert_eq!(
        heads.recv_timeout(Duration::from_secs(5)).unwrap()[0],
        "POST /api/llm/_web/known HTTP/1.1"
    );
    let head = heads.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(head[0], "POST /api/llm/_web/fetch HTTP/1.1");
    assert!(
        head.contains(&"X-Harness-Approved-Host: found.test".to_owned()),
        "{head:?}"
    );
    let logged = control.event();
    assert!(
        logged.contains(" allowed found.test 443 research"),
        "{logged}"
    );

    // Another page of the same host still asks: hits are pages, not hosts.
    let waiting = thread::spawn(move || request(proxy, &fetch("https://found.test/other")));
    assert!(control.event().starts_with("event log "));
    assert_eq!(control.event(), "event pending 1 found.test 443");
    control.send("decide 1 deny");
    assert_eq!(waiting.join().unwrap().status, 403);

    // The deny list comes first; the server is not even asked.
    while heads.try_recv().is_ok() {}
    let blocked = request(proxy, &fetch("https://blocked.test/found"));
    assert_eq!(blocked.status, 403);
    assert!(heads.recv_timeout(Duration::from_millis(300)).is_err());
}

#[test]
fn direct_requests_get_an_explanation() {
    let (_gateway, proxy) = start(allow_loopback);
    let response = request(proxy, "GET / HTTP/1.1\r\nHost: gateway\r\n\r\n");
    assert_eq!(response.status, 400);
    assert!(response.body().contains("HTTP-Proxy"));
}

/// `llm.harness` to an `https://` server: the gateway speaks TLS itself and
/// checks the certificate against the configured name and CA.
#[cfg(feature = "tls")]
mod tls {
    use std::sync::Arc;

    use harness_gateway::pem;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use rustls::{ServerConfig, ServerConnection, StreamOwned};

    use super::*;

    const CA: &str = include_str!("fixtures/ca.pem");
    const SERVER_CHAIN: &str = include_str!("fixtures/server.pem");
    const SERVER_KEY: &str = include_str!("fixtures/server.key");

    /// Like `http_server`, over TLS with the test certificates.
    fn https_server() -> (u16, Receiver<Vec<String>>) {
        let chain = pem::certificates(SERVER_CHAIN)
            .unwrap()
            .into_iter()
            .map(CertificateDer::from)
            .collect();
        let key = pem::blocks(SERVER_KEY, "PRIVATE KEY").unwrap().remove(0);
        let config =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(chain, PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)))
                .unwrap();
        let config = Arc::new(config);
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for socket in listener.incoming().flatten() {
                let connection = ServerConnection::new(config.clone()).unwrap();
                let mut reader = BufReader::new(StreamOwned::new(connection, socket));
                // A failed handshake ends the first read.
                let head: Vec<String> = reader
                    .by_ref()
                    .lines()
                    .map_while(Result::ok)
                    .take_while(|line| !line.is_empty())
                    .collect();
                if head.is_empty() {
                    continue;
                }
                drain_body(&mut reader, &head);
                let _ = tx.send(head);
                let tls = reader.get_mut();
                let _ = tls.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
                );
                tls.conn.send_close_notify();
                let _ = tls.flush();
            }
        });
        (port, rx)
    }

    /// The test CA in the wire form of the `llm` request.
    fn ca_field() -> String {
        pem::certificates(CA)
            .unwrap()
            .iter()
            .map(|der| pem::base64_encode(der))
            .collect::<Vec<_>>()
            .join(",")
    }

    #[test]
    fn llm_requests_reach_an_https_server() {
        let (gateway, proxy) = start(address::is_public);
        let (port, heads) = https_server();
        let token = "device-token-0123456789abcdef";
        let chat = "POST http://llm.harness/c1/v1/chat/completions HTTP/1.1\r\n\
                    Authorization: Bearer agent-made-this-up\r\n\
                    Content-Type: application/json\r\nContent-Length: 2\r\n\r\n{}";
        let mut control = Control::open(&gateway);

        // Mozilla's roots do not know the test CA.
        control.send(&format!("llm https://harness.test:{port}/api/llm {token}"));
        let unknown = request(proxy, chat);
        assert_eq!(unknown.status, 502);
        assert_eq!(unknown.headers["x-harness-gateway"], "tls-failed");
        let body = unknown.body();
        assert!(body.contains("UnknownIssuer"), "{body}");

        let ca = ca_field();
        for (url, host) in [
            (
                format!("https://harness.test:{port}/api/llm"),
                format!("harness.test:{port}"),
            ),
            (
                format!("https://127.0.0.1:{port}/api/llm"),
                format!("127.0.0.1:{port}"),
            ),
        ] {
            control.send(&format!("llm {url} {token} {ca}"));
            let response = request(proxy, chat);
            assert_eq!(response.status, 200, "{url}");
            assert_eq!(response.body(), "hello");
            let head = heads.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(head[0], "POST /api/llm/c1/v1/chat/completions HTTP/1.1");
            assert!(head.contains(&format!("Host: {host}")), "{head:?}");
            assert!(
                head.contains(&format!("Authorization: Bearer {token}")),
                "{head:?}"
            );
            assert!(!head.iter().any(|h| h.contains("made-this-up")), "{head:?}");
        }

        // The same certificate under a name it does not carry.
        control.send(&format!(
            "llm https://other.test:{port}/api/llm {token} {ca}"
        ));
        let wrong = request(proxy, chat);
        assert_eq!(wrong.status, 502);
        assert_eq!(wrong.headers["x-harness-gateway"], "tls-failed");
        let body = wrong.body();
        assert!(body.contains("not valid for name"), "{body}");
        assert!(!body.contains(token), "{body}");
    }
}
