//! The pages of the window: approvals, projects, computer and log.

use eframe::egui::{self, RichText};
use hearth_core::config::{HostChange, HostWrites, RemoteChange};
use hearth_core::host::{Decision, GrantKind, Request, audit, inbox};
use hearth_core::lang::tr;
use hearth_core::project::ProjectName;
use hearth_core::tr;

use crate::app::{HearthApp, logo_texture};
use crate::jobs::open_terminal;
use crate::model::{self, ProjectRow};
use crate::theme;

/// What the pages keep between frames.
#[derive(Default)]
pub struct State {
    logo: Option<egui::TextureHandle>,
    /// Address typed for pairing.
    pub server: String,
    /// A project waiting for "remove" to be confirmed, and whether its
    /// agent data stays.
    remove: Option<(String, bool)>,
    /// The project whose record the log page shows; none: the service.
    record: Option<String>,
    /// A domain typed per project, for its allow or deny list.
    domains: std::collections::HashMap<String, String>,
}

impl State {
    /// The logo, loaded on first use.
    pub fn logo(&mut self, ctx: &egui::Context) -> egui::TextureHandle {
        self.logo.get_or_insert_with(|| logo_texture(ctx)).clone()
    }
}

fn muted(ui: &egui::Ui, text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(theme::tokens(ui.ctx()).muted_foreground)
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(14.0);
    ui.label(RichText::new(title).strong().size(15.0));
    ui.add_space(4.0);
}

fn empty(ui: &mut egui::Ui, title: &str, text: &str) {
    theme::card(ui.ctx()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).strong());
        ui.label(muted(ui, text));
    });
}

// ── Approvals ──────────────────────────────────────────────────────────

