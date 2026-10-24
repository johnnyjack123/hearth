//! `llm-harness-setup.exe`: the one file to download. It carries harness,
//! harnessd and harness-vmd, puts them into
//! `%LOCALAPPDATA%\Programs\llm-harness` (no admin rights) and runs
//! `harness setup`, which asks before everything else it changes. Run it
//! again to update: it stops the background service first, whose files
//! would otherwise be locked.

use std::fs;
use std::io::{BufRead as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The programs, embedded by `build.rs`; empty outside release builds.
const PROGRAMS: [(&str, &[u8]); 3] = [
    (
        "harness.exe",
        include_bytes!(concat!(env!("OUT_DIR"), "/harness.exe")),
    ),
    (
        "harnessd.exe",
        include_bytes!(concat!(env!("OUT_DIR"), "/harnessd.exe")),
    ),
    (
        "harness-vmd.exe",
        include_bytes!(concat!(env!("OUT_DIR"), "/harness-vmd.exe")),
    ),
];

fn target_dir() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(local).join("Programs").join("llm-harness"))
}

/// Writes `bytes` next to `path` and moves it over, so a failure never
/// leaves half a program behind.
fn replace(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let fresh = path.with_extension("exe.new");
    fs::write(&fresh, bytes)?;
    fs::rename(&fresh, path).inspect_err(|_| {
        let _ = fs::remove_file(&fresh);
    })
}

fn install(dir: &Path) -> Result<(), String> {
    if PROGRAMS.iter().any(|(_, bytes)| bytes.is_empty()) {
        return Err(
            "Dieser Build enthält keine Programme; das Einrichtungsprogramm kommt aus dem \
             Release (GitHub, Workflow „Windows“)."
                .to_owned(),
        );
    }
    let harness = dir.join("harness.exe");
    if harness.is_file() {
        // The service holds its programs open; setup starts it again.
        let _ = Command::new(&harness).args(["daemon", "stop"]).status();
    }
    fs::create_dir_all(dir).map_err(|e| format!("{} anlegen: {e}", dir.display()))?;
    for (name, bytes) in PROGRAMS {
        replace(&dir.join(name), bytes).map_err(|e| {
            format!(
                "{name} lässt sich nicht ersetzen ({e}). Läuft noch `harness` (etwa `harness run`)? \
                 Bitte beenden und noch einmal starten."
            )
        })?;
    }
    Ok(())
}

fn wait_for_enter() {
    print!("\nFenster mit Enter schließen. ");
    let _ = std::io::stdout().flush();
    let _ = std::io::stdin().lock().lines().next();
}

fn main() {
    println!("LLM Harness {} – Einrichtung\n", env!("CARGO_PKG_VERSION"));
    let Some(dir) = target_dir() else {
        println!("LOCALAPPDATA ist nicht gesetzt; wohin installieren?");
        wait_for_enter();
        std::process::exit(1);
    };
    if let Err(error) = install(&dir) {
        println!("✗ {error}");
        wait_for_enter();
        std::process::exit(1);
    }
    println!("✓ Programme in {}\n", dir.display());
    let status = Command::new(dir.join("harness.exe")).arg("setup").status();
    let ok = status.is_ok_and(|s| s.success());
    if !ok {
        println!(
            "\nNoch nicht alles eingerichtet. Nach dem Beheben (etwa einem Neustart) dieses \
             Programm oder `harness setup` erneut starten."
        );
    }
    wait_for_enter();
    std::process::exit(i32::from(!ok));
}
