use std::io::{Read, Write};
use std::sync::Arc;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, StreamOwned};
use ureq::unversioned::resolver::DefaultResolver;
use ureq::unversioned::transport::{
    Buffers, ConnectProxyConnector, ConnectionDetails, Connector, LazyBuffers, NextTimeout,
    TcpConnector, Transport, TransportAdapter,
};

/// The connector wraps ureq's TCP/proxy plumbing, retaining its deadlines.
/// The configured verifier checks the certificate on every new connection.
pub fn agent(
    config: ureq::config::Config,
    tls: Arc<ClientConfig>,
    origin: &str,
) -> Result<ureq::Agent, ureq::Error> {
    make_agent(config, tls, origin, None)
}

/// A connector completes TLS, then deliberately stops before returning a
/// transport to the HTTP layer. CONNECT proxy requests may establish a tunnel;
/// no application request or server token can be sent by discovery.
/// TLS/CONNECT I/O shares a five-second budget. DNS and TCP setup use ureq's
/// bounded per-phase timeouts, which can add setup latency before this budget
/// is checked by the transport.
pub fn discover(
    origin: &str,
    verifier: Arc<super::Verifier>,
) -> Result<super::CertificateInfo, ureq::Error> {
    let config = ureq::Agent::config_builder()
        .max_redirects(0)
        .timeout_global(Some(std::time::Duration::from_secs(5)))
        .build();
    let tls = super::client_config(verifier.clone())?;
    let deadline = std::time::Instant::now()
        .checked_add(std::time::Duration::from_secs(5))
        .ok_or(ureq::Error::Tls("discovery deadline overflow"))?;
    let agent = make_agent(config, tls, origin, Some(deadline))?;
    match agent.get(origin).call() {
        Err(ureq::Error::Other(error)) if error.is::<DiscoveryComplete>() => {
            verifier.observed().map_err(Into::into)
        }
        Err(error) => Err(error),
        Ok(_) => Err(ureq::Error::Tls("discovery unexpectedly sent HTTP")),
    }
}

fn make_agent(
    config: ureq::config::Config,
    tls: Arc<ClientConfig>,
    origin: &str,
    discovery_deadline: Option<std::time::Instant>,
) -> Result<ureq::Agent, ureq::Error> {
    let origin = endpoint(origin)?;
    let proxy_tls = super::client_config(super::Verifier::new(None, false)?)?;
    let connector =
        ().chain(ConnectProxyConnector::default())
            .chain(TcpConnector::default())
            .chain(TlsConnector {
                tls,
                proxy_tls,
                origin,
                discovery_deadline,
            });
    Ok(ureq::Agent::with_parts(
        config,
        connector,
        DefaultResolver::default(),
    ))
}

#[derive(Debug)]
struct TlsConnector {
    tls: Arc<ClientConfig>,
    proxy_tls: Arc<ClientConfig>,
    origin: (String, u16),
    discovery_deadline: Option<std::time::Instant>,
}

#[derive(Debug)]
struct DiscoveryComplete;
impl std::fmt::Display for DiscoveryComplete {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TLS discovery complete")
    }
}
impl std::error::Error for DiscoveryComplete {}

impl<In: Transport> Connector<In> for TlsConnector {
    type Out = Box<dyn Transport>;

