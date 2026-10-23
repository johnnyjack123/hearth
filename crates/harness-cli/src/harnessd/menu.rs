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
}

/// What the tray read last.
#[derive(Debug, Default, Clone)]
pub struct View {
    /// The status file of the running service, if it is current.
    pub status: Option<Value>,
    /// Open merge requests: project and id.
    pub held: Vec<(String, String)>,
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
        View {
            status,
            held,
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
        !self.questions().is_empty() || !self.held.is_empty()
    }

    fn server(&self) -> Option<&str> {
        self.str("server").filter(|s| s.starts_with("http"))
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
                            q.get("session")?.as_str()?.to_owned(),
                            q.get("id")?.as_u64()?,
                            q.get("host")?.as_str()?.to_owned(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Texts in the user's language: German or English.
#[derive(Debug, Clone, Copy)]
pub struct Language {
    /// Whether Windows runs in German.
    pub german: bool,
}

impl Language {
    fn t<'a>(self, de: &'a str, en: &'a str) -> &'a str {
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
                    "Nicht gekoppelt (harness login)",
                    "Not paired (harness login)",
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
        format!(
            "LLM Harness\n{}\n{}",
            self.connection(view),
            self.vm(view, now)
        )
    }

    /// The menu for `view`; `log` is the service's log file.
    pub fn menu(self, view: &View, log: &Path, now: u64) -> Vec<Entry> {
        let mut entries = vec![
            Entry::Item(self.connection(view), None),
            Entry::Item(self.vm(view, now), None),
        ];
        let server = view.server().map(str::to_owned);
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
                Some(Action::Copy(format!("harness merge --approve {id}"))),
            ));
        }
        entries.push(Entry::Separator);
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
            .questions()
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
            })
            .collect();
        notices.extend(view.held.iter().map(|(project, id)| Notice {
            key: format!("h:{id}"),
            text: if self.german {
                format!(
                    "{project}: Merge wegen schwerer Funde angehalten. Freigeben im Terminal: \
                     harness merge --approve {id}"
                )
            } else {
                format!(
                    "{project}: merge held for severe findings. Approve in a terminal: harness \
                     merge --approve {id}"
                )
            },
            warning: true,
            target: server.clone(),
        }));
        notices
    }
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
    fn connected_with_a_question_and_a_held_merge() {
        let mut view = view(json!({
            "pid": 7, "connection": "connected", "server": "http://pi:8085", "error": null,
            "vm": "running", "replies": 0, "terminals": 0, "stopAt": 1_000 + 12 * 60,
            "questions": [{ "session": "c1", "id": 3, "host": "example.com" }]
        }));
        view.held.push(("demo".into(), "K7QF2M".into()));
        assert!(view.connected() && view.attention());
        let menu = actions(&DE.menu(&view, Path::new("C:/h/daemon.log"), 1_000));
        assert_eq!(menu[0], ("Verbunden mit pi:8085".into(), None));
        assert_eq!(
            menu[1],
            ("VM läuft · stoppt im Leerlauf in 12 min".into(), None)
        );
        assert_eq!(menu[2].1, Some(Action::Open("http://pi:8085/c/c1".into())));
        assert_eq!(
            menu[3].1,
            Some(Action::Copy("harness merge --approve K7QF2M".into()))
        );
        assert_eq!(menu[4].1, Some(Action::Open("http://pi:8085".into())));
        assert_eq!(menu[5].1, Some(Action::StopVm));
        assert_eq!(menu[6].1, Some(Action::Open("C:/h/daemon.log".into())));
        assert_eq!(menu[7].1, Some(Action::Quit));
        let notices = DE.notices(&view);
        assert_eq!(notices.len(), 2);
        assert_eq!(notices[0].target.as_deref(), Some("http://pi:8085/c/c1"));
        assert!(notices[1].warning && notices[1].text.contains("K7QF2M"));
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
        assert_eq!(menu[3], ("Stop VM".into(), None));
        assert!(!offline.connected() && !offline.attention());

        let starting = View::default();
        let menu = actions(&DE.menu(&starting, Path::new("log"), 0));
        assert_eq!(menu[0].0, "Dienst startet …");
        assert_eq!(menu[2], ("Web-App öffnen".into(), None));
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
}
