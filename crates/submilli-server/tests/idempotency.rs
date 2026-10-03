//! End-to-end coverage of `Idempotency-Key` on `POST /v1/sessions/{id}/execute`.
//!
//! Every program here calls a mock HTTP server, so the mock's hit count is a
//! direct count of executions: "the program ran once" is observed, not inferred
//! from the response. The mock can also hold a request open, which is what lets
//! the concurrency test prove a duplicate arrived *during* an execution rather
//! than after it — without that, the duplicate takes the replay branch and the
//! wait path is never exercised.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::idempotency_store::{
    FileIdempotencyStore, IdempotencyStore, InMemoryIdempotencyStore, LedgerEntry, code_fingerprint,
};
use submilli_server::session_store::{DurableSessionStore, InMemoryDurableSessionStore};
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

const BLUEPRINT: &str = "idempotency";

const POLICY: &str = "\
name: idempotency
allow_insecure_http: true
default: deny
vfs: none
permissions:
  main:
    - capability: http.get
      action: allow
";

/// A mock the guest calls once per execution. `hits` counts executions; when
/// gated, each request parks until the test releases it, giving a hold point
/// the test controls.
struct Mock {
    port: u16,
    hits: Arc<AtomicUsize>,
    arrived: Receiver<()>,
    release: Sender<()>,
}

impl Mock {
    /// Block until a request reaches the mock — i.e. a program is mid-execution.
    fn wait_for_arrival(&self) {
        self.arrived
            .recv_timeout(Duration::from_secs(10))
            .expect("a program should have reached the mock");
    }

    fn release_one(&self) {
        self.release.send(()).expect("mock still listening");
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

fn spawn_mock(gated: bool) -> Mock {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let (arrived_tx, arrived) = channel();
    let (release, release_rx) = channel::<()>();
    let release_rx = Arc::new(Mutex::new(release_rx));

    let counter = Arc::clone(&hits);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            {
                let mut reader = BufReader::new(&stream);
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    if line.trim_end().is_empty() {
                        break; // end of headers
                    }
                }
            }
            counter.fetch_add(1, Ordering::SeqCst);
            let _ = arrived_tx.send(());
            if gated {
                // Park here until the test says the execution may finish.
                let _ = release_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(30));
            }
            let body = "hit";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });

    Mock {
        port,
        hits,
        arrived,
        release,
    }
}

fn calls_mock(port: u16) -> String {
    format!(
        r#"import {{ get, Response }} from "submilli:http";
function main(): string {{
  const r: Response = get("http://127.0.0.1:{port}/hit");
  return r.body;
}}"#
    )
}

fn router_with_ledger() -> (Router, Arc<dyn IdempotencyStore>) {
    let ledger: Arc<dyn IdempotencyStore> = Arc::new(InMemoryIdempotencyStore::default());
    let blueprint = submilli_blueprint::parse(POLICY).expect("valid blueprint");
    let state = AppState::new(ServerConfig {
        blueprints: Some(Arc::new(
            InMemoryBlueprintStore::seed([blueprint]).expect("seed blueprints"),
        )),
        idempotency_store: Some(Arc::clone(&ledger)),
        ..ServerConfig::default()
    })
    .expect("build AppState");
    (app(state), ledger)
}

async fn send(router: &Router, req: Request<Body>) -> (StatusCode, String) {
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

fn as_json(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or(Value::Null)
}

async fn open_session(router: &Router) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/sessions")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "blueprint": BLUEPRINT }).to_string()))
        .unwrap();
    let (status, body) = send(router, req).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    as_json(&body)["session_id"].as_str().unwrap().to_string()
}

fn execute_request(session: &str, code: &str, key: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(format!("/v1/sessions/{session}/execute"))
        .header("content-type", "application/json");
    if let Some(key) = key {
        builder = builder.header("Idempotency-Key", key);
    }
    builder
        .body(Body::from(json!({ "code": code }).to_string()))
        .unwrap()
}