/// What waits for the user: questions of the direct mode, held merges and
/// requested program files.
pub fn approvals(app: &mut HearthApp, ui: &mut egui::Ui) {
    let t = theme::tokens(ui.ctx());
    let pending: Vec<inbox::Pending> = app
        .snapshot
        .approvals
        .iter()
        .filter(|item| !app.answered.contains(&item.id))
        .cloned()
        .collect();
    if pending.is_empty() && app.snapshot.merges.is_empty() && app.snapshot.files.is_empty() {
        empty(
            ui,
            tr("Nichts wartet auf dich.", "Nothing waits for you."),
            tr(
                "Hier erscheinen Befehle und Zugriffe von Agents im Direktmodus, angehaltene Übernahmen und Programmdateien, die der Browser angefragt hat.",
                "Commands and file access of agents in the direct mode, held takeovers and program files the browser asked for appear here.",
            ),
        );
        return;
    }
    for item in pending {
        theme::card(ui.ctx()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                theme::badge(ui, &item.id, t.warning);
                ui.label(RichText::new(&item.summary).strong());
            });
            let chat = item.title.clone().unwrap_or_else(|| item.chat.clone());
            ui.label(muted(ui, format!("{} · {chat}", item.project)));
            let detail = match &item.request {
                Request::Command {
                    command,
                    shell,
                    cwd,
                } => {
                    ui.label(muted(ui, tr!("{shell} in {cwd}", "{shell} in {cwd}")).small());
                    command.clone()
                }
                Request::Read { path, .. } | Request::Write { path, .. } => path.clone(),
            };
            egui::Frame::new()
                .fill(t.muted)
                .corner_radius(egui::CornerRadius::same(theme::CONTROL_RADIUS))
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(detail).monospace());
                });
            ui.label(muted(ui, &item.why));
            if let Some(assessment) = &item.assessment {
                let color = match assessment.percent {
                    75.. => t.destructive,
                    50.. => t.warning,
                    _ => t.muted_foreground,
                };
                ui.label(RichText::new(assessment.text()).color(color).strong());
                if assessment.high() {
                    ui.label(muted(
                        ui,
                        tr(
                            "Nur einmal freigeben: Bei hohem Schadenspotenzial gibt es „im Chat erlauben“ nicht.",
                            "Approve once only: with a high potential for harm there is no “allow in the chat”.",
                        ),
                    ));
                }
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let mut answer = None;
                if ui
                    .add(theme::primary_button(
                        ui.ctx(),
                        tr("Einmal erlauben", "Allow once"),
                    ))
                    .clicked()
                {
                    answer = Some(Decision::Once);
                }
                if item.chat_wide {
                    let label = if matches!(item.request, Request::Command { .. }) {
                        tr(
                            "Diesen Befehl im Chat erlauben",
                            "Allow this command in the chat",
                        )
                    } else {
                        tr("Im Chat erlauben", "Allow in the chat")
                    };
                    if ui.button(label).clicked() {
                        answer = Some(Decision::Chat);
                    }
                }
                if ui.button(tr("Ablehnen", "Deny")).clicked() {
                    answer = Some(Decision::Deny);
                }
                if let Some(decision) = answer {
                    match inbox::submit(app.config.root(), &item.id, decision) {
                        Ok(()) => {
                            app.answered.insert(item.id.clone());
                            app.reload_soon();
                        }
                        Err(error) => app.say(
                            tr!(
                                "Antwort nicht gespeichert: {error}",
                                "Answer not saved: {error}"
                            ),
                            true,
                        ),
                    }
                }
            });
        });
        ui.add_space(8.0);
    }
    for merge in app.snapshot.merges.clone() {
        theme::card(ui.ctx()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                theme::badge(ui, &merge.id, t.warning);
                ui.label(RichText::new(tr!("Übernahme in {} angehalten", "Takeover into {} held", merge.project)).strong());
            });
            ui.label(muted(
                ui,
                tr(
                    "Die Prüfung hat etwas gefunden. Freigeben geht nur im Terminal, nach einem Blick auf die Funde.",
                    "The check found something. Approving works only in a terminal, after a look at the findings.",
                ),
            ));
            if ui.button(tr("Im Terminal prüfen …", "Check in a terminal …")).clicked() {
                let dir = app.snapshot.projects.iter().find(|p| p.name == merge.project).map(|p| p.path.clone());
                let dir = dir.unwrap_or_else(|| app.config.root().to_owned());
                if let Err(error) = open_terminal(&dir, &["merge", "--approve", &merge.id]) {
                    app.say(error, true);
                }
            }
        });
        ui.add_space(8.0);
    }
    for file in app.snapshot.files.clone() {
        theme::card(ui.ctx()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                theme::badge(ui, &file.id, t.warning);
                ui.label(
                    RichText::new(tr!("Programmdatei „{}“", "Program file “{}”", file.title))
                        .strong(),
                );
            });
            ui.label(RichText::new(&file.path).monospace());
            ui.label(muted(
                ui,
                format!("{} · {} KB", file.kind, file.size / 1024),
            ));
            ui.horizontal(|ui| {
                let busy = app.jobs.busy();
                if ui
                    .add_enabled(
                        !busy,
                        theme::primary_button(ui.ctx(), tr("Freigeben", "Approve")),
                    )
                    .clicked()
                {
                    app.jobs.start(
                        ui.ctx(),
                        tr!(
                            "Programmdatei {} freigeben",
                            "Approve program file {}",
                            file.id
                        ),
                        &["app", "approve", &file.id, "--yes"],
                    );
                }
                if ui
                    .add_enabled(!busy, egui::Button::new(tr("Ablehnen", "Deny")))
                    .clicked()
                {
                    app.jobs.start(
                        ui.ctx(),
                        tr!("Programmdatei {} ablehnen", "Deny program file {}", file.id),
                        &["app", "deny", &file.id],
                    );
                }
            });
        });
        ui.add_space(8.0);
    }
}

// ── Projects ───────────────────────────────────────────────────────────

