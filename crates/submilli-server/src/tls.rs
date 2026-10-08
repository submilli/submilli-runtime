//! Certificate loading and a bounded, concurrent TLS listener for axum.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio_rustls::{TlsAcceptor, server::TlsStream};

const MAX_PEM_BYTES: u64 = 1024 * 1024;
const MAX_PENDING_HANDSHAKES: usize = 128;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub enum TlsError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Pem {
        path: PathBuf,
        source: rustls::pki_types::pem::Error,
    },
    EmptyCertificateChain(PathBuf),
    TooLarge(PathBuf),
    Identity(rustls::Error),
}

impl std::fmt::Display for TlsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "reading TLS file `{}`: {source}", path.display())
            }
            Self::Pem { path, source } => {
                write!(f, "invalid TLS PEM `{}`: {source}", path.display())
            }
            Self::EmptyCertificateChain(path) => write!(
                f,
                "certificate file `{}` contains no certificates",
                path.display()
            ),
            Self::TooLarge(path) => write!(f, "TLS file `{}` exceeds 1 MiB", path.display()),
            Self::Identity(error) => write!(
                f,
                "TLS certificate and private key must be valid and match: {error}"
            ),
        }
    }
}

impl std::error::Error for TlsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Pem { source, .. } => Some(source),
            Self::Identity(error) => Some(error),
            _ => None,
        }
    }
}

pub fn certificates(path: &Path) -> Result<Vec<CertificateDer<'static>>, TlsError> {
    let bytes = read_pem(path)?;
    let certificates = CertificateDer::pem_slice_iter(&bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| TlsError::Pem {
            path: path.into(),
            source,
        })?;
    if certificates.is_empty() {
        return Err(TlsError::EmptyCertificateChain(path.into()));
    }
    Ok(certificates)
}

pub fn load(cert_file: &Path, key_file: &Path) -> Result<Arc<rustls::ServerConfig>, TlsError> {
    let certs = certificates(cert_file)?;
    let key =
        PrivateKeyDer::from_pem_slice(&read_pem(key_file)?).map_err(|source| TlsError::Pem {
            path: key_file.into(),
            source,
        })?;
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(TlsError::Identity)?
    .with_no_client_auth()
    .with_single_cert(certs, key)
    .map_err(TlsError::Identity)?;
    Ok(Arc::new(config))
}

fn read_pem(path: &Path) -> Result<Vec<u8>, TlsError> {
    use std::io::Read;
    let mut bytes = Vec::new();
    let read = || -> std::io::Result<Vec<u8>> {
        std::fs::File::open(path)?
            .take(MAX_PEM_BYTES + 1)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    };
    let bytes = read().map_err(|source| TlsError::Io {
        path: path.into(),
        source,
    })?;
    if bytes.len() as u64 > MAX_PEM_BYTES {
        return Err(TlsError::TooLarge(path.into()));
    }
    Ok(bytes)
}

pub(crate) struct Listener {
    tcp: TcpListener,
    acceptor: TlsAcceptor,
    pending: JoinSet<Option<(TlsStream<tokio::net::TcpStream>, SocketAddr)>>,
}

impl Listener {
    pub(crate) fn new(tcp: TcpListener, config: Arc<rustls::ServerConfig>) -> Self {
        Self {
            tcp,
            acceptor: TlsAcceptor::from(config),
            pending: JoinSet::new(),
        }
    }
}