async fn execute(
    router: &Router,
    session: &str,
    code: &str,
    key: Option<&str>,
) -> (StatusCode, String) {
    send(router, execute_request(session, code, key)).await
}

// --- unkeyed behaviour is unchanged -----------------------------------------

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn without_a_key_every_request_executes() {
    let mock = spawn_mock(false);
    let (router, _ledger) = router_with_ledger();
    let session = open_session(&router).await;
    let code = calls_mock(mock.port);

    let (first, _) = execute(&router, &session, &code, None).await;
    let (second, _) = execute(&router, &session, &code, None).await;

    assert_eq!(first, StatusCode::OK);
    assert_eq!(second, StatusCode::OK);
    assert_eq!(mock.hits(), 2, "an unkeyed request must always run");
}

// --- replay -----------------------------------------------------------------

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn a_sequential_duplicate_runs_once_and_replays_byte_for_byte() {
    let mock = spawn_mock(false);
    let (router, _ledger) = router_with_ledger();
    let session = open_session(&router).await;
    let code = calls_mock(mock.port);

    let (first_status, first_body) = execute(&router, &session, &code, Some("k1")).await;
    let (second_status, second_body) = execute(&router, &session, &code, Some("k1")).await;

    assert_eq!(mock.hits(), 1, "the program must run exactly once");
    assert_eq!(first_status, second_status);
    assert_eq!(second_body, first_body, "a replay must be byte-identical");
    assert_eq!(as_json(&first_body)["result"], json!("hit"));
}

/// R5, the wait path. The mock holds the first execution open, so the duplicate
/// provably arrives while it is still running — firing two requests and hoping
/// they overlap would prove R4 (replay) instead.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_concurrent_duplicate_waits_for_the_original() {
    let mock = spawn_mock(true);
    let (router, _ledger) = router_with_ledger();
    let session = open_session(&router).await;
    let code = calls_mock(mock.port);

    let first = {
        let (router, session, code) = (router.clone(), session.clone(), code.clone());
        tokio::spawn(async move { execute(&router, &session, &code, Some("k1")).await })
    };
    // The first program is now inside the mock and cannot finish yet.
    mock.wait_for_arrival();

    let second = {
        let (router, session, code) = (router.clone(), session.clone(), code.clone());
        tokio::spawn(async move { execute(&router, &session, &code, Some("k1")).await })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !second.is_finished(),
        "the duplicate must be waiting on the in-flight execution, not answered"
    );

    mock.release_one();
    let (first_status, first_body) = first.await.unwrap();
    let (second_status, second_body) = second.await.unwrap();

    assert_eq!(mock.hits(), 1, "only one execution may reach the mock");
    assert_eq!(first_status, second_status);
    assert_eq!(second_body, first_body);
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn a_failing_program_replays_its_error_body_without_re_running() {
    let mock = spawn_mock(false);
    let (router, _ledger) = router_with_ledger();
    let session = open_session(&router).await;
    // Hits the mock, *then* fails: the mock is the execution counter, so byte
    // equality alone would hold even if the program ran twice. `readText` is
    // denied by this blueprint, which traps at runtime after the call lands.
    let code = format!(
        r#"import {{ get, Response }} from "submilli:http";
import {{ readText }} from "submilli:fs";
function main(): string {{
  const r: Response = get("http://127.0.0.1:{}/hit");
  return readText("/denied.txt") ?? r.body;
}}"#,
        mock.port
    );

    let (first_status, first_body) = execute(&router, &session, &code, Some("k1")).await;
    let (_, second_body) = execute(&router, &session, &code, Some("k1")).await;

    assert_eq!(first_status, StatusCode::OK);
    assert!(
        !as_json(&first_body)["error"].is_null(),
        "expected a failure body: {first_body}"
    );
    assert_eq!(second_body, first_body, "a failure replays verbatim too");
    assert_eq!(
        mock.hits(),
        1,
        "a replayed failure must not re-run the program"
    );
}

