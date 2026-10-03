//! `GET /v1/sessions/{id}/last-run` over the one-shot `/v1/execute` path: each
//! execute mints a fresh session id and records its result/console under it, so
//! the run stays readable by that id even though the request is stateless.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_blueprint::Blueprint;
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

const BLUEPRINT_NAME: &str = "test";

fn router() -> Router {
    let blueprints = Arc::new(
        InMemoryBlueprintStore::seed([Blueprint {
            name: BLUEPRINT_NAME.into(),
            ..Default::default()
        }])
        .expect("seed blueprints"),
    );
    let config = ServerConfig {
        blueprints: Some(blueprints),
        ..ServerConfig::default()
    };
    app(AppState::new(config).expect("build AppState"))
}

async fn send(router: &Router, req: Request<Body>) -> (StatusCode, Value) {
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, parsed)
}

/// Run a one-shot `/v1/execute`; its response carries the generated `session_id`.
async fn execute(router: &Router, code: &str) -> (StatusCode, Value) {
    let body = json!({ "blueprint": BLUEPRINT_NAME, "code": code });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/execute")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    send(router, req).await
}

async fn last_run(router: &Router, session_id: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/sessions/{session_id}/last-run"))
        .body(Body::empty())
        .unwrap();
    send(router, req).await
}

fn session_of(exec: &Value) -> String {
    exec["session_id"]
        .as_str()
        .expect("session_id present")
        .to_string()
}

#[tokio::test]
async fn returns_404_without_prior_run() {
    let router = router();
    let (status, _) = last_run(&router, "unknown-session").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn returns_console_for_void_success() {
    let router = router();
    let (status, exec) = execute(&router, r#"function main(): void { console.log("hi"); }"#).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(exec["result"], Value::Null);
    assert_eq!(exec["console"], json!([]));

    let (status, body) = last_run(&router, &session_of(&exec)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    assert_eq!(body["console"], json!(["hi"]));
    assert_eq!(body["error"], Value::Null);
}

#[tokio::test]
async fn returns_value_and_console_for_nonvoid_success() {
    let router = router();
    let (_, exec) = execute(
        &router,
        r#"function main(): number { console.log("debug"); return 7; }"#,
    )
    .await;
    assert_eq!(exec["result"], json!("7"));
    assert_eq!(exec["console"], json!([]));

    let (status, body) = last_run(&router, &session_of(&exec)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], json!("7"));
    assert_eq!(body["console"], json!(["debug"]));
    assert_eq!(body["error"], Value::Null);
}

#[tokio::test]
async fn returns_error_state() {
    let router = router();
    let (_, exec) = execute(
        &router,
        r#"function main(): void { console.log("before"); assert(false, "boom"); }"#,
    )
    .await;
    assert_eq!(exec["error"]["kind"], json!("runtime_error"));

    let (status, body) = last_run(&router, &session_of(&exec)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    assert_eq!(body["console"], json!(["before"]));
    assert_eq!(body["error"]["kind"], json!("runtime_error"));
}

#[tokio::test]
async fn isolation_between_runs() {
    let router = router();
    let (_, a) = execute(
        &router,
        r#"function main(): void { console.log("apple"); }"#,
    )
    .await;
    let (_, b) = execute(
        &router,
        r#"function main(): void { console.log("banana"); }"#,
    )
    .await;

    // Each one-shot mints its own id, so the two runs never share a last-run.
    let (_, la) = last_run(&router, &session_of(&a)).await;
    let (_, lb) = last_run(&router, &session_of(&b)).await;
    assert_eq!(la["console"], json!(["apple"]));
    assert_eq!(lb["console"], json!(["banana"]));
}

#[tokio::test]
async fn generated_session_id_is_retrievable() {
    let router = router();
    let (_, exec) = execute(&router, r#"function main(): void { console.log("anon"); }"#).await;
    let (status, body) = last_run(&router, &session_of(&exec)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["console"], json!(["anon"]));
}
