//! A project's egress gateway seen from the host: which rules follow from
//! the configuration, its container, and the control connection.
//!
//! The gateway itself is the `harness-gateway` crate, compiled into
//! [`GATEWAY_IMAGE`]. The host never reaches its control socket directly;
//! every request runs `harness-gateway ctl` in the gateway container through
//! the provider, so the VM needs no extra port and instances cannot get at
//! the controls.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

pub use harness_gateway::control::{Event, GrantInfo, LogEntry, Reply, Request, Scope, Status};
pub use harness_gateway::{Host, LLM_HOST, LlmUpstream, Mode, Policy, Reason};

use crate::config::{DomainPattern, NetworkPreset, Project};
use crate::image::{CONTEXT_HASH_LABEL, GATEWAY_IMAGE, ImageStatus};
use crate::project::ProjectName;
use crate::provider::{ContainerStatus, ExecSpec, GatewaySpec, Labels, NetworkSpec, Provider};
use crate::sync::sanitize;
use crate::{Error, PROJECT_LABEL, Result};

/// Name the instances reach the gateway under; `HTTP(S)_PROXY` in the base
/// image points to `http://gateway:8888`.
pub const GATEWAY_ALIAS: &str = "gateway";

/// Normal network all gateways share for their way out.
pub const EGRESS_NETWORK: &str = "harness-egress";

/// Domains of the preset `development`: package registries and git hosts.
pub const DEVELOPMENT_DOMAINS: &[&str] = &[
    // JavaScript. npm also serves pnpm to corepack; node-gyp fetches the
    // Node headers from nodejs.org.
    "registry.npmjs.org",
    "registry.yarnpkg.com",
    "nodejs.org",
    // Python
    "pypi.org",
    "files.pythonhosted.org",
    // Rust
    "crates.io",
    "index.crates.io",
    "static.crates.io",
    "static.rust-lang.org",
    // Go
    "proxy.golang.org",
    "sum.golang.org",
    // Git hosts. GitHub serves archives, releases and raw files from
    // subdomains (codeload, objects, raw, release-assets); uv downloads its
    // Python builds from there too.
    "github.com",
    "*.github.com",
    "*.githubusercontent.com",
    "gitlab.com",
    "bitbucket.org",
];

/// How long a request waits for the gateway's control socket after a fresh
/// start before giving up.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

/// The domains of [`DEVELOPMENT_DOMAINS`] as patterns.
pub fn development_domains() -> Vec<DomainPattern> {
    DEVELOPMENT_DOMAINS
        .iter()
        .map(|domain| domain.parse().expect("valid built-in domain"))
        .collect()
}

/// The rules a project's configuration amounts to.
pub fn policy(project: &Project) -> Policy {
    let mut allow = project.llm_apis.clone();
    match project.network {
        NetworkPreset::Off => {}
        NetworkPreset::Development => {
            allow.extend(development_domains());
            allow.extend(project.allow.iter().cloned());
        }
        NetworkPreset::Custom | NetworkPreset::Open => allow.extend(project.allow.iter().cloned()),
    }
    let mut seen = HashSet::new();
    allow.retain(|pattern| seen.insert(pattern.clone()));
    Policy {
        mode: if project.network == NetworkPreset::Open {
            Mode::Open
        } else {
            Mode::Allowlist
        },
        ask: project.ask,
        allow,
        deny: project.deny.clone(),
    }
}

/// Whether a project's gateway exists and runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GatewayState {
    /// No container.
    Missing,
    /// Created or stopped.
    Stopped,
    /// Running.
    Running,
}

/// State of the project's gateway container.
pub fn state(provider: &dyn Provider, name: &ProjectName) -> Result<GatewayState> {
    Ok(match provider.container(&name.gateway())? {
        None => GatewayState::Missing,
        Some(info) if info.status == ContainerStatus::Running => GatewayState::Running,
        Some(_) => GatewayState::Stopped,
    })
}

