//! `hearth-setup.exe`: the one file to download. It carries hearth,
//! hearthd and hearth-vmd, puts them into
//! `%LOCALAPPDATA%\Programs\hearth` (no admin rights) and runs
//! `hearth setup`, which asks before everything else it changes. Run it
//! again to update: it stops the background service and waits for it and
//! its tray icon to end. A program still running after that (`hearth run`
//! in a terminal) is moved aside, which Windows allows, and keeps running
//! from there; the next start takes the new one.

#[macro_use]
mod lang;

use std::fs;
use std::io::{BufRead as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use lang::tr;

/// The programs, embedded by `build.rs`; empty outside release builds.
const PROGRAMS: [(&str, &[u8]); 4] = [
    (
        "hearth.exe",
        include_bytes!(concat!(env!("OUT_DIR"), "/hearth.exe")),
    ),
    (
        "hearthd.exe",
        include_bytes!(concat!(env!("OUT_DIR"), "/hearthd.exe")),
    ),
    (
        "hearth-vmd.exe",
        include_bytes!(concat!(env!("OUT_DIR"), "/hearth-vmd.exe")),
    ),
    (
        "hearth-app.exe",
        include_bytes!(concat!(env!("OUT_DIR"), "/hearth-app.exe")),
    ),
];

fn target_dir() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(local).join("Programs").join("hearth"))
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
    let harness = dir.join("hearth.exe");
    if harness.is_file() {
        // The service holds its programs open; setup starts it again.
        let _ = Command::new(&harness).args(["daemon", "stop"]).status();
        let service = [harness.clone(), dir.join("hearthd.exe")];
        if service.iter().any(|program| in_use(program)) {
            println!(
                "{}",
                tr(
                    "Warte, bis der Hintergrunddienst beendet ist …",
                    "Waiting for the background service to end …"
                )
            );
            if !wait_until_free(&service, STOP_WAIT) {
                println!(
                    "{}",
                    tr(
                        "hearth läuft noch (etwa `hearth run` in einem Terminal). Es läuft mit \
                         der alten Version weiter, jeder neue Start nimmt die neue.",
                        "hearth is still running (e.g. `hearth run` in a terminal). It keeps \
                         running with the old version, every new start uses the new one."
                    )
                );
            }
        }
    }
    fs::create_dir_all(dir)
        .map_err(|e| tr!("{} anlegen: {e}", "Creating {}: {e}", dir.display()))?;
    remove_old(dir);
    for (name, bytes) in programs {
        replace(&dir.join(name), bytes).map_err(|e| {
            tr!(
                "{name} lässt sich nicht ersetzen ({e}).",
                "{name} cannot be replaced ({e})."
            )
        })?;
    }
    Ok(())
}

fn install(dir: &Path) -> Result<(), String> {
    if PROGRAMS.iter().any(|(_, bytes)| bytes.is_empty()) {
        return Err(tr(
            "Dieser Build enthält keine Programme; das Einrichtungsprogramm kommt aus dem \
             Release (GitHub, Workflow „Windows“).",
            "This build contains no programs; the setup program comes from the release \
             (GitHub, workflow “Windows”).",
        )
        .to_owned());
    }
    install_into(dir, &PROGRAMS)
}

/// Programs of `names` that are not in `dir`. Windows Defender sometimes
/// takes an unsigned program for malware (a guess of its machine learning,
/// `…!ml`) and deletes it, often a minute after it was written; setup then
/// cannot start, and the background service neither.
fn missing<'a>(dir: &Path, names: &[&'a str]) -> Vec<&'a str> {
    names
        .iter()
        .copied()
        .filter(|name| !dir.join(name).is_file())
        .collect()
}

fn quarantine_hint(names: &[&str]) -> String {
    tr!(
        "✗ {} fehlt nach der Installation. Meist hat Windows Defender die Datei für \
         Schadsoftware gehalten und entfernt, ein Fehlalarm bei unsignierten Programmen \
         (etwa „Trojan:Win32/Bearfoos.A!ml“).\n  Windows-Sicherheit → Viren- & \
         Bedrohungsschutz → Schutzverlauf: den Eintrag öffnen, „Aktionen“ → \
         „Wiederherstellen“. Danach dieses Programm erneut starten.",
        "✗ {} is missing after the installation. Usually Windows Defender took it for \
         malware and removed it, a false alarm with unsigned programs (e.g. \
         “Trojan:Win32/Bearfoos.A!ml”).\n  Windows Security → Virus & threat protection \
         → Protection history: open the entry, “Actions” → “Restore”. Then start this \
         program again.",
        names.join(", ")
    )
}

