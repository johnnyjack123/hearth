use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use super::*;

/// A project folder with a few files, a folder outside it and hearth's
/// configuration next to them.
struct Fixture {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    outside: PathBuf,
    hearth: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("project");
        let outside = tmp.path().join("elsewhere");
        let hearth = tmp.path().join("hearth-config");
        for dir in [&root, &outside, &hearth] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("src/main.rs"),
            "fn main() {\n    println!(\"hi\");\n}\n",
        )
        .unwrap();
        fs::write(root.join(".env"), "TOKEN=secret-value\n").unwrap();
        fs::write(root.join(".env.example"), "TOKEN=\n").unwrap();
        fs::write(outside.join("notes.txt"), "far away\n").unwrap();
        Fixture {
            _tmp: tmp,
            root,
            outside,
            hearth,
        }
    }

    fn session(&self) -> Session {
        let policy = Policy::new(&self.root, std::slice::from_ref(&self.hearth), false).unwrap();
        Session::new(policy, Shell::detect())
    }
}

fn run(session: &Session, call: &ToolCall) -> Output {
    session.execute(call, &AtomicBool::new(false))
}

fn read(path: &str) -> ToolCall {
    ToolCall::Read {
        path: path.to_owned(),
        offset: None,
        limit: None,
    }
}

fn write(path: &str, content: &str) -> ToolCall {
    ToolCall::Write {
        path: path.to_owned(),
        content: content.to_owned(),
    }
}

#[test]
fn the_project_folder_is_free_and_everything_else_asks() {
    let f = Fixture::new();
    let s = f.session();
    assert_eq!(s.plan(&read("src/main.rs")), Plan::Run);
    assert_eq!(s.plan(&write("src/new.rs", "x")), Plan::Run);
    assert_eq!(s.plan(&ToolCall::List { path: None }), Plan::Run);

    let outside = f.outside.join("notes.txt").display().to_string();
    assert!(
        matches!(s.plan(&read(&outside)), Plan::Ask(Request::Read { ref reason, .. }) if reason == "outside")
    );
    assert!(matches!(
        s.plan(&write(&outside, "x")),
        Plan::Ask(Request::Write { .. })
    ));
    // `..` climbs out of the project.
    assert!(matches!(
        s.plan(&read("../elsewhere/notes.txt")),
        Plan::Ask(_)
    ));
    assert!(matches!(
        s.plan(&read("src/../../elsewhere/notes.txt")),
        Plan::Ask(_)
    ));
    // Not a way back in either: still the project after resolving.
    assert_eq!(s.plan(&read("src/../src/main.rs")), Plan::Run);
}

#[test]
fn secrets_ask_to_be_read_and_hooks_ask_to_be_written() {
    let f = Fixture::new();
    let s = f.session();
    assert!(
        matches!(s.plan(&read(".env")), Plan::Ask(Request::Read { ref reason, .. }) if reason == "secret")
    );
    assert_eq!(s.plan(&read(".env.example")), Plan::Run);
    assert!(matches!(s.plan(&read("deploy/server.KEY")), Plan::Ask(_)));
    assert_eq!(s.plan(&read("id_rsa.pub")), Plan::Run);

    for protected in [
        ".git/hooks/pre-commit",
        ".git/config",
        ".GIT/hooks/post-merge",
        ".husky/pre-commit",
        ".vscode/tasks.json",
        "sub/.vscode/settings.json",
        ".claude/settings.json",
        ".pi/extensions/x.ts",
        ".gitattributes",
        ".mcp.json",
        ".envrc",
        "docs/desktop.ini",
        "start.lnk",
    ] {
        assert!(
            matches!(s.plan(&write(protected, "x")), Plan::Ask(Request::Write { ref reason, .. }) if reason == "protected"),
            "{protected} should ask"
        );
        // Reading them is fine.
        assert_eq!(s.plan(&read(protected)), Plan::Run, "{protected}");
    }
    // Look-alikes stay free.
    for free in [
        "src/gitstuff.rs",
        "my.git.txt",
        "vscode/readme.md",
        "src/envrc.rs",
    ] {
        assert_eq!(s.plan(&write(free, "x")), Plan::Run, "{free}");
    }
}

