use std::sync::mpsc::Receiver;

use super::*;

fn gateway() -> Gateway {
    Gateway::new(Options::default())
}

fn host(text: &str) -> Host {
    Host::parse(text).unwrap()
}

fn patterns(list: &[&str]) -> Vec<DomainPattern> {
    list.iter().map(|p| p.parse().unwrap()).collect()
}

fn apply(gateway: &Gateway, request: &str) -> Vec<String> {
    let (tx, _rx) = mpsc::sync_channel(10);
    gateway.handle(request.parse().unwrap(), &tx, &mut None)
}

/// A registered watcher and the channel its events arrive on.
fn watch(gateway: &Gateway) -> (u64, SyncSender<String>, Receiver<String>) {
    let (tx, rx) = mpsc::sync_channel(100);
    let mut id = None;
    assert_eq!(gateway.handle(Request::Watch, &tx, &mut id), ["ok"]);
    (id.unwrap(), tx, rx)
}

fn verdict(gateway: &Gateway, target: &str) -> Result<Reason, Reason> {
    match gateway.check(&host(target), 443) {
        Verdict::Allow(reason) => Ok(reason),
        Verdict::Deny(reason) => Err(reason),
        Verdict::Ask(_) => panic!("{target} unexpectedly asks"),
    }
}

#[test]
fn blocks_everything_until_a_policy_arrives() {
    let gateway = gateway();
    assert_eq!(verdict(&gateway, "example.com"), Err(Reason::NotAllowed));
    assert_eq!(verdict(&gateway, "1.1.1.1"), Err(Reason::IpLiteral));
}

#[test]
fn allowlist_open_and_deny() {
    let gateway = gateway();
    let policy = Policy {
        mode: Mode::Allowlist,
        ask: false,
        allow: patterns(&["*.npmjs.org", "github.com"]),
        deny: patterns(&["evil.npmjs.org"]),
    };
    assert_eq!(apply(&gateway, &format!("policy {policy}")), ["ok"]);
    assert_eq!(
        verdict(&gateway, "registry.npmjs.org"),
        Ok(Reason::Allowlist)
    );
    assert_eq!(verdict(&gateway, "GitHub.com."), Ok(Reason::Allowlist));
    assert_eq!(verdict(&gateway, "evil.npmjs.org"), Err(Reason::DenyList));
    assert_eq!(verdict(&gateway, "api.github.com"), Err(Reason::NotAllowed));
    assert_eq!(verdict(&gateway, "140.82.121.4"), Err(Reason::IpLiteral));

    let open = Policy {
        mode: Mode::Open,
        ..policy
    };
    apply(&gateway, &format!("policy {open}"));
    assert_eq!(verdict(&gateway, "example.com"), Ok(Reason::Open));
    assert_eq!(verdict(&gateway, "140.82.121.4"), Ok(Reason::Open));
    // Deny wins in every mode.
    assert_eq!(verdict(&gateway, "evil.npmjs.org"), Err(Reason::DenyList));
}

#[test]
fn llm_upstreams() {
    let gateway = gateway();
    let token = "abcdefghijklmnop0123";
    assert_eq!(
        apply(
            &gateway,
            &format!("llm http://192.168.127.254:8080/x {token}")
        ),
        ["ok"]
    );
    let url = |gateway: &Gateway| gateway.llm().map(|(upstream, _)| upstream.url());
    assert_eq!(
        url(&gateway).as_deref(),
        Some("http://192.168.127.254:8080/x")
    );

    let https = apply(&gateway, &format!("llm https://hearth.example/x {token}"));
    if cfg!(feature = "tls") {
        assert_eq!(https, ["ok"]);
        assert_eq!(url(&gateway).as_deref(), Some("https://hearth.example/x"));
    } else {
        // Never plain HTTP instead, and not the old server either.
        assert!(
            https[0].starts_with("error ") && https[0].contains("TLS"),
            "{https:?}"
        );
        assert_eq!(url(&gateway), None);
    }

    assert_eq!(apply(&gateway, "llm none"), ["ok"]);
    assert_eq!(url(&gateway), None);
}

