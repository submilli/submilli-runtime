//! End-to-end SSRF-policy tests against a **real** local HTTP server.
//!
//! A real `.subm` script (`fixtures/ssrf/script.subm`) calls `http.get` on a
//! loopback mock through the full `/v1/execute` path. The server's
//! [`NetworkPolicy`] decides whether the loopback resolution is permitted:
//! deny-private blocks it; an `allow_localhost` / `allow_cidr` opt-out lets it
//! through. This exercises the real `ureq` client + `PolicyResolver`.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_server::{AppState, NetworkPolicy, ServerConfig, app};
use tower::ServiceExt;

const SCRIPT: &str = include_str!("fixtures/ssrf/script.subm");
const BLUEPRINT: &str = include_str!("fixtures/ssrf/blueprint.yaml");

/// Spawn a one-shot loopback server that answers every request with `200 ok`.
/// If the policy blocks the request it is never contacted; the thread reaps
/// itself at the deadline.
fn spawn_ok_mock() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();

    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf);
                    let body = "ok";
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(resp.as_bytes());
                    let _ = stream.flush();
                    return;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return,
            }
        }
    });

    port
}

async fn post(router: &Router, path: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

/// Build a server with the given policy, register the blueprint, and run the
/// loopback-hitting script against a fresh mock. Returns the `/v1/execute` body.
async fn run_against_loopback(policy: NetworkPolicy) -> Value {
    run_against_loopback_host(policy, "127.0.0.1").await
}

/// As [`run_against_loopback`], with the script targeting `host` instead of the
/// literal address, so the policy is exercised at DNS resolution rather than
/// by the literal-IP pre-check.
async fn run_against_loopback_host(policy: NetworkPolicy, host: &str) -> Value {
    let port = spawn_ok_mock();
    let config = ServerConfig {
        network_policy: policy,
        ..ServerConfig::default()
    };
    let router = app(AppState::new(config).expect("AppState"));

    let (status, body) = post(&router, "/v1/blueprints", json!({ "yaml": BLUEPRINT })).await;
    assert_eq!(status, StatusCode::OK, "blueprint add failed: {body}");

    let code = SCRIPT
        .replace("127.0.0.1", host)
        .replace("__PORT__", &port.to_string());
    let (status, body) = post(
        &router,
        "/v1/execute",
        json!({ "code": code, "blueprint": "ssrf-test" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deny_private_blocks_loopback() {
    let body = run_against_loopback(NetworkPolicy::deny_private()).await;

    assert!(
        body["result"].is_null(),
        "blocked request must not return a result: {body}"
    );
    assert!(
        !body["error"].is_null(),
        "expected the loopback request to be blocked, got: {body}"
    );
    // The error names the policy reason so an operator/LLM can act on it.
    // (Host-fn errors currently surface as the message without the wasm
    // call-site backtrace under the async runtime — see FOLLOWUP in the PR.)
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("blocked by network policy"),
        "expected the policy reason: {message}"
    );
}

/// A host name that resolves only to loopback is refused inside the resolver.
/// reqwest reports that as a generic send failure; the policy's reason has to
/// survive from the bottom of its error chain, or the guest can't tell a block
/// from an outage.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deny_private_blocks_loopback_by_host_name() {
    let body = run_against_loopback_host(NetworkPolicy::deny_private(), "localhost").await;

    assert!(
        body["result"].is_null(),
        "blocked request must not return a result: {body}"
    );
    let message = body["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains(
            "blocked by network policy: localhost resolves only to private/loopback IP space"
        ),
        "expected the policy reason for a host name: {message}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn allow_localhost_permits_loopback() {
    let body = run_against_loopback(NetworkPolicy::deny_private().allow_localhost(true)).await;

    assert!(body["error"].is_null(), "execute errored: {body}");
    assert_eq!(
        body["result"],
        json!("ok"),
        "script did not get 200: {body}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn allow_ip_cidr_permits_loopback() {
    let policy = NetworkPolicy::deny_private().allow_cidr("127.0.0.0/8".parse().unwrap());
    let body = run_against_loopback(policy).await;

    assert!(body["error"].is_null(), "execute errored: {body}");
    assert_eq!(
        body["result"],
        json!("ok"),
        "script did not get 200: {body}"
    );
}
