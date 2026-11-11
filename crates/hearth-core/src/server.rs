//! The hearth server as seen from this computer: pairing (`hearth login`),
//! the device token, and where gateways send LLM requests.
//!
//! The server is the only place that holds the user's API keys
//! (ARCHITECTURE.md §6.4). A paired computer gets a device token that is only
//! good for the server's LLM pass-through; it lives in the OS credential
//! store and reaches the VM only inside the gateway, never an instance.

use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use hearth_gateway::{LlmUpstream, pem};
use hmac::{Hmac, KeyInit, Mac};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ureq::Agent;
use ureq::tls::{Certificate, RootCerts, TlsConfig, TlsProvider};

use crate::config::ServerConfig;
use crate::lang::tr;
use crate::{Error, Result, tr};

/// Name of the credential store entries ("service" in keyring terms).
const KEYRING_SERVICE: &str = "hearth";

/// Path of the LLM pass-through on the server; `llm.hearth/<rest>` maps to
/// `<server><prefix>/<rest>`.
pub const LLM_PROXY_PATH: &str = "/api/llm/proxy";

/// Server address used when neither the command line nor `config.toml` names
/// one: the development server on this machine.
pub const DEFAULT_SERVER_URL: &str = "http://localhost:8080";

const TIMEOUT: Duration = Duration::from_secs(15);

/// Time limit for Jev's assessment of a diff (several calls on the server).
const ASSESS_TIMEOUT: Duration = Duration::from_secs(60);

/// Most text of added lines per file Jev reads (server limit, characters).
pub const MAX_ASSESSED_CHARS: usize = 12_000;

/// Most files Jev assesses per merge (server limit).
pub const MAX_ASSESSED_FILES: usize = 60;

/// Largest file the device downloads from a linked chat: the server's
/// upload limit (20 MiB) plus room for the response.
pub const MAX_DEVICE_FILE_BYTES: u64 = 21 * 1024 * 1024;

/// Added lines of one changed file, for Jev.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffFile {
    /// File in the repository.
    pub path: String,
    /// `A`, `M` or `T`.
    pub status: String,
    /// The added lines, shortened to [`MAX_ASSESSED_CHARS`].
    pub added: String,
}

/// How dangerous Jev finds one file's change.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FileDanger {
    /// File in the repository.
    pub path: String,
    /// From 0 (harmless) to 1 (malicious).
    pub danger: f64,
}

/// Jev's answer for a merge check.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DiffAssessment {
    /// Jev's model, if Jev was asked.
    pub model: Option<String>,
    /// Danger per file; `None` if Jev was not asked or failed.
    pub files: Option<Vec<FileDanger>>,
    /// Cost in US dollars.
    pub cost: Option<f64>,
    /// `no_helpers` (no OpenRouter key on the server), `disabled` (switched
    /// off for this device in the web app) or `failed`.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Answer to a pairing start (`POST /api/pairing`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingStart {
    /// Secret this device polls with.
    pub device_code: String,
    /// Code the user confirms, `ABCD-EFGH`.
    pub user_code: String,
    /// Confirmation page.
    pub verification_uri: String,
    /// Confirmation page with the code filled in.
    pub verification_uri_complete: String,
    /// Seconds until the pairing expires.
    pub expires_in: u64,
    /// Seconds between polls.
    pub interval: u64,
}

/// State of a pairing (`POST /api/pairing/token`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum PairingPoll {
    /// Not confirmed yet.
    Pending,
    /// The user refused.
    Denied,
    /// Timed out, or the token was already picked up.
    Expired,
    /// Confirmed; the token is handed out once.
    Approved {
        /// The new device's id.
        #[serde(rename = "deviceId")]
        device_id: String,
        /// The device token.
        token: String,
    },
}

/// The paired device and its account (`GET /api/device`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DeviceSelf {
    /// The device.
    pub device: DeviceInfo,
    /// The account.
    pub user: UserInfo,
}

