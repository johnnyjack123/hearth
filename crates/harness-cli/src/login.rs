//! `harness login` / `harness logout`: pairing this computer with the harness
//! server, so harnesses get LLM access through it without ever holding a key.

use std::env;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};
use harness_core::Provider;
use harness_core::config::ConfigDir;
use harness_core::gateway::LlmUpstream;
use harness_core::lang::tr;
use harness_core::server::{
    self, DEFAULT_SERVER_URL, PairingPoll, ServerCa, ServerClient, delete_token, load_token,
    save_token,
};
use harness_core::tr;

/// Pairs this computer with a server (device flow).
pub fn login(config: &ConfigDir, url: Option<String>, open_browser: bool) -> anyhow::Result<bool> {
    let server = config.global()?.server;
    let url = url
        .or_else(|| server.url.clone())
        .unwrap_or_else(|| DEFAULT_SERVER_URL.to_owned());
    // A server with its own CA needs it from the first request on.
    let ca = ServerCa::from_config(&server)?;
    let client = ServerClient::new(&url, ca.as_ref())?;
    let previous = load_token(client.url())?;
    let pairing = client.start_pairing(&device_name())?;

    println!(
        "{}",
        tr!(
            "Bestätige diesen Code in der Web-App von {}:",
            "Confirm this code in the web app of {}:",
            client.url()
        )
    );
    println!();
    println!("    {}", pairing.user_code);
    println!();
    println!("    {}", pairing.verification_uri_complete);
    println!();
    if open_browser && open(&pairing.verification_uri_complete) {
        println!(
            "{}",
            tr(
                "Der Browser öffnet die Seite. Warte auf Bestätigung … (Strg+C bricht ab)",
                "The browser opens the page. Waiting for confirmation … (Ctrl+C cancels)"
            )
        );
    } else {
        println!(
            "{}",
            tr(
                "Warte auf Bestätigung … (Strg+C bricht ab)",
                "Waiting for confirmation … (Ctrl+C cancels)"
            )
        );
    }

    let expired = || {
        tr(
            "Der Code ist abgelaufen. `harness login` erneut starten",
            "The code has expired. Start `harness login` again",
        )
    };
    let deadline = Instant::now() + Duration::from_secs(pairing.expires_in);
    let interval = Duration::from_secs(pairing.interval.clamp(1, 30));
    let token = loop {
        if Instant::now() >= deadline {
            bail!(expired());
        }
        thread::sleep(interval);
        match client.poll_pairing(&pairing.device_code)? {
            PairingPoll::Pending => {}
            PairingPoll::Denied => bail!(tr(
                "In der Web-App abgelehnt, das Gerät ist nicht gekoppelt",
                "Refused in the web app, the device is not paired"
            )),
            PairingPoll::Expired => bail!(expired()),
            PairingPoll::Approved { token, .. } => break token,
        }
    };

    save_token(client.url(), &token)
        .context(tr("Geräte-Token speichern", "Saving the device token"))?;
    config.set_server_url(Some(client.url()))?;
    let me = client.device(&token)?;
    println!(
        "{}",
        tr!(
            "Angemeldet als {} ({}), Gerät „{}“.",
            "Signed in as {} ({}), device “{}”.",
            me.user.name,
            me.user.email,
            me.device.name
        )
    );
    // A re-login replaces the old token; it should not stay valid.
    if let Some(old) = previous.filter(|old| *old != token) {
        let _ = client.revoke(&old);
    }
    println!(
        "{}",
        tr(
            "Laufende Gateways übernehmen den Zugang mit `harness net start`.",
            "Running gateways take over the access with `harness net start`."
        )
    );
    announce_projects(config);
    // Web chats need this computer connected; the background service does
    // that from now on, also after the next logon.
    if crate::daemon::automatic() {
        match crate::daemon::install(config) {
            Ok(true) => println!(
                "{}",
                tr(
                    "Hintergrunddienst läuft und startet bei jeder Anmeldung: Web-Chats deines \
                     Kontos können die Projekte dieses Rechners nutzen (`harness daemon`).",
                    "Background service running and starting at every sign-in: web chats of your \
                     account can use the projects of this computer (`harness daemon`)."
                )
            ),
            Ok(false) => println!(
                "{}",
                tr(
                    "Hintergrunddienst startet nicht, siehe `harness daemon`.",
                    "Background service does not start, see `harness daemon`."
                )
            ),
            Err(error) => println!(
                "{}",
                tr!(
                    "Hintergrunddienst nicht eingerichtet: {error:#}",
                    "Background service not set up: {error:#}"
                )
            ),
        }
    }
    Ok(true)
}

