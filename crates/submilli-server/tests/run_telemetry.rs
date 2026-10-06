//! `ServerConfig::run_telemetry` decides whether a failed run reaches a Sentry
//! client bound in the process, as the CLI's own telemetry binds one when it is on.
//! Its own test binary, because the client is process-wide.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sentry::test::TestTransport;
use serde_json::{Value, json};
use submilli_server::{AppState, RunTelemetry, ServerConfig, app};
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

async fn run_failing_program(telemetry: RunTelemetry) {
    let config = ServerConfig {
        run_telemetry: telemetry,
        ..in_memory_config::config()
    };
    let router = app(AppState::new(config).expect("AppState"));
    let (status, body) = post(&router, "/v1/blueprints", json!({ "yaml": BLUEPRINT })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = post(
        &router,
        "/v1/execute",
        json!({ "code": FAILING, "blueprint": "demo" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["error"]["kind"], "runtime_error", "{body}");
}

#[test]
fn a_failed_run_reaches_sentry_only_when_run_telemetry_reports() {
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
}
