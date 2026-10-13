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
fn direct_requests_get_an_explanation() {
    let (_gateway, proxy) = start(allow_loopback);
    let response = request(proxy, "GET / HTTP/1.1\r\nHost: gateway\r\n\r\n");
    assert_eq!(response.status, 400);
    assert!(response.body().contains("HTTP-Proxy"));
}