/// The projects with their mode, offer to web chats and its settings.
pub fn projects(app: &mut HearthApp, ui: &mut egui::Ui) {
    let t = theme::tokens(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(muted(
            ui,
            tr(
                "Jedes Projekt ist ein Ordner auf diesem Rechner. Web-Chats sehen nur Projekte, die du hier freigibst.",
                "Every project is a folder on this computer. Web chats only see projects you offer here.",
            ),
        ));
    });
    ui.add_space(4.0);
    if ui
        .add_enabled(
            !app.jobs.running(tr("Projekt anlegen", "Add project")),
            theme::primary_button(ui.ctx(), tr("Ordner hinzufügen …", "Add folder …")),
        )
        .clicked()
        && let Some(folder) = rfd::FileDialog::new()
            .set_title(tr(
                "Ordner für das Projekt wählen",
                "Choose the project's folder",
            ))
            .pick_folder()
    {
        let folder = folder.to_string_lossy().into_owned();
        app.jobs.start(
            ui.ctx(),
            tr("Projekt anlegen", "Add project"),
            &["init", &folder],
        );
    }
    ui.add_space(10.0);
    if app.snapshot.projects.is_empty() {
        empty(
            ui,
            tr("Noch kein Projekt.", "No project yet."),
            tr(
                "Füge einen Ordner hinzu. Er wird zum Git-Repository, falls er noch keins ist, und steht dann in Web-Chats unter „Umgebung“ zur Auswahl.",
                "Add a folder. It becomes a Git repository if it is none yet, and web chats can then choose it under “Environment”.",
            ),
        );
    }
    for row in app.snapshot.projects.clone() {
        project_card(app, ui, &row);
        ui.add_space(10.0);
    }
    for (name, error) in app.snapshot.broken.clone() {
        theme::card(ui.ctx()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(&name).strong());
            ui.label(RichText::new(error).color(t.destructive));
        });
    }
}