#[test]
fn hearth_itself_is_never_written_and_odd_paths_are_refused() {
    let f = Fixture::new();
    let s = f.session();
    let config = f.hearth.join("projects/x.toml").display().to_string();
    assert!(matches!(s.plan(&write(&config, "x")), Plan::Refuse(_)));
    // Reading it asks like any other place outside.
    assert!(matches!(s.plan(&read(&config)), Plan::Ask(_)));

    for odd in [
        r"\\server\share\x",
        "//server/share/x",
        r"\\?\C:\x",
        r"\\.\pipe\x",
        "file.txt:hidden",
        "",
    ] {
        assert!(matches!(s.plan(&read(odd)), Plan::Refuse(_)), "{odd:?}");
    }
    #[cfg(windows)]
    assert!(matches!(
        s.plan(&write("src/con.txt", "x")),
        Plan::Refuse(_)
    ));
}

#[test]
fn writes_can_ask_everywhere() {
    let f = Fixture::new();
    let policy = Policy::new(&f.root, &[], true).unwrap();
    let s = Session::new(policy, Shell::detect());
    assert!(
        matches!(s.plan(&write("src/new.rs", "x")), Plan::Ask(Request::Write { ref reason, .. }) if reason == "writes")
    );
    assert_eq!(s.plan(&read("src/main.rs")), Plan::Run);
}

#[test]
fn a_link_inside_the_project_that_points_out_counts_as_outside() {
    let f = Fixture::new();
    let link = f.root.join("shortcut");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&f.outside, &link).unwrap();
    #[cfg(windows)]
    {
        // A junction needs no rights, unlike a symbolic link.
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&f.outside)
            .output()
            .unwrap();
        assert!(made.status.success(), "{made:?}");
    }
    let s = f.session();
    assert!(
        matches!(s.plan(&read("shortcut/notes.txt")), Plan::Ask(Request::Read { ref reason, .. }) if reason == "outside")
    );
    assert!(matches!(
        s.plan(&write("shortcut/new.txt", "x")),
        Plan::Ask(Request::Write { .. })
    ));
    // Searching the project does not wander through it.
    let found = run(
        &s,
        &ToolCall::Find {
            pattern: "*.txt".to_owned(),
            path: None,
        },
    );
    assert!(!found.text.contains("notes.txt"), "{}", found.text);
}

#[test]
fn chat_wide_answers_cover_the_same_command_and_the_same_folder() {
    let f = Fixture::new();
    let mut s = f.session();
    let command = ToolCall::Shell {
        command: "echo hi".to_owned(),
        timeout: None,
    };
    let Plan::Ask(request) = s.plan(&command) else {
        panic!("commands always ask");
    };
    assert!(request.chat_wide());
    s.grant(&request, Decision::Once);
    assert!(matches!(s.plan(&command), Plan::Ask(_)), "once is once");
    s.grant(&request, Decision::Chat);
    assert_eq!(s.plan(&command), Plan::Run);
    // Another command text asks again.
    assert!(matches!(
        s.plan(&ToolCall::Shell {
            command: "echo hi; rm -rf x".to_owned(),
            timeout: None
        }),
        Plan::Ask(_)
    ));

    let folder = f.outside.display().to_string();
    let listing = ToolCall::List {
        path: Some(folder.clone()),
    };
    let Plan::Ask(request) = s.plan(&listing) else {
        panic!("outside asks");
    };
    s.grant(&request, Decision::Chat);
    assert_eq!(s.plan(&listing), Plan::Run);
    assert_eq!(
        s.plan(&read(&f.outside.join("notes.txt").display().to_string())),
        Plan::Run
    );

    // Writes are never granted for the chat.
    let note = f.outside.join("new.txt").display().to_string();
    let Plan::Ask(request) = s.plan(&write(&note, "x")) else {
        panic!("outside asks");
    };
    assert!(!request.chat_wide());
    s.grant(&request, Decision::Chat);
    assert!(matches!(s.plan(&write(&note, "x")), Plan::Ask(_)));
}

#[test]
fn read_pages_through_long_files() {
    let f = Fixture::new();
    let s = f.session();
    let text: String = (1..=2_500).map(|n| format!("line {n}\n")).collect();
    fs::write(f.root.join("long.txt"), text).unwrap();
    let first = run(&s, &read("long.txt"));
    assert!(!first.is_error);
    assert!(first.text.starts_with("line 1\n"));
    assert!(
        first
            .text
            .contains("[Showing lines 1-2000 of 2500. Use offset=2001 to continue.]"),
        "{}",
        &first.text[first.text.len() - 200..]
    );
    let rest = run(
        &s,
        &ToolCall::Read {
            path: "long.txt".to_owned(),
            offset: Some(2_001),
            limit: Some(10),
        },
    );
    assert!(rest.text.starts_with("line 2001\n"), "{}", rest.text);
    assert!(rest.text.contains("Use offset=2011"));

    fs::write(f.root.join("blob.bin"), [0u8, 1, 2, 3]).unwrap();
    assert!(run(&s, &read("blob.bin")).text.contains("binary file"));
    assert!(run(&s, &read("missing.txt")).is_error);
    assert!(run(&s, &read("src")).text.contains("folder"));
}