/// A paired device.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    /// Device id.
    pub id: String,
    /// Name it was paired under.
    pub name: String,
}

/// The account a device acts for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UserInfo {
    /// Email.
    pub email: String,
    /// Display name.
    pub name: String,
}

/// A credential of the account, without its key (`GET /api/llm/providers`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCredential {
    /// Credential id; part of the pass-through path.
    pub id: String,
    /// Provider kind, e.g. `anthropic` or `openai-compatible`.
    pub kind: String,
    /// User-chosen name.
    pub label: String,
    /// Custom endpoint, if any.
    pub base_url: Option<String>,
    /// Models of user-run endpoints; `None` for providers with a known catalog.
    pub models: Option<Vec<ModelInfo>>,
}

/// A model offered by a credential.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ModelInfo {
    /// Provider-specific id.
    pub id: String,
    /// Display name.
    pub name: String,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    code: String,
    message: String,
}

/// The server's answer to [`ServerClient::prove_device`].
#[derive(Deserialize)]
struct SignedProof {
    origins: Vec<String>,
    signature: String,
}

/// Prefix of the text a device proof signs; the server's is the same.
const DEVICE_PROOF_CONTEXT: &str = "hearth-device-proof";

/// Addresses of a server shown at most, e.g. in a refusal.
const MAX_ORIGINS: usize = 10;

/// What [`ServerClient::prove_device`] found out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceProof {
    /// The server does not hold this device (or predates bound proofs).
    NotHeld,
    /// The server holds it and answers under the address dialled.
    Held,
    /// A server holding it answered, but names other addresses as its own:
    /// the address dialled only relays to it, or is missing from the
    /// server's `TRUSTED_ORIGINS`. The token must not go there.
    Elsewhere {
        /// The server's own addresses (sanitized, at most [`MAX_ORIGINS`]).
        origins: Vec<String>,
    },
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// What a device proof is built from: the hash the server stores for
/// `token` (its key) and the hash of that, which names the device.
fn device_lookup(token: &str) -> (String, String) {
    let stored = hex(&Sha256::digest(token.as_bytes()));
    let lookup = hex(&Sha256::digest(stored.as_bytes()));
    (stored, lookup)
}

/// The signature a server holding the device gives to `nonce` together with
/// its `origins`, in hex.
fn device_proof(stored: &str, nonce: &str, origins: &[String]) -> String {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(stored.as_bytes())
        .expect("HMAC takes keys of any length");
    let mut parts = vec![DEVICE_PROOF_CONTEXT, nonce];
    parts.extend(origins.iter().map(String::as_str));
    mac.update(parts.join("\n").as_bytes());
    hex(&mac.finalize().into_bytes())
}

/// Whether a server URL is safe to use without TLS: `https://`, or `http://`
/// to this computer or the home network (private and Tailscale addresses,
/// names without a dot, `.local`, `.lan`, `.home.arpa`, `.ts.net`). Over
/// the internet a plain connection hands the device token to anyone who
/// reads along (client review of 10.11., finding 10).
pub fn private_or_encrypted(url: &str) -> bool {
    let Some(rest) = url.trim().strip_prefix("http://") else {
        return true;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => authority
            .rsplit_once(':')
            .map_or(authority, |(host, _)| host),
    }
    .to_lowercase();
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    // Tailscale and other carrier-grade NAT (100.64.0.0/10).
                    || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64)
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback()
                    || (v6.segments()[0] & 0xfe00) == 0xfc00
                    || (v6.segments()[0] & 0xffc0) == 0xfe80
            }
        };
    }
    !host.contains('.')
        || [".local", ".lan", ".home.arpa", ".ts.net", ".internal"]
            .iter()
            .any(|suffix| host.ends_with(suffix))
}

/// Whether two URLs have the same scheme, host and port; paths do not count.
pub fn same_origin(a: &str, b: &str) -> bool {
    fn base(url: &str) -> String {
        let (scheme, rest) = url.trim().split_once("://").unwrap_or(("", url));
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        origin_of(&format!("{scheme}://{authority}"))
    }
    base(a) == base(b)
}

