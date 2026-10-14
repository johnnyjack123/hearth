//! The gateway on the VM: an instance in the internal project network reaches
//! the internet only through it, by the project's rules.
//!
//! Ignored by default; needs a running machine, both images and internet
//! access: `cargo test -p harness-core --test gateway_vm -- --ignored --nocapture`

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use harness_core::config::{FileMode, NetworkPreset, Project};
use harness_core::gateway::{
    self, Event, GatewayControl, GatewayState, Host, Reason, Reply, Scope,
};
use harness_core::image::{BASE_IMAGE, GATEWAY_IMAGE, ImageStatus};
use harness_core::podman::{DEFAULT_MACHINE, PodmanProvider};
use harness_core::project::ProjectName;
use harness_core::provider::{
    ContainerSpec, ExecOutput, ExecSpec, Mount, MountSource, Provider, Resources, VolumeSpec,
};
use harness_core::server;

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
