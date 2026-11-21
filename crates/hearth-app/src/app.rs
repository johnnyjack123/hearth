//! The window: navigation on the left, a page in the middle, the running
//! actions at the bottom.

use std::collections::HashSet;
use std::fs;
use std::time::Duration;

use eframe::egui::{self, RichText};
use hearth_core::config::ConfigDir;
use hearth_core::lang::tr;

use crate::jobs::{Jobs, Outcome};
use crate::model::{self, Refresher, Snapshot};
use crate::{FOCUS_FILE, mark, pages, theme};

/// The pages of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    /// What waits for the user (the start page).
    #[default]
    Approvals,
    /// Projects and their mode.
    Projects,
    /// Server, background service, VM.
    Computer,
    /// Logs and records.
    Log,
}

impl Page {
    const ALL: [Page; 4] = [Page::Approvals, Page::Projects, Page::Computer, Page::Log];

    /// Name for `--page`.
    pub fn name(self) -> &'static str {
        match self {
            Page::Approvals => "approvals",
            Page::Projects => "projects",
            Page::Computer => "computer",
            Page::Log => "log",
        }
    }

    /// The page with this `--page` name.
    pub fn from_name(name: &str) -> Option<Page> {
        Page::ALL.into_iter().find(|page| page.name() == name)
    }

    fn label(self) -> &'static str {
        match self {
            Page::Approvals => tr("Freigaben", "Approvals"),
            Page::Projects => tr("Projekte", "Projects"),
            Page::Computer => tr("Rechner", "Computer"),
            Page::Log => tr("Protokoll", "Log"),
        }
    }
}

/// A message under the page header, e.g. after a switch.
pub struct Notice {
    /// The text.
    pub text: String,
    /// Whether it reports an error.
    pub error: bool,
}

/// State of the whole window.
pub struct HearthApp {
    /// The configuration folder.
    pub config: ConfigDir,
    /// The page shown.
    pub page: Page,
    /// What was read from the folder last.
    pub snapshot: Snapshot,
    refresher: Refresher,
    /// Actions running or done.
    pub jobs: Jobs,
    /// The latest message.
    pub notice: Option<Notice>,
    /// Questions answered here, hidden until the service drops them.
    pub answered: HashSet<String>,
    seen: HashSet<String>,
    /// State of the pages (dialogs, inputs).
    pub pages: pages::State,
}

impl HearthApp {
    /// A window on `config`, opened at `page`.
    pub fn new(config: ConfigDir, page: Page) -> Self {
        let snapshot = model::load(&config);
        let seen = snapshot
            .approvals
            .iter()
            .map(|item| item.id.clone())
            .collect();
        HearthApp {
            config,
            page,
            snapshot,
            refresher: Refresher::new(Duration::from_millis(800)),
            jobs: Jobs::default(),
            notice: None,
            answered: HashSet::new(),
            seen,
            pages: pages::State::default(),
        }
    }

    /// Shows a message under the page header.
    pub fn say(&mut self, text: impl Into<String>, error: bool) {
        self.notice = Some(Notice {
            text: text.into(),
            error,
        });
    }