fn project_card(app: &mut HearthApp, ui: &mut egui::Ui, row: &ProjectRow) {
    let t = theme::tokens(ui.ctx());
    let Ok(name) = ProjectName::new(&row.name) else {
        return;
    };
    theme::card(ui.ctx()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(&row.name).strong().size(16.0));
            if row.remote {
                theme::badge(ui, tr("Web-Chats", "Web chats"), t.success);
            }
            theme::badge(
                ui,
                if row.host.enabled { tr("Direkt", "Direct") } else { tr("Sandbox", "Sandbox") },
                if row.host.enabled { t.warning } else { t.primary },
            );
        });
        ui.label(RichText::new(row.path.display().to_string()).monospace().small().color(t.muted_foreground));
        ui.add_space(6.0);

        let mut direct = row.host.enabled;
        ui.horizontal(|ui| {
            ui.label(tr("Modus:", "Mode:"));
            // Linux runs the sandbox in the computer's rootless Podman,
            // without a VM (weaker: the containers share the kernel).
            let sandbox = if cfg!(target_os = "linux") {
                tr("Sandbox (Container)", "Sandbox (container)")
            } else {
                tr("Sandbox (VM)", "Sandbox (VM)")
            };
            ui.radio_value(&mut direct, false, sandbox);
            ui.radio_value(&mut direct, true, tr("Direkt auf dem Rechner", "Directly on the computer"));
        });
        if direct != row.host.enabled {
            let change = HostChange { enabled: Some(direct), ..HostChange::default() };
            set_host(app, &name, change);
        }
        ui.label(muted(
            ui,
            if row.host.enabled {
                tr(
                    "Der Agent arbeitet direkt in diesem Ordner. Jeder Befehl und jeder Zugriff außerhalb des Ordners braucht deine Freigabe, manches ist ganz gesperrt (siehe unten); freigegebene Befehle laufen mit deinen Rechten.",
                    "The agent works right in this folder. Every command and every access outside the folder needs your approval, some things are closed altogether (see below); approved commands run with your rights.",
                )
            } else if cfg!(target_os = "linux") {
                tr(
                    "Der Agent arbeitet in einem Podman-Container auf einer Kopie des Repositorys; seine Arbeit übernimmst du per Merge mit Prüfung. Unter Linux teilt der Container den Kernel dieses Rechners, das trennt schwächer als eine VM.",
                    "The agent works in a Podman container on a copy of the repository; you take its work over by a checked merge. On Linux the container shares this computer's kernel, which separates less than a VM.",
                )
            } else {
                tr(
                    "Der Agent arbeitet in einer isolierten Linux-VM auf einer Kopie des Repositorys; seine Arbeit übernimmst du per Merge mit Prüfung.",
                    "The agent works in an isolated Linux VM on a copy of the repository; you take its work over by a checked merge.",
                )
            },
        ));
        ui.add_space(4.0);

        let mut remote = row.remote;
        if ui.checkbox(&mut remote, tr("Für Web-Chats deines Kontos freigeben", "Offer to the web chats of your account")).changed() {
            set_remote(app, &name, RemoteChange { enabled: Some(remote), ..RemoteChange::default() });
        }
        if row.host.enabled {
            let mut browser = row.host.browser_approvals;
            if ui.checkbox(&mut browser, tr("Freigaben auch im Web-Chat erlauben", "Allow approvals in the web chat too")).changed() {
                set_host(app, &name, HostChange { browser_approvals: Some(browser), ..HostChange::default() });
            }
            if row.host.browser_approvals {
                ui.label(RichText::new(tr(
                    "Wer den Server kontrolliert, kann damit Befehle auf diesem Rechner freigeben.",
                    "Whoever controls the server can approve commands on this computer with that.",
                ))
                .color(t.warning));
            }
            let mut ask = row.host.writes == HostWrites::Ask;
            if ui.checkbox(&mut ask, tr("Auch jedes Schreiben im Ordner fragen", "Ask for every write in the folder too")).changed() {
                let writes = if ask { HostWrites::Ask } else { HostWrites::Free };
                set_host(app, &name, HostChange { writes: Some(writes), ..HostChange::default() });
            }
            egui::CollapsingHeader::new(tr("Was darf der Agent?", "What may the agent do?"))
                .id_salt(("rights", &row.name))
                .show(ui, |ui| rights(ui, row.host.writes == HostWrites::Ask));
            chat_grants(app, ui, &row.name);
        } else {
            let mut merge = row.merge;
            if ui
                .checkbox(&mut merge, tr("Übernahmen aus dem Browser ohne Freigabe hier", "Takeovers from the browser without an approval here"))
                .changed()
            {
                set_remote(app, &name, RemoteChange { merge: Some(merge), ..RemoteChange::default() });
            }
            ui.label(muted(ui, tr!("Netz: {} · Programme: {}", "Network: {} · Programs: {}", row.network, row.apps.join(", "))).small());
            egui::CollapsingHeader::new(tr("Netz und Programme der Sandbox", "Network and programs of the sandbox"))
                .id_salt(("sandbox", &row.name))
                .show(ui, |ui| sandbox_settings(app, ui, row));
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if !row.host.enabled
                && ui.button(tr("pi im Terminal …", "pi in a terminal …")).clicked()
                && let Err(error) = open_terminal(&row.path, &["run"])
            {
                app.say(error, true);
            }
            if row.host.enabled && ui.button(tr("Protokoll", "Record")).clicked() {
                app.pages.record = Some(row.name.clone());
                app.page = crate::app::Page::Log;
            }
            if ui.button(tr("Entfernen …", "Remove …")).clicked() {
                app.pages.remove = Some((row.name.clone(), true));
            }
        });
    });
}

/// What the agent of a project in the direct mode may do, in three lists.
fn rights(ui: &mut egui::Ui, writes_ask: bool) {
    let t = theme::tokens(ui.ctx());
    let rights = hearth_core::host::rights(writes_ask);
    for (title, color, items) in [
        (
            tr("Ohne Rückfrage", "Without asking"),
            t.success,
            &rights.free,
        ),
        (
            tr("Mit deiner Freigabe", "With your approval"),
            t.warning,
            &rights.ask,
        ),
        (
            tr(
                "Nie, auch nicht mit Freigabe",
                "Never, not even with approval",
            ),
            t.destructive,
            &rights.never,
        ),
    ] {
        ui.label(RichText::new(title).strong().color(color));
        for item in items {
            ui.label(format!("• {item}"));
        }
        ui.add_space(4.0);
    }
    ui.label(muted(
        ui,
        tr(
            "Vor jedem Befehl bewertet Jev sein Schadenspotenzial, wenn dein Konto einen OpenRouter-Zugang hat und Jev für diesen Rechner an ist (in der Web-App unter Einstellungen, Geräte). Ein Befehl kann Geheimnisse auch lesen, ohne sie zu nennen (etwa grep über den ganzen Ordner); sieh dir deshalb jeden an.",
            "Before each command Jev rates its potential for harm when your account has OpenRouter access and Jev is on for this computer (in the web app under Settings, Devices). A command can read secrets without naming them (grep over the whole folder, say); so look at each one.",
        ),
    ).small());
}

