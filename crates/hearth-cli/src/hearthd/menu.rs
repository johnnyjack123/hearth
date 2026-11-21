//! What the tray icon says and offers, from what the service wrote: texts
//! of the tooltip and the menu, the menu's actions and the notifications.
//! Plain functions without Win32, so they can be tested.

use std::fs;
use std::path::Path;

use serde_json::Value;

/// What the user can pick in the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Opens a URL or file with its default program.
    Open(String),
    /// Copies text to the clipboard.
    Copy(String),
    /// Asks the service to stop the VM.
    StopVm,
    /// Asks whether to approve the program file request with this id.
    ApproveFile(String),
    /// Opens the desktop app, at a page if given (`hearth-app --page`).
    OpenApp(Option<String>),
    /// Ends the service and the tray until the next sign-in.
    Quit,
}

/// A line of the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// Text; greyed out without an action.
    Item(String, Option<Action>),
    /// A dividing line.
    Separator,
}

/// A notification the tray shows once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// Identifies it, so it is shown only once.
    pub key: String,
    /// The text.
    pub text: String,
    /// Warning rather than information.
    pub warning: bool,
    /// What a click on it opens.
    pub target: Option<String>,
    /// The program file request a click on it asks about.
    pub file: Option<String>,
}

/// A program file the browser asked for (`app-requests/`, written by
/// `hearth`), as the tray shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRequest {
    /// Id for `hearth app approve`.
    pub id: String,
    /// App name it becomes.
    pub name: String,
    /// Name for people, from the browser.
    pub title: String,
    /// The file on this computer.
    pub path: String,
    /// What it is, in German.
    pub kind: String,
    /// Size in bytes.
    pub size: u64,
}

/// What the tray read last.
#[derive(Debug, Default, Clone)]
pub struct View {
    /// The status file of the running service, if it is current.
    pub status: Option<Value>,
    /// Open merge requests: project and id.
    pub held: Vec<(String, String)>,
    /// Program files waiting for approval.
    pub files: Vec<FileRequest>,
    /// Questions of the direct mode (`host-approvals.json`): id and summary.
    pub approvals: Vec<(String, String)>,
    /// The user asked to quit; the service is ending.
    pub quitting: bool,
}