/// `url` as the origin a browser would form: scheme and host in lower case,
/// without the default port, so it compares with the server's addresses.
fn origin_of(url: &str) -> String {
    let url = url.trim().trim_end_matches('/').to_lowercase();
    for (scheme, port) in [("http://", ":80"), ("https://", ":443")] {
        if let Some(rest) = url.strip_prefix(scheme)
            && let Some(host) = rest.strip_suffix(port)
        {
            return format!("{scheme}{host}");
        }
    }
    url
}

/// HTTP client for one harness server.
pub struct ServerClient {
    url: String,
    agent: Agent,
}

/// TLS through Windows (SChannel) with the system's trusted roots. ureq
/// otherwise hands native-tls its own root list, which is empty without
/// the bundled Mozilla roots, and every https connection fails.
pub(crate) fn tls_config() -> TlsConfig {
    TlsConfig::builder()
        .provider(TlsProvider::NativeTls)
        .root_certs(RootCerts::PlatformVerifier)
        .build()
}

/// Largest CA file read; a root certificate is a few KiB.
const MAX_CA_FILE: u64 = 256 * 1024;

/// Root certificates of a server with its own CA (`[server] ca`), trusted
/// for that server instead of the public CAs: by [`ServerClient`] on this
/// computer and by the gateway, which gets them with the LLM upstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerCa {
    path: PathBuf,
    certificates: Vec<Vec<u8>>,
}

impl ServerCa {
    /// Reads the certificates from a PEM file, or a single certificate in
    /// DER form (what Windows exports as `.cer`).
    pub fn load(path: &Path) -> Result<ServerCa> {
        let invalid = |message: String| {
            Error::Server(tr!(
                "CA-Datei {} (server.ca): {message}",
                "CA file {} (server.ca): {message}",
                path.display()
            ))
        };
        let mut bytes = Vec::new();
        fs::File::open(path)
            .and_then(|file| file.take(MAX_CA_FILE + 1).read_to_end(&mut bytes))
            .map_err(|e| invalid(e.to_string()))?;
        if bytes.len() as u64 > MAX_CA_FILE {
            return Err(invalid(
                tr(
                    "zu groß für ein Wurzelzertifikat",
                    "too large for a root certificate",
                )
                .to_owned(),
            ));
        }
        // One DER structure spanning the whole file is never PEM text.
        let der = vec![bytes];
        let certificates = if pem::check_certificates(&der).is_ok() {
            der
        } else {
            let text = String::from_utf8(der.into_iter().next().unwrap_or_default())
                .map_err(|_| invalid(tr("weder PEM noch DER", "neither PEM nor DER").to_owned()))?;
            pem::certificates(&text).map_err(invalid)?
        };
        Ok(ServerCa {
            path: path.to_owned(),
            certificates,
        })
    }

    /// The CA configured in `[server]`, if any.
    pub fn from_config(server: &ServerConfig) -> Result<Option<ServerCa>> {
        server.ca.as_deref().map(ServerCa::load).transpose()
    }

    /// The file the certificates came from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The certificates (DER).
    pub fn certificates(&self) -> &[Vec<u8>] {
        &self.certificates
    }
}

impl ServerClient {
    /// A client for `url` (`http://` or `https://`, no path). With `ca`, an
    /// `https://` server must present a certificate from that CA; the
    /// system's roots then do not count for it.
    pub fn new(url: &str, ca: Option<&ServerCa>) -> Result<ServerClient> {
        let url = normalize_url(url)?;
        let tls = match ca {
            Some(ca) if url.starts_with("https://") => {
                let roots: Vec<Certificate<'static>> = ca
                    .certificates()
                    .iter()
                    .map(|der| Certificate::from_der(der).to_owned())
                    .collect();
                TlsConfig::builder()
                    .provider(TlsProvider::NativeTls)
                    .root_certs(RootCerts::Specific(Arc::new(roots)))
                    .build()
            }
            _ => tls_config(),
        };
        let config = Agent::config_builder()
            .tls_config(tls)
            // Error bodies carry the reason; they are read below.
            .http_status_as_error(false)
            .timeout_global(Some(TIMEOUT))
            .build();
        Ok(ServerClient {
            url,
            agent: Agent::new_with_config(config),
        })
    }

