//! Server-specific TLS verification. A pin replaces issuer trust only; names,
//! validity, key usage, and TLS handshake signatures remain checked.

use std::sync::{Arc, Mutex};

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
};
use sha2::{Digest, Sha256};
use x509_parser::prelude::*;

mod transport;
pub use transport::{agent, discover};

#[derive(Clone, Debug)]
pub struct CertificateInfo {
    pub fingerprint: String,
    pub names: String,
    pub validity: String,
    pub publicly_trusted: bool,
}

pub fn fingerprint(cert: &CertificateDer<'_>) -> Result<String, rustls::Error> {
    let parsed = rustls::server::ParsedCertificate::try_from(cert)?;
    let digest = Sha256::digest(parsed.subject_public_key_info().as_ref());
    Ok(format!("sha256:{digest:x}"))
}

/// Discovery is used only for a credential-free handshake. It permits an
/// unknown issuer after checking the leaf; the caller must obtain approval
/// before using this identity for HTTP requests.
#[derive(Debug)]
pub struct Verifier {
    standard: Arc<WebPkiServerVerifier>,
    provider: Arc<CryptoProvider>,
    pin: Option<String>,
    discovery: bool,
    check_name: bool,
    observed: Mutex<Option<CertificateInfo>>,
}

impl Verifier {
    pub fn new(pin: Option<String>, discovery: bool) -> Result<Arc<Self>, rustls::Error> {
        Self::build(pin, discovery, true)
    }

    /// Local health probes verify the key from the server's configured PEM
    /// instead of its advertised hostname, which may not resolve to loopback.
    pub fn local_probe(pin: String) -> Result<Arc<Self>, rustls::Error> {
        Self::build(Some(pin), false, false)
    }

    fn build(
        pin: Option<String>,
        discovery: bool,
        check_name: bool,
    ) -> Result<Arc<Self>, rustls::Error> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let standard =
            WebPkiServerVerifier::builder_with_provider(Arc::new(roots), provider.clone())
                .build()
                .map_err(|e| {
                    rustls::Error::General(format!("building certificate verifier: {e}"))
                })?;
        Ok(Arc::new(Self {
            standard,
            provider,
            pin,
            discovery,
            check_name,
            observed: Mutex::new(None),
        }))
    }

    pub fn observed(&self) -> Result<CertificateInfo, rustls::Error> {
        self.observed
            .lock()
            .map_err(|_| rustls::Error::General("certificate observation lock poisoned".into()))?
            .clone()
            .ok_or_else(|| rustls::Error::General("server did not present a certificate".into()))
    }
}

impl ServerCertVerifier for Verifier {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        name: &ServerName<'_>,
        ocsp: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let actual = fingerprint(cert)?;
        if let Some(expected) = &self.pin {
            if expected != &actual {
                return Err(rustls::Error::General(format!(
                    "server public key changed: expected {expected}, received {actual}; verify the change before removing the saved trust"
                )));
            }
            validate_leaf(cert, name, now, self.check_name)?;
            return Ok(ServerCertVerified::assertion());
        }
        let result = self
            .standard
            .verify_server_cert(cert, intermediates, name, ocsp, now);
        if !self.discovery {
            return result;
        }
        let publicly_trusted = match result {
            Ok(_) => true,
            Err(rustls::Error::InvalidCertificate(CertificateError::UnknownIssuer)) => {
                validate_leaf(cert, name, now, true)?;
                false
            }
            Err(e) => return Err(e),
        };
        let (_, parsed) = parse_x509_certificate(cert.as_ref()).map_err(|_| bad_encoding())?;
        let names = parsed
            .subject_alternative_name()
            .map_err(|_| bad_encoding())?
            .map_or_else(
                || "no subject alternative names".into(),
                |san| display_names(&san.value.general_names),
            );
        let info = CertificateInfo {
            fingerprint: actual,
            names,
            validity: format!(
                "{} to {}",
                parsed.validity().not_before,
                parsed.validity().not_after
            ),
            publicly_trusted,
        };
        *self.observed.lock().map_err(|_| {
            rustls::Error::General("certificate observation lock poisoned".into())
        })? = Some(info);
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            signature,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub fn client_config(verifier: Arc<Verifier>) -> Result<Arc<ClientConfig>, rustls::Error> {
    let builder =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()?;
    Ok(Arc::new(
        builder
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_no_client_auth(),
    ))
}

fn validate_leaf(
    cert: &CertificateDer<'_>,
    name: &ServerName<'_>,
    now: UnixTime,
    check_name: bool,
) -> Result<(), rustls::Error> {
    let parsed = rustls::server::ParsedCertificate::try_from(cert)?;
    if check_name {
        rustls::client::verify_server_name(&parsed, name)?;
    }
    let (_, x509) = parse_x509_certificate(cert.as_ref()).map_err(|_| bad_encoding())?;
    let seconds = i64::try_from(now.as_secs()).map_err(|_| bad_encoding())?;
    if seconds < x509.validity().not_before.timestamp() {
        return Err(rustls::Error::InvalidCertificate(
            CertificateError::NotValidYet,
        ));
    }
    if seconds > x509.validity().not_after.timestamp() {
        return Err(rustls::Error::InvalidCertificate(CertificateError::Expired));
    }
    if let Some(usage) = x509.extended_key_usage().map_err(|_| bad_encoding())?
        && !(usage.value.server_auth || usage.value.any)
    {
        return Err(rustls::Error::InvalidCertificate(
            CertificateError::InvalidPurpose,
        ));
    }
    if let Some(usage) = x509.key_usage().map_err(|_| bad_encoding())?
        && !usage.value.digital_signature()
    {
        return Err(rustls::Error::InvalidCertificate(
            CertificateError::InvalidPurpose,
        ));
    }
    Ok(())
}

fn bad_encoding() -> rustls::Error {
    rustls::Error::InvalidCertificate(CertificateError::BadEncoding)
}

/// The certificate's names as a person compares them with the host they
/// meant: `localhost, 10.0.0.1`. Host names, addresses, e-mail names, and URIs
/// print bare; anything rarer keeps the parser's labelled form.
fn display_names(names: &[GeneralName<'_>]) -> String {
    names
        .iter()
        .map(|name| match name {
            GeneralName::DNSName(s) | GeneralName::RFC822Name(s) | GeneralName::URI(s) => {
                (*s).to_owned()
            }
            GeneralName::IPAddress(bytes) => match *bytes {
                [a, b, c, d] => std::net::Ipv4Addr::new(*a, *b, *c, *d).to_string(),
                _ => <[u8; 16]>::try_from(*bytes).map_or_else(
                    |_| name.to_string(),
                    |octets| std::net::Ipv6Addr::from(octets).to_string(),
                ),
            },
            other => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests;