#[test]
fn write_and_edit_change_files_exactly() {
    let f = Fixture::new();
    let s = f.session();
    let out = run(&s, &write("deep/new/file.txt", "a\nb\nc\n"));
    assert!(!out.is_error, "{}", out.text);
    assert_eq!(
        fs::read_to_string(f.root.join("deep/new/file.txt")).unwrap(),
        "a\nb\nc\n"
    );

    let edit = |edits: &[(&str, &str)]| ToolCall::Edit {
        path: "deep/new/file.txt".to_owned(),
        edits: edits
            .iter()
            .map(|(old, new)| Replacement {
                old_text: (*old).to_owned(),
                new_text: (*new).to_owned(),
            })
            .collect(),
    };
    let out = run(&s, &edit(&[("b\n", "B\nB2\n"), ("a", "A")]));
    assert!(!out.is_error, "{}", out.text);
    assert_eq!(
        fs::read_to_string(f.root.join("deep/new/file.txt")).unwrap(),
        "A\nB\nB2\nc\n"
    );

    assert!(run(&s, &edit(&[("zzz", "y")])).text.contains("not found"));
    fs::write(f.root.join("deep/new/file.txt"), "x x\n").unwrap();
    assert!(
        run(&s, &edit(&[("x", "y")]))
            .text
            .contains("more than once")
    );
    assert!(
        run(&s, &edit(&[("x x", "y"), ("x\n", "z")]))
            .text
            .contains("overlap")
    );

    // CRLF files match passages with LF and keep CRLF.
    fs::write(f.root.join("crlf.txt"), "one\r\ntwo\r\nthree\r\n").unwrap();
    let out = run(
        &s,
        &ToolCall::Edit {
            path: "crlf.txt".to_owned(),
            edits: vec![Replacement {
                old_text: "one\ntwo\n".to_owned(),
                new_text: "1\n2\n".to_owned(),
            }],
        },
    );
    assert!(!out.is_error, "{}", out.text);
    assert_eq!(
        fs::read_to_string(f.root.join("crlf.txt")).unwrap(),
        "1\r\n2\r\nthree\r\n"
    );
}

#[test]
fn find_and_grep_respect_gitignore_and_skip_secrets() {
    let f = Fixture::new();
    let s = f.session();
    fs::write(f.root.join(".gitignore"), "target/\n").unwrap();
    fs::create_dir_all(f.root.join("target")).unwrap();
    fs::write(f.root.join("target/out.rs"), "fn built() {}\n").unwrap();
    fs::create_dir_all(f.root.join(".git")).unwrap();
    fs::write(f.root.join(".git/HEAD.rs"), "fn hidden() {}\n").unwrap();

    let found = run(
        &s,
        &ToolCall::Find {
            pattern: "*.rs".to_owned(),
            path: None,
        },
    );
    assert_eq!(found.text, "src/main.rs");

    let grep = |pattern: &str| {
        run(
            &s,
            &ToolCall::Grep {
                pattern: pattern.to_owned(),
                path: None,
                glob: None,
                ignore_case: false,
                literal: false,
            },
        )
    };
    let hits = grep(r"println!\(");
    assert!(hits.text.starts_with("src/main.rs:2:"), "{}", hits.text);
    // The secret in .env is not searched, and the model is told.
    let hits = grep("secret-value");
    assert!(hits.text.contains("No matches."), "{}", hits.text);
    assert!(hits.text.contains("look like secrets"), "{}", hits.text);
    assert!(grep("built").text.contains("No matches."));
    assert!(grep("(").is_error);

    let literal = run(
        &s,
        &ToolCall::Grep {
            pattern: "println!(".to_owned(),
            path: Some("src".to_owned()),
            glob: Some("*.rs".to_owned()),
            ignore_case: true,
            literal: true,
        },
    );
    assert!(
        literal.text.starts_with("src/main.rs:2:"),
        "{}",
        literal.text
    );
}

#[test]
fn list_shows_folders_first() {
    let f = Fixture::new();
    let s = f.session();
    let out = run(&s, &ToolCall::List { path: None });
    let lines: Vec<&str> = out.text.lines().collect();
    assert_eq!(lines[0], "src/");
    assert!(
        lines.iter().any(|line| line.starts_with(".env  ")),
        "{}",
        out.text
    );
}

