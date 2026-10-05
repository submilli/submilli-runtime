//! End-to-end auth-proxy test against a **real** local HTTP server.
//!
//! A real `.subm` script (`fixtures/auth_proxy/script.subm`) calls `http.get`;
//! a real blueprint (`fixtures/auth_proxy/blueprint.yaml`) injects an
//! `Authorization` header from a store secret. The script runs through the full
//! server `/v1/execute` path — blueprint → `BlueprintAuthProxy` → the real
//! `ureq` client → the mock server, which validates the header on the wire.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::{AppState, FileSecretStore, KeySource, SecretStore, ServerConfig, app};
use tower::ServiceExt;

/// Build a router with a preloaded blueprint store.
fn router_with_blueprint(config_base: ServerConfig, yaml: &str) -> Router {
    let blueprint = submilli_blueprint::parse(yaml).expect("valid blueprint");
    let store = Arc::new(InMemoryBlueprintStore::seed([blueprint]).expect("seed blueprints"));
    app(AppState::new(ServerConfig {
        blueprints: Some(store),
        ..config_base
    })
    .expect("AppState"))
}

const SCRIPT: &str = include_str!("fixtures/auth_proxy/script.subm");
const BLUEPRINT: &str = include_str!("fixtures/auth_proxy/blueprint.yaml");
const TOKEN_KEY: &str = "api/token";
const TOKEN: &str = "s3cr3t-xyz";
const STORE_TOKEN: &str = "store-tok-789";
const BEARER_KEY: &str = "api/bearer";
const BEARER_TOKEN: &str = "bearer-tok-456";
const BASIC_KEY: &str = "api/basic";
const BASIC_PASSWORD: &str = "hunter2";

/// A secret store over a fresh temp dir, with a single key pre-loaded.
async fn store_with(key: &str, value: &str) -> Arc<dyn SecretStore> {
    let tmp = tempfile::tempdir().expect("tempdir");
    let key_path = tmp.path().join("key.b64");
    std::fs::write(
        &key_path,
        base64::engine::general_purpose::STANDARD.encode([9u8; 32]),
    )
    .unwrap();
    let store: Arc<dyn SecretStore> = Arc::new(
        FileSecretStore::open(tmp.path().join("secrets"), &KeySource::File(key_path)).unwrap(),
    );
    store.put(key, value).await.unwrap();
    // Keep the tempdir alive for the store's lifetime by leaking it; the test
    // process is short-lived and the OS reclaims it.
    std::mem::forget(tmp);
    store
}

