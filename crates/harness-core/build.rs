//! Embeds the gateway program the release workflow built for the VM
//! (`HARNESS_GATEWAY_BINARY`, .github/workflows/windows.yml), so the gateway
//! image is only packed in the VM, not compiled there. Without it (any other
//! build) an empty stand-in tells `image.rs` to compile from the sources.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-env-changed=HARNESS_GATEWAY_BINARY");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let target = out.join("harness-gateway");
    let Some(source) = env::var_os("HARNESS_GATEWAY_BINARY").map(PathBuf::from) else {
        fs::write(&target, b"").expect("Platzhalter schreiben");
        return;
    };
    println!("cargo::rerun-if-changed={}", source.display());
    let program =
        fs::read(&source).unwrap_or_else(|e| panic!("{} einbetten: {e}", source.display()));
    // The VM runs on the architecture of the computer (Hyper-V emulates
    // nothing): a gateway for the other one would not start there.
    let machine: u16 = match env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => 0x3e,
        Ok("aarch64") => 0xb7,
        other => panic!("kein Gateway für die Architektur {other:?}"),
    };
    let fits = program.len() > 20
        && program.starts_with(b"\x7fELF")
        && u16::from_le_bytes([program[18], program[19]]) == machine;
    assert!(
        fits,
        "{} ist kein Linux-Programm für diese Architektur",
        source.display()
    );
    fs::write(&target, program).expect("Gateway einbetten");
}
