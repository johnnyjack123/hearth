//! The gateway on the VM: an instance in the internal project network reaches
//! the internet only through it, by the project's rules, and its model only
//! through `llm.harness`, also to `https://` servers.
//!
//! Ignored by default; needs a running machine, both images and internet
//! access: `cargo test -p harness-core --test gateway_vm -- --ignored --nocapture`

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, TcpListener, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use harness_core::config::{FileMode, NetworkPreset, Project};
use harness_core::gateway::{
    self, Event, GatewayControl, GatewayState, Host, LLM_HOST, Reason, Reply, Scope,
};
use harness_core::image::{BASE_IMAGE, GATEWAY_IMAGE, ImageStatus};
use harness_core::podman::{DEFAULT_MACHINE, PodmanProvider};
use harness_core::project::ProjectName;
use harness_core::provider::{
    ContainerSpec, ExecOutput, ExecSpec, Mount, MountSource, Provider, Resources, VolumeSpec,
};
use harness_core::server::{self, ServerCa};
use harness_gateway::pem;

struct Cleanup<'a> {
    provider: &'a PodmanProvider,
    name: &'a ProjectName,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let p = self.provider;
        let _ = p.remove_container(&self.name.instance());
        let _ = p.remove_container(&self.name.gateway());
        let _ = p.remove_volume(&self.name.home_volume());
        let _ = p.remove_network(&self.name.network());
    }
}

/// Removes one more container; declared after [`Cleanup`], so it goes
/// before the network.
struct RemoveContainer<'a>(&'a PodmanProvider, &'a str);

impl Drop for RemoveContainer<'_> {
    fn drop(&mut self) {
        let _ = self.0.remove_container(self.1);
    }
}

fn project(name: &ProjectName, network: NetworkPreset) -> Project {
    Project {
        name: name.clone(),
        file: PathBuf::from("unused.toml"),
        path: PathBuf::from("/unused"),
        harness: "pi".into(),
        mode: FileMode::Isolated,
        network,
        ask: true,
        allow: vec![],
        deny: vec!["example.org".parse().unwrap()],
        llm_apis: vec!["api.deepseek.com".parse().unwrap()],
        resources: Resources::default(),
    }
}

fn timed<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let value = f();
    println!("{label}: {} ms", start.elapsed().as_millis());
    value
}

/// A minimal HTTP server on 127.0.0.1 that answers `200 ok` and reports the
/// request heads it saw.
fn fake_server() -> (u16, Receiver<Vec<String>>) {
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
            let _ = (&stream)
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });
    (port, rx)
}

/// HTTP status curl saw through the proxy; for CONNECT the proxy's refusal
/// shows up as curl exit 56 with the status in stderr.
fn http_status(output: &ExecOutput) -> String {
    if output.code == 0 {
        output.stdout.trim().to_owned()
    } else {
        format!("curl {}: {}", output.code, output.stderr.trim())
    }
}