// --- conflict ---------------------------------------------------------------

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn the_same_key_with_different_code_is_refused_without_executing() {
    let mock = spawn_mock(false);
    let (router, _ledger) = router_with_ledger();
    let session = open_session(&router).await;
    let code = calls_mock(mock.port);

    execute(&router, &session, &code, Some("k1")).await;
    let (status, body) = execute(
        &router,
        &session,
        &format!("{code}\n// a different program"),
        Some("k1"),
    )
    .await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(as_json(&body)["error"], json!("idempotency_conflict"));
    assert_eq!(mock.hits(), 1, "a conflict must not execute");
}

// --- scoping ----------------------------------------------------------------

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn the_same_key_in_two_sessions_executes_in_each() {
    let mock = spawn_mock(false);
    let (router, _ledger) = router_with_ledger();
    let first_session = open_session(&router).await;
    let second_session = open_session(&router).await;
    let code = calls_mock(mock.port);

    execute(&router, &first_session, &code, Some("k1")).await;
    execute(&router, &second_session, &code, Some("k1")).await;

    assert_eq!(mock.hits(), 2, "keys are scoped to their session");
}

// --- session-level refusals take precedence (KTD2) --------------------------

#[tokio::test]
async fn a_key_on_an_unknown_session_reports_the_unknown_session() {
    let (router, ledger) = router_with_ledger();

    let (status, body) = execute(
        &router,
        "no-such-session",
        "function main(): void {}",
        Some("k1"),
    )
    .await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(as_json(&body)["error"], json!("unknown session"));
    assert!(
        ledger.session_ids().await.is_empty(),
        "an unknown session must leave no ledger entry"
    );
}

/// KTD2: a session-level refusal answers before the key is ever considered.
///
/// The restart is what makes this test mean what it says. Harness secrets are
/// memory-only, so a rehydrated session is *known* (no 404) but *unbound* —
/// the one state that reaches the `session_requires_secrets` branch. Share no
/// session store and the second process 404s instead, and the test passes on a
/// path the previous test already covers.
#[tokio::test]
async fn an_unbound_harness_secret_is_reported_before_the_key_is_considered() {
    const NEEDS_SECRET: &str = "name: needs-secret\ndefault: deny\nvfs: none\nsecrets:\n  TOKEN:\n    harness:\n      required: true\n";

    let ledger: Arc<dyn IdempotencyStore> = Arc::new(InMemoryIdempotencyStore::default());
    let sessions: Arc<dyn DurableSessionStore> = Arc::new(InMemoryDurableSessionStore::default());
    let build = || {
        AppState::new(ServerConfig {
            blueprints: Some(Arc::new(
                InMemoryBlueprintStore::seed([
                    submilli_blueprint::parse(NEEDS_SECRET).expect("valid blueprint")
                ])
                .expect("seed blueprints"),
            )),
            idempotency_store: Some(Arc::clone(&ledger)),
            session_store: Some(Arc::clone(&sessions)),
            ..ServerConfig::default()
        })
        .expect("build AppState")
    };

    let router = app(build());
    let req = Request::builder()
        .method("POST")
        .uri("/v1/sessions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "blueprint": "needs-secret", "secrets": { "TOKEN": "t" } }).to_string(),
        ))
        .unwrap();
    let (status, body) = send(&router, req).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let session = as_json(&body)["session_id"].as_str().unwrap().to_string();

    // Restart: the durable record comes back, the memory-only binding does not.
    let restarted_state = build();
    restarted_state.boot().await;
    let restarted = app(restarted_state);

    let (status, body) =
        execute(&restarted, &session, "function main(): void {}", Some("k1")).await;

    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "the session is known but unbound, so the secrets refusal must win: {body}"
    );
    assert_eq!(as_json(&body)["error"], json!("session_requires_secrets"));
    assert!(
        ledger.session_ids().await.is_empty(),
        "a session-level refusal must not touch the ledger"
    );
}