    /// The server's base URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Starts a pairing under `name`.
    pub fn start_pairing(&self, name: &str) -> Result<PairingStart> {
        let response = self
            .agent
            .post(format!("{}/api/pairing", self.url))
            .send_json(serde_json::json!({ "name": name }));
        self.read(response)
    }

    /// Asks whether the user has confirmed the pairing yet.
    pub fn poll_pairing(&self, device_code: &str) -> Result<PairingPoll> {
        let response = self
            .agent
            .post(format!("{}/api/pairing/token", self.url))
            .send_json(serde_json::json!({ "deviceCode": device_code }));
        self.read(response)
    }

    /// Whether this server holds the device of `token`, proved without
    /// sending the token: `hearth login` keeps the device when the same
    /// server moves to a new address, and a mistyped address learns nothing.
    /// The server answers a random challenge with an HMAC keyed with the
    /// token's hash, which only it stores, over the challenge and the
    /// addresses it answers under. A server that only relays the challenge to
    /// the real one thus shows up as [`DeviceProof::Elsewhere`].
    pub fn prove_device(&self, token: &str) -> Result<DeviceProof> {
        let (stored, lookup) = device_lookup(token);
        let mut challenge = [0u8; 32];
        getrandom::fill(&mut challenge).map_err(|e| Error::Server(e.to_string()))?;
        let nonce = hex(&challenge);
        let response = self
            .agent
            .post(format!("{}/api/pairing/prove", self.url))
            .send_json(serde_json::json!({ "lookup": lookup, "nonce": nonce }));
        // Another server, or one from before bound proofs, knows no such
        // device.
        let Ok(SignedProof { origins, signature }) = self.read::<SignedProof>(response) else {
            return Ok(DeviceProof::NotHeld);
        };
        if signature != device_proof(&stored, &nonce, &origins) {
            return Ok(DeviceProof::NotHeld);
        }
        let dialled = origin_of(&self.url);
        if origins.iter().any(|origin| origin_of(origin) == dialled) {
            return Ok(DeviceProof::Held);
        }
        Ok(DeviceProof::Elsewhere {
            origins: origins
                .iter()
                .take(MAX_ORIGINS)
                .map(|origin| crate::sync::sanitize(origin).chars().take(200).collect())
                .collect(),
        })
    }

    /// The device a token belongs to; fails for revoked tokens.
    pub fn device(&self, token: &str) -> Result<DeviceSelf> {
        let response = self
            .agent
            .get(format!("{}/api/device", self.url))
            .header("authorization", format!("Bearer {token}"))
            .call();
        self.read(response)
    }

    /// The account's credentials, as harnesses need them.
    pub fn credentials(&self, token: &str) -> Result<Vec<DeviceCredential>> {
        let response = self
            .agent
            .get(format!("{}/api/llm/providers", self.url))
            .header("authorization", format!("Bearer {token}"))
            .call();
        self.read(response)
    }

    /// Tells the server which projects this computer offers to web chats,
    /// so they show up in the account even while it is offline.
    pub fn set_projects(
        &self,
        token: &str,
        projects: &[crate::tunnel::RemoteProject],
    ) -> Result<()> {
        let response = self
            .agent
            .put(format!("{}/api/device/projects", self.url))
            .header("authorization", format!("Bearer {token}"))
            .send_json(serde_json::json!({ "projects": projects }));
        match response {
            Ok(response) if response.status().is_success() => Ok(()),
            other => self.read::<serde_json::Value>(other).map(drop),
        }
    }