/// Tells the server which projects this computer offers, so they appear in
/// the account right away; problems are only a note.
pub fn announce_projects(config: &ConfigDir) {
    let paired = match paired(config) {
        Ok(Some(paired)) => paired,
        Ok(None) => return,
        Err(error) => {
            println!(
                "{}",
                tr!(
                    "Projekte nicht an den Server gemeldet: {error:#}",
                    "Projects not reported to the server: {error:#}"
                )
            );
            return;
        }
    };
    let (client, token) = paired;
    if let Err(error) = client.set_projects(&token, &crate::connect::offered(config)) {
        println!(
            "{}",
            tr!(
                "Projekte nicht an den Server gemeldet: {error}",
                "Projects not reported to the server: {error}"
            )
        );
    }
}

/// Revokes this computer's token and forgets it.
pub fn logout(config: &ConfigDir) -> anyhow::Result<bool> {
    let server = config.global()?.server;
    let Some(url) = server.url.clone() else {
        println!("{}", tr("Nicht angemeldet.", "Not signed in."));
        return Ok(true);
    };
    if crate::daemon::running(config) {
        crate::daemon::stop(config);
    }
    // Running gateways hold a copy of the token; only the server can make
    // it worthless right away.
    let mut revoked = true;
    if let Some(token) = load_token(&url)? {
        let revoke = ServerCa::from_config(&server)
            .and_then(|ca| ServerClient::new(&url, ca.as_ref()))
            .and_then(|client| client.revoke(&token));
        match revoke {
            Ok(()) => println!(
                "{}",
                tr!("Gerät auf {url} entfernt.", "Device removed on {url}.")
            ),
            Err(error) => {
                revoked = false;
                println!(
                    "{}",
                    tr!(
                        "Konnte das Gerät auf dem Server nicht entfernen ({error}). Bitte in der \
                         Web-App unter Einstellungen → Geräte löschen.",
                        "Could not remove the device on the server ({error}). Please delete it in \
                         the web app under Settings → Devices."
                    )
                );
            }
        }
        delete_token(&url)?;
    }
    config.set_server_url(None)?;
    if revoked {
        println!(
            "{}",
            tr(
                "Abgemeldet. Laufende Gateways haben ab sofort keinen LLM-Zugang mehr.",
                "Signed out. Running gateways have no LLM access any more from now on."
            )
        );
    } else {
        println!(
            "{}",
            tr(
                "Abgemeldet. Laufende Gateways nutzen das Token, bis es in der Web-App gelöscht ist.",
                "Signed out. Running gateways use the token until it is deleted in the web app."
            )
        );
    }
    Ok(true)
}

/// The paired server and this computer's token, if any.
pub fn paired(config: &ConfigDir) -> anyhow::Result<Option<(ServerClient, String)>> {
    let server = config.global()?.server;
    let Some(url) = &server.url else {
        return Ok(None);
    };
    let Some(token) = load_token(url)? else {
        return Ok(None);
    };
    let ca = ServerCa::from_config(&server)?;
    Ok(Some((ServerClient::new(url, ca.as_ref())?, token)))
}

/// Where gateways send `llm.harness` requests, if this computer is paired.
/// Problems become a note instead of an error: without LLM access the
/// gateway still works, harnesses then need their own provider login.
pub fn gateway_llm(
    config: &ConfigDir,
    provider: &dyn Provider,
) -> anyhow::Result<(Option<LlmUpstream>, String)> {
    let server = config.global()?.server;
    let Some(url) = &server.url else {
        return Ok((
            None,
            tr(
                "nicht eingerichtet (harness login)",
                "not set up (harness login)",
            )
            .to_owned(),
        ));
    };
    let Some(token) = load_token(url)? else {
        return Ok((
            None,
            tr!(
                "kein Geräte-Token für {url} (harness login)",
                "no device token for {url} (harness login)"
            ),
        ));
    };
    let ca = match ServerCa::from_config(&server) {
        Ok(ca) => ca,
        Err(error) => return Ok((None, error.to_string())),
    };
    match server::llm_upstream(url, provider.host_address(), &token, ca.as_ref()) {
        Ok(upstream) if upstream.ca().is_empty() => {
            Ok((Some(upstream), tr!("über {url}", "through {url}")))
        }
        Ok(upstream) => Ok((
            Some(upstream),
            tr!("über {url} (eigene CA)", "through {url} (own CA)"),
        )),
        Err(error) => Ok((None, error.to_string())),
    }
}

fn device_name() -> String {
    let host = env::var("COMPUTERNAME")
        .or_else(|_| env::var("HOSTNAME"))
        .unwrap_or_default();
    let host: String = host
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(40)
        .collect();
    if host.is_empty() {
        "harness".to_owned()
    } else {
        tr!("harness auf {host}", "harness on {host}")
    }
}

/// Opens a web page in the default browser; returns whether that worked.
fn open(url: &str) -> bool {
    // The URL comes from the server; only ever hand web addresses to the OS.
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return false;
    }
    let mut command = if cfg!(windows) {
        let mut command = Command::new("rundll32.exe");
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = Command::new("open");
        command.arg(url);
        command
    } else {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}
