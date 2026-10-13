//! `harness-gateway serve` runs the proxy in the gateway container;
//! `harness-gateway ctl` talks to it from inside the same container and is
//! what the host runs through `podman exec`.

use std::env;
use std::process::ExitCode;

const USAGE: &str = "\
Aufruf:
  harness-gateway serve [--listen ADRESSE] [--control SOCKET]
  harness-gateway ctl [--control SOCKET] [ANFRAGE ...]

ctl sendet die Anfragen (je ein Argument) und beendet sich nach den Antworten.
Ohne Anfragen leitet es stdin zum Gateway und dessen Antworten nach stdout.";

/// Port the base image's `HTTP(S)_PROXY` points to.
const DEFAULT_LISTEN: &str = "0.0.0.0:8888";

/// In /tmp because the read-only root leaves only the tmpfs mounts writable.
const DEFAULT_CONTROL: &str = "/tmp/harness-gateway/control.sock";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some((command, rest)) = args.split_first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let result = match command.as_str() {
        "serve" => options(rest, &["--listen", "--control"]).and_then(|(flags, extra)| {
            if !extra.is_empty() {
                return Err(format!("unerwartete Argumente: {}", extra.join(" ")));
            }
            platform::serve(
                flag(&flags, "--listen").unwrap_or(DEFAULT_LISTEN),
                flag(&flags, "--control").unwrap_or(DEFAULT_CONTROL),
            )
        }),
        "ctl" => options(rest, &["--control"]).and_then(|(flags, requests)| {
            platform::ctl(
                flag(&flags, "--control").unwrap_or(DEFAULT_CONTROL),
                &requests,
            )
        }),
        "--version" => {
            println!("harness-gateway {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(code) => code,
        Err(message) => {
            eprintln!("harness-gateway: {message}");
            ExitCode::from(2)
        }
    }
}

type Flags<'a> = Vec<(&'a str, &'a str)>;

/// Splits leading `--flag value` pairs from the remaining arguments.
fn options<'a>(args: &'a [String], known: &[&str]) -> Result<(Flags<'a>, Vec<String>), String> {
    let mut flags = Vec::new();
    let mut rest = args;
    while let Some(flag) = rest.first().filter(|a| a.starts_with("--")) {
        if !known.contains(&flag.as_str()) {
            return Err(format!("unbekannte Option {flag}"));
        }
        let value = rest
            .get(1)
            .ok_or_else(|| format!("{flag} braucht einen Wert"))?;
        flags.push((flag.as_str(), value.as_str()));
        rest = &rest[2..];
    }
    Ok((flags, rest.to_vec()))
}

fn flag<'a>(flags: &Flags<'a>, name: &str) -> Option<&'a str> {
    flags
        .iter()
        .rev()
        .find(|(f, _)| *f == name)
        .map(|(_, v)| *v)
}

#[cfg(unix)]
mod platform {
    use std::fs;
    use std::io::{self, BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
    use std::process::ExitCode;
    use std::thread;
    use std::time::Duration;

    use harness_gateway::control::WATCH_IDLE_SECS;
    use harness_gateway::{Gateway, Options, proxy};

    pub fn serve(listen: &str, control: &str) -> Result<ExitCode, String> {
        let gateway = Gateway::new(Options {
            print_log: true,
            ..Options::default()
        });
        let listener = TcpListener::bind(listen).map_err(|e| format!("{listen}: {e}"))?;
        let control_listener = bind_control(Path::new(control))?;
        let controller = gateway.clone();
        thread::spawn(move || {
            for stream in control_listener.incoming().flatten() {
                let gateway = controller.clone();
                thread::spawn(move || {
                    // Watchers ping; anything silent for longer is gone.
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(WATCH_IDLE_SECS)));
                    if let Ok(reader) = stream.try_clone() {
                        gateway.serve_control(BufReader::new(reader), stream);
                    }
                });
            }
        });
        println!("harness-gateway lauscht auf {listen}, Steuerung über {control}");
        proxy::serve(&gateway, listener).map_err(|e| e.to_string())?;
        Ok(ExitCode::SUCCESS)
    }

    /// The socket lives in a directory only the gateway user can enter; the
    /// instances cannot reach this container's filesystem at all, this is
    /// for everything else running in the container.
    fn bind_control(path: &Path) -> Result<UnixListener, String> {
        if let Some(dir) = path.parent() {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
                .map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
        let listener = UnixListener::bind(path).map_err(|e| format!("{}: {e}", path.display()))?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(listener)
    }

    pub fn ctl(control: &str, requests: &[String]) -> Result<ExitCode, String> {
        let stream = UnixStream::connect(control)
            .map_err(|e| format!("Gateway nicht erreichbar ({control}): {e}"))?;
        if requests.is_empty() {
            return relay(stream);
        }
        let mut writer = stream.try_clone().map_err(|e| e.to_string())?;
        for request in requests.iter().map(String::as_str).chain(["bye"]) {
            if request.contains('\n') {
                return Err("Anfragen dürfen keinen Zeilenumbruch enthalten".to_owned());
            }
            writer
                .write_all(format!("{request}\n").as_bytes())
                .map_err(|e| e.to_string())?;
        }
        let mut failed = false;
        let stdout = io::stdout();
        let mut out = stdout.lock();
        for line in BufReader::new(stream).lines() {
            let line = line.map_err(|e| e.to_string())?;
            failed |= line.starts_with("error ");
            let _ = writeln!(out, "{line}");
        }
        Ok(if failed {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        })
    }

    /// stdin → gateway, gateway → stdout, until the gateway closes. Exits
    /// then even if stdin is still open, because `podman exec` does not
    /// always forward the end of stdin.
    fn relay(stream: UnixStream) -> Result<ExitCode, String> {
        let mut to_gateway = stream.try_clone().map_err(|e| e.to_string())?;
        thread::spawn(move || {
            let _ = io::copy(&mut io::stdin().lock(), &mut to_gateway);
            let _ = to_gateway.shutdown(std::net::Shutdown::Write);
        });
        let stdout = io::stdout();
        let mut out = stdout.lock();
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
                break;
            }
        }
        Ok(ExitCode::SUCCESS)
    }
}

#[cfg(not(unix))]
mod platform {
    use std::process::ExitCode;

    pub fn serve(_listen: &str, _control: &str) -> Result<ExitCode, String> {
        Err("läuft nur unter Linux (im Gateway-Container)".to_owned())
    }

    pub fn ctl(_control: &str, _requests: &[String]) -> Result<ExitCode, String> {
        Err("läuft nur unter Linux (im Gateway-Container)".to_owned())
    }
}
