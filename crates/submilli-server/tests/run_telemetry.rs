//! `ServerConfig::run_telemetry` decides whether a failed run, and the metrics a
//! run's outside calls emit, reach a Sentry client bound in the process, as the
//! CLI's own telemetry binds one when it is on. Its own test binary, with one test,
//! because the client is process-wide.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sentry::test::TestTransport;
use serde_json::{Value, json};
use submilli_server::{AppState, NetworkPolicy, RunTelemetry, ServerConfig, app};
use tower::ServiceExt;

const BLUEPRINT: &str = "name: demo\n";
const FAILING: &str = "function main(): number { throw new Error(\"boom\"); }";

async fn post(router: &axum::Router, path: &str, body: Value) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// The metric an HTTP call a program makes is reported under.
const HTTP_METRIC: &str = "submilli.server.http.duration_ms";

async fn run_failing_program(telemetry: RunTelemetry) {
    let router = router(telemetry, BLUEPRINT).await;
    let (status, body) = post(
        &router,
        "/v1/execute",
        json!({ "code": FAILING, "blueprint": "demo" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["error"]["kind"], "runtime_error", "{body}");
}

async fn router(telemetry: RunTelemetry, blueprint: &str) -> axum::Router {
    let config = ServerConfig {
        run_telemetry: telemetry,
        network_policy: NetworkPolicy::deny_private().allow_localhost(true),
        ..in_memory_config::config()
    };
    let router = app(AppState::new(config).expect("AppState"));
    let (status, body) = post(&router, "/v1/blueprints", json!({ "yaml": blueprint })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    router
}

/// One `200 ok` per connection, on loopback.
fn ok_fixture() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buffer = [0_u8; 4096];
            let _ = stream.read(&mut buffer);
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });
    port
}

async fn run_http_program(telemetry: RunTelemetry, port: u16) {
    let blueprint = "name: demo\nallow_insecure_http: true\npermissions:\n  main:\n    - capability: http.get\n      action: allow\n";
    let router = router(telemetry, blueprint).await;
    let code = format!(
        "import {{ get, Response }} from \"submilli:http\";\nfunction main(): string {{ const r: Response = get(\"http://127.0.0.1:{port}/\"); return r.body; }}"
    );
    let (status, body) = post(
        &router,
        "/v1/execute",
        json!({ "code": code, "blueprint": "demo" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["result"], "ok", "{body}");
}

/// Whether anything the client sent since the last call names the HTTP metric.
fn sent_http_metric(transport: &TestTransport) -> bool {
    if let Some(client) = sentry::Hub::current().client() {
        client.flush(Some(Duration::from_secs(5)));
    }
    transport
        .fetch_and_clear_envelopes()
        .iter()
        .any(|envelope| {
            let mut bytes = Vec::new();
            envelope.to_writer(&mut bytes).is_ok()
                && String::from_utf8_lossy(&bytes).contains(HTTP_METRIC)
        })
}

#[test]
fn a_run_reaches_sentry_only_when_run_telemetry_reports() {
    let transport = TestTransport::new();
    let _guard = sentry::init(sentry::ClientOptions {
        dsn: Some("https://public@sentry.invalid/1".parse().unwrap()),
        transport: Some(Arc::new(transport.clone())),
        ..Default::default()
    });
    // Built after the client is bound, so every worker thread's hub has it.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(run_failing_program(RunTelemetry::Off));
    let events = transport.fetch_and_clear_events();
    assert!(
        events.is_empty(),
        "run telemetry off still reported: {events:?}"
    );

    // The control: the same failure with the default does reach the client, so the
    // empty result above is the setting at work, not a client that never captures.
    runtime.block_on(run_failing_program(RunTelemetry::default()));
    let events = transport.fetch_and_clear_events();
    assert_eq!(events.len(), 1, "{events:?}");

    // A run's HTTP metrics: none with run telemetry off, and, as the control, some
    // with the default.
    let port = ok_fixture();
    let _ = sent_http_metric(&transport);
    runtime.block_on(run_http_program(RunTelemetry::Off, port));
    assert!(
        !sent_http_metric(&transport),
        "run telemetry off still sent the run's HTTP metrics"
    );
    runtime.block_on(run_http_program(RunTelemetry::default(), port));
    assert!(
        sent_http_metric(&transport),
        "the control sent no HTTP metric"
    );
}