/// Starts the project's gateway, creating its networks and container as
/// needed, applies the project's rules and sets where `llm.harness` leads
/// (`None`: nowhere, harnesses then need their own provider access). A
/// container from an older gateway image is replaced, so fixes to the
/// gateway take effect.
pub fn ensure_running(
    provider: &dyn Provider,
    project: &Project,
    llm: Option<&LlmUpstream>,
) -> Result<()> {
    match GATEWAY_IMAGE.status(provider)? {
        ImageStatus::Current => {}
        ImageStatus::Missing => {
            return Err(Error::Unsupported(format!(
                "Gateway-Image {} fehlt → harness image build",
                GATEWAY_IMAGE.tag
            )));
        }
        ImageStatus::Outdated => {
            return Err(Error::Unsupported(format!(
                "Gateway-Image {} ist veraltet → harness image build",
                GATEWAY_IMAGE.tag
            )));
        }
    }
    let name = &project.name;
    let project_labels: Labels = [(PROJECT_LABEL.to_owned(), name.to_string())].into();
    provider.ensure_network(&NetworkSpec {
        name: EGRESS_NETWORK.to_owned(),
        internal: false,
        labels: Labels::new(),
    })?;
    provider.ensure_network(&NetworkSpec {
        name: name.network(),
        internal: true,
        labels: project_labels.clone(),
    })?;

    let container = name.gateway();
    let hash = GATEWAY_IMAGE.context_hash();
    let mut started = true;
    match provider.container(&container)? {
        Some(info) if info.labels.get(CONTEXT_HASH_LABEL) == Some(&hash) => {
            if info.status == ContainerStatus::Running {
                started = false;
            } else {
                provider.start_container(&container)?;
            }
        }
        existing => {
            if existing.is_some() {
                provider.remove_container(&container)?;
            }
            let mut labels = project_labels;
            labels.insert(CONTEXT_HASH_LABEL.to_owned(), hash);
            provider.create_gateway(&GatewaySpec {
                name: container.clone(),
                image: GATEWAY_IMAGE.tag.to_owned(),
                internal_network: name.network(),
                alias: GATEWAY_ALIAS.to_owned(),
                egress_network: EGRESS_NETWORK.to_owned(),
                labels,
                command: Vec::new(),
            })?;
            provider.start_container(&container)?;
        }
    }

    let control = GatewayControl::new(provider, name);
    if started {
        // Right after a start the gateway may not listen on its socket yet.
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        while let Err(error) = control.send(&[Request::Ping]) {
            if Instant::now() >= deadline || !matches!(error, Error::Gateway(_)) {
                return Err(error);
            }
            thread::sleep(Duration::from_millis(300));
        }
    }
    // One call for both: the token goes through stdin anyway.
    control
        .send_private(&[Request::Policy(policy(project)), Request::Llm(llm.cloned())])
        .map(drop)
}

/// Stops the project's gateway; its approvals and refusals are forgotten.
pub fn stop(provider: &dyn Provider, name: &ProjectName) -> Result<()> {
    provider.stop_container(&name.gateway())
}

/// Requests to a running gateway.
pub struct GatewayControl<'a> {
    provider: &'a dyn Provider,
    container: String,
}

impl<'a> GatewayControl<'a> {
    /// Control of the named project's gateway.
    pub fn new(provider: &'a dyn Provider, name: &ProjectName) -> Self {
        GatewayControl {
            provider,
            container: name.gateway(),
        }
    }

    fn ctl(&self, requests: &[Request]) -> ExecSpec {
        let mut command = vec!["harness-gateway".to_owned(), "ctl".to_owned()];
        command.extend(requests.iter().map(ToString::to_string));
        ExecSpec {
            container: self.container.clone(),
            command,
            ..ExecSpec::default()
        }
    }

