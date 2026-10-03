use super::*;

fn store(directory: &Path) -> Store {
    Store {
        path: directory.join("server-trust.json"),
    }
}
fn pin(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

#[test]
fn trust_is_scoped_to_normalized_host_and_port() {
    assert_eq!(
        authority(&https_url("https://EXAMPLE.COM/path").unwrap()).unwrap(),
        "example.com:443"
    );
    assert_eq!(
        authority(&https_url("https://example.com:8443/").unwrap()).unwrap(),
        "example.com:8443"
    );
    assert_eq!(
        authority(&https_url("https://[::1]:8128").unwrap()).unwrap(),
        "[::1]:8128"
    );
    assert!(https_url("http://example.com").is_err());
    assert!(https_url("https://user:secret@example.com").is_err());
}

#[test]
fn store_persists_keys_and_requires_removal_before_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let store = store(directory.path());
    store.add("example.com:443", &pin('a')).unwrap();
    store.add("example.com:8443", &pin('b')).unwrap();
    assert!(store.add("example.com:443", &pin('b')).is_err());
    assert_eq!(store.read().unwrap().pins.len(), 2);
    store.remove("example.com:443").unwrap();
    store.add("example.com:443", &pin('b')).unwrap();
    assert_eq!(store.read().unwrap().pins["example.com:443"], pin('b'));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&store.path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn corrupt_or_unknown_version_store_is_never_reset() {
    let directory = tempfile::tempdir().unwrap();
    let store = store(directory.path());
    for contents in [
        "broken",
        "{\"version\":2,\"pins\":{}}",
        "{\"version\":1,\"pins\":{\"a:443\":\"bad\"}}",
    ] {
        std::fs::write(&store.path, contents).unwrap();
        assert!(store.add("example.com:443", &pin('a')).is_err());
        assert_eq!(std::fs::read_to_string(&store.path).unwrap(), contents);
    }
}

#[test]
fn concurrent_updates_keep_every_host() {
    let directory = tempfile::tempdir().unwrap();
    std::thread::scope(|scope| {
        for number in 0..8 {
            let path = directory.path();
            scope.spawn(move || {
                store(path)
                    .add(&format!("host{number}:443"), &pin('a'))
                    .unwrap();
            });
        }
    });
    assert_eq!(store(directory.path()).read().unwrap().pins.len(), 8);
}

#[test]
fn independently_supplied_fingerprint_must_match() {
    let url = https_url("https://example.com").unwrap();
    let info = CertificateInfo {
        fingerprint: pin('a'),
        names: String::new(),
        validity: String::new(),
        publicly_trusted: false,
    };
    assert!(approve(&url, &info, Some(&pin('a'))).is_ok());
    assert!(approve(&url, &info, Some(&pin('b'))).is_err());
    assert!(normalize_fingerprint("sha256:short").is_err());
    assert_eq!(normalize_fingerprint(&pin('A')).unwrap(), pin('a'));
}