#[test]
fn commands_run_in_the_project_with_their_exit_code() {
    let f = Fixture::new();
    let s = f.session();
    let out = run(
        &s,
        &ToolCall::Shell {
            command: "echo hello".to_owned(),
            timeout: None,
        },
    );
    assert!(!out.is_error, "{}", out.text);
    assert!(out.text.contains("hello"), "{}", out.text);
    assert_eq!(out.exit_code, Some(0));

    let out = run(
        &s,
        &ToolCall::Shell {
            command: "exit 3".to_owned(),
            timeout: None,
        },
    );
    assert!(out.is_error);
    assert_eq!(out.exit_code, Some(3));
    assert!(out.text.contains("[Exit code 3]"), "{}", out.text);

    // The working folder is the project.
    let out = run(
        &s,
        &ToolCall::Shell {
            command: "ls".to_owned(),
            timeout: None,
        },
    );
    assert!(out.text.contains("src"), "{}", out.text);
}

#[test]
fn a_command_ends_at_its_time_limit_or_when_cancelled() {
    let f = Fixture::new();
    let s = f.session();
    let started = Instant::now();
    let out = run(
        &s,
        &ToolCall::Shell {
            command: "sleep 30".to_owned(),
            timeout: Some(1),
        },
    );
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "{:?}",
        started.elapsed()
    );
    assert!(out.is_error);
    assert_eq!(out.exit_code, None);
    assert!(out.text.contains("time limit"), "{}", out.text);

    let cancel = AtomicBool::new(true);
    let out = s.execute(
        &ToolCall::Shell {
            command: "sleep 30".to_owned(),
            timeout: None,
        },
        &cancel,
    );
    assert!(out.text.contains("Cancelled"), "{}", out.text);
}

#[test]
fn hearth_variables_stay_out_of_commands() {
    let f = Fixture::new();
    let s = f.session();
    // SAFETY: tests in this module do not read this variable concurrently.
    unsafe { std::env::set_var("HEARTH_TEST_SECRET", "do-not-leak") };
    let out = run(
        &s,
        &ToolCall::Shell {
            command: if s.shell().name.contains("PowerShell") {
                "echo \"[$env:HEARTH_TEST_SECRET]\"".to_owned()
            } else {
                "echo \"[$HEARTH_TEST_SECRET]\"".to_owned()
            },
            timeout: None,
        },
    );
    assert!(out.text.contains("[]"), "{}", out.text);
}

#[test]
fn bounded_output_keeps_start_and_end() {
    let mut bounded_input = String::new();
    for n in 0..20_000 {
        bounded_input.push_str(&format!("{n:06}\n"));
    }
    let f = Fixture::new();
    fs::write(f.root.join("big.txt"), &bounded_input).unwrap();
    let s = f.session();
    let command = if s.shell().name.contains("PowerShell") {
        "Get-Content big.txt"
    } else {
        "cat big.txt"
    };
    let out = run(
        &s,
        &ToolCall::Shell {
            command: command.to_owned(),
            timeout: None,
        },
    );
    assert!(out.text.starts_with("000000"), "{}", &out.text[..50]);
    assert!(out.text.contains("019999"));
    assert!(out.text.contains("bytes of output left out"));
    assert!(out.text.len() < shell::MAX_OUTPUT + 500);
}

#[test]
fn the_record_keeps_entries_per_project() {
    let tmp = tempfile::tempdir().unwrap();
    let mut entry = audit::Entry::now("chat-1", "shell", "cargo test", "once", "app");
    entry.exit = Some(0);
    audit::record(tmp.path(), "demo", &entry).unwrap();
    audit::record(
        tmp.path(),
        "demo",
        &audit::Entry::now("chat-1", "read", "/etc/passwd", "deny", "app"),
    )
    .unwrap();
    let recent = audit::recent(tmp.path(), "demo", 10);
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0], entry);
    assert!(audit::recent(tmp.path(), "other", 10).is_empty());
    assert!(Path::new(&audit::path(tmp.path(), "demo")).exists());
}

#[test]
fn calls_parse_as_the_server_sends_them() {
    let call: ToolCall =
        serde_json::from_str(r#"{"tool":"grep","input":{"pattern":"x","ignoreCase":true}}"#)
            .unwrap();
    assert_eq!(
        call,
        ToolCall::Grep {
            pattern: "x".to_owned(),
            path: None,
            glob: None,
            ignore_case: true,
            literal: false
        }
    );
    let call: ToolCall = serde_json::from_str(
        r#"{"tool":"edit","input":{"path":"a","edits":[{"oldText":"x","newText":"y"}]}}"#,
    )
    .unwrap();
    assert_eq!(call.name(), "edit");
    assert!(serde_json::from_str::<ToolCall>(r#"{"tool":"exec","input":{}}"#).is_err());
}