#[test]
fn grants_expire_and_deny_list_wins() {
    let gateway = gateway();
    apply(&gateway, "policy allowlist noask - blocked.example");
    assert_eq!(apply(&gateway, "allow docs.rs session"), ["ok"]);
    assert_eq!(apply(&gateway, "allow *.example.org 3600"), ["ok"]);
    assert_eq!(verdict(&gateway, "docs.rs"), Ok(Reason::Grant));
    assert_eq!(verdict(&gateway, "a.example.org"), Ok(Reason::Grant));
    assert_eq!(verdict(&gateway, "example.org"), Err(Reason::NotAllowed));

    let refused = apply(&gateway, "allow blocked.example session");
    assert!(refused[0].starts_with("error "), "{refused:?}");
    assert!(apply(&gateway, "allow x.example once")[0].starts_with("error "));

    // An expired grant no longer counts and disappears from the status.
    gateway.lock().grants.push(Grant {
        pattern: "old.example".parse().unwrap(),
        until: Some(Instant::now() - Duration::from_secs(1)),
    });
    assert_eq!(verdict(&gateway, "old.example"), Err(Reason::NotAllowed));
    let status = gateway.lock().status(Instant::now());
    assert_eq!(status.grants.len(), 2);
    assert!(status.grants.iter().any(|g| g.remaining_secs.is_none()));
    assert!(
        status
            .grants
            .iter()
            .any(|g| g.remaining_secs.is_some_and(|s| s <= 3600))
    );

    // deny revokes a grant for the session.
    assert_eq!(apply(&gateway, "deny docs.rs"), ["ok"]);
    assert_eq!(verdict(&gateway, "docs.rs"), Err(Reason::DeniedByUser));
    // ... and a later allow lifts the refusal again.
    apply(&gateway, "allow docs.rs 60");
    assert_eq!(verdict(&gateway, "docs.rs"), Ok(Reason::Grant));
}

#[test]
fn asks_only_while_someone_watches() {
    let gateway = gateway();
    apply(&gateway, "policy allowlist ask - -");
    assert_eq!(verdict(&gateway, "example.com"), Err(Reason::NotAllowed));

    let (id, _tx, rx) = watch(&gateway);
    let Verdict::Ask(first) = gateway.check(&host("example.com"), 443) else {
        panic!("expected a question");
    };
    assert_eq!(rx.try_recv().unwrap(), "event pending 1 example.com 443");
    // A second request to the same host joins the open question.
    let Verdict::Ask(second) = gateway.check(&host("example.com"), 443) else {
        panic!("expected a question");
    };
    assert_eq!(first.id, second.id);
    assert!(rx.try_recv().is_err());

    // The last watcher leaving answers open questions with "unanswered".
    gateway.lock().remove_watcher(id);
    assert_eq!(gateway.await_decision(&first), (false, Reason::Unanswered));
    assert_eq!(verdict(&gateway, "example.com"), Err(Reason::NotAllowed));
}

#[test]
fn decisions() {
    let gateway = gateway();
    apply(&gateway, "policy allowlist ask - -");
    let (_id, tx, rx) = watch(&gateway);
    let ask = |target: &str| match gateway.check(&host(target), 443) {
        Verdict::Ask(pending) => pending,
        _ => panic!("{target} does not ask"),
    };
    let decide = |line: &str| gateway.handle(line.parse().unwrap(), &tx, &mut None);

    // once: only the waiting request, no grant.
    let pending = ask("a.example");
    let waiter = {
        let gateway = gateway.clone();
        let pending = pending.clone();
        thread::spawn(move || gateway.await_decision(&pending))
    };
    assert_eq!(decide(&format!("decide {} allow once", pending.id)), ["ok"]);
    assert_eq!(waiter.join().unwrap(), (true, Reason::Approved));
    let _ = ask("a.example");

    // session: a grant for the host; open questions it covers resolve.
    let pending = ask("b.example");
    decide(&format!("decide {} allow session", pending.id));
    assert_eq!(gateway.await_decision(&pending), (true, Reason::Approved));
    assert_eq!(verdict(&gateway, "b.example"), Ok(Reason::Grant));

    // deny: refused for the session, without asking again.
    let pending = ask("c.example");
    decide(&format!("decide {} deny", pending.id));
    assert_eq!(
        gateway.await_decision(&pending),
        (false, Reason::DeniedByUser)
    );
    assert_eq!(verdict(&gateway, "c.example"), Err(Reason::DeniedByUser));

    // Unknown or already answered ids are errors.
    assert!(decide(&format!("decide {} deny", pending.id))[0].starts_with("error "));

    // An allow from elsewhere answers the matching open question.
    let pending = ask("x.d.example");
    apply(&gateway, "allow *.d.example 60");
    assert_eq!(gateway.await_decision(&pending), (true, Reason::Grant));

    let events: Vec<String> = rx.try_iter().collect();
    assert!(events.contains(&format!("event resolved {} allowed grant", pending.id)));
    assert!(events.iter().any(|e| e.ends_with("blocked denied-by-user")));
}