impl View {
    /// Reads the status file of the service with process id `pid` (a file
    /// of an earlier run says nothing about this one) and the open merge
    /// requests in the configuration folder `root`.
    pub fn read(root: &Path, pid: u32) -> View {
        let status = fs::read_to_string(root.join("daemon-status.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .filter(|s| pid != 0 && s.get("pid").and_then(Value::as_u64) == Some(u64::from(pid)));
        let mut held: Vec<(String, String)> = fs::read_dir(root.join("merge-requests"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| fs::read_to_string(entry.path()).ok())
            .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
            .filter_map(|r| {
                Some((
                    r.get("project")?.as_str()?.to_owned(),
                    r.get("id")?.as_str()?.to_owned(),
                ))
            })
            .collect();
        held.sort();
        let mut files: Vec<FileRequest> = fs::read_dir(root.join("app-requests"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| fs::read_to_string(entry.path()).ok())
            .filter_map(|text| serde_json::from_str::<Value>(&text).ok())
            .filter_map(|r| {
                let text = |key: &str| Some(r.get(key)?.as_str()?.to_owned());
                Some(FileRequest {
                    id: text("id")?,
                    name: text("name")?,
                    title: text("title")?,
                    path: text("path")?,
                    kind: text("kind")?,
                    size: r.get("size")?.as_u64()?,
                })
            })
            .collect();
        files.sort_by(|a, b| a.id.cmp(&b.id));
        let approvals = fs::read_to_string(root.join("host-approvals.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<Value>>(&text).ok())
            .unwrap_or_default()
            .iter()
            .filter_map(|q| {
                Some((
                    q.get("id")?.as_str()?.to_owned(),
                    q.get("summary")?.as_str()?.to_owned(),
                ))
            })
            .collect();
        View {
            status,
            held,
            files,
            approvals,
            quitting: false,
        }
    }

    fn str(&self, key: &str) -> Option<&str> {
        self.status.as_ref()?.get(key)?.as_str()
    }

    fn number(&self, key: &str) -> Option<u64> {
        self.status.as_ref()?.get(key)?.as_u64()
    }

    /// Whether the service is connected to the server.
    pub fn connected(&self) -> bool {
        self.str("connection") == Some("connected")
    }

    /// Whether something waits for the user.
    pub fn attention(&self) -> bool {
        !self.questions().is_empty()
            || !self.held.is_empty()
            || !self.files.is_empty()
            || !self.approvals.is_empty()
    }

    fn server(&self) -> Option<&str> {
        self.str("server").filter(|s| s.starts_with("http"))
    }

    /// hearth's version (the service's, else this tray's) and the
    /// server's, once it said.
    fn versions(&self) -> (String, Option<String>) {
        let harness = self
            .str("version")
            .unwrap_or(env!("CARGO_PKG_VERSION"))
            .to_owned();
        (harness, self.str("serverVersion").map(str::to_owned))
    }

    /// Open gateway questions: chat, id, host.
    fn questions(&self) -> Vec<(String, u64, String)> {
        self.status
            .as_ref()
            .and_then(|s| s.get("questions"))
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|q| {
                        Some((
                            q.get("session")?
                                .as_str()
                                .filter(|s| chat_id(s))?
                                .to_owned(),
                            q.get("id")?.as_u64()?,
                            q.get("host")?.as_str()?.to_owned(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Whether `id` looks like a chat id of the server (as
/// `hearth_core::config::valid_chat_id`, which hearthd does not link): it
/// becomes part of a URL the tray hands to Windows.
fn chat_id(id: &str) -> bool {
    (1..=100).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Texts in the user's language: German or English.
#[derive(Debug, Clone, Copy)]
pub struct Language {
    /// Whether Windows runs in German.
    pub german: bool,
}

impl Language {
    /// The language of this process: `HEARTH_LANG` if it names one,
    /// German in tests, otherwise German exactly when `windows_german`
    /// says so. The same rule as `hearth_core::lang`, which hearthd does
    /// not link.
    pub fn current(windows_german: impl FnOnce() -> bool) -> Language {
        Language::select(std::env::var("HEARTH_LANG").ok().as_deref(), || {
            cfg!(test) || windows_german()
        })
    }

    /// The language `value` names (`de`, `en`, also `de-AT` and the like,
    /// in any case), else German exactly when `fallback` says so.
    fn select(value: Option<&str>, fallback: impl FnOnce() -> bool) -> Language {
        let primary = value
            .map(|v| {
                v.trim()
                    .split(['-', '_'])
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase()
            })
            .unwrap_or_default();
        let german = match primary.as_str() {
            "de" => true,
            "en" => false,
            _ => fallback(),
        };
        Language { german }
    }

    /// `de` in German, `en` otherwise.
    pub fn t<'a>(self, de: &'a str, en: &'a str) -> &'a str {
        if self.german { de } else { en }
    }

    /// The connection, e.g. "Verbunden mit pi:8085".
    pub fn connection(self, view: &View) -> String {
        if view.quitting {
            return self.t("Wird beendet …", "Shutting down …").to_owned();
        }
        let host = view
            .server()
            .map(|s| {
                s.trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .to_owned()
            })
            .unwrap_or_default();
        match view.str("connection") {
            Some("connected") => format!("{} {host}", self.t("Verbunden mit", "Connected to")),
            Some("connecting") => {
                format!("{} {host} …", self.t("Verbinde mit", "Connecting to"))
            }
            Some("offline") => format!(
                "{} {}",
                self.t("Nicht verbunden:", "Not connected:"),
                view.str("error").unwrap_or("?")
            ),
            Some("unpaired") => self
                .t(
                    "Nicht gekoppelt (hearth login)",
                    "Not paired (hearth login)",
                )
                .to_owned(),
            _ => self.t("Dienst startet …", "Service starting …").to_owned(),
        }
    }

    /// The VM, e.g. "VM läuft · stoppt im Leerlauf in 12 min"; `now` in
    /// Unix seconds.
    pub fn vm(self, view: &View, now: u64) -> String {
        let busy =
            view.number("replies").unwrap_or(0) > 0 || view.number("terminals").unwrap_or(0) > 0;
        match view.str("vm") {
            Some("running") if busy => self
                .t("VM läuft · in Benutzung", "VM running · in use")
                .to_owned(),
            Some("running") => match view.number("stopAt") {
                Some(at) => format!(
                    "{} {} min",
                    self.t(
                        "VM läuft · stoppt im Leerlauf in",
                        "VM running · idle stop in"
                    ),
                    at.saturating_sub(now).div_ceil(60)
                ),
                None => self.t("VM läuft", "VM running").to_owned(),
            },
            Some("stopped") => self
                .t("VM aus, startet bei Bedarf", "VM off, starts when needed")
                .to_owned(),
            Some("starting") => self.t("VM startet …", "VM starting …").to_owned(),
            Some("stopping") => self.t("VM wird gestoppt …", "VM stopping …").to_owned(),
            _ => self
                .t("VM: Zustand unbekannt", "VM: state unknown")
                .to_owned(),
        }
    }

    /// The tooltip: name, connection and VM (Windows shows 127 characters).
    pub fn tooltip(self, view: &View, now: u64) -> String {
        format!("Hearth\n{}\n{}", self.connection(view), self.vm(view, now))
    }

    /// Both versions, and which side to update when they differ: copying
    /// the command for an older server, the setup program for an older
    /// hearth.
    pub fn versions(self, view: &View) -> (String, Option<Action>) {
        let (harness, server) = view.versions();
        let Some(server) = server else {
            return (format!("hearth {harness}"), None);
        };
        match version_order(&server, &harness) {
            Some(std::cmp::Ordering::Less) => (
                if self.german {
                    format!(
                        "Server {server} älter als hearth {harness} – Befehl zum Aktualisieren kopieren"
                    )
                } else {
                    format!("Server {server} older than hearth {harness} – copy update command")
                },
                Some(Action::Copy(SERVER_UPDATE.to_owned())),
            ),
            Some(std::cmp::Ordering::Greater) => (
                if self.german {
                    format!(
                        "hearth {harness} älter als der Server {server} – neues Einrichtungsprogramm ausführen"
                    )
                } else {
                    format!(
                        "hearth {harness} older than the server {server} – run the new setup program"
                    )
                },
                None,
            ),
            _ => (format!("hearth {harness} · Server {server}"), None),
        }
    }

    /// The menu for `view`; `log` is the service's log file.
    pub fn menu(self, view: &View, log: &Path, now: u64) -> Vec<Entry> {
        let (versions, update) = self.versions(view);
        let mut entries = vec![
            Entry::Item(self.connection(view), None),
            Entry::Item(self.vm(view, now), None),
            Entry::Item(versions, update),
        ];
        let server = view.server().map(str::to_owned);
        for (_, summary) in view.approvals.iter().take(5) {
            entries.push(Entry::Item(
                format!("{} {summary}", self.t("Freigabe:", "Approval:")),
                Some(Action::OpenApp(Some("approvals".to_owned()))),
            ));
        }
        for (session, _, host) in view.questions().into_iter().take(5) {
            entries.push(Entry::Item(
                format!(
                    "{} {host}",
                    self.t("Rückfrage im Chat beantworten:", "Answer question in chat:")
                ),
                server
                    .as_ref()
                    .map(|s| Action::Open(format!("{s}/c/{session}"))),
            ));
        }
        for (project, id) in view.held.iter().take(5) {
            entries.push(Entry::Item(
                if self.german {
                    format!("{project}: Merge angehalten – Befehl kopieren ({id})")
                } else {
                    format!("{project}: merge held – copy command ({id})")
                },
                Some(Action::Copy(format!("hearth merge --approve {id}"))),
            ));
        }
        for file in view.files.iter().take(5) {
            entries.push(Entry::Item(
                if self.german {
                    format!("Programm „{}“ freigeben …", file.title)
                } else {
                    format!("Approve program “{}” …", file.title)
                },
                Some(Action::ApproveFile(file.id.clone())),
            ));
        }
        entries.push(Entry::Separator);
        entries.push(Entry::Item(
            self.t("Hearth öffnen", "Open Hearth").to_owned(),
            Some(Action::OpenApp(None)),
        ));
        entries.push(Entry::Item(
            self.t("Web-App öffnen", "Open web app").to_owned(),
            server.map(Action::Open),
        ));
        let vm_on = matches!(view.str("vm"), Some("running" | "starting"));
        entries.push(Entry::Item(
            self.t("VM stoppen", "Stop VM").to_owned(),
            (vm_on && !view.quitting).then_some(Action::StopVm),
        ));
        entries.push(Entry::Item(
            self.t("Protokoll öffnen", "Open log").to_owned(),
            Some(Action::Open(log.to_string_lossy().into_owned())),
        ));
        entries.push(Entry::Separator);
        entries.push(Entry::Item(
            self.t(
                "Beenden (bis zur nächsten Anmeldung)",
                "Quit (until next sign-in)",
            )
            .to_owned(),
            (!view.quitting).then_some(Action::Quit),
        ));
        entries
    }

    /// Notifications for open questions and held merges; the tray shows
    /// each key once.
    pub fn notices(self, view: &View) -> Vec<Notice> {
        let server = view.server().map(str::to_owned);
        let mut notices: Vec<Notice> = view
            .approvals
            .iter()
            .map(|(id, summary)| Notice {
                key: format!("a:{id}"),
                text: if self.german {
                    format!("Ein Agent wartet auf deine Freigabe – {summary}")
                } else {
                    format!("An agent waits for your approval – {summary}")
                },
                warning: true,
                target: Some(APP_APPROVALS.to_owned()),
                file: None,
            })
            .collect();
        notices.extend(
            view.questions()
                .into_iter()
                .map(|(session, id, host)| Notice {
                    key: format!("q:{session}:{id}"),
                    text: if self.german {
                        format!("Ein Agent möchte {host} erreichen. Im Chat beantworten.")
                    } else {
                        format!("An agent wants to reach {host}. Answer in the chat.")
                    },
                    warning: false,
                    target: server.as_ref().map(|s| format!("{s}/c/{session}")),
                    file: None,
                }),
        );
        notices.extend(view.held.iter().map(|(project, id)| Notice {
            key: format!("h:{id}"),
            text: if self.german {
                format!(
                    "{project}: Merge wegen schwerer Funde angehalten. Freigeben im Terminal: \
                     hearth merge --approve {id}"
                )
            } else {
                format!(
                    "{project}: merge held for severe findings. Approve in a terminal: hearth \
                     merge --approve {id}"
                )
            },
            warning: true,
            target: server.clone(),
            file: None,
        }));
        let (harness, server) = view.versions();
        if let Some(server) = server
            .filter(|server| version_order(server, &harness) == Some(std::cmp::Ordering::Less))
        {
            notices.push(Notice {
                key: format!("v:{server}:{harness}"),
                text: if self.german {
                    format!(
                        "Der Server ({server}) ist älter als hearth ({harness}). Auf dem Server: \
                         {SERVER_UPDATE}"
                    )
                } else {
                    format!("The server ({server}) is older than hearth ({harness}). On the server: {SERVER_UPDATE}")
                },
                warning: true,
                target: None,
                file: None,
            });
        }
        notices.extend(view.files.iter().map(|file| Notice {
            key: format!("f:{}", file.id),
            text: if self.german {
                format!(
                    "Der Browser fragt nach einer Programmdatei dieses Rechners: {}. Zum Prüfen \
                     und Freigeben hier klicken.",
                    file.title
                )
            } else {
                format!(
                    "The browser asks for a program file of this computer: {}. Click to check \
                     and approve it.",
                    file.title
                )
            },
            warning: true,
            target: None,
            file: Some(file.id.clone()),
        }));
        notices
    }

    /// The question before a program file goes into the VM: the file
    /// itself, not only what the browser called it.
    pub fn approval_question(self, file: &FileRequest) -> String {
        let size = file.size as f64 / 1024.0 / 1024.0;
        if self.german {
            format!(
                "Ein Browser mit deinem Konto möchte diese Datei als Programm in die VM \
                 geben:\n\n{}\n{}, {size:.1} MB\nApp: {} („{}“)\n\nAgents in Projekten, die \
                 das Programm einschalten, können die Datei lesen und ausführen. Nur \
                 freigeben, wenn du das selbst angefragt hast.\n\nJa: freigeben · Nein: \
                 ablehnen · Abbrechen: später entscheiden",
                file.path, file.kind, file.name, file.title
            )
        } else {
            format!(
                "A browser signed in to your account wants to put this file into the VM as a \
                 program:\n\n{}\n{}, {size:.1} MB\nApp: {} (“{}”)\n\nAgents in projects that \
                 switch the program on can read and run the file. Only approve it if you \
                 asked for it yourself.\n\nYes: approve · No: refuse · Cancel: decide later",
                file.path, file.kind, file.name, file.title
            )
        }
    }
}

/// The target of a notification that opens the app at its approvals.
pub const APP_APPROVALS: &str = "hearth-app:approvals";

/// What updates the server (Docker Compose, as `docs/deploy.md` sets it up).
const SERVER_UPDATE: &str = "docker compose pull && docker compose up -d";

/// Compares versions like `0.1.15` part by part; anything after `+` or `-`
/// does not count. None if either is no version (`dev`). The same rule as
/// in `connect.rs` and `@hearth/shared`'s `compareVersions`.
fn version_order(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    let parts = |version: &str| -> Option<Vec<u64>> {
        let core = version.trim().trim_start_matches('v');
        let core = core.split(['+', '-']).next().unwrap_or("");
        core.split('.').map(|part| part.parse().ok()).collect()
    };
    let (left, right) = (parts(a)?, parts(b)?);
    let length = left.len().max(right.len());
    let pad = |list: Vec<u64>| list.into_iter().chain(std::iter::repeat(0)).take(length);
    Some(pad(left).cmp(pad(right)))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const DE: Language = Language { german: true };
    const EN: Language = Language { german: false };

    fn view(status: Value) -> View {
        View {
            status: Some(status),
            held: Vec::new(),
            files: Vec::new(),
            approvals: Vec::new(),
            quitting: false,
        }
    }

    fn actions(entries: &[Entry]) -> Vec<(String, Option<Action>)> {
        entries
            .iter()
            .filter_map(|e| match e {
                Entry::Item(label, action) => Some((label.clone(), action.clone())),
                Entry::Separator => None,
            })
            .collect()
    }

    #[test]
    fn hearth_lang_overrides_windows() {
        assert!(!Language::select(Some("en"), || true).german);
        assert!(!Language::select(Some("EN-us"), || true).german);
        assert!(Language::select(Some("de"), || false).german);
        assert!(Language::select(Some(" de_AT "), || false).german);
        // Nothing or something unknown: Windows decides.
        assert!(!Language::select(None, || false).german);
        assert!(Language::select(None, || true).german);
        assert!(!Language::select(Some("fr"), || false).german);
        // Tests read German unless HEARTH_LANG says otherwise.
        let set = std::env::var("HEARTH_LANG").ok();
        let expected = Language::select(set.as_deref(), || true).german;
        assert_eq!(Language::current(|| false).german, expected);
    }

    #[test]
    fn connected_with_a_question_and_a_held_merge() {
        let mut view = view(json!({
            "pid": 7, "connection": "connected", "server": "http://pi:8085", "error": null,
            "vm": "running", "replies": 0, "terminals": 0, "stopAt": 1_000 + 12 * 60,
            "questions": [
                { "session": "c1", "id": 3, "host": "example.com" },
                // Not a chat id: never part of a URL handed to Windows.
                { "session": "../x?y=1", "id": 4, "host": "example.org" }
            ]
        }));
        assert_eq!(view.questions().len(), 1);
        view.held.push(("demo".into(), "K7QF2M".into()));
        assert!(view.connected() && view.attention());
        let menu = actions(&DE.menu(&view, Path::new("C:/h/daemon.log"), 1_000));
        assert_eq!(menu[0], ("Verbunden mit pi:8085".into(), None));
        assert_eq!(
            menu[1],
            ("VM läuft · stoppt im Leerlauf in 12 min".into(), None)
        );
        // Before the server says its version: hearth's alone.
        assert_eq!(
            menu[2],
            (format!("hearth {}", env!("CARGO_PKG_VERSION")), None)
        );
        assert_eq!(menu[3].1, Some(Action::Open("http://pi:8085/c/c1".into())));
        assert_eq!(
            menu[4].1,
            Some(Action::Copy("hearth merge --approve K7QF2M".into()))
        );
        assert_eq!(menu[5].1, Some(Action::OpenApp(None)));
        assert_eq!(menu[6].1, Some(Action::Open("http://pi:8085".into())));
        assert_eq!(menu[7].1, Some(Action::StopVm));
        assert_eq!(menu[8].1, Some(Action::Open("C:/h/daemon.log".into())));
        assert_eq!(menu[9].1, Some(Action::Quit));
        let notices = DE.notices(&view);
        assert_eq!(notices.len(), 2);
        assert_eq!(notices[0].target.as_deref(), Some("http://pi:8085/c/c1"));
        assert!(notices[1].warning && notices[1].text.contains("K7QF2M"));
    }

    #[test]
    fn questions_of_the_direct_mode_lead_to_the_app() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("host-approvals.json"),
            r#"[{"id": "W7KD3P", "project": "werkstatt", "summary": "Befehl ausführen: cargo test"}]"#,
        )
        .unwrap();
        let view = View::read(temp.path(), 0);
        assert!(view.attention());
        let menu = actions(&DE.menu(&view, Path::new("log"), 0));
        assert_eq!(
            menu[3],
            (
                "Freigabe: Befehl ausführen: cargo test".into(),
                Some(Action::OpenApp(Some("approvals".into())))
            )
        );
        let notices = DE.notices(&view);
        assert_eq!(notices[0].key, "a:W7KD3P");
        assert_eq!(notices[0].target.as_deref(), Some(APP_APPROVALS));
        assert!(notices[0].text.contains("cargo test"));
    }

    #[test]
    fn offline_or_unknown_offers_less() {
        let offline = view(json!({
            "pid": 7, "connection": "offline", "server": "http://pi:8085",
            "error": "Verbindungsaufbau", "vm": "stopped", "questions": []
        }));
        let menu = actions(&EN.menu(&offline, Path::new("log"), 0));
        assert_eq!(menu[0].0, "Not connected: Verbindungsaufbau");
        assert_eq!(menu[1].0, "VM off, starts when needed");
        // The VM is off: nothing to stop.
        assert_eq!(menu[5], ("Stop VM".into(), None));
        assert!(!offline.connected() && !offline.attention());

        let starting = View::default();
        let menu = actions(&DE.menu(&starting, Path::new("log"), 0));
        assert_eq!(menu[0].0, "Dienst startet …");
        assert_eq!(
            menu[3],
            ("Hearth öffnen".into(), Some(Action::OpenApp(None)))
        );
        assert_eq!(menu[4], ("Web-App öffnen".into(), None));
        let quitting = View {
            quitting: true,
            ..View::default()
        };
        let menu = actions(&DE.menu(&quitting, Path::new("log"), 0));
        assert_eq!(menu[0].0, "Wird beendet …");
        assert_eq!(menu.last().unwrap().1, None);
    }

    #[test]
    fn status_files_of_other_runs_are_ignored() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("daemon-status.json"),
            r#"{"pid": 42, "connection": "connected"}"#,
        )
        .unwrap();
        fs::create_dir(temp.path().join("merge-requests")).unwrap();
        fs::write(
            temp.path().join("merge-requests/K7QF2M.json"),
            r#"{"id": "K7QF2M", "project": "demo"}"#,
        )
        .unwrap();
        assert!(View::read(temp.path(), 42).connected());
        let other = View::read(temp.path(), 43);
        assert!(other.status.is_none());
        assert_eq!(other.held, [("demo".to_owned(), "K7QF2M".to_owned())]);
    }

    #[test]
    fn program_files_wait_for_a_click() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("app-requests")).unwrap();
        fs::write(
            temp.path().join("app-requests/H3PX7Q.json"),
            r#"{"id": "H3PX7Q", "name": "tool", "title": "Mein Tool",
                "path": "C:\\Tools\\tool.zip", "kind": "Archiv (zip)", "size": 3145728,
                "created": 1}"#,
        )
        .unwrap();
        let view = View::read(temp.path(), 0);
        assert!(view.attention());
        let menu = actions(&DE.menu(&view, Path::new("log"), 0));
        assert_eq!(
            menu[3],
            (
                "Programm „Mein Tool“ freigeben …".into(),
                Some(Action::ApproveFile("H3PX7Q".into()))
            )
        );
        let notices = DE.notices(&view);
        assert_eq!(notices[0].file.as_deref(), Some("H3PX7Q"));
        let question = DE.approval_question(&view.files[0]);
        assert!(question.contains("C:\\Tools\\tool.zip"), "{question}");
        assert!(question.contains("Archiv (zip), 3.0 MB"), "{question}");
    }
    #[test]
    fn versions_say_which_side_to_update() {
        use std::cmp::Ordering;
        assert_eq!(version_order("0.1.14", "0.1.15"), Some(Ordering::Less));
        assert_eq!(
            version_order("0.1.15+abc1234", "0.1.15"),
            Some(Ordering::Equal)
        );
        assert_eq!(version_order("0.2", "0.1.15"), Some(Ordering::Greater));
        assert_eq!(version_order("dev", "0.1.15"), None);

        let de = Language { german: true };
        let older = view(
            json!({ "pid": 1, "connection": "connected", "version": "0.1.15", "serverVersion": "0.1.14" }),
        );
        let (label, action) = de.versions(&older);
        assert!(
            label.starts_with("Server 0.1.14 älter als hearth 0.1.15"),
            "{label}"
        );
        assert_eq!(action, Some(Action::Copy(SERVER_UPDATE.to_owned())));
        assert!(
            de.notices(&older)
                .iter()
                .any(|notice| notice.key == "v:0.1.14:0.1.15" && notice.warning)
        );

        let same = view(
            json!({ "pid": 1, "connection": "connected", "version": "0.1.15", "serverVersion": "0.1.15" }),
        );
        assert_eq!(
            de.versions(&same),
            ("hearth 0.1.15 · Server 0.1.15".to_owned(), None)
        );
        assert!(de.notices(&same).is_empty());
        let newer = view(
            json!({ "pid": 1, "connection": "connected", "version": "0.1.14", "serverVersion": "0.1.15" }),
        );
        assert!(de.versions(&newer).0.contains("neues Einrichtungsprogramm"));
        let unknown = view(json!({ "pid": 1, "connection": "connected", "version": "0.1.15" }));
        assert_eq!(de.versions(&unknown).0, "hearth 0.1.15");
    }
}