#[test]
#[ignore = "needs the running Podman machine, both images and internet access"]
fn instances_reach_the_internet_only_through_the_gateway() {
    let machine = std::env::var("HARNESS_MACHINE").unwrap_or_else(|_| DEFAULT_MACHINE.into());
    let provider = PodmanProvider::for_machine(&machine).unwrap();
    for image in [BASE_IMAGE, GATEWAY_IMAGE] {
        assert_eq!(
            image.status(&provider).unwrap(),
            ImageStatus::Current,
            "run `harness image build` first"
        );
    }
    let name = ProjectName::new(&format!("gwtest-{}", std::process::id())).unwrap();
    let _cleanup = Cleanup {
        provider: &provider,
        name: &name,
    };
    let development = project(&name, NetworkPreset::Development);

    // Stands in for the harness server on this Windows machine's loopback.
    let (server_port, server_heads) = fake_server();
    let token = "hd_test-token-0123456789abcdefghijklmnopqrstuvw";
    let upstream = server::llm_upstream(
        &format!("http://127.0.0.1:{server_port}"),
        provider.host_address(),
        token,
        None,
    )
    .unwrap();

    timed("gateway start (create)", || {
        gateway::ensure_running(&provider, &development, None).unwrap()
    });
    timed("gateway start (running, with LLM)", || {
        gateway::ensure_running(&provider, &development, Some(&upstream)).unwrap()
    });
    assert_eq!(
        gateway::state(&provider, &name).unwrap(),
        GatewayState::Running
    );

    // The root filesystem is read-only; npm and corepack need the home volume.
    provider
        .ensure_volume(&VolumeSpec {
            name: name.home_volume(),
            ..VolumeSpec::default()
        })
        .unwrap();
    provider
        .create_container(&ContainerSpec {
            name: name.instance(),
            image: BASE_IMAGE.tag.to_owned(),
            network: name.network(),
            mounts: vec![Mount {
                source: MountSource::Volume(name.home_volume()),
                target: "/home/agent".into(),
                read_only: false,
            }],
            ..ContainerSpec::default()
        })
        .unwrap();
    provider.start_container(&name.instance()).unwrap();
    let instance = name.instance();
    let run = |script: &str| {
        provider
            .exec_output(&ExecSpec {
                container: instance.clone(),
                command: vec!["sh".into(), "-c".into(), script.into()],
                ..ExecSpec::default()
            })
            .unwrap()
    };
    // `--noproxy ''` overrides NO_PROXY=localhost from the image, so even
    // localhost goes to the gateway.
    let curl = |url: &str| {
        let output = run(&format!(
            "curl -sS --max-time 20 --noproxy '' -o /dev/null -w '%{{http_code}}' {url}"
        ));
        let status = http_status(&output);
        println!("{url} → {status}");
        status
    };

    // Preset development.
    let blocked = curl("https://example.com/");
    assert!(
        blocked.starts_with("curl 56") && blocked.contains("403"),
        "{blocked}"
    );
    let deepseek = curl("https://api.deepseek.com/");
    assert!(
        deepseek.len() == 3 && deepseek != "000",
        "DeepSeek not reachable: {deepseek}"
    );
    let npm = timed("npm view react version", || run("npm view react version"));
    assert_eq!(npm.code, 0, "{}", npm.stderr);
    println!("react {}", npm.stdout.trim());
    let pnpm = timed("pnpm --version (corepack download)", || {
        run("cd /tmp && pnpm --version")
    });
    assert_eq!(pnpm.code, 0, "{}", pnpm.stderr);
    println!("pnpm {}", pnpm.stdout.trim());
    // Without the proxy there is neither DNS nor a route.
    let direct = run("curl -sS --max-time 10 --noproxy '*' -o /dev/null https://example.com/");
    assert_ne!(direct.code, 0);
    println!("direct: curl {} {}", direct.code, direct.stderr.trim());

    // llm.harness: through the gateway and gvproxy to the server on Windows,
    // with the device token instead of the agent's made-up key.
    let llm = run(
        "curl -sS --max-time 20 -o /dev/null -w '%{http_code}' -X POST \
         -H 'Authorization: Bearer agent-made-this-up' -H 'Content-Type: application/json' \
         -d '{\"model\":\"m\"}' http://llm.harness/c1/chat/completions",
    );
    assert_eq!(http_status(&llm), "200", "{}", llm.stderr);
    let head = server_heads.recv_timeout(Duration::from_secs(5)).unwrap();
    println!("server saw: {}", head[0]);
    assert_eq!(head[0], "POST /api/llm/proxy/c1/chat/completions HTTP/1.1");
    assert!(
        head.contains(&format!("Authorization: Bearer {token}")),
        "{head:?}"
    );
    assert!(!head.iter().any(|h| h.contains("made-this-up")), "{head:?}");

    // undici (pi's SDKs) tunnels plain http too; `--proxytunnel` does the same.
    let tunneled = run(
        "curl -sS --max-time 20 --proxytunnel -o /dev/null -w '%{http_code}' -X POST \
         -H 'Authorization: Bearer agent-made-this-up' -H 'Content-Type: application/json' \
         -d '{\"model\":\"m\"}' http://llm.harness/c1/chat/completions",
    );
    assert_eq!(http_status(&tunneled), "200", "{}", tunneled.stderr);
    let head = server_heads.recv_timeout(Duration::from_secs(5)).unwrap();
    println!("server saw (tunneled): {}", head[0]);
    assert_eq!(head[0], "POST /api/llm/proxy/c1/chat/completions HTTP/1.1");
    assert!(
        head.contains(&format!("Authorization: Bearer {token}")),
        "{head:?}"
    );
    assert!(!head.iter().any(|h| h.contains("made-this-up")), "{head:?}");

    let control = GatewayControl::new(&provider, &name);
    let status = control.status().unwrap();
    assert_eq!(status.llm.as_deref(), Some(upstream.url().as_str()));
    println!("hosts contacted:");
    for entry in &status.log {
        println!(
            "  {} {}:{} {}",
            entry.allowed, entry.host, entry.port, entry.reason
        );
    }
    let logged = |host: &str, allowed: bool| {
        let host = Host::parse(host).unwrap();
        status
            .log
            .iter()
            .any(|e| e.host == host && e.allowed == allowed)
    };
    assert!(logged("registry.npmjs.org", true));
    assert!(logged("example.com", false));

    // Preset open: the public internet, but never host, LAN or VM.
    control
        .apply(&gateway::policy(&project(&name, NetworkPreset::Open)))
        .unwrap();
    assert_eq!(curl("https://example.com/"), "200");
    for url in [
        "http://host.containers.internal:8080/",
        "http://192.168.127.254:8080/",
        "http://192.168.127.1/",
        "http://10.88.0.1/",
        "http://169.254.169.254/",
        "http://gateway:8888/",
        "http://localhost:8080/",
        "http://[::1]:8080/",
    ] {
        assert_eq!(curl(url), "403", "{url} must stay blocked");
    }
    let denied = curl("https://example.org/");
    assert!(denied.contains("403"), "deny list ignored: {denied}");

    // Back to development, with someone watching: unknown hosts wait for an
    // answer instead of failing.
    control.apply(&gateway::policy(&development)).unwrap();
    let mut watch = control.watch().unwrap();
    let waiting = {
        let provider = provider.clone();
        let instance = instance.clone();
        thread::spawn(move || {
            provider
                .exec_output(&ExecSpec {
                    container: instance,
                    command: vec![
                        "curl".into(),
                        "-sS".into(),
                        "--max-time".into(),
                        "60".into(),
                        "-o".into(),
                        "/dev/null".into(),
                        "-w".into(),
                        "%{http_code}".into(),
                        "https://example.net/".into(),
                    ],
                    ..ExecSpec::default()
                })
                .unwrap()
        })
    };
    let asked = Instant::now();
    let question = loop {
        match watch.next(Duration::from_secs(30)).unwrap() {
            Some(Reply::Event(Event::Pending { id, host, .. })) => break (id, host),
            Some(_) => {}
            None => panic!("no question within 30 s"),
        }
    };
    assert_eq!(question.1, Host::parse("example.net").unwrap());
    println!("question after {} ms", asked.elapsed().as_millis());
    watch.decide(question.0, Some(Scope::Once)).unwrap();
    let answered = waiting.join().unwrap();
    assert_eq!(http_status(&answered), "200");
    let resolved = loop {
        match watch.next(Duration::from_secs(10)).unwrap() {
            Some(Reply::Event(Event::Resolved {
                id,
                allowed,
                reason,
            })) if id == question.0 => {
                break (allowed, reason);
            }
            Some(_) => {}
            None => panic!("no resolution event"),
        }
    };
    assert_eq!(resolved, (true, Reason::Approved));
    drop(watch);

    // Nobody watching any more: refused at once, until allowed explicitly.
    let started = Instant::now();
    assert!(curl("https://example.net/").contains("403"));
    assert!(started.elapsed() < Duration::from_secs(10));
    control
        .allow(&"example.net".parse().unwrap(), Scope::Session)
        .unwrap();
    assert_eq!(curl("https://example.net/"), "200");
    let status = control.status().unwrap();
    assert_eq!(status.grants.len(), 1);
    assert_eq!(status.watchers, 0);

    gateway::stop(&provider, &name).unwrap();
    assert_eq!(
        gateway::state(&provider, &name).unwrap(),
        GatewayState::Stopped
    );
}