#[test]
fn questions_time_out() {
    let gateway = Gateway::new(Options {
        ask_timeout: Duration::from_millis(50),
        ..Options::default()
    });
    apply(&gateway, "policy allowlist ask - -");
    let (_id, _tx, _rx) = watch(&gateway);
    let Verdict::Ask(pending) = gateway.check(&host("slow.example"), 443) else {
        panic!("expected a question");
    };
    assert_eq!(
        gateway.await_decision(&pending),
        (false, Reason::Unanswered)
    );
    assert!(gateway.lock().pending.is_empty());
}

#[test]
fn watch_reports_open_questions_and_the_log() {
    let gateway = gateway();
    apply(&gateway, "policy allowlist ask - -");
    let (_first, _tx1, _rx1) = watch(&gateway);
    let _ = gateway.check(&host("early.example"), 8443);

    let (tx, rx) = mpsc::sync_channel(10);
    let mut id = None;
    assert_eq!(
        gateway.handle(Request::Watch, &tx, &mut id),
        ["ok", "event pending 1 early.example 8443"]
    );
    gateway.record(true, &host("example.com"), 443, Reason::Open);
    let event = rx.recv().unwrap();
    assert!(
        event.starts_with("event log ") && event.ends_with(" allowed example.com 443 open"),
        "{event}"
    );

    let status = apply(&gateway, "status");
    assert_eq!(status.last().unwrap(), "ok");
    let parsed = Status::from_lines(&status[..status.len() - 1]).unwrap();
    assert_eq!(parsed.watchers, 2);
    assert_eq!(parsed.pending.len(), 1);
    assert_eq!(parsed.log.len(), 1);
}

#[test]
fn log_is_bounded() {
    let gateway = Gateway::new(Options {
        log_capacity: 3,
        ..Options::default()
    });
    for port in 1..=5 {
        gateway.record(false, &host("example.com"), port, Reason::NotAllowed);
    }
    let log = &gateway.lock().log;
    assert_eq!(log.len(), 3);
    assert_eq!(log.front().unwrap().port, 3);
}

#[test]
fn control_sessions() {
    let gateway = gateway();
    let input = "policy open noask - -\nnonsense\n\nstatus\nbye\nstatus\n";
    let output = Arc::new(Mutex::new(Vec::new()));
    gateway.serve_control(input.as_bytes(), SharedBuffer(output.clone()));
    let text = String::from_utf8(output.lock().unwrap().clone()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "ok");
    assert!(lines[1].starts_with("error unbekannter Befehl"), "{text}");
    assert_eq!(lines[2], "policy open noask - -");
    // Nothing after bye is processed.
    assert_eq!(lines.last(), Some(&"ok"));
    assert_eq!(lines.iter().filter(|l| l.starts_with("policy")).count(), 1);

    // A line without end (or over the limit) ends the session.
    let output = Arc::new(Mutex::new(Vec::new()));
    gateway.serve_control("status".as_bytes(), SharedBuffer(output.clone()));
    let text = String::from_utf8(output.lock().unwrap().clone()).unwrap();
    assert!(text.starts_with("error "), "{text}");
}

struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for SharedBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
