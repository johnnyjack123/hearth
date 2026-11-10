//! `hearth-vmd`: the Windows service that starts and stops the Podman
//! machine for one user (ROADMAP Phase 2b, docs/spikes/phase-0.md §12.3).
//! Installed with `hearth vmd install`, which asks once for admin rights.
//!
//! `hearth-vmd console` runs the same loop in the foreground; it needs an
//! elevated prompt and is meant for debugging.

#[cfg(windows)]
mod config;
#[cfg(windows)]
mod log;
#[cfg(windows)]
mod machine;
#[cfg(windows)]
mod pipe;
#[cfg(windows)]
mod service;

#[cfg(windows)]
fn main() {
    let console = std::env::args().nth(1).as_deref() == Some("console");
    let result = if console {
        log::echo();
        serve()
    } else {
        service::run(serve)
    };
    if let Err(error) = result {
        eprintln!("hearth-vmd: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!(
        "{}",
        hearth_core::lang::tr(
            "hearth-vmd gibt es nur unter Windows.",
            "hearth-vmd exists only on Windows."
        )
    );
    std::process::exit(1);
}

/// Loads the configuration and answers requests until the process ends.
#[cfg(windows)]
fn serve() -> Result<(), String> {
    use std::sync::Mutex;

    use hearth_core::tr;
    use hearth_core::vmd::{Request, Response, service_dir};

    // Only administrators may have shaped what SYSTEM reads from here
    // (security review M8).
    if let Some(root) = service_dir().parent() {
        hearth_core::vmd::check_service_root(root, None, false)
            .map_err(|error| error.to_string())?;
    }
    let config = config::Config::load(&service_dir().join("vmd.toml"))?;
    let user_sid = config.user_sid.clone();
    let vm = machine::Vm::new(config);
    vm.prepare_dirs();
    log::line(&tr!(
        "Dienst bereit, Pipe {}",
        "Service ready, pipe {}",
        pipe::default_name()
    ));
    // One change at a time; `status` does not wait for it.
    let busy = Mutex::new(());
    let handler = move |request: Request| {
        let outcome = match request {
            Request::Status => Ok(String::new()),
            Request::Start
            | Request::Stop
            | Request::Restart
            | Request::Compact
            | Request::StopIdle => {
                let _guard = busy.lock().unwrap_or_else(|p| p.into_inner());
                log::line(&tr!("Anfrage: {request:?}", "Request: {request:?}"));
                let outcome = match request {
                    Request::Start => vm.start(),
                    Request::Stop => vm.stop(),
                    Request::Compact => vm.compact(),
                    Request::StopIdle => vm.stop_idle(),
                    _ => vm.stop().and_then(|_| vm.start()),
                };
                log::line(&tr!("Ergebnis: {outcome:?}", "Outcome: {outcome:?}"));
                outcome
            }
        };
        let state = vm.state();
        match outcome {
            Ok(message) => Response {
                ok: true,
                state,
                message,
            },
            Err(message) => Response {
                ok: false,
                state,
                message,
            },
        }
    };
    pipe::serve(pipe::default_name(), &user_sid, handler)
        .map_err(|e| format!("Pipe {}: {e}", pipe::default_name()))
}