fn program_names() -> Vec<&'static str> {
    PROGRAMS.iter().map(|(name, _)| *name).collect()
}

fn wait_for_enter() {
    print!(
        "{}",
        tr(
            "\nFenster mit Enter schließen. ",
            "\nPress Enter to close the window. "
        )
    );
    let _ = std::io::stdout().flush();
    let _ = std::io::stdin().lock().lines().next();
}

fn main() {
    println!(
        "{}",
        tr!(
            "Hearth {} – Einrichtung\n",
            "Hearth {} – setup\n",
            env!("CARGO_PKG_VERSION")
        )
    );
    let Some(dir) = target_dir() else {
        println!(
            "{}",
            tr(
                "LOCALAPPDATA ist nicht gesetzt; wohin installieren?",
                "LOCALAPPDATA is not set; where to install?"
            )
        );
        wait_for_enter();
        std::process::exit(1);
    };
    if let Err(error) = install(&dir) {
        println!("✗ {error}");
        wait_for_enter();
        std::process::exit(1);
    }
    println!(
        "{}",
        tr!("✓ Programme in {}\n", "✓ Programs in {}\n", dir.display())
    );
    let gone = missing(&dir, &program_names());
    if !gone.is_empty() {
        println!("{}", quarantine_hint(&gone));
        wait_for_enter();
        std::process::exit(1);
    }
    let status = Command::new(dir.join("hearth.exe")).arg("setup").status();
    let ok = status.is_ok_and(|s| s.success());
    // Defender may strike only once setup started the programs.
    let gone = missing(&dir, &program_names());
    if !gone.is_empty() {
        println!("\n{}", quarantine_hint(&gone));
    } else if !ok {
        println!(
            "{}",
            tr(
                "\nNoch nicht alles eingerichtet. Nach dem Beheben (etwa einem Neustart) dieses \
                 Programm oder `hearth setup` erneut starten.",
                "\nNot everything is set up yet. After fixing it (e.g. with a restart), start \
                 this program or `hearth setup` again."
            )
        );
    }
    wait_for_enter();
    std::process::exit(i32::from(!ok || !gone.is_empty()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_programs_that_vanished() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("hearthd.exe"), b"x").unwrap();
        let names = ["hearth.exe", "hearthd.exe", "hearth-vmd.exe"];
        assert_eq!(
            missing(dir.path(), &names),
            ["hearth.exe", "hearth-vmd.exe"]
        );
        let hint = quarantine_hint(&["hearth.exe"]);
        assert!(hint.contains("hearth.exe fehlt"), "{hint}");
        assert!(hint.contains("Schutzverlauf"), "{hint}");
        assert!(!hint.contains("  Schadsoftware"), "{hint}");
    }

    // Runs a copy of Windows' ping; the setup program exists for Windows only.
    #[cfg(windows)]
    #[test]
    fn running_programs_move_aside_instead_of_failing() {
        let dir = tempfile::tempdir().unwrap();
        // A copy of ping stands in for a harness that still runs.
        let running = dir.path().join("hearthd.exe");
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
            &[("hearthd.exe", b"neu"), ("hearth-vmd.exe", b"vmd")],
        );
        assert_eq!(installed, Ok(()));
        assert_eq!(fs::read(&running).unwrap(), b"neu");
        assert_eq!(fs::read(dir.path().join("hearth-vmd.exe")).unwrap(), b"vmd");
        let aside = dir.path().join("hearthd.exe.old1");
        assert!(aside.exists());
        assert!(!dir.path().join("hearthd.exe.new").exists());

        // The next update deletes what moved aside once it ended.
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(wait_until_free(
            std::slice::from_ref(&aside),
            Duration::from_secs(5)
        ));
        install_into(dir.path(), &[("hearthd.exe", b"neuer")]).unwrap();
        assert!(!aside.exists());
        assert_eq!(fs::read(&running).unwrap(), b"neuer");
    }
}