    /// Asks Jev, through the server and with the user's OpenRouter key, how
    /// dangerous each changed file is. Jev's calls can take a while, hence
    /// the longer time limit.
    pub fn assess_diff(&self, token: &str, files: &[DiffFile]) -> Result<DiffAssessment> {
        let response = self
            .agent
            .post(format!("{}/api/device/assessments/diff", self.url))
            .config()
            .timeout_global(Some(ASSESS_TIMEOUT))
            .build()
            .header("authorization", format!("Bearer {token}"))
            .send_json(serde_json::json!({ "files": files }));
        self.read(response)
    }

    /// Downloads a file attached in a linked chat (protocol 9): its bytes,
    /// or with `text` the Markdown the model would read. At most
    /// [`MAX_DEVICE_FILE_BYTES`].
    pub fn device_file(&self, token: &str, id: &str, text: bool) -> Result<Vec<u8>> {
        // The id goes into the path; the server makes them, but checked anyway.
        let valid = id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
        if !valid {
            return Err(Error::Server(tr!(
                "unerwartete Datei-ID {id:?}",
                "unexpected file ID {id:?}"
            )));
        }
        let suffix = if text { "/text" } else { "" };
        let response = self
            .agent
            .get(format!("{}/api/device/files/{id}{suffix}", self.url))
            .header("authorization", format!("Bearer {token}"))
            .call();
        match response {
            Ok(mut response) if response.status().is_success() => response
                .body_mut()
                .with_config()
                .limit(MAX_DEVICE_FILE_BYTES)
                .read_to_vec()
                .map_err(|e| {
                    Error::Server(tr!(
                        "Datei vom Server unvollständig: {e}",
                        "file from the server incomplete: {e}"
                    ))
                }),
            other => self.read::<serde_json::Value>(other).map(|_| Vec::new()),
        }
    }

    /// Revokes the token on the server.
    pub fn revoke(&self, token: &str) -> Result<()> {
        let response = self
            .agent
            .delete(format!("{}/api/device", self.url))
            .header("authorization", format!("Bearer {token}"))
            .call();
        match response {
            Ok(response) if response.status().is_success() => Ok(()),
            other => self.read::<serde_json::Value>(other).map(drop),
        }
    }

    fn read<T: DeserializeOwned>(
        &self,
        response: std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<T> {
        let mut response = response.map_err(|e| {
            Error::Server(tr!(
                "{} nicht erreichbar: {e}",
                "{} unreachable: {e}",
                self.url
            ))
        })?;
        let status = response.status();
        if status.is_success() {
            return response.body_mut().read_json::<T>().map_err(|e| {
                Error::Server(tr!(
                    "unerwartete Antwort von {}: {e}",
                    "unexpected answer from {}: {e}",
                    self.url
                ))
            });
        }
        let detail = response
            .body_mut()
            .read_json::<ErrorBody>()
            .map(|body| format!("{} ({})", body.error.message, body.error.code))
            .unwrap_or_else(|_| status.to_string());
        Err(if status.as_u16() == 401 {
            Error::Server(
                tr(
                    "Gerät nicht (mehr) angemeldet. `hearth login` koppelt es neu",
                    "Device not signed in (any more). `hearth login` pairs it again",
                )
                .to_owned(),
            )
        } else {
            Error::Server(tr!(
                "{} antwortet mit {status}: {detail}",
                "{} answers with {status}: {detail}",
                self.url
            ))
        })
    }
}

/// `http(s)://host[:port]` without trailing slash; paths are not allowed,
/// because the API lives at the root.
fn normalize_url(url: &str) -> Result<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let rest = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"));
    let valid = rest.is_some_and(|rest| {
        !rest.is_empty()
            && !rest.contains(['/', '?', '#', '@', ' '])
            && rest.chars().all(|c| !c.is_control())
    });
    if valid {
        Ok(trimmed.to_owned())
    } else {
        Err(Error::Server(tr!(
            "ungültige Server-Adresse {url:?}, erwartet z. B. http://localhost:8080",
            "invalid server address {url:?}, expected e.g. http://localhost:8080"
        )))
    }
}

