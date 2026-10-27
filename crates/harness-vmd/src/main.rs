//! `harness-vmd`: the Windows service that starts and stops the Podman
//! machine for one user (ROADMAP Phase 2b, docs/spikes/phase-0.md §12.3).
//! Installed with `harness vmd install`, which asks once for admin rights.
//!
//! `harness-vmd console` runs the same loop in the foreground; it needs an
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
        eprintln!("harness-vmd: {error}");
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("harness-vmd gibt es nur unter Windows.");
    std::process::exit(1);
}

/// Loads the configuration and answers requests until the process ends.
#[cfg(windows)]
fn serve() -> Result<(), String> {
    use std::sync::Mutex;

    use harness_core::vmd::{Request, Response, service_dir};

    let config = config::Config::load(&service_dir().join("vmd.toml"))?;
    let user_sid = config.user_sid.clone();
    let vm = machine::Vm::new(config);
    vm.prepare_dirs();
    log::line(&format!("Dienst bereit, Pipe {}", pipe::default_name()));
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
                log::line(&format!("Anfrage: {request:?}"));
                let outcome = match request {
                    Request::Start => vm.start(),
                    Request::Stop => vm.stop(),
                    Request::Compact => vm.compact(),
                    Request::StopIdle => vm.stop_idle(),
                    _ => vm.stop().and_then(|_| vm.start()),
                };
                log::line(&format!("Ergebnis: {outcome:?}"));
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