// --- key validation (R13) ---------------------------------------------------

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn an_empty_key_is_rejected_and_creates_no_entry() {
    let mock = spawn_mock(false);
    let (router, ledger) = router_with_ledger();
    let session = open_session(&router).await;

    let (status, body) = execute(&router, &session, &calls_mock(mock.port), Some("")).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(as_json(&body)["error"], json!("idempotency_key_invalid"));
    assert_eq!(mock.hits(), 0, "an invalid key must not execute");
    assert!(ledger.session_ids().await.is_empty());
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn an_over_long_key_is_rejected() {
    let mock = spawn_mock(false);
    let (router, _ledger) = router_with_ledger();
    let session = open_session(&router).await;
    let too_long = "k".repeat(121);

    let (status, body) = execute(
        &router,
        &session,
        &calls_mock(mock.port),
        Some(too_long.as_str()),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(as_json(&body)["error"], json!("idempotency_key_invalid"));
    assert_eq!(mock.hits(), 0);
}

// --- nothing dispatched (R8) ------------------------------------------------

#[tokio::test]
async fn a_pre_dispatch_failure_leaves_no_entry_and_a_retry_runs_again() {
    let ledger: Arc<dyn IdempotencyStore> = Arc::new(InMemoryIdempotencyStore::default());
    // The blueprint lists a package that is not installed, so import resolution
    // fails before anything reaches the runner.
    let blueprint = submilli_blueprint::parse(
        "name: missing-pkg\ndefault: deny\nvfs: none\npackages:\n  - \"@nope/missing\"\n",
    )
    .expect("valid blueprint");
    let state = AppState::new(ServerConfig {
        blueprints: Some(Arc::new(
            InMemoryBlueprintStore::seed([blueprint]).expect("seed blueprints"),
        )),
        idempotency_store: Some(Arc::clone(&ledger)),
        package_store_root: Some(tempfile::tempdir().unwrap().keep()),
        ..ServerConfig::default()
    })
    .expect("build AppState");
    let router = app(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/sessions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "blueprint": "missing-pkg" }).to_string(),
        ))
        .unwrap();
    let (_, body) = send(&router, req).await;
    let session = as_json(&body)["session_id"].as_str().unwrap().to_string();

    let code = r#"import { thing } from "@nope/missing";
function main(): string { return thing(); }"#;
    let (status, first) = execute(&router, &session, code, Some("k1")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(!as_json(&first)["error"].is_null(), "{first}");
    assert_eq!(
        ledger.load(&session, "k1").await.unwrap(),
        None,
        "nothing was dispatched, so nothing may be recorded"
    );

    // The retry must run again rather than replay a cached infrastructure error.
    let (_, second) = execute(&router, &session, code, Some("k1")).await;
    assert!(!as_json(&second)["error"].is_null());
    assert_eq!(ledger.load(&session, "k1").await.unwrap(), None);
}

// --- last-run is not rewritten by a replay ----------------------------------

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn a_replay_leaves_last_run_reporting_the_most_recent_execution() {
    let mock = spawn_mock(false);
    let (router, _ledger) = router_with_ledger();
    let session = open_session(&router).await;

    execute(&router, &session, &calls_mock(mock.port), Some("k1")).await;
    // A later, unkeyed run is now the most recent execution.
    execute(
        &router,
        &session,
        r#"function main(): string { return "second"; }"#,
        None,
    )
    .await;
    execute(&router, &session, &calls_mock(mock.port), Some("k1")).await;

    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/sessions/{session}/last-run"))
        .body(Body::empty())
        .unwrap();
    let (status, body) = send(&router, req).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        as_json(&body)["result"],
        json!("second"),
        "a replay must not rewrite last-run: {body}"
    );
}

// --- the durable store, end to end ------------------------------------------

