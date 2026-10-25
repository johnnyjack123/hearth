//! The proxy server: one thread per client connection.

use std::io::{self, BufReader, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use crate::gateway::{Gateway, Verdict};
use crate::http::{self, HeadError, RequestHead, Target};
use crate::llm::{
    CREDENTIAL_HEADERS, LLM_HOST, URL_HEADER, WEB_FETCH_PATH, WEB_SEARCH_PATH, web_fetch_target,
};
use crate::pattern::Host;
use crate::policy::Reason;
use crate::upstream::Upstream;

/// Time a client gets to send its request head.
const HEAD_TIMEOUT: Duration = Duration::from_secs(30);

/// Time per address to establish the upstream connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Stack of connection threads. Enough for parsing and the system resolver,
/// much less than the 2 MiB default, so hundreds of tunnels stay cheap.
const THREAD_STACK: usize = 256 * 1024;

/// Accepts connections forever (until the listener fails).
pub fn serve(gateway: &Gateway, listener: TcpListener) -> io::Result<()> {
    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                // E.g. out of file descriptors: back off instead of spinning.
                eprintln!("accept: {error}");
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        if !gateway.connection_opened() {
            let _ = http::write_response(
                &mut &stream,
                503,
                &[],
                "Zu viele gleichzeitige Verbindungen über das harness-Gateway.\n",
            );
            continue;
        }
        let worker = gateway.clone();
        let spawned = thread::Builder::new()
            .name("connection".to_owned())
            .stack_size(THREAD_STACK)
            .spawn(move || {
                handle(&worker, stream);
                worker.connection_closed();
            });
        if spawned.is_err() {
            gateway.connection_closed();
        }
    }
    Ok(())
}

fn handle(gateway: &Gateway, client: TcpStream) {
    let _ = client.set_read_timeout(Some(HEAD_TIMEOUT));
    let _ = client.set_nodelay(true);
    let Ok(reader) = client.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(reader);
    let head = match http::read_head(&mut reader) {
        Ok(head) => head,
        Err(HeadError::Closed | HeadError::Io(_)) => return,
        Err(HeadError::TooLarge) => {
            let _ = http::write_response(&mut &client, 431, &[], "Anfrage zu groß.\n");
            return;
        }
        Err(HeadError::Bad(message)) => {
            let _ = http::write_response(&mut &client, 400, &[], &format!("{message}\n"));
            return;
        }
    };
    let (host, port) = head.target.host_port();
    if host.name() == Some(LLM_HOST) {
        forward_llm(gateway, &client, reader, &head);
        return;
    }

    let reason = match decide(gateway, host, port) {
        Ok(reason) => reason,
        Err(reason) => {
            gateway.record(false, host, port, reason);
            refuse(&client, host, reason, None);
            return;
        }
    };
    let socket = match connect(gateway, host, port) {
        Ok(socket) => socket,
        Err(failure) => {
            gateway.record(false, host, port, failure);
            refuse(&client, host, failure, None);
            return;
        }
    };
    let _ = socket.set_nodelay(true);
    let Ok(mut upstream) = Upstream::plain(socket) else {
        return;
    };
    gateway.record(true, host, port, reason);

    let _ = client.set_read_timeout(None);
    let started = match &head.target {
        Target::Tunnel { .. } => {
            (&client).write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
        }
        Target::Forward { .. } => match http::forward_head(&head) {
            Some(text) => upstream.writer.write_all(text.as_bytes()),
            None => return,
        },
    };
    if started.is_ok() {
        relay(&client, reader, upstream);
    }
}

/// What the project's rules say about `host`, asking the user if they
/// say so.
fn decide(gateway: &Gateway, host: &Host, port: u16) -> Result<Reason, Reason> {
    match gateway.check(host, port) {
        Verdict::Allow(reason) => Ok(reason),
        Verdict::Deny(reason) => Err(reason),
        Verdict::Ask(pending) => match gateway.await_decision(&pending) {
            (true, reason) => Ok(reason),
            (false, reason) => Err(reason),
        },
    }
}