    /// Sends requests in one `podman exec` and returns the data lines of
    /// all answers. Any `error` answer fails the whole call.
    pub fn send(&self, requests: &[Request]) -> Result<Vec<String>> {
        let output = self.provider.exec_output(&self.ctl(requests))?;
        let lines: Vec<&str> = output.stdout.lines().collect();
        collect_answers(&lines, requests.len(), || {
            let detail = sanitize(output.stderr.trim());
            if detail.is_empty() {
                format!("keine vollständige Antwort (Exit-Code {})", output.code)
            } else {
                detail
            }
        })
    }

    /// Like [`GatewayControl::send`], but the requests go through stdin.
    /// For requests carrying secrets: arguments of `podman exec` end up in
    /// the VM's process list and podman's event log, stdin does not.
    pub fn send_private(&self, requests: &[Request]) -> Result<Vec<String>> {
        let mut child = self.provider.exec_piped(&self.ctl(&[]))?;
        let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            return Err(Error::Gateway("keine Verbindung zu ctl".to_owned()));
        };
        let mut text = String::new();
        for request in requests.iter().chain([&Request::Bye]) {
            text.push_str(&format!("{request}\n"));
        }
        // Everything is written before reading, and the gateway closes the
        // session after `bye`, which ends `ctl` and with it stdout.
        let written = stdin
            .write_all(text.as_bytes())
            .and_then(|()| stdin.flush());
        drop(stdin);
        let lines: Vec<String> = BufReader::new(stdout)
            .lines()
            .map_while(|l| l.ok())
            .collect();
        let status = child.wait();
        if let Err(error) = written {
            return Err(Error::Gateway(format!("Senden fehlgeschlagen: {error}")));
        }
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        collect_answers(&lines, requests.len(), || {
            format!(
                "keine vollständige Antwort ({})",
                status.map_or_else(|e| e.to_string(), |s| s.to_string())
            )
        })
    }

    /// Sets (or with `None` clears) where `llm.harness` forwards to,
    /// including the device token.
    pub fn set_llm(&self, upstream: Option<&LlmUpstream>) -> Result<()> {
        self.send_private(&[Request::Llm(upstream.cloned())])
            .map(drop)
    }

    /// Replaces the gateway's rules.
    pub fn apply(&self, policy: &Policy) -> Result<()> {
        self.send(&[Request::Policy(policy.clone())]).map(drop)
    }

    /// Allows a domain until the gateway stops or for some seconds.
    pub fn allow(&self, pattern: &DomainPattern, scope: Scope) -> Result<()> {
        self.send(&[Request::Allow {
            pattern: pattern.clone(),
            scope,
        }])
        .map(drop)
    }

    /// Blocks a domain until the gateway stops.
    pub fn deny(&self, pattern: &DomainPattern) -> Result<()> {
        self.send(&[Request::Deny(pattern.clone())]).map(drop)
    }

    /// Rules, approvals, open questions and the recent log.
    pub fn status(&self) -> Result<Status> {
        let lines = self.send(&[Request::Status])?;
        Status::from_lines(&lines).map_err(Error::Gateway)
    }

    /// Opens a long-lived connection that receives events and can answer
    /// questions. While it is open, requests to unknown hosts wait for an
    /// answer (if the project asks).
    pub fn watch(&self) -> Result<Watch> {
        let mut child = self.provider.exec_piped(&self.ctl(&[]))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            return Err(Error::Gateway("keine Verbindung zu ctl".to_owned()));
        };
        let (tx, replies) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(Reply::parse(&line)).is_err() {
                    break;
                }
            }
        });
        let mut watch = Watch {
            child,
            stdin,
            replies,
        };
        watch.send(&Request::Watch)?;
        Ok(watch)
    }
}

/// Data lines of the answers to `expected` requests plus the `bye` that
/// `ctl` appends; any `error` answer or a missing answer fails.
fn collect_answers(
    lines: &[&str],
    expected: usize,
    incomplete: impl FnOnce() -> String,
) -> Result<Vec<String>> {
    let mut data = Vec::new();
    let mut answered = 0;
    for line in lines {
        match Reply::parse(line) {
            Reply::Ok => answered += 1,
            Reply::Error(message) => return Err(Error::Gateway(sanitize(&message))),
            Reply::Data(line) => data.push(line),
            Reply::Event(_) => {}
        }
    }
    if answered != expected + 1 {
        return Err(Error::Gateway(incomplete()));
    }
    Ok(data)
}