/// Every other test here injects the in-memory ledger, so the fsync/rename
/// layer the whole design rests on would otherwise never run under the HTTP
/// surface. This drives the real `FileIdempotencyStore` through the endpoint
/// and reads the resulting tree back off disk.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn the_file_backed_ledger_records_and_replays_through_the_endpoint() {
    let mock = spawn_mock(false);
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("ledger");
    let ledger: Arc<dyn IdempotencyStore> =
        Arc::new(FileIdempotencyStore::new(root.clone()).expect("file ledger"));
    let blueprint = submilli_blueprint::parse(POLICY).expect("valid blueprint");
    let router = app(AppState::new(ServerConfig {
        blueprints: Some(Arc::new(
            InMemoryBlueprintStore::seed([blueprint]).expect("seed blueprints"),
        )),
        idempotency_store: Some(Arc::clone(&ledger)),
        ..ServerConfig::default()
    })
    .expect("build AppState"));

    let session = open_session(&router).await;
    let code = calls_mock(mock.port);
    let (_, first_body) = execute(&router, &session, &code, Some("k1")).await;

    // One directory for the session, one file for the key, recorded as completed.
    let entries: Vec<_> = std::fs::read_dir(
        root.join(
            std::fs::read_dir(&root)
                .expect("store root")
                .next()
                .expect("a session directory")
                .expect("readable")
                .file_name(),
        ),
    )
    .expect("session dir")
    .filter_map(Result::ok)
    .map(|e| e.path())
    .collect();
    assert_eq!(entries.len(), 1, "one entry file per key: {entries:?}");
    let on_disk = std::fs::read_to_string(&entries[0]).expect("entry readable");
    assert!(
        on_disk.contains("\"state\": \"completed\""),
        "the outcome must be durable, got: {on_disk}"
    );

    let (_, second_body) = execute(&router, &session, &code, Some("k1")).await;
    assert_eq!(mock.hits(), 1, "the replay must come off disk, not re-run");
    assert_eq!(second_body, first_body);
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn a_replay_carries_the_same_session_header_and_content_type() {
    let mock = spawn_mock(false);
    let (router, _ledger) = router_with_ledger();
    let session = open_session(&router).await;
    let code = calls_mock(mock.port);

    let first = router
        .clone()
        .oneshot(execute_request(&session, &code, Some("k1")))
        .await
        .unwrap();
    let first_headers = first.headers().clone();
    let _ = first.into_body().collect().await.unwrap();

    let second = router
        .clone()
        .oneshot(execute_request(&session, &code, Some("k1")))
        .await
        .unwrap();

    // The replay bypasses `Json`, so its headers are hand-built and could drift
    // from what the original caller received.
    for header in ["mcp-session-id", "content-type"] {
        assert_eq!(
            second.headers().get(header),
            first_headers.get(header),
            "a replay must carry the same `{header}` as the original"
        );
    }
    assert_eq!(
        second.headers().get("mcp-session-id").unwrap(),
        session.as_str()
    );
    assert_eq!(
        second.headers().get("content-type").unwrap(),
        "application/json"
    );
}

// --- restart recovery (R7) --------------------------------------------------

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn a_reservation_that_survived_a_restart_is_refused_as_indeterminate() {
    let mock = spawn_mock(false);
    let (router, ledger) = router_with_ledger();
    let session = open_session(&router).await;
    let code = calls_mock(mock.port);

    // The shape a crash between reservation and outcome leaves behind: an entry
    // still in the `reserved` state with nothing live holding it.
    ledger
        .put(LedgerEntry::reserved(
            &session,
            "k1",
            code_fingerprint(&code),
        ))
        .await
        .unwrap();

    let (status, body) = execute(&router, &session, &code, Some("k1")).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(as_json(&body)["error"], json!("idempotency_incomplete"));
    assert_eq!(mock.hits(), 0, "an indeterminate key must never re-execute");
}

#[tokio::test]
async fn a_fingerprint_mismatch_against_an_indeterminate_entry_is_a_conflict() {
    let (router, ledger) = router_with_ledger();
    let session = open_session(&router).await;
    ledger
        .put(LedgerEntry::reserved(
            &session,
            "k1",
            code_fingerprint("some other program"),
        ))
        .await
        .unwrap();

    let (status, body) = execute(&router, &session, "function main(): void {}", Some("k1")).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(as_json(&body)["error"], json!("idempotency_conflict"));
}