/// `web_fetch` of an agent: the server loads the page, but only for a host
/// that passed the project's rules here, exactly like a direct connection.
/// Returns that host, or answers the client itself and returns `None`.
fn check_web_fetch(gateway: &Gateway, client: &TcpStream, head: &RequestHead) -> Option<Host> {
    let mut urls = head
        .headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case(URL_HEADER))
        .map(|(_, value)| value.as_str());
    let target = match (urls.next(), urls.next()) {
        (Some(url), None) => web_fetch_target(url),
        _ => Err(format!("web_fetch braucht genau einen Header {URL_HEADER}")),
    };
    let (host, port) = match target {
        Ok(target) => target,
        Err(message) => {
            let _ = http::write_response(&mut &*client, 400, &[], &format!("{message}\n"));
            return None;
        }
    };
    match decide(gateway, &host, port) {
        Ok(reason) => {
            gateway.record(true, &host, port, reason);
            Some(host)
        }
        Err(reason) => {
            gateway.record(false, &host, port, reason);
            refuse(client, &host, reason, None);
            None
        }
    }
}

/// A request to `llm.harness`: forwarded to the server's LLM endpoint with
/// the device token instead of whatever credentials the client sent.
///
/// The upstream is the one target outside the address filter (on a local
/// server it is the VM's route to Windows' localhost). That is safe because
/// the host sets it; the agent only picks the path below its prefix. Clients
/// always speak plain HTTP here; for an `https://` server the gateway
/// encrypts itself, which is the only way the token stays out of the
/// instance.
///
/// [`WEB_FETCH_PATH`] is the one path that names another target, the page
/// the server should load; its host has to pass the project's rules first.
fn forward_llm(
    gateway: &Gateway,
    client: &TcpStream,
    mut reader: BufReader<TcpStream>,
    head: &RequestHead,
) {
    let (host, port) = head.target.host_port();
    let tunneled;
    let head = match &head.target {
        Target::Forward { .. } => head,
        // undici's proxy agent (pi) tunnels even plain http:// URLs. Inside
        // the tunnel comes ordinary HTTP, which the gateway reads itself.
        Target::Tunnel { port: 80, .. } => {
            if (&*client)
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
                .is_err()
            {
                return;
            }
            tunneled = match http::read_tunneled_head(&mut reader, host, port) {
                Ok(head) => head,
                Err(HeadError::Bad(message)) => {
                    let _ = http::write_response(&mut &*client, 400, &[], &format!("{message}\n"));
                    return;
                }
                Err(_) => return,
            };
            &tunneled
        }
        Target::Tunnel { .. } => {
            // TLS to a name only the gateway knows cannot work.
            let _ = http::write_response(
                &mut &*client,
                400,
                &[],
                "llm.harness nur per http:// (das Gateway verschlüsselt selbst).\n",
            );
            return;
        }
    };
    let Target::Forward { path, .. } = &head.target else {
        return;
    };
    // Paths below `/_` belong to harness, not to a credential.
    let is_web_fetch = path == WEB_FETCH_PATH;
    if path.starts_with("/_") && !is_web_fetch && path != WEB_SEARCH_PATH {
        let _ = http::write_response(&mut &*client, 404, &[], "Unbekannter Pfad.\n");
        return;
    }
    let Some((upstream, connector)) = gateway.llm() else {
        gateway.record(false, host, port, Reason::NoLlm);
        refuse(client, host, Reason::NoLlm, None);
        return;
    };
    let Some(upstream_path) = upstream.path(path) else {
        let _ = http::write_response(&mut &*client, 400, &[], "Pfad nicht erlaubt.\n");
        return;
    };
    // Asks only once the request could go through otherwise.
    let web_fetch = if is_web_fetch {
        match check_web_fetch(gateway, client, head) {
            Some(host) => Some(host.to_string()),
            None => return,
        }
    } else {
        None
    };
    let options = gateway.options();
    let addresses = match upstream.host() {
        Host::Ip(ip) => Ok(vec![SocketAddr::new(*ip, upstream.port())]),
        Host::Name(name) => (options.resolve)(name, upstream.port()),
    };
    let server = addresses.ok().and_then(|addresses| {
        addresses
            .iter()
            .find_map(|address| TcpStream::connect_timeout(address, CONNECT_TIMEOUT).ok())
    });
    let Some(server) = server else {
        gateway.record(false, host, port, Reason::ConnectFailed);
        refuse(client, host, Reason::ConnectFailed, None);
        return;
    };
    let _ = server.set_nodelay(true);
    let mut server = match connector.open(upstream.host(), server) {
        Ok(server) => server,
        Err((reason, detail)) => {
            if options.print_log {
                println!("{LLM_HOST} → {}: {detail}", upstream.url());
            }
            gateway.record(false, host, port, reason);
            refuse(client, host, reason, Some(&detail));
            return;
        }
    };
    gateway.record(true, host, port, Reason::Llm);

    let bearer = format!("Bearer {}", upstream.token());
    let mut add = vec![("Authorization", bearer.as_str())];
    if let Some(host) = &web_fetch {
        add.push(("X-Harness-Approved-Host", host));
    }
    let text = http::rewrite_head(
        head,
        &upstream_path,
        &upstream.host_header(),
        CREDENTIAL_HEADERS,
        &add,
    );
    let _ = client.set_read_timeout(None);
    if server.writer.write_all(text.as_bytes()).is_ok() {
        relay(client, reader, server);
    }
}

