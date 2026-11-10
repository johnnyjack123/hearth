//! TLS to an `https://` hearth server: rustls with the ring provider,
//! Mozilla's root certificates compiled in (the image has no CA bundle) or
//! the server's own CA instead.
//!
//! The relay copies both directions in separate blocking threads, but a
//! rustls connection is one state machine. Both threads therefore share it
//! behind a mutex that is never held during socket I/O: the reading thread
//! waits for ciphertext without the lock, the writing thread encrypts under
//! the lock and sends afterwards. Whoever sends takes the socket's write
//! lock before releasing the connection, so records leave in the order in
//! which rustls numbered them.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore};

use super::{Sink, Upstream};
use crate::pattern::Host;
use crate::policy::Reason;

/// Time for the TLS handshake; the server answers at once or not at all.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

/// Ciphertext read from the socket at a time: one full TLS record.
const READ_BUFFER: usize = 16 * 1024 + 256;

/// Client settings for one upstream: Mozilla's roots, or only `ca` if set.
/// Certificate and host name are always verified; there is deliberately no
/// way to turn that off.
pub(super) fn client_config(ca: &[Vec<u8>]) -> Result<Arc<ClientConfig>, String> {
    let roots = if ca.is_empty() {
        RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        }
    } else {
        let mut roots = RootCertStore::empty();
        for (index, der) in ca.iter().enumerate() {
            roots
                .add(CertificateDer::from(der.clone()))
                .map_err(|e| format!("CA-Zertifikat {} unbrauchbar: {e}", index + 1))?;
        }
        roots
    };
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("TLS-Einstellungen: {e}"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    // The gateway rewrites HTTP/1.1 heads and relays the body as it is; a
    // server must not pick HTTP/2.
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

/// Handshake with `host` over `socket`. The certificate must be valid for
/// `host` (SNI and name check; an IP literal is checked against the IP
/// addresses in the certificate).
pub(super) fn open(
    config: &Arc<ClientConfig>,
    host: &Host,
    socket: TcpStream,
) -> Result<Upstream, (Reason, String)> {
    let failed = |e: &dyn std::fmt::Display| {
        (
            Reason::TlsFailed,
            format!("TLS-Verbindung zu {host} fehlgeschlagen: {e}"),
        )
    };
    let name = match host {
        Host::Ip(ip) => ServerName::from(*ip),
        Host::Name(name) => ServerName::try_from(name.clone()).map_err(|e| failed(&e))?,
    };
    let mut connection = ClientConnection::new(config.clone(), name).map_err(|e| failed(&e))?;
    let timeouts = |limit: Option<Duration>| {
        socket
            .set_read_timeout(limit)
            .and_then(|()| socket.set_write_timeout(limit))
    };
    timeouts(Some(HANDSHAKE_TIMEOUT)).map_err(|e| (Reason::ConnectFailed, e.to_string()))?;
    connection
        .complete_io(&mut &socket)
        .map_err(|e| failed(&e))?;
    if connection.is_handshaking() {
        return Err(failed(&"Server hat die Verbindung beendet"));
    }
    // A streamed LLM answer can pause for minutes.
    timeouts(None).map_err(|e| (Reason::ConnectFailed, e.to_string()))?;

    let clone = || {
        socket
            .try_clone()
            .map_err(|e| (Reason::ConnectFailed, e.to_string()))
    };
    let shared = Arc::new(Shared {
        connection: Mutex::new(connection),
        output: Mutex::new(clone()?),
    });
    Ok(Upstream {
        writer: Box::new(TlsWriter {
            shared: shared.clone(),
        }),
        reader: Box::new(TlsReader {
            shared,
            socket: clone()?,
            buffer: vec![0; READ_BUFFER],
            start: 0,
            end: 0,
        }),
        socket,
    })
}

struct Shared {
    connection: Mutex<ClientConnection>,
    /// The socket for sending; the reader has its own handle.
    output: Mutex<TcpStream>,
}

impl Shared {
    fn connection(&self) -> MutexGuard<'_, ClientConnection> {
        self.connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Sends what rustls has queued. Takes the socket before releasing the
    /// connection (lock order: connection, then socket), so no other
    /// thread can queue later records and send them first.
    fn send(&self, mut connection: MutexGuard<'_, ClientConnection>) -> io::Result<()> {
        let mut records = Vec::new();
        while connection.wants_write() {
            connection.write_tls(&mut records)?;
        }
        if records.is_empty() {
            return Ok(());
        }
        let mut output = self.output.lock().unwrap_or_else(PoisonError::into_inner);
        drop(connection);
        output.write_all(&records)
    }
}

/// Client to server: encrypts and sends.
struct TlsWriter {
    shared: Arc<Shared>,
}

impl Write for TlsWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let mut connection = self.shared.connection();
        // After the handshake rustls encrypts right away; everything queued
        // is sent below, so the next call finds its buffer empty again.
        let written = connection.writer().write(data)?;
        self.shared.send(connection)?;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Sink for TlsWriter {
    fn finish(&mut self) {
        let mut connection = self.shared.connection();
        connection.send_close_notify();
        let _ = self.shared.send(connection);
        let output = self
            .shared
            .output
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let _ = output.shutdown(Shutdown::Write);
    }
}

/// Server to client: reads ciphertext without holding the lock, then
/// decrypts under it.
struct TlsReader {
    shared: Arc<Shared>,
    socket: TcpStream,
    buffer: Vec<u8>,
    /// Ciphertext in `buffer[start..end]` rustls has not taken yet.
    start: usize,
    end: usize,
}

impl Read for TlsReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        loop {
            {
                let mut connection = self.shared.connection();
                // Plaintext first: rustls takes no more ciphertext while its
                // plaintext buffer is full.
                match connection.reader().read(out) {
                    Ok(read) => return Ok(read),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                    // TCP closed without close_notify. Truncation shows in the
                    // HTTP framing, which the client checks.
                    Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(0),
                    Err(e) => return Err(e),
                }
                if self.start < self.end {
                    let mut pending = &self.buffer[self.start..self.end];
                    let taken = connection.read_tls(&mut pending)?;
                    // Nothing taken: close_notify arrived, the rest is void.
                    self.start = if taken == 0 {
                        self.end
                    } else {
                        self.start + taken
                    };
                    let processed = connection.process_new_packets();
                    // Alerts, or answers to the server's key updates.
                    let sent = self.shared.send(connection);
                    if let Err(error) = processed {
                        return Err(io::Error::new(io::ErrorKind::InvalidData, error));
                    }
                    sent?;
                    continue;
                }
            }
            let read = self.socket.read(&mut self.buffer)?;
            if read == 0 {
                return Ok(0);
            }
            self.start = 0;
            self.end = read;
        }
    }
}

#[cfg(test)]
mod tests;
