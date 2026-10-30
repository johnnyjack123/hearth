//! `llm-harness-setup.exe`: the one file to download. It carries harness,
//! harnessd and harness-vmd, puts them into
//! `%LOCALAPPDATA%\Programs\llm-harness` (no admin rights) and runs
//! `harness setup`, which asks before everything else it changes. Run it
//! again to update: it stops the background service and waits for it and
//! its tray icon to end. A program still running after that (`harness run`
//! in a terminal) is moved aside, which Windows allows, and keeps running
//! from there; the next start takes the new one.

use std::fs;
use std::io::{BufRead as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

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

/// How long the background service and its tray icon get to end.
const STOP_WAIT: Duration = Duration::from_secs(30);

/// Whether a program file is running: Windows lets nobody open a running
/// program for writing.
fn in_use(path: &Path) -> bool {
    path.is_file() && fs::OpenOptions::new().write(true).open(path).is_err()
}

/// Waits until none of `paths` runs any more, at most `limit`. Returns
/// whether they all ended.
fn wait_until_free(paths: &[PathBuf], limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if !paths.iter().any(|path| in_use(path)) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Deletes programs moved aside by an earlier update, as far as they ended.
fn remove_old(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().contains(".exe.old") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Writes `bytes` next to `path` and moves it over, so a failure never
/// leaves half a program behind. A running program cannot be replaced but
/// can be renamed: it moves aside and keeps running from there. Returns
/// whether it had to.
fn replace(path: &Path, bytes: &[u8]) -> std::io::Result<bool> {
    let fresh = path.with_extension("exe.new");
    fs::write(&fresh, bytes)?;
    if fs::rename(&fresh, path).is_ok() {
        return Ok(false);
    }
    let aside = (1..)
        .map(|n| path.with_extension(format!("exe.old{n}")))
        .find(|candidate| !candidate.exists())
        .expect("a free name");
    let moved = fs::rename(path, &aside).and_then(|()| fs::rename(&fresh, path));
    if moved.is_err() {
        let _ = fs::remove_file(&fresh);
    }
    moved.map(|()| true)
}

/// Puts `programs` into `dir`; stops a running background service of an
/// earlier version first and waits for it and its tray icon to end.
fn install_into(dir: &Path, programs: &[(&str, &[u8])]) -> Result<(), String> {
    let harness = dir.join("harness.exe");
    if harness.is_file() {
        // The service holds its programs open; setup starts it again.
        let _ = Command::new(&harness).args(["daemon", "stop"]).status();
        let service = [harness.clone(), dir.join("harnessd.exe")];
        if service.iter().any(|program| in_use(program)) {
            println!("Warte, bis der Hintergrunddienst beendet ist …");
            if !wait_until_free(&service, STOP_WAIT) {
                println!(
                    "harness läuft noch (etwa `harness run` in einem Terminal). Es läuft mit der \
                     alten Version weiter, jeder neue Start nimmt die neue."
                );
            }
        }
    }
    fs::create_dir_all(dir).map_err(|e| format!("{} anlegen: {e}", dir.display()))?;
    remove_old(dir);
    for (name, bytes) in programs {
        replace(&dir.join(name), bytes)
            .map_err(|e| format!("{name} lässt sich nicht ersetzen ({e})."))?;
    }
    Ok(())
}

fn install(dir: &Path) -> Result<(), String> {
    if PROGRAMS.iter().any(|(_, bytes)| bytes.is_empty()) {
        return Err(
            "Dieser Build enthält keine Programme; das Einrichtungsprogramm kommt aus dem \
             Release (GitHub, Workflow „Windows“)."
                .to_owned(),
        );
    }
    install_into(dir, &PROGRAMS)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_programs_move_aside_instead_of_failing() {
        let dir = tempfile::tempdir().unwrap();
        // A copy of ping stands in for a harness that still runs.
        let running = dir.path().join("harnessd.exe");
        let system = std::env::var_os("SystemRoot")
            .map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        fs::copy(system.join("System32").join("PING.EXE"), &running).unwrap();
        let mut child = Command::new(&running)
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(in_use(&running));
        assert!(!wait_until_free(
            std::slice::from_ref(&running),
            Duration::from_millis(300)
        ));

        let installed = install_into(
            dir.path(),
            &[("harnessd.exe", b"neu"), ("harness-vmd.exe", b"vmd")],
        );
        assert_eq!(installed, Ok(()));
        assert_eq!(fs::read(&running).unwrap(), b"neu");
        assert_eq!(
            fs::read(dir.path().join("harness-vmd.exe")).unwrap(),
            b"vmd"
        );
        let aside = dir.path().join("harnessd.exe.old1");
        assert!(aside.exists());
        assert!(!dir.path().join("harnessd.exe.new").exists());

        // The next update deletes what moved aside once it ended.
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(wait_until_free(
            std::slice::from_ref(&aside),
            Duration::from_secs(5)
        ));
        install_into(dir.path(), &[("harnessd.exe", b"neuer")]).unwrap();
        assert!(!aside.exists());
        assert_eq!(fs::read(&running).unwrap(), b"neuer");
    }
}