    fn connect(
        &self,
        details: &ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        let mut transport = chained
            .ok_or(ureq::Error::Tls("missing TCP transport"))?
            .boxed();
        if let Some(deadline) = self.discovery_deadline {
            transport = Box::new(DiscoveryTransport {
                transport,
                deadline,
            });
        }
        if !details.needs_tls() {
            return Ok(Some(transport));
        }
        let authority = details
            .uri
            .authority()
            .ok_or(ureq::Error::Tls("missing server authority"))?;
        let host = authority
            .host()
            .trim_start_matches('[')
            .trim_end_matches(']');
        let name = ServerName::try_from(host.to_owned())
            .map_err(|_| ureq::Error::Tls("invalid server name"))?;
        let is_origin = endpoint(&details.uri.to_string())? == self.origin;
        let config = if is_origin {
            self.tls.clone()
        } else {
            self.proxy_tls.clone()
        };
        let connection = ClientConnection::new(config, name)?;
        let mut stream = StreamOwned::new(connection, DeadlineIo::new(transport));
        stream.sock.set_timeout(details.timeout)?;
        if is_origin && self.discovery_deadline.is_some() {
            while stream.conn.is_handshaking() {
                stream.conn.complete_io(&mut stream.sock)?;
            }
            return Err(ureq::Error::Other(Box::new(DiscoveryComplete)));
        }
        Ok(Some(Box::new(TlsTransport {
            stream,
            buffers: LazyBuffers::new(
                details.config.input_buffer_size(),
                details.config.output_buffer_size(),
            ),
        })))
    }
}

struct TlsTransport {
    stream: StreamOwned<ClientConnection, DeadlineIo>,
    buffers: LazyBuffers,
}

impl std::fmt::Debug for TlsTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TlsTransport")
    }
}

impl Transport for TlsTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.stream.sock.set_timeout(timeout)?;
        let output = self
            .buffers
            .output()
            .get(..amount)
            .ok_or(ureq::Error::Tls("invalid TLS output buffer length"))?;
        self.stream.write_all(output)?;
        Ok(())
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        self.stream.sock.set_timeout(timeout)?;
        let amount = self.stream.read(self.buffers.input_append_buf())?;
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }

    fn is_open(&mut self) -> bool {
        self.stream.sock.transport.get_mut().is_open()
    }
    fn is_tls(&self) -> bool {
        true
    }
}

fn endpoint(url: &str) -> Result<(String, u16), ureq::Error> {
    let url = url::Url::parse(url).map_err(|_| ureq::Error::Tls("invalid TLS endpoint"))?;
    let host = url
        .host_str()
        .ok_or(ureq::Error::Tls("missing TLS endpoint host"))?
        .to_owned();
    let port = url
        .port_or_known_default()
        .ok_or(ureq::Error::Tls("missing TLS endpoint port"))?;
    Ok((host, port))
}

/// This absolute budget also bounds CONNECT parsing and outer proxy TLS;
/// ureq otherwise passes the same relative timeout on each partial read.
#[derive(Debug)]
struct DiscoveryTransport {
    transport: Box<dyn Transport>,
    deadline: std::time::Instant,
}

impl DiscoveryTransport {
    fn remaining(&self, requested: NextTimeout) -> Result<NextTimeout, ureq::Error> {
        use ureq::unversioned::transport::time::Duration;
        let remaining = self
            .deadline
            .checked_duration_since(std::time::Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(ureq::Error::Timeout(ureq::Timeout::Global))?;
        match requested.after {
            Duration::Exact(duration) if duration < remaining => Ok(requested),
            _ => Ok(NextTimeout {
                after: Duration::Exact(remaining),
                reason: ureq::Timeout::Global,
            }),
        }
    }
}

impl Transport for DiscoveryTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.transport.buffers()
    }
    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.transport
            .transmit_output(amount, self.remaining(timeout)?)
    }
    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        self.transport.await_input(self.remaining(timeout)?)
    }
    fn is_open(&mut self) -> bool {
        self.transport.is_open()
    }
    fn is_tls(&self) -> bool {
        self.transport.is_tls()
    }
}

/// Rustls may perform many socket operations in one complete_io call. Recompute
/// the remaining budget for each, so partial progress cannot extend a deadline.
struct DeadlineIo {
    transport: TransportAdapter,
    deadline: Option<std::time::Instant>,
    reason: ureq::Timeout,
}

