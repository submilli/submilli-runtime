//! The server's fuel, stack and time settings, exercised through
//! `POST /v1/execute`.
//!
//! Fuel is set on each store and the stack on the shared engine, so both have
//! to reach the path a request takes, not only `RuntimeConfig`. A limit also has
//! to be reported under its own kind wherever the program reaches it: in `main`,
//! in top-level statements, or under a callback a host function invoked.

use std::sync::Arc;
use std::time::Duration;

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

const TOP_LEVEL_LOOP: &str = r#"let total = 0;
while (true) { total = total + 1; }
export function main(): number { return total; }"#;

/// A handler around a callback that spins: reaching it would return `"caught"`.
const CAUGHT_LOOP_IN_CALLBACK: &str = r#"export function main(): string {
    try {
        [1].forEach((x: number) => { while (true) { } });
    } catch (e) {
        return "caught";
    }
    return "done";
}"#;

const HEALTHY: &str = "export function main(): number { return 42; }";

async fn assert_still_serves(router: &Router) {
    let healthy = execute(router, HEALTHY).await;
    assert_eq!(healthy["error"], Value::Null, "{healthy}");
    assert_eq!(healthy["result"], "42", "{healthy}");
}

fn assert_failed_with(response: &Value, kind: &str, message: &str) {
    assert_eq!(response["result"], Value::Null, "{response}");
    assert_eq!(response["error"]["kind"], kind, "{response}");
    let rendered = response["error"]["message"].as_str().unwrap_or_default();
    assert!(rendered.contains(message), "{response}");
}

#[tokio::test(flavor = "multi_thread")]
async fn top_level_statements_out_of_fuel_report_fuel_exhausted() {
    let limited = router(RuntimeConfig {
        fuel: 1_000_000,
        ..RuntimeConfig::default()
    });
    let response = execute(&limited, TOP_LEVEL_LOOP).await;
    assert_failed_with(&response, "fuel_exhausted", "fuel exhausted");
    // Points at the loop, as a limit reached in `main` points at its line.
    assert_failed_with(&response, "fuel_exhausted", "at <top level> (");
    assert_failed_with(
        &response,
        "fuel_exhausted",
        "2 | while (true) { total = total + 1; }",
    );
    assert_still_serves(&limited).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn top_level_statements_past_the_memory_cap_report_memory_exhausted() {
    const TOP_LEVEL_ALLOCATION: &str = r#"let s = "x";
for (let i = 0; i < 25; i++) { s = s + s; }
export function main(): number { return s.length; }"#;
    let limited = router(RuntimeConfig {
        max_store_bytes: 50 * 1024 * 1024,
        ..RuntimeConfig::default()
    });
    let response = execute(&limited, TOP_LEVEL_ALLOCATION).await;
    assert_failed_with(&response, "memory_exhausted", "memory exhausted");
    assert_still_serves(&limited).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn top_level_statements_are_bounded_by_the_execution_timeout() {
    let limited = router(RuntimeConfig {
        timeout: Some(Duration::from_secs(1)),
        ..RuntimeConfig::default()
    });
    let response = tokio::time::timeout(Duration::from_secs(15), execute(&limited, TOP_LEVEL_LOOP))
        .await
        .expect("the top-level loop must be interrupted");
    assert_failed_with(&response, "timeout", "timeout exceeded");
    assert_still_serves(&limited).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_top_level_throw_is_reported_with_its_message() {
    const TOP_LEVEL_THROW: &str = r#"const limits: number[] = [1];
console.log("before the throw");
if (limits.length === 1) { throw new RangeError("refused at the top level"); }
export function main(): number { return 1; }"#;
    let router = router(RuntimeConfig::default());
    let response = execute(&router, TOP_LEVEL_THROW).await;
    assert_eq!(response["result"], Value::Null, "{response}");
    assert_eq!(response["error"]["kind"], "runtime_error", "{response}");
    // The same header and frame layout a throw in `main` is rendered under.
    let message = response["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.starts_with("error: RangeError: refused at the top level\n  at <top level> ("),
        "{response}"
    );
    assert!(
        message.contains(
            "3 | if (limits.length === 1) { throw new RangeError(\"refused at the top level\"); }"
        ),
        "the source line of the throw: {response}"
    );
    assert!(
        response.to_string().contains("before the throw"),
        "console output before the failure is kept: {response}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn host_work_spends_fuel() {
    // A hundred iterations of Wasm cost next to nothing; the host copies the
    // 100,000-unit string in and out each time (about 25,000 fuel per
    // iteration at the COPY rate), and that is what runs out: the limit below
    // is more than one copy per iteration would cost, less than two.
    const HOST_HEAVY_LOOP: &str = r#"export function main(): number {
    let s = "x".repeat(100000);
    for (let i = 0; i < 100; i++) { s = s.toUpperCase(); }
    return s.length;
}"#;
    let generous = router(RuntimeConfig {
        fuel: 100_000_000,
        ..RuntimeConfig::default()
    });
    let response = execute(&generous, HOST_HEAVY_LOOP).await;
    assert_eq!(response["result"], "100000", "{response}");
    let limited = router(RuntimeConfig {
        fuel: 2_000_000,
        ..RuntimeConfig::default()
    });
    let response = execute(&limited, HOST_HEAVY_LOOP).await;
    assert_failed_with(&response, "fuel_exhausted", "fuel exhausted");
    assert_still_serves(&limited).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn fuel_spent_under_a_callback_is_not_catchable() {
    let limited = router(RuntimeConfig {
        fuel: 1_000_000,
        ..RuntimeConfig::default()
    });
    let response = execute(&limited, CAUGHT_LOOP_IN_CALLBACK).await;
    assert_failed_with(&response, "fuel_exhausted", "fuel exhausted");
    assert_still_serves(&limited).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn stack_exhaustion_under_a_callback_is_not_catchable() {
    const CAUGHT_RECURSION_IN_CALLBACK: &str = r#"function recurse(n: number): number {
    return recurse(n + 1) + 1;
}
export function main(): string {
    try {
        [1].forEach((x: number) => { recurse(x); });
    } catch (e) {
        return "caught";
    }
    return "done";
}"#;
    let router = router(RuntimeConfig::default());
    let response = execute(&router, CAUGHT_RECURSION_IN_CALLBACK).await;
    assert_failed_with(&response, "runtime_error", "call stack exhausted");
    assert_still_serves(&router).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_timeout_under_a_callback_is_not_catchable() {
    let limited = router(RuntimeConfig {
        timeout: Some(Duration::from_secs(1)),
        ..RuntimeConfig::default()
    });
    let response = tokio::time::timeout(
        Duration::from_secs(15),
        execute(&limited, CAUGHT_LOOP_IN_CALLBACK),
    )
    .await
    .expect("the loop must be interrupted");
    assert_failed_with(&response, "timeout", "timeout exceeded");
    assert_still_serves(&limited).await;
}