/// The device token for `server_url`, if this computer is paired. A token
/// stored before the rename (0.2.0) moves to the new entry on first use, so
/// the device stays paired.
pub fn load_token(server_url: &str) -> Result<Option<String>> {
    match entry(server_url)?.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => {
            let old = keyring::Entry::new(
                crate::legacy::OLD_KEYRING_SERVICE,
                &normalize_url(server_url)?,
            )
            .and_then(|entry| entry.get_password());
            match old {
                Ok(token) => {
                    save_token(server_url, &token)?;
                    Ok(Some(token))
                }
                Err(_) => Ok(None),
            }
        }
        Err(e) => Err(credential_error(e)),
    }
}

/// Stores the device token for `server_url`.
pub fn save_token(server_url: &str, token: &str) -> Result<()> {
    entry(server_url)?
        .set_password(token)
        .map_err(credential_error)
}

/// Forgets the device token for `server_url`; no error if there is none.
pub fn delete_token(server_url: &str) -> Result<()> {
    match entry(server_url)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(credential_error(e)),
    }
}

fn entry(server_url: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, &normalize_url(server_url)?).map_err(credential_error)
}

fn credential_error(error: keyring::Error) -> Error {
    Error::Server(tr!(
        "Anmeldeinformationsspeicher: {error}",
        "Credential Manager: {error}"
    ))
}

