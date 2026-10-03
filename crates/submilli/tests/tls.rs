//! CLI certificate trust at the process boundary, including rejected requests.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use rcgen::{CertificateParams, KeyPair};

struct Server {
    url: String,
    config: Arc<Mutex<Arc<rustls::ServerConfig>>>,
    requests: Arc<Mutex<Vec<String>>>,
    response: Arc<Mutex<String>>,
    finished: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    fn new(config: Arc<rustls::ServerConfig>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!(
            "https://localhost:{}",
            listener.local_addr().unwrap().port()
        );
        let config = Arc::new(Mutex::new(config));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let body =
            r#"{"status":"running","bind_addr":"127.0.0.1:8128","pid":1,"active_sessions":0}"#;
        let response = Arc::new(Mutex::new(format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )));
        let finished = Arc::new(AtomicBool::new(false));
        let worker = std::thread::spawn({
            let (config, requests, response, finished) = (
                config.clone(),
                requests.clone(),
                response.clone(),
                finished.clone(),
            );
            move || {
                while !finished.load(Ordering::Relaxed) {
                    let (socket, _) = match listener.accept() {
                        Ok(connection) => connection,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        Err(error) => panic!("accept: {error}"),
                    };
                    socket.set_nonblocking(false).unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    socket
                        .set_write_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let connection =
                        rustls::ServerConnection::new(config.lock().unwrap().clone()).unwrap();
                    let mut stream = rustls::StreamOwned::new(connection, socket);
                    let mut bytes = [0; 8192];
                    if let Ok(amount) = stream.read(&mut bytes)
                        && amount > 0
                    {
                        requests
                            .lock()
                            .unwrap()
                            .push(String::from_utf8_lossy(&bytes[..amount]).into_owned());
                        let _ = stream.write_all(response.lock().unwrap().as_bytes());
                        stream.conn.send_close_notify();
                        let _ = stream.flush();
                    }
                }
            }
        });
        Self {
            url,
            config,
            requests,
            response,
            finished,
            worker: Some(worker),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.finished.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn identity(key: &KeyPair, serial: u64) -> (Arc<rustls::ServerConfig>, String) {
    let mut params =
        CertificateParams::new(vec!["localhost".into(), "runtime.invalid".into()]).unwrap();
    params.serial_number = Some(serial.into());
    let cert = params.self_signed(key).unwrap();
    let pin = submilli_shared::tls::fingerprint(cert.der()).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![cert.der().clone()],
        rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into()),
    )
    .unwrap();
    (Arc::new(config), pin)
}

fn cli(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_submilli"))
        .args(args)
        .env_clear()
        .env("SUBMILLI_HOME", home)
        .env(
            "SUBMILLI_SERVER_TOKEN",
            "admin-token-0123456789abcdef0123456789",
        )
        .output()
        .unwrap()
}