/// Name of the stand-in HTTPS server; the test certificate is valid for it.
const TLS_SERVER: &str = "harness-tls-test";

/// Serves HTTPS on 8443 with the test certificate (Node in the base image)
/// and echoes what it received: request line, Host and Authorization.
const TLS_SERVER_SCRIPT: &str = r#"
const https = require("https");
const pem = (name) => Buffer.from(process.env[name], "base64");
https.createServer({ cert: pem("TLS_CERT"), key: pem("TLS_KEY") }, (req, res) => {
  req.resume();
  req.on("end", () => {
    res.writeHead(200, { "content-type": "text/plain" });
    res.end(`${req.method} ${req.url}\nhost=${req.headers.host}\nauth=${req.headers.authorization}\n`);
  });
}).listen(8443);
"#;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../harness-gateway/tests/fixtures"
);

#[test]
#[ignore = "needs the running Podman machine, both images and internet access"]
fn llm_harness_reaches_https_servers() {
    let machine = std::env::var("HARNESS_MACHINE").unwrap_or_else(|_| DEFAULT_MACHINE.into());
    let provider = PodmanProvider::for_machine(&machine).unwrap();
    for image in [BASE_IMAGE, GATEWAY_IMAGE] {
        assert_eq!(
            image.status(&provider).unwrap(),
            ImageStatus::Current,
            "run `harness image build` first"
        );
    }
    let name = ProjectName::new(&format!("gwtls-{}", std::process::id())).unwrap();
    let _cleanup = Cleanup {
        provider: &provider,
        name: &name,
    };
    let _ = provider.remove_container(TLS_SERVER);
    let _server_cleanup = RemoveContainer(&provider, TLS_SERVER);
    let project = project(&name, NetworkPreset::Off);
    let token = "hd_test-token-0123456789abcdefghijklmnopqrstuvw";
    let ca = ServerCa::load(&PathBuf::from(FIXTURES).join("ca.pem")).unwrap();
    let upstream = |url: &str, ca: Option<&ServerCa>| {
        server::llm_upstream(url, provider.host_address(), token, ca).unwrap()
    };

    // A public server with a certificate from a public CA.
    timed("gateway start (https://example.com)", || {
        gateway::ensure_running(
            &provider,
            &project,
            Some(&upstream("https://example.com", None)),
        )
        .unwrap()
    });
    let control = GatewayControl::new(&provider, &name);

    let fixture =
        |file: &str| pem::base64_encode(&fs::read(PathBuf::from(FIXTURES).join(file)).unwrap());
    for (container, command, env) in [
        (name.instance(), vec![], vec![]),
        (
            TLS_SERVER.to_owned(),
            vec![
                "node".to_owned(),
                "-e".to_owned(),
                TLS_SERVER_SCRIPT.to_owned(),
            ],
            vec![
                ("TLS_CERT".to_owned(), fixture("server.pem")),
                ("TLS_KEY".to_owned(), fixture("server.key")),
            ],
        ),
    ] {
        provider
            .create_container(&ContainerSpec {
                name: container.clone(),
                image: BASE_IMAGE.tag.to_owned(),
                network: name.network(),
                command,
                env: env.into_iter().collect(),
                ..ContainerSpec::default()
            })
            .unwrap();
        provider.start_container(&container).unwrap();
    }
    let instance = name.instance();
    let run = |script: &str| {
        provider
            .exec_output(&ExecSpec {
                container: instance.clone(),
                command: vec!["sh".into(), "-c".into(), script.into()],
                ..ExecSpec::default()
            })
            .unwrap()
    };
    // Status line and body of a chat request to llm.harness, as an agent
    // would send it with its made-up key.
    let chat = || {
        let output = run(
            "curl -sS --max-time 30 -i -X POST -H 'Authorization: Bearer agent-made-this-up' \
             -H 'Content-Type: application/json' -d '{}' http://llm.harness/c1/chat/completions",
        );
        assert_eq!(output.code, 0, "{}", output.stderr);
        let text = output.stdout.replace('\r', "");
        let status = text.lines().next().unwrap_or_default().to_owned();
        println!("  → {status}");
        (status, text)
    };
    // The gateway's verdict about the last llm.harness request.
    let last_llm = || {
        let status = control.status().unwrap();
        let host = Host::parse(LLM_HOST).unwrap();
        let entry = status.log.iter().rev().find(|e| e.host == host).unwrap();
        (entry.allowed, entry.reason)
    };

    println!("https://example.com (Mozilla roots):");
    let (status, text) = chat();
    // Whatever example.com answers to the path, it is not the gateway.
    assert!(!text.contains("X-Harness-Gateway"), "{text}");
    assert!(status.starts_with("HTTP/1.1 "), "{status}");
    assert_eq!(last_llm(), (true, Reason::Llm));

    // The same server under an address its certificate does not name. A CDN
    // may already refuse the handshake without a name (no SNI); either way
    // the request must not get through. The exact name check follows below
    // with the own CA.
    let ip = ("example.com", 443)
        .to_socket_addrs()
        .unwrap()
        .map(|a| a.ip())
        .find(IpAddr::is_ipv4)
        .unwrap();
    println!("https://{ip} (example.com by address):");
    control
        .set_llm(Some(&upstream(&format!("https://{ip}"), None)))
        .unwrap();
    let (status, text) = chat();
    assert!(status.contains(" 502 "), "{text}");
    assert!(text.contains("X-Harness-Gateway: tls-failed"), "{text}");
    println!("  {}", text.lines().last().unwrap_or_default());
    assert_eq!(last_llm(), (false, Reason::TlsFailed));

    // A server with its own CA, like Caddy's `tls internal`.
    let own = format!("https://{TLS_SERVER}:8443");
    println!("{own} without its CA:");
    control.set_llm(Some(&upstream(&own, None))).unwrap();
    // Node needs a moment to listen; the first try may find it closed.
    let deadline = Instant::now() + Duration::from_secs(20);
    let (status, text) = loop {
        let (status, text) = chat();
        if !text.contains("connect-failed") || Instant::now() > deadline {
            break (status, text);
        }
        thread::sleep(Duration::from_millis(500));
    };
    assert!(status.contains(" 502 "), "{text}");
    assert!(text.contains("UnknownIssuer"), "{text}");
    assert_eq!(last_llm(), (false, Reason::TlsFailed));

    println!("{own} with its CA:");
    control.set_llm(Some(&upstream(&own, Some(&ca)))).unwrap();
    let (status, text) = chat();
    assert!(status.contains(" 200 "), "{text}");
    assert!(
        text.contains("POST /api/llm/proxy/c1/chat/completions\n"),
        "{text}"
    );
    assert!(
        text.contains(&format!("host={TLS_SERVER}:8443\n")),
        "{text}"
    );
    assert!(text.contains(&format!("auth=Bearer {token}\n")), "{text}");
    assert!(!text.contains("made-this-up"), "{text}");
    assert_eq!(last_llm(), (true, Reason::Llm));
    let status = control.status().unwrap();
    assert_eq!(
        status.llm.as_deref(),
        Some(format!("{own}/api/llm/proxy").as_str())
    );

    // The same server by its address, which the certificate does not name.
    let address = run(&format!("getent hosts {TLS_SERVER}"));
    let address = address.stdout.split_whitespace().next().unwrap().to_owned();
    println!("https://{address}:8443 with the CA:");
    control
        .set_llm(Some(&upstream(
            &format!("https://{address}:8443"),
            Some(&ca),
        )))
        .unwrap();
    let (status, text) = chat();
    assert!(status.contains(" 502 "), "{text}");
    assert!(text.contains("not valid for name"), "{text}");
    assert_eq!(last_llm(), (false, Reason::TlsFailed));
}