impl axum::serve::Listener for Listener {
    type Io = TlsStream<tokio::net::TcpStream>;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            tokio::select! {
                completed = self.pending.join_next(), if !self.pending.is_empty() => {
                    match completed {
                        Some(Ok(Some(connection))) => return connection,
                        Some(Err(error)) => tracing::warn!(%error, "TLS handshake task failed"),
                        _ => {},
                    }
                }
                incoming = self.tcp.accept(), if self.pending.len() < MAX_PENDING_HANDSHAKES => {
                    match incoming {
                        Ok((socket, addr)) => {
                            let acceptor = self.acceptor.clone();
                            self.pending.spawn(async move {
                                match tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(socket)).await {
                                    Ok(Ok(stream)) => Some((stream, addr)),
                                    _ => None,
                                }
                            });
                        }
                        Err(error) => {
                            tracing::warn!(%error, "TLS TCP accept failed");
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }
                }
            }
        }
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.tcp.local_addr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> (tempfile::TempDir, rcgen::CertifiedKey<rcgen::KeyPair>) {
        let directory = tempfile::tempdir().unwrap();
        let identity = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        std::fs::write(directory.path().join("cert.pem"), identity.cert.pem()).unwrap();
        std::fs::write(
            directory.path().join("key.pem"),
            identity.signing_key.serialize_pem(),
        )
        .unwrap();
        (directory, identity)
    }

    #[test]
    fn loading_refuses_malformed_missing_and_mismatched_files() {
        let (directory, _) = files();
        let cert = directory.path().join("cert.pem");
        let key = directory.path().join("key.pem");
        assert!(load(&cert, &key).is_ok());
        let other = rcgen::KeyPair::generate().unwrap();
        std::fs::write(&key, other.serialize_pem()).unwrap();
        assert!(load(&cert, &key).is_err());
        std::fs::write(&key, "broken").unwrap();
        assert!(load(&cert, &key).is_err());
        std::fs::write(&cert, "broken").unwrap();
        assert!(load(&cert, &key).is_err());
        assert!(certificates(&directory.path().join("absent.pem")).is_err());
    }

    #[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn https_serves_authenticated_routes_while_a_handshake_stalls() {
        let (directory, identity) = files();
        let tls = load(
            &directory.path().join("cert.pem"),
            &directory.path().join("key.pem"),
        )
        .unwrap();
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = tcp.local_addr().unwrap();
        let pin = submilli_shared::tls::fingerprint(identity.cert.der()).unwrap();
        let token = "admin-token-0123456789abcdef0123456789";
        let state = crate::AppState::new(crate::ServerConfig {
            auth: crate::AuthConfig::Tokens(vec![
                crate::ApiToken::new("admin", crate::Role::Admin, token).unwrap(),
            ]),
            ..crate::config::test_config()
        })
        .await
        .unwrap();
        let shutdown = state.shutdown_signal();
        let server = tokio::spawn({
            let shutdown = shutdown.clone();
            async move {
                axum::serve(Listener::new(tcp, tls), crate::app(state))
                    .with_graceful_shutdown(async move { shutdown.notified().await })
                    .await
                    .unwrap();
            }
        });
        let stalled = tokio::net::TcpStream::connect(addr).await.unwrap();
        tokio::task::spawn_blocking(move || {
            let tls = submilli_shared::tls::client_config(
                submilli_shared::tls::Verifier::new(Some(pin), false).unwrap(),
            )
            .unwrap();
            let base = format!("https://localhost:{}", addr.port());
            let agent = submilli_shared::tls::agent(
                ureq::Agent::config_builder()
                    .http_status_as_error(false)
                    .timeout_global(Some(Duration::from_secs(2)))
                    .build(),
                tls,
                &base,
            )
            .unwrap();
            assert_eq!(
                agent
                    .get(&format!("{base}/healthz"))
                    .call()
                    .unwrap()
                    .status(),
                200
            );
            assert_eq!(
                agent
                    .get(&format!("{base}/v1/status"))
                    .call()
                    .unwrap()
                    .status(),
                401
            );
            assert_eq!(
                agent
                    .get(&format!("{base}/v1/status"))
                    .header("Authorization", format!("Bearer {token}"))
                    .call()
                    .unwrap()
                    .status(),
                200
            );
            assert_ne!(
                agent
                    .post(&format!("{base}/mcp"))
                    .header("Authorization", format!("Bearer {token}"))
                    .header("Content-Type", "application/json")
                    .send("{}")
                    .unwrap()
                    .status(),
                401
            );
            assert_eq!(
                agent
                    .post(&format!("{base}/v1/shutdown"))
                    .header("Authorization", format!("Bearer {token}"))
                    .send_empty()
                    .unwrap()
                    .status(),
                200
            );
        })
        .await
        .unwrap();
        drop(stalled);
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }
}
