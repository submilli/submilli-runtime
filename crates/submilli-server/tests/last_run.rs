//! `GET /v1/sessions/{id}/last-run`: the last run of a session, kept in the
//! server database with the run's end and gone with the session.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

#[path = "support/request_records.rs"]
mod request_records;
use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::database::ServerDatabase;
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

const BLUEPRINT: &str = "name: test\ndefault: allow\n";

fn config(blueprint: &str) -> ServerConfig {
    ServerConfig {
        blueprints: Some(Arc::new(
            InMemoryBlueprintStore::seed([submilli_blueprint::parse(blueprint).unwrap()]).unwrap(),
        )),
        ..in_memory_config::config()
    }
}

async fn router_with(config: ServerConfig) -> (AppState, Router) {
    let state = AppState::new(config).await.expect("build AppState");
    state.boot().await.expect("boot");
    (state.clone(), app(state))
}

async fn router() -> Router {
    router_with(config(BLUEPRINT)).await.1
}

/// A server over the database in `dir`, which outlives it.
fn persistent(dir: &Path) -> ServerConfig {
    ServerConfig {
        database_path: Some(dir.join("server.db")),
        session_cipher: Some(Arc::new(
            submilli_shared::secret_store::SecretCipher::from_key([7; 32]),
        )),
        ..config(BLUEPRINT)
    }
}

async fn send(router: &Router, req: Request<Body>) -> (StatusCode, Vec<u8>) {
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, bytes.to_vec())
}

fn parse(bytes: &[u8]) -> Value {
    if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(bytes).unwrap()
    }
}