/// Spawn a one-shot HTTP server that records the `Authorization` header it
/// receives and answers 200 only when it equals `expected`, else 403.
fn spawn_mock(expected: String) -> (u16, Arc<Mutex<Option<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let captured = Arc::new(Mutex::new(None));
    let sink = captured.clone();

    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false).unwrap();
                    let auth = {
                        let mut reader = BufReader::new(&stream);
                        let mut found = None;
                        loop {
                            let mut line = String::new();
                            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                                break;
                            }
                            let line = line.trim_end();
                            if line.is_empty() {
                                break; // end of headers
                            }
                            if let Some((key, value)) = line.split_once(':')
                                && key.trim().eq_ignore_ascii_case("authorization")
                            {
                                found = Some(value.trim().to_string());
                            }
                        }
                        found
                    };
                    *sink.lock().unwrap() = auth.clone();
                    let ok = auth.as_deref() == Some(expected.as_str());
                    let (status, body) = if ok {
                        ("200 OK", "ok")
                    } else {
                        ("403 Forbidden", "no")
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{body}",
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

    (port, captured)
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

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn auth_proxy_injects_header_to_real_server() {
    let store = store_with(TOKEN_KEY, TOKEN).await;
    let (port, captured) = spawn_mock(format!("Bearer {TOKEN}"));

    let router = router_with_blueprint(
        ServerConfig {
            secret_store: Some(store),
            ..in_memory_config::config()
        },
        BLUEPRINT,
    );

    // Run the real script against the mock, with the dynamic port substituted in.
    let code = SCRIPT.replace("__PORT__", &port.to_string());
    let (status, body) = post(
        &router,
        "/v1/execute",
        json!({ "code": code, "blueprint": "auth-demo" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["error"].is_null(), "execute errored: {body}");
    assert_eq!(
        body["result"],
        json!("ok"),
        "script did not get 200: {body}"
    );

    // The header arrived on the wire, resolved from the store secret.
    assert_eq!(
        captured.lock().unwrap().as_deref(),
        Some(format!("Bearer {TOKEN}").as_str()),
        "mock did not see the injected Authorization header"
    );
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn auth_proxy_injects_store_secret_to_real_server() {
    let (port, captured) = spawn_mock(format!("Bearer {STORE_TOKEN}"));

    let store = store_with("api/token", STORE_TOKEN).await;
    let config = ServerConfig {
        secret_store: Some(store),
        ..in_memory_config::config()
    };
    let router = app(AppState::new(config).expect("AppState"));

    let blueprint = "name: auth-store-demo\nallow_insecure_http: true\nsecrets:\n  TOK:\n    store: api/token\nauth_proxy:\n  - host: 127.0.0.1\n    allow_insecure_http: true\n    headers:\n      Authorization: \"Bearer ${secrets.TOK}\"\npermissions:\n  main:\n    - capability: http.get\n      action: allow\n";
    let (status, body) = post(&router, "/v1/blueprints", json!({ "yaml": blueprint })).await;
    assert_eq!(status, StatusCode::OK, "blueprint add failed: {body}");

    let code = SCRIPT.replace("__PORT__", &port.to_string());
    let (status, body) = post(
        &router,
        "/v1/execute",
        json!({ "code": code, "blueprint": "auth-store-demo" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["error"].is_null(), "execute errored: {body}");
    assert_eq!(
        body["result"],
        json!("ok"),
        "script did not get 200: {body}"
    );
    assert_eq!(
        captured.lock().unwrap().as_deref(),
        Some(format!("Bearer {STORE_TOKEN}").as_str()),
        "mock did not see the header resolved from the secret store"
    );
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn auth_proxy_bearer_method_injects_header() {
    let store = store_with(BEARER_KEY, BEARER_TOKEN).await;
    let (port, captured) = spawn_mock(format!("Bearer {BEARER_TOKEN}"));

    let blueprint = format!(
        "name: auth-bearer-demo\nallow_insecure_http: true\nsecrets:\n  TOK:\n    store: {BEARER_KEY}\nauth_proxy:\n  - host: 127.0.0.1\n    allow_insecure_http: true\n    auth:\n      bearer: TOK\npermissions:\n  main:\n    - capability: http.get\n      action: allow\n"
    );
    let router = router_with_blueprint(
        ServerConfig {
            secret_store: Some(store),
            ..in_memory_config::config()
        },
        &blueprint,
    );

    let code = SCRIPT.replace("__PORT__", &port.to_string());
    let (status, body) = post(
        &router,
        "/v1/execute",
        json!({ "code": code, "blueprint": "auth-bearer-demo" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["error"].is_null(), "execute errored: {body}");
    assert_eq!(
        body["result"],
        json!("ok"),
        "script did not get 200: {body}"
    );
    assert_eq!(
        captured.lock().unwrap().as_deref(),
        Some(format!("Bearer {BEARER_TOKEN}").as_str()),
        "mock did not see the injected `auth: bearer` header"
    );
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn auth_proxy_basic_method_injects_base64_header() {
    let store = store_with(BASIC_KEY, BASIC_PASSWORD).await;
    let expected = format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("alice:{BASIC_PASSWORD}"))
    );
    let (port, captured) = spawn_mock(expected.clone());

    let blueprint = format!(
        "name: auth-basic-demo\nallow_insecure_http: true\nsecrets:\n  PW:\n    store: {BASIC_KEY}\nauth_proxy:\n  - host: 127.0.0.1\n    allow_insecure_http: true\n    auth:\n      basic:\n        username: alice\n        password: PW\npermissions:\n  main:\n    - capability: http.get\n      action: allow\n"
    );
    let router = router_with_blueprint(
        ServerConfig {
            secret_store: Some(store),
            ..in_memory_config::config()
        },
        &blueprint,
    );

    let code = SCRIPT.replace("__PORT__", &port.to_string());
    let (status, body) = post(
        &router,
        "/v1/execute",
        json!({ "code": code, "blueprint": "auth-basic-demo" }),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["error"].is_null(), "execute errored: {body}");
    assert_eq!(
        body["result"],
        json!("ok"),
        "script did not get 200: {body}"
    );
    assert_eq!(
        captured.lock().unwrap().as_deref(),
        Some(expected.as_str()),
        "mock did not see the base64-encoded `auth: basic` header"
    );
}

#[tokio::test]
async fn plain_http_denials_reach_execute_without_touching_the_network() {
    for (blueprint, rule) in [(false, false), (false, true), (true, false)] {
        let yaml = format!(
            "name: gates\ndefault: allow\nallow_insecure_http: {blueprint}\nsecrets:\n  K: {{ harness: {{}} }}\nauth_proxy:\n- host: 127.0.0.1\n  allow_insecure_http: {rule}\n  auth: {{ bearer: K }}\n"
        );
        let router = router_with_blueprint(in_memory_config::config(), &yaml);
        for operation in [
            "get(\"http://127.0.0.1:1/?token=never-print-this\");",
            "download(\"http://127.0.0.1:1/?token=never-print-this\", \"/payload\");",
        ] {
            let code = format!(
                r#"import {{ get, download }} from "submilli:http";
                function main(): string {{
                    try {{ {operation} return "allowed"; }}
                    catch (error) {{ return (error as Error).message; }}
                }}"#
            );
            let (_, body) = post(
                &router,
                "/v1/execute",
                json!({
                    "blueprint": "gates", "code": code,
                }),
            )
            .await;
            assert!(body["error"].is_null(), "{body}");
            let error = body["result"].as_str().unwrap();
            assert!(error.contains("HTTPS required"), "{body}");
            assert!(!error.contains("never-print-this"), "{body}");
            assert!(
                error.contains(if blueprint {
                    "auth_proxy rule"
                } else {
                    "blueprint"
                }),
                "{body}"
            );
        }
    }
}