/// The "for this chat" answers of the project's running chats, each with a
/// button to take it back.
fn chat_grants(app: &mut HearthApp, ui: &mut egui::Ui, project: &str) {
    let chats: Vec<inbox::ChatGrants> = app
        .snapshot
        .grants
        .iter()
        .filter(|chat| chat.project == project)
        .cloned()
        .collect();
    if chats.is_empty() {
        return;
    }
    ui.add_space(4.0);
    ui.label(RichText::new(tr("Erlaubt für ganze Chats", "Allowed for whole chats")).strong());
    for chat in chats {
        ui.label(muted(ui, chat.title.clone().unwrap_or_else(|| chat.chat.clone())).small());
        for grant in &chat.grants {
            ui.horizontal(|ui| {
                let kind = match grant.kind {
                    GrantKind::Command => tr("Befehl", "Command"),
                    GrantKind::Folder => tr("Ordner lesen", "Read folder"),
                    GrantKind::File => tr("Datei lesen", "Read file"),
                };
                if ui.small_button(tr("Zurücknehmen", "Take back")).clicked() {
                    match inbox::request_revoke(app.config.root(), &grant.id) {
                        Ok(()) => app.reload_soon(),
                        Err(error) => app.say(
                            tr!("Nicht zurückgenommen: {error}", "Not taken back: {error}"),
                            true,
                        ),
                    }
                }
                let first = grant.target.lines().next().unwrap_or("");
                ui.label(format!("{kind}:"));
                ui.label(RichText::new(first).monospace());
            });
        }
    }
}