fn post(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

async fn open_session(router: &Router) -> String {
    let (status, body) = send(router, post("/v1/sessions", json!({"blueprint": "test"}))).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    parse(&body)["session_id"].as_str().unwrap().to_owned()
}

async fn execute(router: &Router, session: &str, code: &str) -> Value {
    let (status, body) = send(
        router,
        post(
            &format!("/v1/sessions/{session}/execute"),
            json!({"code": code}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    parse(&body)
}

async fn one_shot(router: &Router, code: &str) -> Value {
    let (status, body) = send(
        router,
        post("/v1/execute", json!({"blueprint": "test", "code": code})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    parse(&body)
}

async fn last_run_bytes(router: &Router, session: &str) -> (StatusCode, Vec<u8>) {
    send(
        router,
        Request::builder()
            .uri(format!("/v1/sessions/{session}/last-run"))
            .body(Body::empty())
            .unwrap(),
    )
    .await
}

async fn last_run(router: &Router, session: &str) -> (StatusCode, Value) {
    let (status, bytes) = last_run_bytes(router, session).await;
    (status, parse(&bytes))
}

async fn delete(router: &Router, session: &str) {
    let (status, _) = send(
        router,
        Request::builder()
            .method("DELETE")
            .uri(format!("/v1/sessions/{session}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

async fn sql(database: &ServerDatabase, statement: &'static str) {
    database
        .transaction(move |connection| {
            Box::pin(async move {
                sqlx::query(statement).execute(connection).await?;
                Ok(())
            })
        })
        .await
        .unwrap();
}

fn large_result() -> String {
    format!(
        r#"function main(): string {{ console.log("large"); return "x".repeat({}); }}"#,
        100 * 1024
    )
}

fn spill_dir(dir: &Path, session: &str) -> std::path::PathBuf {
    dir.join("server.db-last-runs").join(session)
}

#[tokio::test]
async fn returns_404_without_prior_run() {
    let router = router().await;
    assert_eq!(
        last_run(&router, "unknown-session").await.0,
        StatusCode::NOT_FOUND
    );
    let session = open_session(&router).await;
    assert_eq!(last_run(&router, &session).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn keeps_the_console_a_successful_run_does_not_return() {
    let router = router().await;
    let session = open_session(&router).await;
    let exec = execute(
        &router,
        &session,
        r#"function main(): number { console.log("debug"); return 7; }"#,
    )
    .await;
    assert_eq!(exec["result"], json!("7"));
    assert_eq!(exec["console"], json!([]));
    let (status, bytes) = last_run_bytes(&router, &session).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, br#"{"result":"7","console":["debug"],"error":null}"#);
}

#[tokio::test]
async fn returns_error_state() {
    let router = router().await;
    let session = open_session(&router).await;
    let exec = execute(
        &router,
        &session,
        r#"function main(): void { console.log("before"); assert(false, "boom"); }"#,
    )
    .await;
    assert_eq!(exec["error"]["kind"], json!("runtime_error"));
    let (status, body) = last_run(&router, &session).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], Value::Null);
    assert_eq!(body["console"], json!(["before"]));
    assert_eq!(body["error"], exec["error"]);
}

#[tokio::test]
async fn the_latest_run_replaces_the_one_before_and_sessions_stay_apart() {
    let router = router().await;
    let (a, b) = (open_session(&router).await, open_session(&router).await);
    execute(
        &router,
        &a,
        r#"function main(): void { console.log("1"); }"#,
    )
    .await;
    execute(
        &router,
        &a,
        r#"function main(): void { console.log("2"); }"#,
    )
    .await;
    execute(
        &router,
        &b,
        r#"function main(): void { console.log("b"); }"#,
    )
    .await;
    assert_eq!(last_run(&router, &a).await.1["console"], json!(["2"]));
    assert_eq!(last_run(&router, &b).await.1["console"], json!(["b"]));
}

#[tokio::test]
async fn concurrent_large_runs_leave_one_of_them_and_one_file() {
    let dir = tempfile::tempdir().unwrap();
    let (_state, router) = router_with(persistent(dir.path())).await;
    let session = open_session(&router).await;
    let code = |n: usize| {
        format!(
            r#"function main(): string {{ return "{n}".repeat({}); }}"#,
            100 * 1024
        )
    };
    let runs = (0..8).map(|n| {
        let (router, session, code) = (router.clone(), session.clone(), code(n));
        async move { execute(&router, &session, &code).await }
    });
    let results: Vec<Value> = futures::future::join_all(runs).await;
    let (status, body) = last_run(&router, &session).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        results.iter().any(|exec| exec["result"] == body["result"]),
        "the last run is one of the runs"
    );
    // A replaced file goes just after its run's response.
    let files = || {
        std::fs::read_dir(spill_dir(dir.path(), &session))
            .unwrap()
            .count()
    };
    for _ in 0..100 {
        if files() == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(files(), 1, "every replaced file is gone");
}

#[tokio::test]
async fn a_one_shot_run_returns_its_console_and_keeps_no_last_run() {
    let router = router().await;
    let exec = one_shot(
        &router,
        r#"function main(): number { console.log("hi"); return 7; }"#,
    )
    .await;
    assert_eq!(exec["result"], json!("7"));
    assert_eq!(exec["console"], json!(["hi"]));
    let session = exec["session_id"].as_str().unwrap();
    assert_eq!(last_run(&router, session).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_a_session_deletes_its_last_run_and_its_file() {
    let dir = tempfile::tempdir().unwrap();
    let (_state, router) = router_with(persistent(dir.path())).await;
    let session = open_session(&router).await;
    execute(&router, &session, &large_result()).await;
    assert!(spill_dir(dir.path(), &session).exists());
    delete(&router, &session).await;
    assert_eq!(last_run(&router, &session).await.0, StatusCode::NOT_FOUND);
    assert!(!spill_dir(dir.path(), &session).exists());
}

#[tokio::test]
async fn a_restarted_server_keeps_last_runs_small_and_large() {
    let dir = tempfile::tempdir().unwrap();
    let (state, router) = router_with(persistent(dir.path())).await;
    let small = open_session(&router).await;
    let large = open_session(&router).await;
    execute(
        &router,
        &small,
        r#"function main(): number { console.log("canary"); return 7; }"#,
    )
    .await;
    execute(&router, &large, &large_result()).await;
    let before = (
        last_run_bytes(&router, &small).await,
        last_run_bytes(&router, &large).await,
    );
    assert_eq!(before.1.0, StatusCode::OK);
    assert_eq!(
        parse(&before.1.1)["result"].as_str().unwrap().len(),
        100 * 1024
    );
    state.database().unwrap().close().await.unwrap();
    drop((state, router));

    let database = std::fs::read(dir.path().join("server.db")).unwrap();
    assert!(
        !database.windows(6).any(|window| window == b"canary"),
        "a last run is sealed in the database"
    );
    let (_state, router) = router_with(persistent(dir.path())).await;
    assert_eq!(last_run_bytes(&router, &small).await, before.0);
    assert_eq!(last_run_bytes(&router, &large).await, before.1);
}

#[tokio::test]
async fn a_last_run_over_the_limit_answers_413_and_the_run_its_result() {
    let (_state, router) = router_with(ServerConfig {
        max_last_run_size: Some(64),
        ..config(BLUEPRINT)
    })
    .await;
    let session = open_session(&router).await;
    let exec = execute(
        &router,
        &session,
        r#"function main(): string { return "y".repeat(100); }"#,
    )
    .await;
    assert_eq!(exec["result"].as_str().unwrap().len(), 100);
    let (status, body) = last_run(&router, &session).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(
        body,
        json!({"error": "last_run_too_large", "bytes": 139, "limit": 64})
    );
}

#[tokio::test]
async fn a_failed_write_keeps_the_run_response_and_a_later_run_recovers() {
    let records = request_records::RequestRecords::ephemeral();
    let (_state, router) = router_with(ServerConfig {
        database: Some(records.database.clone()),
        ..config(BLUEPRINT)
    })
    .await;
    let session = open_session(&router).await;
    execute(&router, &session, "function main(): number { return 1; }").await;
    sql(
        &records.database,
        "CREATE TRIGGER fail_last_run BEFORE INSERT ON last_runs \
         BEGIN SELECT RAISE(FAIL, 'injected'); END",
    )
    .await;
    let exec = execute(&router, &session, "function main(): number { return 2; }").await;
    assert_eq!(exec["result"], "2");
    assert!(exec["error"].is_null(), "{exec}");
    assert_eq!(
        last_run(&router, &session).await.0,
        StatusCode::NOT_FOUND,
        "the run before is not the latest"
    );
    sql(&records.database, "DROP TRIGGER fail_last_run").await;
    execute(&router, &session, "function main(): number { return 3; }").await;
    assert_eq!(last_run(&router, &session).await.1["result"], "3");
}

#[tokio::test]
async fn a_failed_read_is_internal_even_for_a_missing_record() {
    let records = request_records::RequestRecords::ephemeral();
    let (_state, router) = router_with(ServerConfig {
        database: Some(records.database.clone()),
        ..config(BLUEPRINT)
    })
    .await;
    let session = open_session(&router).await;
    execute(&router, &session, "function main(): number { return 7; }").await;
    sql(&records.database, "ALTER TABLE last_runs RENAME TO hidden").await;
    for id in [session.as_str(), "missing"] {
        assert_eq!(
            last_run_bytes(&router, id).await,
            (StatusCode::INTERNAL_SERVER_ERROR, Vec::new())
        );
    }
    sql(&records.database, "ALTER TABLE hidden RENAME TO last_runs").await;
    assert_eq!(last_run(&router, &session).await.1["result"], "7");
}