impl DeadlineIo {
    fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            transport: TransportAdapter::new(transport),
            deadline: None,
            reason: ureq::Timeout::Global,
        }
    }

    fn set_timeout(&mut self, timeout: NextTimeout) -> std::io::Result<()> {
        self.reason = timeout.reason;
        self.deadline = match timeout.after {
            ureq::unversioned::transport::time::Duration::Exact(duration) => Some(
                std::time::Instant::now()
                    .checked_add(duration)
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "TLS timeout is too large",
                        )
                    })?,
            ),
            ureq::unversioned::transport::time::Duration::NotHappening => None,
        };
        Ok(())
    }

    fn update_timeout(&mut self) -> std::io::Result<()> {
        use ureq::unversioned::transport::time::Duration;
        let after = match self.deadline {
            Some(deadline) => Duration::Exact(
                deadline
                    .checked_duration_since(std::time::Instant::now())
                    .filter(|remaining| !remaining.is_zero())
                    .ok_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::TimedOut, "TLS deadline elapsed")
                    })?,
            ),
            None => Duration::NotHappening,
        };
        self.transport.set_timeout(NextTimeout {
            after,
            reason: self.reason,
        });
        Ok(())
    }
}

impl Read for DeadlineIo {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.update_timeout()?;
        self.transport.read(buffer)
    }
}

impl Write for DeadlineIo {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.update_timeout()?;
        self.transport.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.update_timeout()?;
        self.transport.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct PartialInput {
        buffers: LazyBuffers,
    }
    impl Transport for PartialInput {
        fn buffers(&mut self) -> &mut dyn Buffers {
            &mut self.buffers
        }
        fn transmit_output(&mut self, _: usize, _: NextTimeout) -> Result<(), ureq::Error> {
            Ok(())
        }
        fn await_input(&mut self, _: NextTimeout) -> Result<bool, ureq::Error> {
            std::thread::sleep(std::time::Duration::from_millis(5));
            self.buffers.input_append_buf()[0] = 1;
            self.buffers.input_appended(1);
            Ok(true)
        }
        fn is_open(&mut self) -> bool {
            true
        }
    }

    #[test]
    fn origin_comparison_normalizes_default_ports_and_ip_addresses() {
        assert_eq!(
            endpoint("https://EXAMPLE.COM:443/").unwrap(),
            endpoint("https://example.com/").unwrap()
        );
        assert_eq!(
            endpoint("https://[0:0:0:0:0:0:0:1]:443/").unwrap(),
            endpoint("https://[::1]/").unwrap()
        );
        assert_ne!(
            endpoint("https://example.com:8443/").unwrap(),
            endpoint("https://example.com/").unwrap()
        );
        assert_ne!(
            endpoint("https://proxy.example/").unwrap(),
            endpoint("https://example.com/").unwrap()
        );
    }

    #[test]
    fn discovery_keeps_the_proxy_deadline_when_origin_tls_starts() {
        let proxy = DiscoveryTransport {
            transport: Box::new(PartialInput {
                buffers: LazyBuffers::new(16, 16),
            }),
            deadline: std::time::Instant::now() - std::time::Duration::from_secs(1),
        };
        let mut origin = DiscoveryTransport {
            transport: Box::new(proxy),
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(5),
        };
        let requested = NextTimeout {
            after: ureq::unversioned::transport::time::Duration::from_secs(5),
            reason: ureq::Timeout::Global,
        };
        assert!(matches!(
            origin.await_input(requested),
            Err(ureq::Error::Timeout(ureq::Timeout::Global))
        ));
        assert!(matches!(
            origin.transmit_output(0, requested),
            Err(ureq::Error::Timeout(ureq::Timeout::Global))
        ));
    }

    #[test]
    fn partial_progress_does_not_restart_the_tls_deadline() {
        let transport = PartialInput {
            buffers: LazyBuffers::new(16, 16),
        };
        let mut io = DeadlineIo::new(Box::new(transport));
        io.set_timeout(NextTimeout {
            after: ureq::unversioned::transport::time::Duration::from_millis(20),
            reason: ureq::Timeout::Global,
        })
        .unwrap();
        let mut byte = [0];
        let mut reads = 0;
        loop {
            match io.read(&mut byte) {
                Ok(1) => reads += 1,
                Err(error) => {
                    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
                    break;
                }
                other => panic!("unexpected read {other:?}"),
            }
            assert!(reads < 10, "the deadline restarted on partial progress");
        }
    }
}
