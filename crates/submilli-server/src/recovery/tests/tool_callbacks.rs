use super::*;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const CODE: &str = r#"import tools from "@tools";
function main(): number { const first = tools.echo({value: 1}); return first + tools.echo({value: first}); }"#;
const ONE: &str =
    r#"import tools from "@tools"; function main(): number { return tools.echo({value: 1}); }"#;

async fn request(
    router: &Router,
    uri: &str,
    value: Value,
    key: Option<&str>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(key) = key {
        req = req.header("idempotency-key", key);
    }
    let response = tokio::time::timeout(
        Duration::from_secs(15),
        router
            .clone()
            .oneshot(req.body(Body::from(value.to_string())).unwrap()),
    )
    .await
    .unwrap()
    .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| json!({"body": String::from_utf8_lossy(&bytes)}));
    (status, value)
}

async fn session(state: &AppState, timeout: &str) -> (Router, String) {
    session_with_vfs(state, timeout, "none").await
}

async fn session_with_vfs(state: &AppState, timeout: &str, vfs: &str) -> (Router, String) {
    let blueprint = submilli_blueprint::parse(&format!(
        "name: callbacks\ndefault: allow\nvfs: {vfs}\ntool_call_timeout: {timeout}\n"
    ))
    .unwrap();
    state.blueprints().upsert(blueprint).await.unwrap();
    let router = crate::app(state.clone());
    let (status, body) = request(&router, "/v1/sessions", json!({"blueprint":"callbacks", "tools":[{
        "name":"echo", "input_schema":{"type":"object", "properties":{"value":{"type":"number"}}, "required":["value"]}, "output_schema":{"type":"number"}
    }]}), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    (router, body["session_id"].as_str().unwrap().into())
}

async fn execute(router: &Router, session: &str, source: &str) -> Value {
    let (status, body) = request(
        router,
        &format!("/v1/sessions/{session}/execute"),
        json!({"code":source}),
        Some("key"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

fn call(body: &Value) -> String {
    assert_eq!(body["status"], "suspended", "{body}");
    body["calls"][0]["call_id"].as_str().unwrap().into()
}

fn resume_router(state: AppState) -> Router {
    // Exercise the real handler while explicitly controlling startup; the full
    // router normally boots automatically before dispatching its first request.
    Router::new()
        .route(
            "/v1/runs/{run}/resume",
            axum::routing::post(crate::handlers::runs::resume),
        )
        .with_state(state)
}

async fn resume(router: &Router, run: &str, call: &str, value: i32) -> (StatusCode, Value) {
    request(
        router,
        &format!("/v1/runs/{run}/resume"),
        json!({"results":[{"call_id":call, "ok":true, "value":value}]}),
        None,
    )
    .await
}

async fn outcomes(state: &AppState, run: &str) -> i64 {
    let run = run.to_owned();
    state
        .database()
        .unwrap()
        .transaction(move |connection| {
            Box::pin(async move {
                Ok(
                    sqlx::query_scalar("SELECT count(*) FROM durable_outcomes WHERE run_id=?")
                        .bind(run)
                        .fetch_one(connection)
                        .await?,
                )
            })
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn duplicate_execute_and_resume_preserve_one_effect_and_terminal_retry_needs_no_history() {
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 10_000_000).await;
    state.boot().await.unwrap();
    let (router, session) = session(&state, "30s").await;
    let first = execute(&router, &session, CODE).await;
    let run = first["execution_id"].as_str().unwrap();
    let first_id = call(&first);
    assert_eq!(execute(&router, &session, CODE).await, first);
    let (a, b) = tokio::join!(
        resume(&router, run, &first_id, 3),
        resume(&router, run, &first_id, 3)
    );
    assert_eq!(a.0, StatusCode::OK, "{a:?}");
    assert_eq!(b, a);
    let second = a.1;
    let second_id = call(&second);
    assert_ne!(first_id, second_id);
    assert_eq!(second["calls"][0]["args"]["value"], 3);
    assert_eq!(outcomes(&state, run).await, 1);
    // A conflicting old answer is idempotent, and cannot answer the next tool.
    assert_eq!(resume(&router, run, &first_id, 99).await.1, second);
    assert_eq!(outcomes(&state, run).await, 1);
    let (status, done) = resume(&router, run, &second_id, 4).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["result"], "7", "{done}");
    until(&state, &run.into(), "completed").await;
    stop(&state).await;
    state.session_manager().retire_run(&run.into()).await;
    assert_eq!(outcomes(&state, run).await, 0);
    // Completed retries have no dependency on decoding the replay program.
    let damaged = run.to_owned();
    state
        .database()
        .unwrap()
        .transaction(move |connection| {
            Box::pin(async move {
                sqlx::query("UPDATE run_programs SET content=x'00' WHERE run_id=?")
                    .bind(damaged)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    // Reopen to discard all memory; only the terminal idempotency result remains.
    state.database().unwrap().close().await.unwrap();
    drop(router);
    drop(state);
    let reopened = app(root.path(), 10_000_000).await;
    let router = crate::app(reopened.clone());
    assert_eq!(
        resume(&router, run, &second_id, 99).await,
        (StatusCode::OK, done.clone())
    );
    assert_eq!(execute(&router, &session, CODE).await, done);
}

#[tokio::test]
async fn resume_before_recovery_waits_then_continues_the_reconstructed_call() {
    let root = tempfile::tempdir().unwrap();
    let first = app(root.path(), 10_000_000).await;
    first.boot().await.unwrap();
    let (router, session) = session(&first, "30s").await;
    let suspended = execute(&router, &session, CODE).await;
    let run = suspended["execution_id"].as_str().unwrap().to_owned();
    let first_id = call(&suspended);
    stop(&first).await;
    first.database().unwrap().close().await.unwrap();
    drop(router);
    drop(first);
    let second = app(root.path(), 10_000_000).await;
    let router = resume_router(second.clone());
    let submission = tokio::spawn({
        let router = router.clone();
        let run = run.clone();
        let first_id = first_id.clone();
        async move { resume(&router, &run, &first_id, 6).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !submission.is_finished(),
        "a submission must wait for recovery"
    );
    second.boot().await.unwrap();
    let (status, next) = submission.await.unwrap();
    assert_eq!(status, StatusCode::OK, "{next}");
    let second_id = call(&next);
    assert_eq!(next["calls"][0]["args"]["value"], 6);
    assert_eq!(resume(&router, &run, &first_id, 99).await.1, next);
    assert_eq!(outcomes(&second, &run).await, 1);
    // Recover a second time, now with a committed first answer in the prefix.
    stop(&second).await;
    second.database().unwrap().close().await.unwrap();
    drop(router);
    drop(second);
    let third = app(root.path(), 10_000_000).await;
    third.boot().await.unwrap();
    let router = crate::app(third.clone());
    assert_eq!(execute(&router, &session, CODE).await, next);
    assert_eq!(resume(&router, &run, &first_id, 99).await.1, next);
    let (status, done) = resume(&router, &run, &second_id, 7).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert_eq!(done["result"], "13");
    stop(&third).await;
}

#[tokio::test]
async fn timeout_is_committed_once_and_late_resume_returns_the_terminal_result() {
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 10_000_000).await;
    state.boot().await.unwrap();
    let (router, session) = session(&state, "1s").await;
    let first = execute(&router, &session, ONE).await;
    let id = call(&first);
    let run = first["execution_id"].as_str().unwrap();
    let done = until(&state, &run.into(), "completed").await;
    assert!(done["response"]["error"].is_object(), "{done}");
    assert_eq!(resume(&router, run, &id, 99).await.1, done["response"]);
    assert!(outcomes(&state, run).await <= 1);
    stop(&state).await;
}

#[tokio::test]
async fn shutdown_rejects_a_resume_waiting_for_recovery() {
    let root = tempfile::tempdir().unwrap();
    let first = app(root.path(), 10_000_000).await;
    first.boot().await.unwrap();
    let (router, session) = session(&first, "30s").await;
    let wait = execute(&router, &session, ONE).await;
    let id = call(&wait);
    let run = wait["execution_id"].as_str().unwrap().to_owned();
    stop(&first).await;
    first.database().unwrap().close().await.unwrap();
    drop(router);
    drop(first);
    let second = app(root.path(), 10_000_000).await;
    let router = resume_router(second.clone());
    let request = tokio::spawn(async move { resume(&router, &run, &id, 1).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!request.is_finished());
    stop(&second).await;
    assert_eq!(request.await.unwrap().0, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
async fn durable_tool_result_over_http_socket() {
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 10_000_000).await;
    state.boot().await.unwrap();
    let (router, session) = session(&state, "30s").await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::new();
    let first: Value = client
        .post(format!("{url}/v1/sessions/{session}/execute"))
        .header("idempotency-key", "key")
        .json(&json!({"code":ONE}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let call_id = call(&first);
    let run = first["execution_id"].as_str().unwrap();
    let body = json!({"results":[{"call_id":call_id,"ok":true,"value":42}]});
    for _ in 0..2 {
        let done: Value = client
            .post(format!("{url}/v1/runs/{run}/resume"))
            .json(&body)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(done["result"], "42", "{done}");
    }
    stop(&state).await;
    server.abort();
    let _ = server.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tool_crash_child() {
    let Some(root) = std::env::var_os("SUBMILLI_RECOVERY_CRASH_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let state = app(&root, 10_000_000).await;
    state.boot().await.unwrap();
    let (router, session) = session(&state, "30s").await;
    let first = execute(&router, &session, CODE).await;
    std::fs::write(root.join("tool-call.json"), first.to_string()).unwrap();
    super::process::checkpoint("tool-wait");
    let run = first["execution_id"].as_str().unwrap();
    resume(&router, run, &call(&first), 3).await;
    panic!("tool crash checkpoint was never reached");
}

#[tokio::test]
async fn killed_process_replays_tools_before_and_after_answer_commit() {
    use std::process::{Command, Stdio};
    for point in ["tool-wait", "tool-answer", "tool-committed"] {
        let root = tempfile::tempdir().unwrap();
        let log = std::fs::File::create(root.path().join("child.log")).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "recovery::tests::tool_callbacks::tool_crash_child",
                "--nocapture",
            ])
            .env("SUBMILLI_RECOVERY_CRASH_ROOT", root.path())
            .env("SUBMILLI_RECOVERY_CRASH_POINT", point)
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !root.path().join("checkpoint").exists() {
            if child.try_wait().unwrap().is_some() || std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!(
                    "{point}: {}",
                    std::fs::read_to_string(root.path().join("child.log")).unwrap()
                );
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        child.kill().unwrap();
        child.wait().unwrap();
        let first: Value =
            serde_json::from_slice(&std::fs::read(root.path().join("tool-call.json")).unwrap())
                .unwrap();
        let run = first["execution_id"].as_str().unwrap();
        let first_id = call(&first);
        let state = app(root.path(), 10_000_000).await;
        state.boot().await.unwrap();
        let router = crate::app(state.clone());
        let recovered = execute(&router, first["session_id"].as_str().unwrap(), CODE).await;
        if point == "tool-committed" {
            assert_eq!(recovered["calls"][0]["args"]["value"], 3);
        } else {
            assert_eq!(recovered, first);
        }
        let (status, next) = resume(&router, run, &first_id, 3).await;
        assert_eq!(status, StatusCode::OK, "{point}: {next}");
        assert_eq!(outcomes(&state, run).await, 1);
        let (status, done) = resume(&router, run, &call(&next), 4).await;
        assert_eq!(status, StatusCode::OK, "{point}: {done}");
        assert_eq!(done["result"], "7");
        assert_eq!(until(&state, &run.into(), "completed").await["attempt"], 2);
        stop(&state).await;
        state.database().unwrap().close().await.unwrap();
    }
}

#[tokio::test]
async fn committed_cancellation_takes_precedence_over_a_pending_memory_view() {
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 10_000_000).await;
    state.boot().await.unwrap();
    let (router, session) = session(&state, "30s").await;
    let pending = execute(&router, &session, ONE).await;
    let id = RunId::from(pending["execution_id"].as_str().unwrap());
    let mut unit = state.units().begin().await.unwrap();
    let mut run = unit.get_run(&id).await.unwrap().unwrap();
    run.cancel().unwrap();
    unit.save_run(run).await.unwrap();
    unit.commit().await.unwrap();
    assert!(matches!(
        state.session_manager().runs().get(&id).unwrap().current(),
        crate::runs::RunState::Suspended(_)
    ));
    let observed = durable_runs::wait(&state, &id)
        .await
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(observed["status"], "cancelled", "{observed}");
    let response = crate::handlers::runs::get(
        axum::extract::State(state.clone()),
        axum::extract::Path(id.to_string()),
    )
    .await;
    let value: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(value["status"], "cancelled", "{value}");
    stop(&state).await;
}

#[tokio::test]
async fn a_recovering_registry_entry_preserves_durable_get_metadata() {
    let root = tempfile::tempdir().unwrap();
    let state = app(root.path(), 10_000_000).await;
    let id = accept(&state, "session", super::CODE, Some("key")).await;
    let run = recovery::claim(
        state.units().as_ref(),
        &id,
        state.idempotency().generation(),
    )
    .await
    .unwrap()
    .unwrap();
    let (cancel, _) = oneshot::channel();
    let entry = state
        .session_manager()
        .runs()
        .insert_durable(&id, "session", cancel);
    let response = crate::handlers::runs::get(
        axum::extract::State(state.clone()),
        axum::extract::Path(id.to_string()),
    )
    .await;
    let value: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(value["status"], "running");
    assert_eq!(value["durable"], true);
    assert_eq!(value["attempt"], run.attempt());
    assert_eq!(value["revision"], run.revision());
    entry.unavailable();
    state.session_manager().runs().forget(&entry);
}

mod lifecycle;
