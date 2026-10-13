//! The harness server as seen from this computer: pairing (`harness login`),
//! the device token, and where gateways send LLM requests.
//!
//! The server is the only place that holds the user's API keys
//! (ARCHITECTURE.md §6.4). A paired computer gets a device token that is only
//! good for the server's LLM pass-through; it lives in the OS credential
//! store and reaches the VM only inside the gateway, never an instance.

use std::time::Duration;

use harness_gateway::LlmUpstream;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use ureq::Agent;
use ureq::tls::{TlsConfig, TlsProvider};

use crate::{Error, Result};

/// Name of the credential store entries ("service" in keyring terms).
const KEYRING_SERVICE: &str = "llm-harness";

/// Path of the LLM pass-through on the server; `llm.harness/<rest>` maps to
/// `<server><prefix>/<rest>`.
pub const LLM_PROXY_PATH: &str = "/api/llm/proxy";

/// Server address used when neither the command line nor `config.toml` names
/// one: the development server on this machine.
pub const DEFAULT_SERVER_URL: &str = "http://localhost:8080";

const TIMEOUT: Duration = Duration::from_secs(15);

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

/// HTTP client for one harness server.
pub struct ServerClient {
    url: String,
    agent: Agent,
}

impl ServerClient {
    /// A client for `url` (`http://` or `https://`, no path).
    pub fn new(url: &str) -> Result<ServerClient> {
        let url = normalize_url(url)?;
        let config = Agent::config_builder()
            .tls_config(
                TlsConfig::builder()
                    .provider(TlsProvider::NativeTls)
                    .build(),
            )
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
        let mut response =
            response.map_err(|e| Error::Server(format!("{} nicht erreichbar: {e}", self.url)))?;
        let status = response.status();
        if status.is_success() {
            return response
                .body_mut()
                .read_json::<T>()
                .map_err(|e| Error::Server(format!("unerwartete Antwort von {}: {e}", self.url)));
        }
        let detail = response
            .body_mut()
            .read_json::<ErrorBody>()
            .map(|body| format!("{} ({})", body.error.message, body.error.code))
            .unwrap_or_else(|_| status.to_string());
        Err(if status.as_u16() == 401 {
            Error::Server(
                "Gerät nicht (mehr) angemeldet. `harness login` koppelt es neu".to_owned(),
            )
        } else {
            Error::Server(format!("{} antwortet mit {status}: {detail}", self.url))
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
        Err(Error::Server(format!(
            "ungültige Server-Adresse {url:?}, erwartet z. B. http://localhost:8080"
        )))
    }
}

/// The device token for `server_url`, if this computer is paired.
pub fn load_token(server_url: &str) -> Result<Option<String>> {
    match entry(server_url)?.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
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
    Error::Server(format!("Anmeldeinformationsspeicher: {error}"))
}

/// Where a gateway sends `llm.harness` requests for `server_url`.
///
/// A server on this machine's loopback is reached from the VM through
/// `host_address` (gvproxy's mapping of the host's 127.0.0.1). The gateway
/// only speaks plain HTTP to its upstream so far.
pub fn llm_upstream(
    server_url: &str,
    host_address: Option<&str>,
    token: &str,
) -> Result<LlmUpstream> {
    let url = normalize_url(server_url)?;
    let Some(authority) = url.strip_prefix("http://") else {
        return Err(Error::Unsupported(
            "Das Gateway erreicht den Server bisher nur per http:// (TLS im Gateway fehlt noch)"
                .to_owned(),
        ));
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.ends_with(':') => (host, Some(port)),
        _ => (authority, None),
    };
    let loopback = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
    let host = match (loopback, host_address) {
        (true, Some(address)) => address,
        (true, None) => {
            return Err(Error::Unsupported(
                "Der Server läuft auf diesem Rechner, aber der Provider kennt keine Host-Adresse"
                    .to_owned(),
            ));
        }
        (false, _) => host,
    };
    let base = match port {
        Some(port) => format!("http://{host}:{port}{LLM_PROXY_PATH}"),
        None => format!("http://{host}{LLM_PROXY_PATH}"),
    };
    LlmUpstream::new(&base, token).map_err(Error::Server)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "hd_abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG";

    #[test]
    fn urls() {
        assert_eq!(
            normalize_url("http://localhost:8080/").unwrap(),
            "http://localhost:8080"
        );
        assert_eq!(
            normalize_url(" https://harness.example ").unwrap(),
            "https://harness.example"
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
        let local = llm_upstream("http://localhost:8080", Some("192.168.127.254"), TOKEN).unwrap();
        assert_eq!(local.url(), "http://192.168.127.254:8080/api/llm/proxy");
        let ip = llm_upstream("http://127.0.0.1:9000/", Some("192.168.127.254"), TOKEN).unwrap();
        assert_eq!(ip.url(), "http://192.168.127.254:9000/api/llm/proxy");
        let lan = llm_upstream("http://harness.lan", Some("192.168.127.254"), TOKEN).unwrap();
        assert_eq!(lan.url(), "http://harness.lan/api/llm/proxy");
        assert!(llm_upstream("https://harness.example", Some("x"), TOKEN).is_err());
        assert!(llm_upstream("http://localhost:8080", None, TOKEN).is_err());
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
