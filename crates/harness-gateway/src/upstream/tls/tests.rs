//! Handshakes against an in-process rustls server with the test CA from
//! `tests/fixtures` (see `generate.sh` there).

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};

use super::*;
use crate::pem;

const CA: &str = include_str!("../../../tests/fixtures/ca.pem");
const OTHER_CA: &str = include_str!("../../../tests/fixtures/other-ca.pem");
const SERVER_CHAIN: &str = include_str!("../../../tests/fixtures/server.pem");
const SERVER_KEY: &str = include_str!("../../../tests/fixtures/server.key");

/// A TLS echo server on loopback, presenting the test server certificate
/// and its intermediate. Returns the port.
fn echo_server() -> u16 {
    let chain = pem::certificates(SERVER_CHAIN)
        .unwrap()
        .into_iter()
        .map(CertificateDer::from)
        .collect();
    let key = pem::blocks(SERVER_KEY, "PRIVATE KEY").unwrap().remove(0);
    let config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(chain, PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)))
            .unwrap();
    let config = Arc::new(config);
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for socket in listener.incoming().flatten() {
            let config = config.clone();
            thread::spawn(move || {
                let connection = ServerConnection::new(config).unwrap();
                let mut tls = StreamOwned::new(connection, socket);
                let mut buffer = [0; 4096];
                // Echo until the client's close_notify (or a failed
                // handshake), then close cleanly.
                while let Ok(read @ 1..) = tls.read(&mut buffer) {
                    if tls.write_all(&buffer[..read]).is_err() {
                        return;
                    }
                }
                tls.conn.send_close_notify();
                let _ = tls.flush();
            });
        }
    });
    port
}

fn config(ca: &str) -> Arc<ClientConfig> {
    let certificates = if ca.is_empty() {
        Vec::new()
    } else {
        pem::certificates(ca).unwrap()
    };
    client_config(&certificates).unwrap()
}

fn open_as(
    config: &Arc<ClientConfig>,
    name: &str,
    port: u16,
) -> Result<Upstream, (Reason, String)> {
    let socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    open(config, &Host::parse(name).unwrap(), socket)
}

/// Sends `data` while reading the echo concurrently, as the relay does.
fn round_trip(upstream: Upstream, data: Vec<u8>) -> Vec<u8> {
    let Upstream {
        mut writer,
        mut reader,
        ..
    } = upstream;
    let sender = thread::spawn(move || {
        for chunk in data.chunks(10_000) {
            writer.write_all(chunk).unwrap();
        }
        writer.finish();
    });
    let mut echoed = Vec::new();
    reader.read_to_end(&mut echoed).unwrap();
    sender.join().unwrap();
    echoed
}

#[test]
fn own_ca_with_names_and_addresses() {
    let port = echo_server();
    let trusted = config(CA);
    for name in [
        "harness.test",
        "HARNESS.test.",
        "harness-tls-test",
        "127.0.0.1",
    ] {
        let upstream =
            open_as(&trusted, name, port).unwrap_or_else(|(_, detail)| panic!("{name}: {detail}"));
        assert_eq!(round_trip(upstream, b"ping".to_vec()), b"ping", "{name}");
    }
    // ::1 is in the certificate too, but the server listens on IPv4 only;
    // the name check alone must accept it.
    let socket = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    assert!(open(&trusted, &Host::Ip(Ipv6Addr::LOCALHOST.into()), socket).is_ok());
}

#[test]
fn wrong_names_are_rejected() {
    let port = echo_server();
    let trusted = config(CA);
    for name in ["other.test", "test", "127.0.0.2", "[::2]"] {
        match open_as(&trusted, name, port) {
            Ok(_) => panic!("{name} accepted"),
            Err((reason, detail)) => {
                assert_eq!(reason, Reason::TlsFailed);
                assert!(detail.contains("not valid for name"), "{name}: {detail}");
                assert!(detail.starts_with("TLS-Verbindung zu "), "{detail}");
            }
        }
    }
}

#[test]
fn unknown_cas_are_rejected() {
    let port = echo_server();
    // Mozilla's roots do not know the test CA, and a configured CA replaces
    // them instead of adding to them.
    for ca in ["", OTHER_CA] {
        match open_as(&config(ca), "harness.test", port) {
            Ok(_) => panic!("accepted with {ca:?}"),
            Err((reason, detail)) => {
                assert_eq!(reason, Reason::TlsFailed);
                assert!(detail.contains("UnknownIssuer"), "{detail}");
            }
        }
    }
}

#[test]
fn both_directions_at_once() {
    let port = echo_server();
    let upstream = open_as(&config(CA), "harness.test", port).unwrap();
    // Much more than any TLS or socket buffer, so writer and reader really
    // run at the same time.
    let data: Vec<u8> = (0..4_000_000u32).map(|i| (i % 251) as u8).collect();
    let echoed = round_trip(upstream, data.clone());
    assert_eq!(echoed.len(), data.len());
    assert!(echoed == data, "echo differs");
}

#[test]
fn unusable_cas_fail_early() {
    let error = client_config(&[vec![0x30, 0x03, 1, 2, 3]]).unwrap_err();
    assert!(error.starts_with("CA-Zertifikat 1 unbrauchbar"), "{error}");
    assert!(client_config(&[]).is_ok());
}