/// The sandbox's network rules and programs of a project, changed through
/// `hearth net` and `hearth app`, so a running gateway takes them at once
/// as in the terminal.
fn sandbox_settings(app: &mut HearthApp, ui: &mut egui::Ui, row: &ProjectRow) {
    let busy = app.jobs.busy();
    ui.label(RichText::new(tr("Netz", "Network")).strong());
    ui.horizontal_wrapped(|ui| {
        for (preset, label) in [
            ("off", tr("aus", "off")),
            ("development", tr("Entwicklung", "development")),
            ("open", tr("offen", "open")),
        ] {
            let selected = row.network == preset;
            if ui
                .add_enabled(!busy, egui::Button::selectable(selected, label))
                .clicked()
                && !selected
            {
                app.jobs.start(
                    ui.ctx(),
                    tr!("Netz von {}: {label}", "Network of {}: {label}", row.name),
                    &["net", "--project", &row.name, "preset", preset],
                );
            }
        }
    });
    if row.network == "open" {
        ui.label(RichText::new(tr(
            "Offen: Der Agent erreicht jede öffentliche Adresse und kann alles, was er in der Sandbox sieht, nach außen schicken.",
            "Open: the agent reaches every public address and can send out anything it sees in the sandbox.",
        ))
        .color(theme::tokens(ui.ctx()).warning));
    }
    for (title, list) in [
        (tr("Erlaubt", "Allowed"), &row.allow),
        (tr("Gesperrt", "Blocked"), &row.deny),
    ] {
        if !list.is_empty() {
            ui.label(muted(ui, format!("{title}: {}", list.join(", "))).small());
        }
    }
    let domain = app.pages.domains.entry(row.name.clone()).or_default();
    let mut add = None;
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(domain)
                .hint_text("api.example.com, *.example.com")
                .desired_width(240.0),
        );
        let ready = !domain.trim().is_empty() && !busy;
        if ui
            .add_enabled(ready, egui::Button::new(tr("Erlauben", "Allow")))
            .clicked()
        {
            add = Some("allow");
        }
        if ui
            .add_enabled(ready, egui::Button::new(tr("Sperren", "Block")))
            .clicked()
        {
            add = Some("deny");
        }
    });
    if let Some(list) = add {
        let pattern = domain.trim().to_owned();
        domain.clear();
        app.jobs.start(
            ui.ctx(),
            tr!(
                "{} für {}: {pattern}",
                "{} for {}: {pattern}",
                list,
                row.name
            ),
            &["net", "--project", &row.name, list, &pattern, "--always"],
        );
    }
    ui.add_space(6.0);
    ui.label(RichText::new(tr("Programme", "Programs")).strong());
    ui.label(muted(ui, tr(
        "Werden beim nächsten Start der Sandbox installiert, je Programm einmal für alle Projekte.",
        "Installed when the sandbox starts next, each once for all projects.",
    )).small());
    for entry in app.snapshot.catalog.clone() {
        let mut on = row.apps.contains(&entry.name);
        let label = match &entry.description {
            Some(description) => format!("{} – {description}", entry.title),
            None => entry.title.clone(),
        };
        if ui
            .add_enabled(!busy, egui::Checkbox::new(&mut on, label))
            .changed()
        {
            app.jobs.start(
                ui.ctx(),
                tr!(
                    "{}: {} {}",
                    "{}: {} {}",
                    row.name,
                    entry.title,
                    if on { tr("an", "on") } else { tr("aus", "off") }
                ),
                &[
                    "app",
                    "--project",
                    &row.name,
                    if on { "on" } else { "off" },
                    &entry.name,
                ],
            );
        }
    }
}

fn set_host(app: &mut HearthApp, name: &ProjectName, change: HostChange) {
    match app.config.set_host(name, change) {
        Ok(_) => app.say(tr!("{name}: gespeichert.", "{name}: saved."), false),
        Err(error) => app.say(error.to_string(), true),
    }
    app.reload_soon();
}

fn set_remote(app: &mut HearthApp, name: &ProjectName, change: RemoteChange) {
    match app.config.set_remote(name, change) {
        Ok(_) => app.say(tr!("{name}: gespeichert.", "{name}: saved."), false),
        Err(error) => app.say(error.to_string(), true),
    }
    app.reload_soon();
}

// ── Computer ───────────────────────────────────────────────────────────