/// A watching control connection; see [`GatewayControl::watch`].
pub struct Watch {
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<Reply>,
}

impl Watch {
    /// Seconds between pings, well below the gateway's idle limit.
    pub const PING_INTERVAL: Duration = Duration::from_secs(15);

    fn send(&mut self, request: &Request) -> Result<()> {
        writeln!(self.stdin, "{request}")
            .and_then(|()| self.stdin.flush())
            .map_err(|e| Error::Gateway(format!("Verbindung unterbrochen: {e}")))
    }

    /// Answers a question: `Some(scope)` allows, `None` refuses.
    pub fn decide(&mut self, id: u64, allow: Option<Scope>) -> Result<()> {
        self.send(&Request::Decide { id, allow })
    }

    /// Tells the gateway this watcher is still there.
    pub fn ping(&mut self) -> Result<()> {
        self.send(&Request::Ping)
    }

    /// The next line from the gateway, or `None` after `timeout`. An error
    /// means the connection ended.
    pub fn next(&self, timeout: Duration) -> Result<Option<Reply>> {
        match self.replies.recv_timeout(timeout) {
            Ok(reply) => Ok(Some(reply)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err(Error::Gateway(
                "Verbindung zum Gateway beendet (gestoppt?)".to_owned(),
            )),
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // `bye` lets the gateway close the session, which ends `ctl` and the
        // exec inside the VM; killing only the local podman might leave it
        // running until the idle timeout.
        let _ = self.send(&Request::Bye);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::FileMode;
    use crate::provider::Resources;

    fn project(network: NetworkPreset) -> Project {
        Project {
            name: ProjectName::new("demo").unwrap(),
            file: PathBuf::from("demo.toml"),
            path: PathBuf::from("/repo"),
            harness: "pi".into(),
            mode: FileMode::Isolated,
            network,
            ask: true,
            allow: vec!["docs.rs".parse().unwrap(), "github.com".parse().unwrap()],
            deny: vec!["gist.github.com".parse().unwrap()],
            llm_apis: vec!["api.deepseek.com".parse().unwrap()],
            resources: Resources::default(),
        }
    }

    fn allows(policy: &Policy, domain: &str) -> bool {
        policy.allow.iter().any(|p| p.matches(domain))
    }

    #[test]
    fn presets_become_policies() {
        let off = policy(&project(NetworkPreset::Off));
        assert_eq!(off.mode, Mode::Allowlist);
        assert_eq!(off.allow.len(), 1);
        assert!(allows(&off, "api.deepseek.com"));
        assert!(!allows(&off, "docs.rs"));
        assert!(off.ask);

        let dev = policy(&project(NetworkPreset::Development));
        for domain in [
            "api.deepseek.com",
            "registry.npmjs.org",
            "files.pythonhosted.org",
            "codeload.github.com",
            "objects.githubusercontent.com",
            "docs.rs",
        ] {
            assert!(allows(&dev, domain), "{domain}");
        }
        // github.com is in the preset and in `allow`; listed once.
        let github: DomainPattern = "github.com".parse().unwrap();
        assert_eq!(dev.allow.iter().filter(|p| **p == github).count(), 1);
        assert_eq!(dev.deny, project(NetworkPreset::Development).deny);

        let custom = policy(&project(NetworkPreset::Custom));
        assert!(allows(&custom, "docs.rs"));
        assert!(!allows(&custom, "registry.npmjs.org"));

        let open = policy(&project(NetworkPreset::Open));
        assert_eq!(open.mode, Mode::Open);
        assert_eq!(open.deny.len(), 1);
    }

    #[test]
    fn development_domains_are_valid() {
        assert_eq!(development_domains().len(), DEVELOPMENT_DOMAINS.len());
    }
}