/// Resolves `host` and connects to the first public address that answers.
/// The checked address is the one connected to, so a second DNS answer
/// cannot sneak in a private address (DNS rebinding).
fn connect(gateway: &Gateway, host: &Host, port: u16) -> Result<TcpStream, Reason> {
    let options = gateway.options();
    let addresses: Vec<SocketAddr> = match host {
        Host::Ip(ip) => vec![SocketAddr::new(*ip, port)],
        Host::Name(name) => (options.resolve)(name, port).map_err(|_| Reason::ResolveFailed)?,
    };
    let filter = options.address_filter;
    let allowed: Vec<&SocketAddr> = addresses.iter().filter(|a| filter(a.ip())).collect();
    if allowed.is_empty() {
        return Err(if addresses.is_empty() {
            Reason::ResolveFailed
        } else {
            Reason::PrivateAddress
        });
    }
    allowed
        .into_iter()
        .find_map(|address| TcpStream::connect_timeout(address, CONNECT_TIMEOUT).ok())
        .ok_or(Reason::ConnectFailed)
}

/// Error page for a refused connection. CONNECT clients often hide the body,
/// so the reason is also in a header. `detail` explains failures in depth
/// (TLS errors); it never contains secrets.
fn refuse(client: &TcpStream, host: &Host, reason: Reason, detail: Option<&str>) {
    let status = match reason {
        Reason::ResolveFailed | Reason::ConnectFailed | Reason::TlsFailed => 502,
        Reason::NoLlm => 503,
        Reason::Unanswered => 504,
        _ => 403,
    };
    let mut body = format!(
        "harness-Gateway: Verbindung zu {host} blockiert ({}).\n\
         harness gateway: connection to {host} blocked ({}).\n",
        reason.describe(),
        reason.describe_en()
    );
    if matches!(reason, Reason::NotAllowed | Reason::Unanswered) && host.name().is_some() {
        body.push_str(&format!(
            "Freigeben auf dem Host / to allow it, the user runs: harness net allow {host}\n"
        ));
    }
    if let Some(detail) = detail {
        body.push_str(detail);
        body.push('\n');
    }
    let _ = http::write_response(
        &mut &*client,
        status,
        &[("X-Harness-Gateway", reason.as_str())],
        &body,
    );
}

/// Copies bytes both ways until the server is done, then closes both sides.
/// No idle timeout: a streamed LLM answer can pause for minutes while the
/// client sends nothing.
fn relay(client: &TcpStream, mut from_client: BufReader<TcpStream>, upstream: Upstream) {
    let Upstream {
        mut writer,
        mut reader,
        socket,
    } = upstream;
    let uploader = thread::Builder::new()
        .name("upload".to_owned())
        .stack_size(THREAD_STACK)
        .spawn(move || {
            // The BufReader first hands over what it already buffered.
            let _ = io::copy(&mut from_client, &mut writer);
            writer.finish();
        });
    let _ = io::copy(&mut reader, &mut &*client);
    // Server finished: end the client side too, which also unblocks the
    // uploader if the client keeps its half open.
    let _ = client.shutdown(Shutdown::Both);
    let _ = socket.shutdown(Shutdown::Both);
    if let Ok(uploader) = uploader {
        let _ = uploader.join();
    }
}
