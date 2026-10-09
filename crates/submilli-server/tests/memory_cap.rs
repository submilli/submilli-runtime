//! The per-execution memory cap, exercised through `POST /v1/execute`.
//!
//! Deliberately driven through the router rather than a hand-built `Store`.
//! `install_tenant_limits` is opt-in — nothing about constructing a store
//! installs it — and for a period the server never called it, so the engine fell
//! back to its 1 GiB abort-safety cap and a guest could hold twenty times what
//! the cap allows. The unit test covering the cap passed throughout, because it
//! built its own store and installed the limiter itself.
//!
//! So the assertion that matters is not "the engine enforces a cap" but "a
//! request served by this binary is bounded". Only the real path can make it.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

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

/// Doubles a string `rounds` times, so the live value is `2^rounds` UTF-16 code
/// units — twice that in bytes. Growing incrementally rather than in one
/// allocation on purpose: a single oversized request is the easy case, and the
/// path that went unbounded was this one.
fn doubling_program(rounds: u32) -> String {
    format!(
        r#"export function main(): string {{
             let s = "x";
             for (let i = 0; i < {rounds}; i++) {{ s = s + s; }}
             return `len ${{s.length}}`;
           }}"#
    )
}

fn router_with_cap(max_store_bytes: u64) -> Router {
    let blueprints = Arc::new(
        InMemoryBlueprintStore::seed([Blueprint {
            name: BLUEPRINT_NAME.into(),
            ..Default::default()
        }])
        .expect("seed blueprints"),
    );
    let config = ServerConfig {
        blueprints: Some(blueprints),
        runtime: RuntimeConfig {
            max_store_bytes,
            ..RuntimeConfig::default()
        },
        ..in_memory_config::config()
    };
    app(futures::executor::block_on(AppState::new(config)).expect("build AppState"))
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
async fn a_guest_over_the_cap_traps_instead_of_growing() {
    if !nightly_only_requested() {
        eprintln!("server memory cap: skipped; set SUBMILLI_TEST_NIGHTLY_ONLY=1 to run");
        return;
    }
    // 2^25 code units = 64 MB, against the 50 MB default.
    let response = execute(&router_with_cap(50 * 1024 * 1024), &doubling_program(25)).await;

    let error = response["error"].as_object().unwrap_or_else(|| {
        panic!(
            "a 64 MB allocation under a 50 MB cap should have trapped, got result {}",
            response["result"]
        )
    });
    assert_eq!(error["kind"], "memory_exhausted", "{response}");
    let message = error["message"].as_str().unwrap_or_default();
    assert!(
        message.starts_with("memory exhausted") && message.contains("out of memory"),
        "expected `memory exhausted` naming the out-of-memory cause, got: {message}"
    );
}

/// Reaching the cap ends the run like spent fuel does: a program that wraps its
/// work in `try`/`catch` must not carry on with partial data, and the caller
/// must be able to tell the limit from a bug in the program.
#[tokio::test(flavor = "multi_thread")]
async fn a_guest_cannot_catch_reaching_the_cap() {
    if !nightly_only_requested() {
        eprintln!("server memory cap: skipped; set SUBMILLI_TEST_NIGHTLY_ONLY=1 to run");
        return;
    }
    let program = r#"export function main(): string {
             let s = "x";
             try {
               for (let i = 0; i < 25; i++) { s = s + s; }
             } catch (e) {
               return "caught";
             }
             return `len ${s.length}`;
           }"#;
    let router = router_with_cap(50 * 1024 * 1024);

    let response = execute(&router, program).await;
    assert_eq!(response["result"], Value::Null, "{response}");
    assert_eq!(response["error"]["kind"], "memory_exhausted", "{response}");

    let healthy = execute(&router, &doubling_program(4)).await;
    assert_eq!(healthy["error"], Value::Null, "{healthy}");
    assert_eq!(healthy["result"], "len 16");
}

/// The cap has to leave the ordinary case alone, or the test above would pass
/// just as well against a server that refused everything.
#[tokio::test(flavor = "multi_thread")]
async fn a_guest_under_the_cap_runs_normally() {
    if !nightly_only_requested() {
        eprintln!("server memory cap: skipped; set SUBMILLI_TEST_NIGHTLY_ONLY=1 to run");
        return;
    }
    // 2^24 code units = 32 MB, comfortably inside the same 50 MB cap.
    let response = execute(&router_with_cap(50 * 1024 * 1024), &doubling_program(24)).await;

    assert_eq!(response["error"], Value::Null, "unexpected trap");
    assert_eq!(response["result"], "len 16777216");
}

/// An operator raising the budget must actually get it — the cap is a hard
/// ceiling with no in-band way around it, so a knob that silently did nothing
/// would leave a legitimate workload with no recourse.
#[tokio::test(flavor = "multi_thread")]
async fn a_raised_cap_admits_what_the_default_refuses() {
    if !nightly_only_requested() {
        eprintln!("server memory cap: skipped; set SUBMILLI_TEST_NIGHTLY_ONLY=1 to run");
        return;
    }
    let response = execute(&router_with_cap(200 * 1024 * 1024), &doubling_program(25)).await;

    assert_eq!(
        response["error"],
        Value::Null,
        "a 64 MB allocation should fit a 200 MB cap"
    );
    assert_eq!(response["result"], "len 33554432");
}

fn nightly_only_requested() -> bool {
    std::env::var("SUBMILLI_TEST_NIGHTLY_ONLY").is_ok_and(|value| {
        matches!(
            value.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}