/// The pairing, the background service and the VM.
pub fn computer(app: &mut HearthApp, ui: &mut egui::Ui) {
    let t = theme::tokens(ui.ctx());
    let service = app.snapshot.service.clone();

    section(ui, tr("Server", "Server"));
    theme::card(ui.ctx()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        match &app.snapshot.server {
            Some(url) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(url).strong());
                    match service.as_ref().map(|s| s.connection.as_str()) {
                        Some("connected") => theme::badge(ui, tr("verbunden", "connected"), t.success),
                        Some("unpaired") => theme::badge(ui, tr("nicht mehr gekoppelt", "no longer paired"), t.destructive),
                        Some(_) => theme::badge(ui, tr("verbindet", "connecting"), t.warning),
                        None => theme::badge(ui, tr("Dienst aus", "service off"), t.muted_foreground),
                    }
                });
                if let Some(error) = service.as_ref().and_then(|s| s.error.clone()) {
                    ui.label(RichText::new(error).color(t.destructive));
                }
                if ui.add_enabled(!app.jobs.busy(), egui::Button::new(tr("Entkoppeln", "Unpair"))).clicked() {
                    app.jobs.start(ui.ctx(), tr("Entkoppeln", "Unpair"), &["logout"]);
                }
            }
            None => {
                ui.label(muted(ui, tr(
                    "Dieser Rechner ist mit keinem hearth-Server gekoppelt. Gib die Adresse ein; der Browser öffnet sich zum Bestätigen.",
                    "This computer is not paired with a hearth server. Enter its address; the browser opens to confirm.",
                )));
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut app.pages.server).hint_text("https://hearth.example.com").desired_width(320.0));
                    let ready = app.pages.server.starts_with("http://") || app.pages.server.starts_with("https://");
                    if ui.add_enabled(ready && !app.jobs.busy(), theme::primary_button(ui.ctx(), tr("Koppeln", "Pair"))).clicked() {
                        let server = app.pages.server.trim().to_owned();
                        app.jobs.start(ui.ctx(), tr("Mit dem Server koppeln", "Pair with the server"), &["login", &server]);
                    }
                });
            }
        }
    });

    section(ui, tr("Hintergrunddienst", "Background service"));
    theme::card(ui.ctx()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(muted(ui, tr(
            "Hält die Verbindung zum Server, damit Web-Chats deine Projekte nutzen können. Er startet mit der Anmeldung.",
            "Keeps the connection to the server, so web chats can use your projects. It starts when you sign in.",
        )));
        ui.horizontal(|ui| {
            let running = service.is_some();
            theme::badge(ui, if running { tr("läuft", "running") } else { tr("aus", "off") }, if running { t.success } else { t.muted_foreground });
            if let Some(status) = &service
                && status.replies > 0
            {
                ui.label(muted(ui, tr!("{} Antwort(en) unterwegs", "{} reply(ies) on their way", status.replies)));
            }
        });
        ui.horizontal(|ui| {
            let busy = app.jobs.busy();
            if service.is_none() && ui.add_enabled(!busy, theme::primary_button(ui.ctx(), tr("Starten", "Start"))).clicked() {
                app.jobs.start(ui.ctx(), tr("Dienst starten", "Start the service"), &["daemon", "install"]);
            }
            if service.is_some() && ui.add_enabled(!busy, egui::Button::new(tr("Stoppen", "Stop"))).clicked() {
                app.jobs.start(ui.ctx(), tr("Dienst stoppen", "Stop the service"), &["daemon", "stop"]);
            }
        });
    });

    if cfg!(windows)
        || service
            .as_ref()
            .is_some_and(|s| s.vm != "unknown" && !s.vm.is_empty())
    {
        section(ui, tr("Sandbox-VM", "Sandbox VM"));
        theme::card(ui.ctx()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let vm = service.as_ref().map_or("unknown", |s| s.vm.as_str());
            let (label, color) = match vm {
                "running" => (tr("läuft", "running"), t.success),
                "starting" => (tr("startet", "starting"), t.warning),
                "stopping" => (tr("stoppt", "stopping"), t.warning),
                "stopped" => (tr("aus", "off"), t.muted_foreground),
                _ => (tr("unbekannt", "unknown"), t.muted_foreground),
            };
            ui.horizontal(|ui| {
                ui.label(tr("Zustand:", "State:"));
                theme::badge(ui, label, color);
            });
            ui.horizontal(|ui| {
                let busy = app.jobs.busy();
                if ui
                    .add_enabled(
                        !busy && vm != "running",
                        egui::Button::new(tr("Starten", "Start")),
                    )
                    .clicked()
                {
                    app.jobs.start(
                        ui.ctx(),
                        tr("VM starten", "Start the VM"),
                        &["vmd", "start"],
                    );
                }
                if ui
                    .add_enabled(
                        !busy && vm == "running",
                        egui::Button::new(tr("Stoppen", "Stop")),
                    )
                    .clicked()
                {
                    app.jobs
                        .start(ui.ctx(), tr("VM stoppen", "Stop the VM"), &["vmd", "stop"]);
                }
                if ui
                    .add_enabled(!busy, egui::Button::new(tr("Aufräumen", "Clean up")))
                    .clicked()
                {
                    app.jobs.start(
                        ui.ctx(),
                        tr("VM aufräumen", "Clean the VM up"),
                        &["vmd", "clean", "--yes"],
                    );
                }
            });
        });
    }

    section(ui, tr("Einrichtung", "Setup"));
    theme::card(ui.ctx()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let server = service
            .as_ref()
            .and_then(|s| s.server_version.clone())
            .unwrap_or_else(|| "?".to_owned());
        ui.label(muted(
            ui,
            tr!(
                "hearth {} · Server {server}",
                "hearth {} · server {server}",
                env!("CARGO_PKG_VERSION")
            ),
        ));
        if ui
            .add_enabled(
                !app.jobs.busy(),
                egui::Button::new(tr("Einrichtung prüfen", "Check the setup")),
            )
            .clicked()
        {
            app.jobs.start(
                ui.ctx(),
                tr("Einrichtung prüfen", "Check the setup"),
                &["doctor"],
            );
        }
    });
}

