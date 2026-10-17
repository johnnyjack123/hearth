//! Connections from the gateway onwards: plain TCP for tunnels and
//! `http://` servers, TLS for an `https://` harness server.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};

use crate::llm::{LlmUpstream, Scheme};
use crate::pattern::Host;
use crate::policy::Reason;

#[cfg(feature = "tls")]
mod tls;

/// The sending half of an upstream connection.
pub(crate) trait Sink: Write + Send {
    /// The client sent everything: ends the upstream's input, so a server
    /// waiting for the end of the request can answer.
    fn finish(&mut self);
}

impl Sink for TcpStream {
    fn finish(&mut self) {
        let _ = self.shutdown(Shutdown::Write);
    }
}

/// An established upstream connection, split for the relay's two threads.
pub(crate) struct Upstream {
    /// Client to server.
    pub(crate) writer: Box<dyn Sink>,
    /// Server to client.
    pub(crate) reader: Box<dyn Read + Send>,
    /// The socket underneath, to close both directions at the end.
    pub(crate) socket: TcpStream,
}

impl Upstream {
    /// A TCP connection used as it is.
    pub(crate) fn plain(socket: TcpStream) -> io::Result<Upstream> {
        Ok(Upstream {
            writer: Box::new(socket.try_clone()?),
            reader: Box::new(socket.try_clone()?),
            socket,
        })
    }
}

/// How to talk to the LLM upstream, prepared once when the host sets it:
/// for `https://` the trust store is built then, so a CA the TLS stack
/// cannot use fails the host's `llm` request instead of every LLM call.
#[derive(Clone)]
pub(crate) enum Connector {
    /// Plain HTTP.
    Plain,
    /// TLS with the public CAs or the upstream's own CA.
    #[cfg(feature = "tls")]
    Tls(std::sync::Arc<rustls::ClientConfig>),
}

impl Connector {
    /// The connector for `upstream`. Fails for `https://` in a gateway
    /// built without TLS: falling back to plain HTTP would send the device
    /// token unencrypted.
    pub(crate) fn for_upstream(upstream: &LlmUpstream) -> Result<Connector, String> {
        match upstream.scheme() {
            Scheme::Http => Ok(Connector::Plain),
            #[cfg(feature = "tls")]
            Scheme::Https => tls::client_config(upstream.ca()).map(Connector::Tls),
            #[cfg(not(feature = "tls"))]
            Scheme::Https => Err(
                "Dieses Gateway kann kein TLS (ohne Feature \"tls\" gebaut) und erreicht \
                 keinen https://-Server. Gateway-Image neu bauen: harness image build gateway"
                    .to_owned(),
            ),
        }
    }

    /// Wraps a fresh TCP connection to `host`; for TLS this includes the
    /// handshake, which checks the certificate against `host`. The error
    /// carries the reason for the log and a detail for the error page.
    // Only TLS needs the host name.
    #[cfg_attr(not(feature = "tls"), allow(unused_variables))]
    pub(crate) fn open(
        &self,
        host: &Host,
        socket: TcpStream,
    ) -> Result<Upstream, (Reason, String)> {
        match self {
            Connector::Plain => {
                Upstream::plain(socket).map_err(|e| (Reason::ConnectFailed, e.to_string()))
            }
            #[cfg(feature = "tls")]
            Connector::Tls(config) => tls::open(config, host, socket),
        }
    }
}
