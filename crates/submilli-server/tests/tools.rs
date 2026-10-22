//! Application tools over the HTTP session API: a session declares `tools`, a
//! program calls them through `@tools`, each call suspends the run, and the
//! harness answers with `POST /v1/runs/{id}/resume`.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

const BLUEPRINT: &str = "\
name: orders
default: deny
tool_call_timeout: 2s
permissions:
  main:
    - capability: tools.call
      filter: tool == \"query_orders\" or tool == \"approve\"
      action: allow
";

fn tools() -> Value {
    json!([
        {
            "name": "query_orders",
            "description": "Orders of one customer.",
            "input_schema": {
                "type": "object",
                "properties": { "customer": { "type": "string" } },
                "required": ["customer"]
            },
            "output_schema": {
                "type": "object",
                "properties": { "count": { "type": "number" } },
                "required": ["count"]
            }
        },
        { "name": "approve", "input_schema": { "type": "object" } },
        { "name": "refund", "input_schema": { "type": "object" } }
    ])
}

fn state_with(blueprints: &[&str], configure: impl FnOnce(&mut ServerConfig)) -> AppState {
    let blueprints = blueprints
        .iter()
        .map(|yaml| submilli_blueprint::parse(yaml).expect("valid blueprint"));
    let mut config = ServerConfig {
        blueprints: Some(Arc::new(
            InMemoryBlueprintStore::seed(blueprints).expect("seed blueprints"),
        )),
        ..in_memory_config::config()
    };
    configure(&mut config);
    futures::executor::block_on(AppState::new(config)).expect("build AppState")
}

fn router() -> Router {
    app(state_with(&[BLUEPRINT], |_| {}))
}