// ── Log ────────────────────────────────────────────────────────────────

/// The service's log, or a project's record of the direct mode.
pub fn log(app: &mut HearthApp, ui: &mut egui::Ui) {
    let t = theme::tokens(ui.ctx());
    let direct: Vec<String> = app
        .snapshot
        .projects
        .iter()
        .filter(|p| p.host.enabled)
        .map(|p| p.name.clone())
        .collect();
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(
            &mut app.pages.record,
            None,
            tr("Hintergrunddienst", "Background service"),
        );
        for name in &direct {
            ui.selectable_value(
                &mut app.pages.record,
                Some(name.clone()),
                tr!("Direktmodus: {name}", "Direct mode: {name}"),
            );
        }
    });
    ui.add_space(8.0);
    let lines: Vec<String> = match &app.pages.record {
        None => model::tail(&app.config.root().join("daemon.log"), 400),
        Some(project) => audit::recent(app.config.root(), project, 300)
            .into_iter()
            .map(|entry| {
                format!(
                    "{}  {:<6} {:<8} {:<9} {}{}",
                    entry.at / 1000,
                    entry.tool,
                    entry.decision,
                    entry.by,
                    entry.target.replace('\n', " "),
                    entry
                        .exit
                        .map(|code| format!("  [exit {code}]"))
                        .unwrap_or_default()
                )
            })
            .collect(),
    };
    theme::card(ui.ctx()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if lines.is_empty() {
            ui.label(muted(ui, tr("Noch keine Einträge.", "No entries yet.")));
        }
        for line in lines.iter().rev().take(400) {
            ui.label(RichText::new(line).monospace().small().color(t.foreground));
        }
    });
}

// ── Dialogs ────────────────────────────────────────────────────────────

/// Confirmations over the page.
pub fn dialogs(app: &mut HearthApp, ctx: &egui::Context) {
    let Some((name, keep)) = app.pages.remove.clone() else {
        return;
    };
    let mut keep_data = keep;
    let mut close = false;
    egui::Modal::new(egui::Id::unique("remove-project")).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.label(
            RichText::new(tr!("Projekt {name} entfernen?", "Remove project {name}?"))
                .strong()
                .size(16.0),
        );
        ui.label(muted(
            ui,
            tr(
                "hearth vergisst das Projekt; der Ordner auf diesem Rechner bleibt, wie er ist.",
                "hearth forgets the project; the folder on this computer stays as it is.",
            ),
        ));
        ui.checkbox(
            &mut keep_data,
            tr(
                "Arbeitskopie des Agents in der VM behalten",
                "Keep the agent's working copy in the VM",
            ),
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let t = theme::tokens(ui.ctx());
            let remove = egui::Button::new(
                RichText::new(tr("Entfernen", "Remove")).color(t.primary_foreground),
            )
            .fill(t.destructive);
            if ui.add(remove).clicked() {
                let mut args = vec!["project", "remove", name.as_str(), "--yes"];
                if keep_data {
                    args.push("--keep-data");
                }
                app.jobs.start(
                    ui.ctx(),
                    tr!("Projekt {name} entfernen", "Remove project {name}"),
                    &args,
                );
                close = true;
            }
            if ui.button(tr("Abbrechen", "Cancel")).clicked() {
                close = true;
            }
        });
    });
    app.pages.remove = if close { None } else { Some((name, keep_data)) };
}
