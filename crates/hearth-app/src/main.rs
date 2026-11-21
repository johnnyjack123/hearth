//! `hearth-app`: the desktop app of hearth. One window for what the
//! terminal did so far: the projects and their mode (sandbox or direct),
//! the questions of the direct mode and other approvals, the pairing with
//! the server, the background service and the VM, and the logs.
//!
//! It runs only while its window is open; the background service and the
//! tray icon stay separate, small processes. A second start brings the
//! running window to the front instead.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod jobs;
mod mark;
mod model;
mod pages;
mod theme;

#[cfg(test)]
mod tests;

use std::fs::{self, File, OpenOptions};

use eframe::egui;
use hearth_core::config::ConfigDir;

/// A request of a second start to show the running window.
const FOCUS_FILE: &str = "app.focus";

/// Holds the app's lock in the configuration folder; `None` if another
/// window of this configuration is open.
fn single_instance(config: &ConfigDir) -> Option<File> {
    fs::create_dir_all(config.root()).ok()?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(config.root().join("app.lock"))
        .ok()?;
    file.try_lock().ok().map(|()| file)
}

fn main() -> eframe::Result {
    let config = match ConfigDir::open_default() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let page = std::env::args()
        .skip_while(|arg| arg != "--page")
        .nth(1)
        .and_then(|name| app::Page::from_name(&name));
    let Some(_lock) = single_instance(&config) else {
        // The open window notices this and comes to the front.
        let _ = fs::write(
            config.root().join(FOCUS_FILE),
            page.map(app::Page::name).unwrap_or_default(),
        );
        return Ok(());
    };
    let _ = fs::remove_file(config.root().join(FOCUS_FILE));

    // OpenGL needs far less memory than wgpu (measured on Windows ARM64:
    // 78 against 315 MB private).
    let renderer = match std::env::var(RENDERER_ENV).as_deref() {
        Ok("wgpu") => eframe::Renderer::Wgpu,
        _ => eframe::Renderer::Glow,
    };
    let icon = egui::IconData {
        rgba: mark::rgba(64, mark::BRAND),
        width: 64,
        height: 64,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Hearth")
            .with_app_id("io.hearth.app")
            .with_inner_size([1020.0, 700.0])
            .with_min_inner_size([760.0, 480.0])
            .with_icon(icon),
        renderer,
        ..Default::default()
    };
    let result = eframe::run_native(
        "Hearth",
        options,
        Box::new(move |creation| {
            theme::apply(&creation.egui_ctx);
            Ok(Box::new(app::HearthApp::new(
                config,
                page.unwrap_or_default(),
            )))
        }),
    );
    // Without OpenGL (some virtual machines) the window comes up with wgpu;
    // winit starts only once per process, so in a new one.
    if result.is_err() && renderer == eframe::Renderer::Glow {
        drop(_lock);
        let started = std::env::current_exe().and_then(|exe| {
            std::process::Command::new(exe)
                .args(std::env::args_os().skip(1))
                .env(RENDERER_ENV, "wgpu")
                .spawn()
        });
        if started.is_ok() {
            return Ok(());
        }
    }
    result
}

/// `glow` or `wgpu`; set by the app itself when OpenGL failed.
const RENDERER_ENV: &str = "HEARTH_APP_RENDERER";