async fn send(router: &Router, req: Request<Body>) -> (StatusCode, Value) {
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

async fn post(router: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    send(router, req).await
}

async fn get(router: &Router, uri: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    send(router, req).await
}

async fn open(router: &Router, blueprint: &str) -> String {
    let (status, body) = post(
        router,
        "/v1/sessions",
        json!({ "blueprint": blueprint, "tools": tools() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["session_id"].as_str().unwrap().to_owned()
}

async fn execute(router: &Router, session: &str, code: &str) -> (StatusCode, Value) {
    post(
        router,
        &format!("/v1/sessions/{session}/execute"),
        json!({ "code": code }),
    )
    .await
}

async fn resume(router: &Router, run: &str, results: Value) -> (StatusCode, Value) {
    post(
        router,
        &format!("/v1/runs/{run}/resume"),
        json!({ "results": results }),
    )
    .await
}

fn run_id(body: &Value) -> String {
    body["execution_id"].as_str().unwrap().to_owned()
}

fn only_call(body: &Value) -> (String, String, Value) {
    assert_eq!(body["status"], "suspended", "{body}");
    let calls = body["calls"].as_array().unwrap();
    assert_eq!(calls.len(), 1, "{body}");
    (
        calls[0]["call_id"].as_str().unwrap().to_owned(),
        calls[0]["tool"].as_str().unwrap().to_owned(),
        calls[0]["args"].clone(),
    )
}

const COUNT_ORDERS: &str = r#"
import tools from "@tools";
function main(): string {
    const orders = tools.query_orders({ customer: "c_42" });
    return `${orders.count} orders`;
}
"#;

#[tokio::test]
async fn a_tool_call_suspends_the_run_and_a_resume_finishes_it() {
    let router = router();
    let session = open(&router, "orders").await;
    let (status, suspended) = execute(&router, &session, COUNT_ORDERS).await;
    assert_eq!(status, StatusCode::OK);
    let run = run_id(&suspended);
    assert_eq!(suspended["session_id"], session.as_str());
    let (call_id, tool, args) = only_call(&suspended);
    assert_eq!(call_id, format!("{run}:1"));
    assert_eq!(tool, "query_orders");
    assert_eq!(args, json!({ "customer": "c_42" }));

    // The run waits where it is.
    let (_, current) = get(&router, &format!("/v1/runs/{run}")).await;
    assert_eq!(current, suspended);

    let (status, finished) = resume(
        &router,
        &run,
        json!([{ "call_id": call_id, "ok": true, "value": { "count": 3 } }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{finished}");
    assert_eq!(finished["status"], "finished");
    assert_eq!(finished["result"], "3 orders");
    assert_eq!(finished["error"], Value::Null);
    assert_eq!(finished["execution_id"], run.as_str());

    // GET, a repeated resume and the session's last run all agree.
    let (_, current) = get(&router, &format!("/v1/runs/{run}")).await;
    assert_eq!(current, finished);
    let (status, again) = resume(
        &router,
        &run,
        json!([{ "call_id": call_id, "ok": true, "value": { "count": 99 } }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again, finished);
    let (_, last) = get(&router, &format!("/v1/sessions/{session}/last-run")).await;
    assert_eq!(last["result"], "3 orders");
}

#[tokio::test]
async fn harness_failures_and_refusals_throw_in_the_program() {
    let router = router();
    let session = open(&router, "orders").await;
    let code = r#"
import tools from "@tools";
function main(): string {
    let seen = "";
    try { tools.approve(); } catch (e) { seen += e instanceof PermissionDeniedError ? "denied;" : "other;"; }
    try { tools.approve(); } catch (e) { seen += `${e.message};`; }
    try { tools.query_orders({ customer: "c" }); } catch (e) { seen += e instanceof TypeError ? "type;" : "other;"; }
    return seen;
}
"#;
    let (_, mut body) = execute(&router, &session, code).await;
    let run = run_id(&body);
    for answer in [
        json!({ "ok": false, "error": { "kind": "denied", "message": "a person said no" } }),
        json!({ "ok": false, "error": { "kind": "failed", "message": "database down" } }),
        json!({ "ok": true, "value": { "count": "three" } }),
    ] {
        let (call_id, _, _) = only_call(&body);
        let mut answer = answer;
        answer["call_id"] = json!(call_id);
        let (status, next) = resume(&router, &run, json!([answer])).await;
        assert_eq!(status, StatusCode::OK, "{next}");
        body = next;
    }
    assert_eq!(body["status"], "finished", "{body}");
    assert_eq!(body["result"], "denied;@tools.approve: database down;type;");
}

#[tokio::test]
async fn an_uncaught_harness_refusal_reports_source_harness() {
    let router = router();
    let session = open(&router, "orders").await;
    let (_, suspended) = execute(
        &router,
        &session,
        r#"import tools from "@tools"; function main(): void { tools.approve(); }"#,
    )
    .await;
    let (call_id, _, _) = only_call(&suspended);
    let (_, finished) = resume(
        &router,
        &run_id(&suspended),
        json!([{ "call_id": call_id, "ok": false, "error": { "kind": "denied", "message": "no" } }]),
    )
    .await;
    assert_eq!(finished["error"]["kind"], "permission_denied", "{finished}");
    assert_eq!(finished["error"]["source"], "harness");
    assert_eq!(finished["error"]["capability"], "tools.call");
}

#[tokio::test]
async fn a_tool_the_blueprint_does_not_allow_never_reaches_the_harness() {
    let router = router();
    let session = open(&router, "orders").await;
    let (status, finished) = execute(
        &router,
        &session,
        r#"import tools from "@tools"; function main(): void { tools.refund(); }"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(finished["status"], "finished", "{finished}");
    assert_eq!(finished["error"]["kind"], "permission_denied");
    assert_eq!(finished["error"]["source"], "policy");
}

#[tokio::test]
async fn an_unanswered_call_times_out_and_a_late_resume_is_refused() {
    let router = router();
    let session = open(&router, "orders").await;
    let (_, suspended) = execute(
        &router,
        &session,
        r#"import tools from "@tools";
           function main(): string {
               try { tools.approve(); return "answered"; }
               catch (e) { return e instanceof TimeoutError ? "timed out" : "other"; }
           }"#,
    )
    .await;
    let run = run_id(&suspended);
    let (call_id, _, _) = only_call(&suspended);
    // The blueprint's `tool_call_timeout` is 2s.
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let (_, finished) = get(&router, &format!("/v1/runs/{run}")).await;
    assert_eq!(finished["status"], "finished", "{finished}");
    assert_eq!(finished["result"], "timed out");
    let (status, refused) = resume(
        &router,
        &run,
        json!([{ "call_id": call_id, "ok": true, "value": null }]),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(refused["error"], "run_not_suspended");
}

#[tokio::test]
async fn a_resume_retried_after_the_run_finished_returns_the_result() {
    let router = router();
    let session = open(&router, "orders").await;
    let (_, suspended) = execute(&router, &session, COUNT_ORDERS).await;
    let run = run_id(&suspended);
    let (call_id, _, _) = only_call(&suspended);
    let answer = json!([{ "call_id": call_id, "ok": true, "value": { "count": 3 } }]);
    let (_, first) = resume(&router, &run, answer.clone()).await;
    assert_eq!(first["status"], "finished", "{first}");
    let (status, again) = resume(&router, &run, answer).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again, first);
}

#[tokio::test]
async fn after_one_call_times_out_the_run_waits_on_the_harness_no_more() {
    let router = router();
    let session = open(&router, "orders").await;
    let (_, suspended) = execute(
        &router,
        &session,
        r#"import tools from "@tools";
           function main(): string {
               let first = "answered";
               try { tools.approve(); } catch (e) { first = e instanceof TimeoutError ? "timed out" : "other"; }
               try { tools.approve(); return `${first}, then answered`; }
               catch (e) { return `${first}, then ${e instanceof TimeoutError ? "timed out" : "other"}`; }
           }"#,
    )
    .await;
    let run = run_id(&suspended);
    only_call(&suspended);
    // One `tool_call_timeout` (2s): the second call does not suspend.
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let (_, finished) = get(&router, &format!("/v1/runs/{run}")).await;
    assert_eq!(finished["status"], "finished", "{finished}");
    assert_eq!(finished["result"], "timed out, then timed out");
}

#[tokio::test]
async fn the_waiting_run_cap_is_server_wide_and_a_resume_frees_its_slot() {
    let router = app(state_with(&[BLUEPRINT], |config| {
        config.max_suspended_runs = Some(1);
    }));
    let first = open(&router, "orders").await;
    let (_, holding) = execute(&router, &first, COUNT_ORDERS).await;
    let (call_id, _, _) = only_call(&holding);
    let quota = r#"import tools from "@tools";
        function main(): string {
            try { tools.approve(); return "waited"; }
            catch (e) { return e instanceof QuotaExceededError ? "full" : "other"; }
        }"#;
    let second = open(&router, "orders").await;
    let (_, refused) = execute(&router, &second, quota).await;
    assert_eq!(refused["status"], "finished", "{refused}");
    assert_eq!(refused["result"], "full");
    let (_, done) = resume(
        &router,
        &run_id(&holding),
        json!([{ "call_id": call_id, "ok": true, "value": { "count": 1 } }]),
    )
    .await;
    assert_eq!(done["status"], "finished", "{done}");
    let (_, waiting) = execute(&router, &second, quota).await;
    assert_eq!(waiting["status"], "suspended", "{waiting}");
}

#[tokio::test]
async fn a_run_that_never_waits_is_answered_whole_and_not_kept() {
    let router = router();
    let session = open(&router, "orders").await;
    let (status, body) = execute(
        &router,
        &session,
        r#"function main(): string { return "plain"; }"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "finished", "{body}");
    assert_eq!(body["result"], "plain");
    let (status, _) = get(&router, &format!("/v1/runs/{}", run_id(&body))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn resume_refuses_an_unknown_run_or_call_and_a_malformed_answer() {
    let router = router();
    let (status, body) = resume(
        &router,
        "nope",
        json!([{ "call_id": "nope:1", "ok": true, "value": null }]),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "unknown_run");

    let session = open(&router, "orders").await;
    let (_, suspended) = execute(&router, &session, COUNT_ORDERS).await;
    let run = run_id(&suspended);
    let (status, body) = resume(
        &router,
        &run,
        json!([{ "call_id": "other:1", "ok": true, "value": 1 }]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "unknown_call");
    let (call_id, _, _) = only_call(&suspended);
    let (status, body) = resume(&router, &run, json!([{ "call_id": call_id, "ok": false }])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "invalid_request");
    for results in [
        json!([]),
        json!([
            { "call_id": call_id, "ok": true, "value": { "count": 1 } },
            { "call_id": call_id, "ok": true, "value": { "count": 2 } }
        ]),
    ] {
        let (status, body) = resume(&router, &run, results.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{results}");
        assert_eq!(body["error"], "invalid_request", "{results}");
    }
    // Nothing was delivered: the run still waits on the same call.
    let (_, current) = get(&router, &format!("/v1/runs/{run}")).await;
    assert_eq!(current, suspended);
}

#[tokio::test]
async fn a_repeated_execute_with_the_same_key_returns_the_waiting_run() {
    let router = router();
    let session = open(&router, "orders").await;
    let keyed = || {
        Request::builder()
            .method("POST")
            .uri(format!("/v1/sessions/{session}/execute"))
            .header("content-type", "application/json")
            .header("idempotency-key", "turn-1")
            .body(Body::from(json!({ "code": COUNT_ORDERS }).to_string()))
            .unwrap()
    };
    let (_, first) = send(&router, keyed()).await;
    let started = Instant::now();
    let (status, second) = send(&router, keyed()).await;
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(second, first);
    let (call_id, _, _) = only_call(&first);
    let (_, finished) = resume(
        &router,
        &run_id(&first),
        json!([{ "call_id": call_id, "ok": true, "value": { "count": 1 } }]),
    )
    .await;
    let (_, replayed) = send(&router, keyed()).await;
    assert_eq!(replayed, finished);
}

#[tokio::test]
async fn deleting_the_session_ends_its_waiting_run() {
    let router = router();
    let session = open(&router, "orders").await;
    let (_, suspended) = execute(&router, &session, COUNT_ORDERS).await;
    let run = run_id(&suspended);
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/v1/sessions/{session}"))
        .body(Body::empty())
        .unwrap();
    let (status, _) = send(&router, req).await;
    assert!(status.is_success(), "{status}");
    let (status, _) = get(&router, &format!("/v1/runs/{run}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn time_spent_waiting_does_not_count_against_the_execution_time_limit() {
    let router = app(state_with(&[BLUEPRINT], |config| {
        config.runtime.timeout = Some(Duration::from_secs(1));
    }));
    let session = open(&router, "orders").await;
    let (_, suspended) = execute(&router, &session, COUNT_ORDERS).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let (call_id, _, _) = only_call(&suspended);
    let (_, finished) = resume(
        &router,
        &run_id(&suspended),
        json!([{ "call_id": call_id, "ok": true, "value": { "count": 2 } }]),
    )
    .await;
    assert_eq!(finished["error"], Value::Null, "{finished}");
    assert_eq!(finished["result"], "2 orders");
}

#[tokio::test]
async fn a_session_without_tools_answers_as_it_always_has() {
    let router = router();
    let (_, created) = post(&router, "/v1/sessions", json!({ "blueprint": "orders" })).await;
    let session = created["session_id"].as_str().unwrap();
    let (status, body) = execute(
        &router,
        session,
        r#"function main(): string { return "plain"; }"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], "plain");
    assert_eq!(body["status"], "finished", "{body}");
    let (_, body) = execute(
        &router,
        session,
        r#"import tools from "@tools"; function main(): void { tools.approve(); }"#,
    )
    .await;
    assert_eq!(body["error"]["kind"], "compile_error", "{body}");
}

#[tokio::test]
async fn a_one_shot_execute_answers_finished() {
    let router = router();
    let (status, body) = post(
        &router,
        "/v1/execute",
        json!({ "blueprint": "orders", "code": r#"function main(): string { return "once"; }"# }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["result"], "once", "{body}");
    assert_eq!(body["status"], "finished", "{body}");
}

#[tokio::test]
async fn a_blueprint_that_grants_no_tool_hides_tools_and_says_so() {
    let router = app(state_with(&["name: plain\ndefault: deny\n"], |_| {}));
    let (status, created) = post(
        &router,
        "/v1/sessions",
        json!({ "blueprint": "plain", "tools": tools() }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let warnings = created["warnings"].as_array().unwrap();
    assert!(
        warnings
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("grants no `tools.call`")),
        "{created}"
    );
    let session = created["session_id"].as_str().unwrap();
    let (_, body) = execute(
        &router,
        session,
        r#"import tools from "@tools"; function main(): void { tools.approve(); }"#,
    )
    .await;
    assert_eq!(body["error"]["kind"], "compile_error", "{body}");
    assert_eq!(body["status"], "finished", "{body}");
}

#[tokio::test]
async fn a_tool_the_mapper_cannot_express_is_dropped_with_a_warning() {
    let router = router();
    let (_, created) = post(
        &router,
        "/v1/sessions",
        json!({ "blueprint": "orders", "tools": [{ "name": "bad-name", "input_schema": {} }] }),
    )
    .await;
    let warnings = created["warnings"].as_array().unwrap();
    assert!(
        warnings[0]
            .as_str()
            .unwrap()
            .starts_with("@tools: tool `bad-name` dropped"),
        "{created}"
    );
}

async fn get_text(router: &Router, uri: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
async fn session_discovery_adds_tools_to_the_blueprint_answers() {
    let router = router();
    let session = open(&router, "orders").await;
    let (status, found) = get(
        &router,
        &format!("/v1/sessions/{session}/packages/search?q=orders"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        found["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|hit| hit["name"] == "@tools" && hit["source"] == "tools"),
        "{found}"
    );
    let (status, docs) = get_text(
        &router,
        &format!("/v1/sessions/{session}/packages/docs?name=%40tools"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(docs.contains("query_orders"), "{docs}");
    assert!(docs.contains("Orders of one customer."), "{docs}");

    // The blueprint routes know nothing of a session's tools.
    let (_, blueprint) = get(&router, "/v1/blueprints/orders/packages/search?q=orders").await;
    assert!(!blueprint.to_string().contains("@tools"), "{blueprint}");

    // A session without tools answers exactly as its blueprint does.
    let (_, plain) = post(&router, "/v1/sessions", json!({ "blueprint": "orders" })).await;
    let plain = plain["session_id"].as_str().unwrap();
    for query in ["orders", "", "fs"] {
        let (_, session_search) = get(
            &router,
            &format!("/v1/sessions/{plain}/packages/search?q={query}"),
        )
        .await;
        let (_, blueprint_search) = get(
            &router,
            &format!("/v1/blueprints/orders/packages/search?q={query}"),
        )
        .await;
        assert_eq!(session_search, blueprint_search, "{query}");
    }
    let (status, _) = get_text(
        &router,
        &format!("/v1/sessions/{plain}/packages/docs?name=%40tools"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = get(&router, "/v1/sessions/nope/packages/search?q=x").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_session_keeps_its_tools_across_a_restart() {
    let store = tempfile::tempdir().unwrap();
    let durable = |config: &mut ServerConfig| {
        config.database_path = Some(store.path().join("server.db"));
    };
    let session = {
        let state = state_with(&[BLUEPRINT], durable);
        let session = open(&app(state.clone()), "orders").await;
        state.database().expect("database").close().await.unwrap();
        session
    };
    let router = app(state_with(&[BLUEPRINT], durable));
    let (_, suspended) = execute(&router, &session, COUNT_ORDERS).await;
    let (_, tool, _) = only_call(&suspended);
    assert_eq!(tool, "query_orders");
}