/// Where a gateway sends `llm.hearth` requests for `server_url`.
///
/// A server on this machine's loopback is reached from the VM through
/// `host_address` (gvproxy's mapping of the host's 127.0.0.1); only plain
/// HTTP works there, because the certificate would name `localhost`, not
/// that address. An `https://` server gets `ca` along, if set.
pub fn llm_upstream(
    server_url: &str,
    host_address: Option<&str>,
    token: &str,
    ca: Option<&ServerCa>,
) -> Result<LlmUpstream> {
    let url = normalize_url(server_url)?;
    let (scheme, authority) = url.split_once("://").unwrap_or(("http", &url));
    let (host, port) = if authority.starts_with('[') {
        match authority.split_once("]:") {
            Some((host, port)) => (&authority[..=host.len()], Some(port)),
            None => (authority, None),
        }
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        }
    };
    let loopback = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
    let host = match (loopback, host_address) {
        (true, _) if scheme == "https" => {
            return Err(Error::Unsupported(tr!(
                "Einen Server auf diesem Rechner erreicht das Gateway nur per http:// \
                 (http://localhost:{}), nicht per https://",
                "The gateway reaches a server on this computer only by http:// \
                 (http://localhost:{}), not by https://",
                port.unwrap_or("8080")
            )));
        }
        (true, Some(address)) => address,
        (true, None) => {
            return Err(Error::Unsupported(
                tr(
                    "Der Server läuft auf diesem Rechner, aber der Provider kennt keine Host-Adresse",
                    "The server runs on this computer, but the provider does not know a host address",
                )
                .to_owned(),
            ));
        }
        (false, _) => host,
    };
    let base = match port {
        Some(port) => format!("{scheme}://{host}:{port}{LLM_PROXY_PATH}"),
        None => format!("{scheme}://{host}{LLM_PROXY_PATH}"),
    };
    let upstream = LlmUpstream::new(&base, token).map_err(Error::Server)?;
    match ca {
        Some(ca) if scheme == "https" => upstream
            .with_ca(ca.certificates().to_vec())
            .map_err(|e| Error::Server(format!("{}: {e}", ca.path().display()))),
        _ => Ok(upstream),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "hd_abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG";

    #[test]
    fn device_proofs_match_the_server() {
        // The same values are in the server's test (`internet.test.ts`).
        let (stored, lookup) = device_lookup("hd_example-token");
        assert_eq!(
            stored,
            "a71e718f66da4e3bde5b7756f407734208d50d07f5e67f411329e17883285433"
        );
        assert_eq!(
            lookup,
            "a0cc9c1632b9567bfe8d4d7aa836e1298dbf103fcfdcada8481e691668153c1d"
        );
        let origins = [
            "https://hearth.example.org".to_owned(),
            "http://192.168.1.50:8080".to_owned(),
        ];
        assert_eq!(
            device_proof(&stored, "00112233445566778899aabbccddeeff", &origins),
            "0e4b40dd22a09f8f8f810641390c2f8a6ac2ccb5cf35264e5b7d32aacbe4d0a3"
        );
    }

    #[test]
    fn plain_http_only_at_home() {
        for home in [
            "https://hearth.example.org",
            "http://localhost:8080",
            "http://127.0.0.1:8080",
            "http://192.168.1.50:8080",
            "http://10.0.0.2",
            "http://100.101.102.103:8080",
            "http://[::1]:8080",
            "http://[fd12::1]:8080",
            "http://homeserver:8080",
            "http://pi.local:8080",
            "http://homeserver.tail1234.ts.net:8080",
        ] {
            assert!(private_or_encrypted(home), "{home}");
        }
        for outside in [
            "http://hearth.example.org",
            "http://203.0.113.5:8080",
            "http://[2001:db8::1]:8080",
        ] {
            assert!(!private_or_encrypted(outside), "{outside}");
        }
    }

    #[test]
    fn origins_compare_like_a_browser_forms_them() {
        assert_eq!(
            origin_of("https://Hearth.Example.org:443"),
            "https://hearth.example.org"
        );
        assert_eq!(origin_of("http://lan.local:80/"), "http://lan.local");
        assert_eq!(
            origin_of("http://192.168.1.50:8080"),
            "http://192.168.1.50:8080"
        );
        // Another port is another server.
        assert_ne!(
            origin_of("https://hearth.example.org:8443"),
            origin_of("https://hearth.example.org")
        );
        assert!(same_origin(
            "https://Hearth.example.org/device?code=AB",
            "https://hearth.example.org:443"
        ));
        assert!(!same_origin(
            "https://hearth.example.org@evil.example/device",
            "https://hearth.example.org"
        ));
        assert!(!same_origin(
            "http://hearth.example.org/device",
            "https://hearth.example.org"
        ));
    }

    #[test]
    fn urls() {
        assert_eq!(
            normalize_url("http://localhost:8080/").unwrap(),
            "http://localhost:8080"
        );
        assert_eq!(
            normalize_url(" https://hearth.example ").unwrap(),
            "https://hearth.example"
        );
        for bad in [
            "localhost:8080",
            "ftp://x",
            "http://",
            "http://a/b",
            "http://u@x",
            "http://a b",
        ] {
            assert!(normalize_url(bad).is_err(), "{bad:?} accepted");
        }
    }

    #[test]
    fn upstream_for_the_vm() {
        let vm = Some("192.168.127.254");
        let local = llm_upstream("http://localhost:8080", vm, TOKEN, None).unwrap();
        assert_eq!(local.url(), "http://192.168.127.254:8080/api/llm/proxy");
        let ip = llm_upstream("http://127.0.0.1:9000/", vm, TOKEN, None).unwrap();
        assert_eq!(ip.url(), "http://192.168.127.254:9000/api/llm/proxy");
        let v6 = llm_upstream("http://[::1]:9000", vm, TOKEN, None).unwrap();
        assert_eq!(v6.url(), "http://192.168.127.254:9000/api/llm/proxy");
        let lan = llm_upstream("http://hearth.lan", vm, TOKEN, None).unwrap();
        assert_eq!(lan.url(), "http://hearth.lan/api/llm/proxy");
        let lan6 = llm_upstream("http://[fd00::1:5]:8080", vm, TOKEN, None).unwrap();
        assert_eq!(lan6.url(), "http://[fd00::1:5]:8080/api/llm/proxy");
        assert!(llm_upstream("http://localhost:8080", None, TOKEN, None).is_err());

        let remote = llm_upstream("https://hearth.example", vm, TOKEN, None).unwrap();
        assert_eq!(remote.url(), "https://hearth.example/api/llm/proxy");
        assert!(remote.ca().is_empty());
        let port = llm_upstream("https://hearth.example:8443", vm, TOKEN, None).unwrap();
        assert_eq!(port.url(), "https://hearth.example:8443/api/llm/proxy");
        // The certificate would name localhost, not the VM's host address.
        let local_tls = llm_upstream("https://localhost:8443", vm, TOKEN, None).unwrap_err();
        assert!(
            local_tls.to_string().contains("http://localhost:8443"),
            "{local_tls}"
        );
    }

    const CA_PEM: &str = include_str!("../../hearth-gateway/tests/fixtures/ca.pem");
    const SERVER_PEM: &str = include_str!("../../hearth-gateway/tests/fixtures/server.pem");

    #[test]
    fn own_ca() {
        let dir = tempfile::tempdir().unwrap();
        let pem_file = dir.path().join("root.crt");
        fs::write(&pem_file, CA_PEM).unwrap();
        let ca = ServerCa::load(&pem_file).unwrap();
        assert_eq!(ca.certificates().len(), 1);
        assert_eq!(ca.path(), pem_file);

        // DER as Windows exports it, and a chain in PEM.
        let der_file = dir.path().join("root.cer");
        fs::write(&der_file, &ca.certificates()[0]).unwrap();
        assert_eq!(
            ServerCa::load(&der_file).unwrap().certificates(),
            ca.certificates()
        );
        let chain_file = dir.path().join("chain.pem");
        fs::write(&chain_file, SERVER_PEM).unwrap();
        assert_eq!(ServerCa::load(&chain_file).unwrap().certificates().len(), 2);

        let tls = llm_upstream("https://hearth.example", None, TOKEN, Some(&ca)).unwrap();
        assert_eq!(tls.ca(), ca.certificates());
        // Only TLS needs it; an http:// server ignores it.
        let plain = llm_upstream("http://hearth.lan", None, TOKEN, Some(&ca)).unwrap();
        assert!(plain.ca().is_empty());
        assert!(ServerClient::new("https://hearth.example", Some(&ca)).is_ok());

        let config = ServerConfig {
            url: None,
            ca: Some(pem_file.clone()),
        };
        assert_eq!(ServerCa::from_config(&config).unwrap(), Some(ca));
        assert_eq!(
            ServerCa::from_config(&ServerConfig::default()).unwrap(),
            None
        );

        let empty = dir.path().join("empty.pem");
        fs::write(&empty, "nothing here\n").unwrap();
        let big = dir.path().join("big.pem");
        fs::write(&big, CA_PEM.repeat(1000)).unwrap();
        for bad in [dir.path().join("missing.pem"), empty, big] {
            let error = ServerCa::load(&bad).unwrap_err().to_string();
            assert!(error.contains("server.ca"), "{error}");
        }
    }

    #[test]
    fn poll_answers_parse() {
        let pending: PairingPoll = serde_json::from_str(r#"{"status":"pending"}"#).unwrap();
        assert_eq!(pending, PairingPoll::Pending);
        let approved: PairingPoll =
            serde_json::from_str(r#"{"status":"approved","deviceId":"d1","token":"t"}"#).unwrap();
        assert_eq!(
            approved,
            PairingPoll::Approved {
                device_id: "d1".into(),
                token: "t".into()
            }
        );
        let start: PairingStart = serde_json::from_str(
            r#"{"deviceCode":"dc","userCode":"ABCD-EFGH","verificationUri":"u","verificationUriComplete":"u?c","expiresIn":600,"interval":2}"#,
        )
        .unwrap();
        assert_eq!(start.user_code, "ABCD-EFGH");
    }
}