fn error(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn trust_renewal_key_change_and_redirects_are_enforced_before_requests() {
    let home = tempfile::tempdir().unwrap();
    let key = KeyPair::generate().unwrap();
    let (config, pin) = identity(&key, 1);
    let server = Server::new(config);
    let status = || cli(home.path(), &["server", "status", "--server", &server.url]);
    let unknown = status();
    assert!(!unknown.status.success());
    assert!(error(&unknown).contains("trust add"), "{}", error(&unknown));
    assert!(server.requests.lock().unwrap().is_empty());
    let wrong_pin = format!("sha256:{}", "a".repeat(64));
    let wrong = cli(
        home.path(),
        &[
            "server",
            "trust",
            "add",
            "--server",
            &server.url,
            "--fingerprint",
            &wrong_pin,
        ],
    );
    assert!(!wrong.status.success());
    assert!(!home.path().join("server-trust.json").exists());
    let trusted = cli(
        home.path(),
        &[
            "server",
            "trust",
            "add",
            "--server",
            &server.url,
            "--fingerprint",
            &pin,
        ],
    );
    assert!(trusted.status.success(), "{}", error(&trusted));
    assert!(
        server.requests.lock().unwrap().is_empty(),
        "trust discovery sent HTTP"
    );
    let result = status();
    assert!(
        result.status.success(),
        "{}; requests={:?}",
        error(&result),
        server.requests.lock().unwrap()
    );
    assert!(server.requests.lock().unwrap()[0].contains("Bearer admin-token-"));
    *server.config.lock().unwrap() = identity(&key, 2).0;
    assert!(status().status.success(), "same-key renewal failed");
    let request_count = server.requests.lock().unwrap().len();
    *server.config.lock().unwrap() = identity(&KeyPair::generate().unwrap(), 3).0;
    for command in ["status", "stop"] {
        let changed = cli(home.path(), &["server", command, "--server", &server.url]);
        assert!(!changed.status.success());
        assert!(
            error(&changed).contains("public key changed"),
            "{}",
            error(&changed)
        );
    }
    assert_eq!(
        server.requests.lock().unwrap().len(),
        request_count,
        "rejected server received HTTP"
    );
    *server.config.lock().unwrap() = identity(&key, 4).0;
    let other = Server::new(identity(&key, 5).0);
    *server.response.lock().unwrap() = format!(
        "HTTP/1.1 302 Found\r\nLocation: {}/v1/status\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        other.url
    );
    assert!(!status().status.success());
    assert!(
        other.requests.lock().unwrap().is_empty(),
        "redirect target received a request"
    );
    let listed = cli(home.path(), &["server", "trust", "list"]);
    assert!(String::from_utf8_lossy(&listed.stdout).contains(&pin));
    assert!(
        cli(
            home.path(),
            &["server", "trust", "remove", "--server", &server.url]
        )
        .status
        .success()
    );
    assert!(!status().status.success());
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn discovery_and_authenticated_requests_share_connect_proxy_routing() {
    let home = tempfile::tempdir().unwrap();
    let (config, pin) = identity(&KeyPair::generate().unwrap(), 1);
    let server = Server::new(config);
    let port = url::Url::parse(&server.url).unwrap().port().unwrap();
    let origin = format!("https://runtime.invalid:{port}");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_url = format!("http://{}", listener.local_addr().unwrap());
    let tunnels = Arc::new(Mutex::new(Vec::new()));
    let proxy = std::thread::spawn({
        let tunnels = tunnels.clone();
        move || {
            // Trust add discovers once; a pinned status connects once.
            for _ in 0..2 {
                let (mut client, _) = listener.accept().unwrap();
                client
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut header = Vec::new();
                let mut byte = [0];
                while !header.ends_with(b"\r\n\r\n") && header.len() < 8192 {
                    client.read_exact(&mut byte).unwrap();
                    header.extend_from_slice(&byte);
                }
                let header = String::from_utf8(header).unwrap();
                assert!(
                    header.starts_with(&format!("CONNECT runtime.invalid:{port}")),
                    "{header}"
                );
                assert!(
                    !header.to_ascii_lowercase().contains("authorization:"),
                    "server token reached proxy"
                );
                tunnels.lock().unwrap().push(header);
                let mut upstream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
                client
                    .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                    .unwrap();
                let mut upstream_write = upstream.try_clone().unwrap();
                let mut client_read = client.try_clone().unwrap();
                let forward = std::thread::spawn(move || {
                    let _ = std::io::copy(&mut client_read, &mut upstream_write);
                    let _ = upstream_write.shutdown(std::net::Shutdown::Write);
                });
                let _ = std::io::copy(&mut upstream, &mut client);
                let _ = client.shutdown(std::net::Shutdown::Write);
                forward.join().unwrap();
            }
        }
    });
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_submilli"))
            .args(args)
            .env_clear()
            .env("SUBMILLI_HOME", home.path())
            .env("HTTPS_PROXY", &proxy_url)
            .env(
                "SUBMILLI_SERVER_TOKEN",
                "admin-token-0123456789abcdef0123456789",
            )
            .output()
            .unwrap()
    };
    let trusted = run(&[
        "server",
        "trust",
        "add",
        "--server",
        &origin,
        "--fingerprint",
        &pin,
    ]);
    assert!(trusted.status.success(), "{}", error(&trusted));
    assert!(server.requests.lock().unwrap().is_empty());
    let status = run(&["server", "status", "--server", &origin]);
    assert!(status.status.success(), "{}", error(&status));
    proxy.join().unwrap();
    assert_eq!(tunnels.lock().unwrap().len(), 2);
    assert_eq!(server.requests.lock().unwrap().len(), 1);
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[test]
fn an_absent_unpinned_https_server_reports_stopped() {
    let home = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "https://localhost:{}",
        listener.local_addr().unwrap().port()
    );
    drop(listener);
    for command in ["status", "stop"] {
        let result = cli(home.path(), &["server", command, "--server", &url]);
        assert!(
            String::from_utf8_lossy(&result.stdout).contains("stopped"),
            "{}",
            error(&result)
        );
        assert_eq!(result.status.success(), command == "stop");
    }
}
