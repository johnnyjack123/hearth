//! The window off screen: what the pages show, and that their buttons
//! write what the service reads. With `HEARTH_APP_SCREENS=1` they are also
//! drawn (wgpu, no window on the desktop) into `target/app-screens/` for a
//! look by eye; CI runners have no graphics adapter for that.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use hearth_core::config::{ConfigDir, HostChange, RemoteChange};
use hearth_core::host::{Decision, Request, inbox};
use hearth_core::project::ProjectName;

use crate::app::{HearthApp, Page};
use crate::theme;

/// One test at a time draws: several wgpu devices at once crashed the
/// graphics driver of the development machine (Windows ARM64, DirectX 12).
static GPU: Mutex<()> = Mutex::new(());

fn gpu() -> MutexGuard<'static, ()> {
    GPU.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A configuration folder with two projects (one in the direct mode) and
/// a question waiting.
struct Fixture {
    _tmp: tempfile::TempDir,
    config: ConfigDir,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let config = ConfigDir::new(tmp.path().join("home"));
    config.init().unwrap();
    for name in ["website", "werkstatt"] {
        let repo = tmp.path().join(name);
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        config
            .create_project_named(&repo, Some(&ProjectName::new(name).unwrap()))
            .unwrap();
        config
            .set_remote(
                &ProjectName::new(name).unwrap(),
                RemoteChange {
                    enabled: Some(true),
                    ..RemoteChange::default()
                },
            )
            .unwrap();
    }
    config
        .set_host(
            &ProjectName::new("werkstatt").unwrap(),
            HostChange {
                enabled: Some(true),
                ..HostChange::default()
            },
        )
        .unwrap();
    inbox::write_pending(
        config.root(),
        &[inbox::Pending {
            id: "W7KD3P".into(),
            project: "werkstatt".into(),
            chat: "c1".into(),
            title: Some("Tests reparieren".into()),
            request: Request::Command {
                command: "cargo test --workspace".into(),
                shell: "bash".into(),
                cwd: "/home/erika/werkstatt".into(),
            },
            summary: "Befehl ausführen: cargo test --workspace".into(),
            why: "Befehle laufen mit allen deinen Rechten auf diesem Rechner. Build- und Testbefehle führen auch Code aus, den der Agent geschrieben haben kann.".into(),
            chat_wide: true,
            browser: false,
            since: 0,
        }],
    )
    .unwrap();
    Fixture { _tmp: tmp, config }
}

fn screens() -> bool {
    std::env::var_os("HEARTH_APP_SCREENS").is_some()
}

fn harness(config: ConfigDir, page: Page, theme: egui::Theme) -> Harness<'static, HearthApp> {
    let builder = Harness::builder().with_size(egui::vec2(1020.0, 700.0)).with_theme(theme);
    let builder = if screens() { builder.wgpu() } else { builder };
    builder
        .build_eframe(move |cc| {
            theme::apply(&cc.egui_ctx);
            HearthApp::new(config, page)
        })
}

/// Renders the window and keeps the picture for a look by eye.
fn picture(harness: &mut Harness<'static, HearthApp>, name: &str) {
    harness.run_steps(4);
    if !screens() {
        return;
    }
    let Ok(image) = harness.render() else {
        // No graphics adapter here (some CI runners): the checks still ran.
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/app-screens");
    std::fs::create_dir_all(&dir).unwrap();
    image.save(dir.join(format!("{name}.png"))).unwrap();
}

fn decision_file(root: &Path, id: &str) -> PathBuf {
    root.join(inbox::DECISIONS_DIR).join(format!("{id}.json"))
}

#[test]
fn a_question_is_answered_from_the_window() {
    let _gpu = gpu();
    let f = fixture();
    let root = f.config.root().to_owned();
    let mut h = harness(f.config.clone(), Page::Approvals, egui::Theme::Dark);
    picture(&mut h, "approvals-dark");
    h.get_by_label("cargo test --workspace");
    h.get_by_label("Diesen Befehl im Chat erlauben").click();
    h.run_steps(2);
    assert!(decision_file(&root, "W7KD3P").exists());
    assert_eq!(inbox::take(&root, "W7KD3P"), Some(Decision::Chat));
    // Hidden until the service drops it.
    assert!(h.query_by_label("cargo test --workspace").is_none());
}

#[test]
fn projects_switch_their_mode_in_the_window() {
    let _gpu = gpu();
    let f = fixture();
    let config = f.config.clone();
    let mut h = harness(f.config.clone(), Page::Projects, egui::Theme::Light);
    picture(&mut h, "projects-light");
    let website = ProjectName::new("website").unwrap();
    assert!(!config.project_config(&website).unwrap().host.enabled);
    // The first radio button of that name belongs to the first project.
    h.get_all_by_label("Direkt auf dem Rechner")
        .next()
        .unwrap()
        .click();
    h.run_steps(2);
    assert!(config.project_config(&website).unwrap().host.enabled);
    picture(&mut h, "projects-switched");
}

#[test]
fn the_other_pages_draw() {
    let _gpu = gpu();
    let f = fixture();
    let mut h = harness(f.config.clone(), Page::Computer, egui::Theme::Dark);
    picture(&mut h, "computer-dark");
    h.get_by_label("Einrichtung prüfen");
    let mut h = harness(f.config, Page::Log, egui::Theme::Light);
    picture(&mut h, "log-light");
    h.get_by_label("Hintergrunddienst");
}