    /// Reads the folder again at the next frame.
    pub fn reload_soon(&mut self) {
        self.refresher.soon();
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        if !self.refresher.due() {
            return;
        }
        self.snapshot = model::load(&self.config);
        let waiting: HashSet<String> = self
            .snapshot
            .approvals
            .iter()
            .map(|item| item.id.clone())
            .collect();
        self.answered.retain(|id| waiting.contains(id));
        // A new question asks for attention, as a notification would.
        if waiting.iter().any(|id| !self.seen.contains(id)) {
            self.page = Page::Approvals;
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
        }
        self.seen = waiting;
        let focus = self.config.root().join(FOCUS_FILE);
        if let Ok(wanted) = fs::read_to_string(&focus) {
            let _ = fs::remove_file(&focus);
            if let Some(page) = Page::from_name(wanted.trim()) {
                self.page = page;
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }

    fn navigation(&mut self, ui: &mut egui::Ui) {
        let t = theme::tokens(ui.ctx());
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let texture = self.pages.logo(ui.ctx());
            ui.add(egui::Image::new((texture.id(), egui::vec2(26.0, 26.0))));
            ui.label(RichText::new("Hearth").size(17.0).strong());
        });
        ui.add_space(18.0);
        for page in Page::ALL {
            let waiting = if page == Page::Approvals {
                self.snapshot.waiting()
            } else {
                0
            };
            let selected = self.page == page;
            let label = if waiting > 0 {
                format!("{}  ({waiting})", page.label())
            } else {
                page.label().to_owned()
            };
            let text = RichText::new(label).color(if selected {
                t.foreground
            } else {
                t.muted_foreground
            });
            let button = egui::Button::new(text)
                .fill(if selected {
                    t.accent
                } else {
                    egui::Color32::TRANSPARENT
                })
                .stroke(egui::Stroke::NONE)
                .min_size(egui::vec2(ui.available_width(), 34.0));
            if ui.add(button).clicked() {
                self.page = page;
                self.notice = None;
            }
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(12.0);
            let (dot, text) = match &self.snapshot.service {
                Some(status) if status.connection == "connected" => {
                    (t.success, tr("Verbunden", "Connected"))
                }
                Some(status) if status.connection == "unpaired" => {
                    (t.warning, tr("Nicht gekoppelt", "Not paired"))
                }
                Some(_) => (t.warning, tr("Verbindet …", "Connecting …")),
                None => (t.muted_foreground, tr("Dienst aus", "Service off")),
            };
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, dot);
                ui.label(RichText::new(text).small().color(t.muted_foreground));
            });
        });
    }

    fn jobs_panel(&mut self, ui: &mut egui::Ui) {
        let t = theme::tokens(ui.ctx());
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Aufgaben", "Tasks")).strong());
            if !self.jobs.busy() && ui.small_button(tr("Leeren", "Clear")).clicked() {
                self.jobs.clear_finished();
            }
        });
        egui::ScrollArea::vertical()
            .max_height(170.0)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for job in self.jobs.all() {
                    let (color, state) = match job.outcome() {
                        Outcome::Running => (t.primary, tr("läuft", "running")),
                        Outcome::Done => (t.success, tr("fertig", "done")),
                        Outcome::Failed => (t.destructive, tr("fehlgeschlagen", "failed")),
                    };
                    ui.horizontal(|ui| {
                        if job.outcome() == Outcome::Running {
                            ui.spinner();
                        }
                        ui.label(RichText::new(&job.title).strong());
                        theme::badge(ui, state, color);
                    });
                    let lines = job.lines();
                    let shown = &lines[lines.len().saturating_sub(12)..];
                    if !shown.is_empty() {
                        ui.label(
                            RichText::new(shown.join("\n"))
                                .monospace()
                                .small()
                                .color(t.muted_foreground),
                        );
                    }
                    ui.add_space(4.0);
                }
            });
        ui.add_space(6.0);
    }
}

impl eframe::App for HearthApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.refresh(&ctx);
        // Files change without events; once a second is enough and cheap.
        ctx.request_repaint_after(Duration::from_secs(1));
        let t = theme::tokens(&ctx);

        egui::Panel::left("navigation")
            .exact_size(210.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(t.card)
                    .stroke(egui::Stroke::new(1.0, t.border))
                    .inner_margin(egui::Margin::symmetric(10, 0)),
            )
            .show(ui, |ui| self.navigation(ui));
        if !self.jobs.all().is_empty() {
            egui::Panel::bottom("jobs")
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .fill(t.card)
                        .stroke(egui::Stroke::new(1.0, t.border))
                        .inner_margin(egui::Margin::symmetric(16, 0)),
                )
                .show(ui, |ui| self.jobs_panel(ui));
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(t.background)
                    .inner_margin(egui::Margin::symmetric(28, 20)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // set_max_width sets the width; never more than there is.
                        ui.set_max_width(ui.available_width().min(820.0));
                        ui.heading(RichText::new(self.page.label()).strong());
                        ui.add_space(4.0);
                        if let Some(notice) = &self.notice {
                            let color = if notice.error {
                                t.destructive
                            } else {
                                t.success
                            };
                            ui.label(RichText::new(&notice.text).color(color));
                        }
                        ui.add_space(8.0);
                        match self.page {
                            Page::Approvals => pages::approvals(self, ui),
                            Page::Projects => pages::projects(self, ui),
                            Page::Computer => pages::computer(self, ui),
                            Page::Log => pages::log(self, ui),
                        }
                    });
            });
        pages::dialogs(self, &ctx);
    }
}

/// Size of the window icon, also used for the logo in the navigation.
pub const LOGO_SIZE: usize = 64;

/// The logo as a texture, made once.
pub fn logo_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [LOGO_SIZE, LOGO_SIZE],
        &mark::rgba(LOGO_SIZE, mark::BRAND),
    );
    ctx.load_texture("hearth-logo", image, egui::TextureOptions::LINEAR)
}
