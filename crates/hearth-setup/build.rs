//! Embeds the programs the installer unpacks. The release workflow builds
//! them first and points `HEARTH_BUNDLE_DIR` at them; without it (any
//! other build) the installer gets empty stand-ins and says so when run.

use std::env;
use std::fs;
use std::path::PathBuf;

const PROGRAMS: [&str; 4] = ["hearth.exe", "hearthd.exe", "hearth-vmd.exe", "hearth-app.exe"];

fn main() {
    // Windows treats programs with "setup" in their name as installers and
    // asks for admin rights; this one installs per user and must not run
    // elevated, or `hearth setup` would set up the wrong account.
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo::rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo::rustc-link-arg-bins=/MANIFESTUAC:level='asInvoker'");
    }
    println!("cargo::rerun-if-env-changed=HEARTH_BUNDLE_DIR");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let bundle = env::var_os("HEARTH_BUNDLE_DIR").map(PathBuf::from);
    for name in PROGRAMS {
        let target = out.join(name);
        match &bundle {
            Some(dir) => {
                let source = dir.join(name);
                println!("cargo::rerun-if-changed={}", source.display());
                fs::copy(&source, &target)
                    .unwrap_or_else(|e| panic!("{} einbetten: {e}", source.display()));
            }
            None => fs::write(&target, b"").expect("Platzhalter schreiben"),
        }
    }
}
