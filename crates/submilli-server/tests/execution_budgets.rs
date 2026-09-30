//! The server's fuel and stack settings, exercised through `POST /v1/execute`.
//!
//! Fuel is set on each store and the stack on the shared engine, so both have
//! to reach the path a request takes, not only `RuntimeConfig`.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_blueprint::Blueprint;
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::{AppState, RuntimeConfig, ServerConfig, app};
use tower::ServiceExt;

const BLUEPRINT_NAME: &str = "test";

const LOOP: &str = r#"export function main(): number {
    let total = 0;
    for (let i = 0; i < 1000000; i++) { total = total + i; }
    return total;
}"#;

const RECURSION: &str = r#"function depth(n: number): number {
    return n === 0 ? 0 : 1 + depth(n - 1);
}
export function main(): number {
    return depth(500);
}"#;

fn router(runtime: RuntimeConfig) -> Router {
    let blueprints = Arc::new(InMemoryBlueprintStore::seed([Blueprint {
        name: BLUEPRINT_NAME.into(),
        ..Default::default()
    }]));
    let config = ServerConfig {
        blueprints: Some(blueprints),
        runtime,
        ..ServerConfig::default()
    };
    app(AppState::new(config).expect("build AppState"))
}

async fn execute(router: &Router, code: &str) -> Value {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "code": code, "blueprint": BLUEPRINT_NAME }).to_string(),
        ))
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_small_fuel_budget_stops_a_long_loop() {
    let defaults = execute(&router(RuntimeConfig::default()), LOOP).await;
    assert_eq!(defaults["error"], Value::Null, "{defaults}");

    let limited = router(RuntimeConfig {
        fuel: 1_000_000,
        ..RuntimeConfig::default()
    });
    let response = execute(&limited, LOOP).await;
    assert_eq!(response["error"]["kind"], "fuel_exhausted", "{response}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_small_stack_stops_deep_recursion() {
    let defaults = execute(&router(RuntimeConfig::default()), RECURSION).await;
    assert_eq!(defaults["error"], Value::Null, "{defaults}");

    let limited = router(RuntimeConfig {
        max_wasm_stack: 16 * 1024,
        ..RuntimeConfig::default()
    });
    let response = execute(&limited, RECURSION).await;
    let message = response["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("stack"),
        "expected a stack overflow, got: {response}"
    );
}

/// A callback re-enters Wasm on the runtime thread's native stack, which only the
/// server's own runtime sizes for a raised budget. Run on that runtime, deep
/// re-entry has to end the program rather than overflow a worker thread.
#[test]
fn deep_reentry_ends_the_run_on_the_servers_runtime() {
    const REENTRY: &str = "function depth(n: number): number { if (n === 0) { return 0; } return [n].map((x: number) => depth(x - 1))[0] + 1; } export function main(): number { return depth(200000); }";
    let runtime_config = RuntimeConfig {
        max_wasm_stack: 1024 * 1024,
        ..RuntimeConfig::default()
    };
    let server_config = ServerConfig {
        runtime: runtime_config.clone(),
        ..ServerConfig::default()
    };
    let runtime = submilli_server::runtime(&server_config).expect("server runtime");
    let response = runtime.block_on(execute(&router(runtime_config), REENTRY));
    let message = response["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("call stack exhausted"),
        "expected the run to end at the stack limit, got: {response}"
    );
}
