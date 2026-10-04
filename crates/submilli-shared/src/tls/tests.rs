use super::*;
use rcgen::{CertificateParams, KeyPair, date_time_ymd};

fn certificate(key: &KeyPair) -> rcgen::Certificate {
    CertificateParams::new(vec!["localhost".into()])
        .unwrap()
        .self_signed(key)
        .unwrap()
}

fn verify(
    verifier: &Verifier,
    cert: &CertificateDer<'_>,
    host: &str,
) -> Result<ServerCertVerified, rustls::Error> {
    verifier.verify_server_cert(
        cert,
        &[],
        &ServerName::try_from(host.to_owned()).unwrap(),
        &[],
        UnixTime::now(),
    )
}

#[test]
fn a_pin_accepts_renewal_with_the_same_key_but_refuses_a_changed_key() {
    let key = KeyPair::generate().unwrap();
    let first = certificate(&key);
    let verifier = Verifier::new(Some(fingerprint(first.der()).unwrap()), false).unwrap();
    let mut params = CertificateParams::new(vec!["localhost".into()]).unwrap();
    params.serial_number = Some(123.into());
    let renewed = params.self_signed(&key).unwrap();
    assert_ne!(first.der(), renewed.der());
    assert!(verify(&verifier, renewed.der(), "localhost").is_ok());
    let changed = certificate(&KeyPair::generate().unwrap());
    assert!(
        verify(&verifier, changed.der(), "localhost")
            .unwrap_err()
            .to_string()
            .contains("public key changed")
    );
}

#[test]
fn discovery_and_pins_enforce_name_expiry_and_server_key_usage() {
    let key = KeyPair::generate().unwrap();
    let cert = certificate(&key);
    let discovery = Verifier::new(None, true).unwrap();
    assert!(verify(&discovery, cert.der(), "localhost").is_ok());
    assert!(!discovery.observed().unwrap().publicly_trusted);
    assert!(verify(&discovery, cert.der(), "other.example").is_err());
    let standard = Verifier::new(None, false).unwrap();
    assert!(verify(&standard, cert.der(), "localhost").is_err());
    let mut params = CertificateParams::new(vec!["localhost".into()]).unwrap();
    params.not_after = date_time_ymd(2000, 1, 1);
    let expired = params.self_signed(&key).unwrap();
    let pinned = Verifier::new(Some(fingerprint(expired.der()).unwrap()), false).unwrap();
    assert!(verify(&pinned, expired.der(), "localhost").is_err());
    assert!(verify(&discovery, expired.der(), "localhost").is_err());
    params.not_before = date_time_ymd(4090, 1, 1);
    params.not_after = date_time_ymd(4096, 1, 1);
    let future = params.self_signed(&key).unwrap();
    assert!(verify(&pinned, future.der(), "localhost").is_err());
    params.not_before = date_time_ymd(1975, 1, 1);
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ClientAuth];
    let wrong_usage = params.self_signed(&key).unwrap();
    assert!(verify(&pinned, wrong_usage.der(), "localhost").is_err());
    assert!(fingerprint(&CertificateDer::from(vec![0])).is_err());
}

#[test]
fn only_the_local_probe_can_ignore_the_advertised_hostname() {
    let cert = certificate(&KeyPair::generate().unwrap());
    let pin = fingerprint(cert.der()).unwrap();
    assert!(
        verify(
            &Verifier::new(Some(pin.clone()), false).unwrap(),
            cert.der(),
            "127.0.0.1"
        )
        .is_err()
    );
    assert!(
        verify(
            &Verifier::local_probe(pin).unwrap(),
            cert.der(),
            "127.0.0.1"
        )
        .is_ok()
    );
}

#[test]
fn certificate_names_print_as_host_names_and_addresses() {
    let key = KeyPair::generate().unwrap();
    let cert = CertificateParams::new(vec![
        "localhost".into(),
        "runtime.example.com".into(),
        "10.0.0.1".into(),
        "::1".into(),
    ])
    .unwrap()
    .self_signed(&key)
    .unwrap();
    let (_, parsed) = parse_x509_certificate(cert.der().as_ref()).unwrap();
    let san = parsed.subject_alternative_name().unwrap().unwrap();
    assert_eq!(
        display_names(&san.value.general_names),
        "localhost, runtime.example.com, 10.0.0.1, ::1"
    );
}
