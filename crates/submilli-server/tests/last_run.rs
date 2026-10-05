//! `GET /v1/sessions/{id}/last-run` over the one-shot `/v1/execute` path: each
//! execute mints a fresh session id and records its result/console under it, so
//! the run stays readable by that id even though the request is stateless.

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
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

const BLUEPRINT_NAME: &str = "test";

#[path = "common/last_run_store.rs"]
mod last_run_store;

fn router() -> Router {
    router_with_config(in_memory_config::config())
}

fn router_with_config(config: ServerConfig) -> Router {
    let blueprints = Arc::new(
        InMemoryBlueprintStore::seed([Blueprint {
            name: BLUEPRINT_NAME.into(),
            ..Default::default()
        }])
        .expect("seed blueprints"),
    );
    let config = ServerConfig {
        blueprints: Some(blueprints),
        ..config
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

#[tokio::test]
async fn recording_failure_preserves_execution_and_recovers_on_followup() {
    use std::sync::atomic::Ordering;
    let store = Arc::new(last_run_store::FaultStore::default());
    store.fail_writes.store(true, Ordering::SeqCst);
    let router = router_with_config(ServerConfig {
        sessions: Some(store.clone()),
        ..Default::default()
    });
    let logs = CapturedLogs::default();
    let writer = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_max_level(tracing::Level::WARN)
        .with_writer(move || writer.clone())
        .finish();
    // Requests can poll on worker tasks; capture their warnings across threads.
    tracing::subscriber::set_global_default(subscriber).expect("install test log capture");
    let (_, success) = execute(
        &router,
        r#"function main(): number { console.log("captured"); return 7; }"#,
    )
    .await;
    let captured = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(captured.contains("last-run storage failed"), "{captured}");
    assert!(captured.contains("operation=\"record\""), "{captured}");
    assert!(captured.contains(&session_of(&success)), "{captured}");
    assert!(captured.contains("private backend detail"), "{captured}");
    assert!(
        !captured.contains("captured"),
        "guest console must not be logged: {captured}"
    );
    assert_eq!(success["result"], "7");
    assert_eq!(success["console"], json!([]));
    assert!(success["error"].is_null(), "{success}");
    let (_, failed) = execute(&router, r#"function main(): void { console.log("before throw"); throw new Error("original failure"); }"#).await;
    assert_eq!(failed["error"]["kind"], "runtime_error");
    assert!(
        failed["error"]["message"]
            .as_str()
            .unwrap()
            .contains("original failure")
    );
    assert_eq!(failed["console"], json!(["before throw"]));
    assert!(!failed.to_string().contains("private backend detail"));
    assert_eq!(store.writes.load(Ordering::SeqCst), 2);
    assert_eq!(
        last_run(&router, &session_of(&success)).await.0,
        StatusCode::NOT_FOUND
    );

    store.fail_writes.store(false, Ordering::SeqCst);
    let (_, healthy) = execute(&router, r#"function main(): string { return "healthy"; }"#).await;
    let (status, stored) = last_run(&router, &session_of(&healthy)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored["result"], "healthy");
}

#[tokio::test]
async fn read_failure_is_internal_even_for_a_missing_record() {
    use std::sync::atomic::Ordering;
    let store = Arc::new(last_run_store::FaultStore::default());
    let router = router_with_config(ServerConfig {
        sessions: Some(store.clone()),
        ..Default::default()
    });
    let (_, response) = execute(&router, "function main(): number { return 7; }").await;
    store.fail_reads.store(true, Ordering::SeqCst);
    for session in [session_of(&response), "missing".into()] {
        let (status, body) = last_run(&router, &session).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body, Value::Null);
    }
    store.fail_reads.store(false, Ordering::SeqCst);
    assert_eq!(
        last_run(&router, &session_of(&response)).await.1["result"],
        "7"
    );
}

#[tokio::test]
async fn recording_failure_still_tears_down_one_shot_session() {
    use std::sync::atomic::Ordering;
    let store = Arc::new(last_run_store::FaultStore::default());
    store.fail_writes.store(true, Ordering::SeqCst);
    let root = tempfile::tempdir().unwrap();
    let blueprint = submilli_blueprint::parse("name: test\nvfs: per_session\n").unwrap();
    let router = app(AppState::new(ServerConfig {
        sessions: Some(store),
        blueprints: Some(Arc::new(InMemoryBlueprintStore::seed([blueprint]).unwrap())),
        session_storage_root: Some(root.path().into()),
        ..Default::default()
    })
    .unwrap());
    let (_, response) = execute(&router, "function main(): number { return 7; }").await;
    assert_eq!(response["result"], "7");
    assert!(!root.path().join(session_of(&response)).exists());
}

#[tokio::test]
async fn recording_failure_settles_idempotency_without_reexecuting() {
    use std::sync::atomic::Ordering;
    use submilli_server::idempotency_store::{IdempotencyStore, InMemoryIdempotencyStore};
    let store = Arc::new(last_run_store::FaultStore::default());
    store.fail_writes.store(true, Ordering::SeqCst);
    let ledger = Arc::new(InMemoryIdempotencyStore::default());
    let blueprint = submilli_blueprint::parse("name: test\ndefault: allow\n").unwrap();
    let router = app(AppState::new(ServerConfig {
        sessions: Some(store.clone()),
        idempotency_store: Some(ledger.clone()),
        blueprints: Some(Arc::new(InMemoryBlueprintStore::seed([blueprint]).unwrap())),
        ..Default::default()
    })
    .unwrap());
    let (_, created) = send(
        &router,
        Request::builder()
            .method("POST")
            .uri("/v1/sessions")
            .header("content-type", "application/json")
            .body(Body::from(json!({"blueprint": "test"}).to_string()))
            .unwrap(),
    )
    .await;
    let session = session_of(&created);
    let code = r#"import session from "submilli:session";
        function main(): number {
            const old = session.get("count");
            const next = old === null ? 1 : (old as number) + 1;
            session.set("count", next);
            return next;
        }"#;
    let request = |key: &str| {
        Request::builder()
            .method("POST")
            .uri(format!("/v1/sessions/{session}/execute"))
            .header("content-type", "application/json")
            .header("idempotency-key", key)
            .body(Body::from(json!({"code": code}).to_string()))
            .unwrap()
    };
    let (status, first) = send(&router, request("first")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["result"], "1", "{first}");
    assert!(
        ledger
            .load(&session, "first")
            .await
            .unwrap()
            .unwrap()
            .outcome()
            .is_some()
    );
    let (status, replay) = send(&router, request("first")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replay, first);
    assert_eq!(store.writes.load(Ordering::SeqCst), 1);
    let (_, next) = send(&router, request("next")).await;
    assert_eq!(
        next["result"], "2",
        "replay must not increment guest state: {next}"
    );
}

#[derive(Clone, Default)]
struct CapturedLogs(Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for CapturedLogs {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
