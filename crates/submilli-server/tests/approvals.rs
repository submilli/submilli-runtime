//! Approvals over the HTTP session API: a call a rule marks `ask-human` or
//! `ask-admin` suspends the run with an approval call, and the harness, or for
//! `ask-admin` an administrator, decides it.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::header::AUTHORIZATION;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::{ApiToken, AppState, AuthConfig, Role, ServerConfig, app};
use tower::ServiceExt;

const ADMIN: &str = "admin-token-0123456789abcdef0123456789";
const USER: &str = "user-token-0123456789abcdef01234567890";

const BLUEPRINT: &str = "\
name: refunds
default: deny
tool_call_timeout: 2s
admin_approval_timeout: 2s
permissions:
  main:
    - capability: test.refund
      name: large-refunds
      filter: amount > 100
      action: ask-human
    - capability: test.refund
      action: allow
    - capability: test.credit
      action: ask-admin
    - capability: tools.call
      action: allow
";

const REFUND: &str = r#"
import { check } from "submilli:security";
function main(): string {
    check("test.refund", { amount: 250 });
    return "refunded";
}
"#;

const REFUND_OR_REASON: &str = r#"
import { check } from "submilli:security";
function main(): string {
    try {
        check("test.refund", { amount: 250 });
        return "refunded";
    } catch (e) {
        if (e instanceof PermissionDeniedError) {
            return `denied: ${e.reason}`;
        }
        throw e;
    }
}
"#;

const CREDIT: &str = r#"
import { check } from "submilli:security";
function main(): string {
    check("test.credit", { amount: 5000 });
    return "credited";
}
"#;

fn state(auth: AuthConfig) -> AppState {
    let blueprint = submilli_blueprint::parse(BLUEPRINT).expect("valid blueprint");
    let config = ServerConfig {
        blueprints: Some(Arc::new(
            InMemoryBlueprintStore::seed([blueprint]).expect("seed blueprints"),
        )),
        auth,
        ..in_memory_config::config()
    };
    futures::executor::block_on(AppState::new(config)).expect("build AppState")
}

fn router() -> Router {
    app(state(AuthConfig::default()))
}

fn router_with_tokens() -> Router {
    let token = |name, role, token| ApiToken::new(name, role, token).expect("valid token");
    app(state(AuthConfig::Tokens(vec![
        token("ops", Role::Admin, ADMIN),
        token("app", Role::User, USER),
    ])))
}

async fn send(
    router: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        request = request.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = match body {
        Some(body) => request
            .header("content-type", "application/json")
            .body(Body::from(body.to_string())),
        None => request.body(Body::empty()),
    }
    .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn open(router: &Router, token: Option<&str>, body: Value) -> String {
    let (status, body) = send(router, "POST", "/v1/sessions", token, Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["session_id"].as_str().unwrap().to_owned()
}

async fn execute(router: &Router, token: Option<&str>, session: &str, code: &str) -> Value {
    let (status, body) = send(
        router,
        "POST",
        &format!("/v1/sessions/{session}/execute"),
        token,
        Some(json!({ "code": code })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

async fn resume(
    router: &Router,
    token: Option<&str>,
    run: &str,
    results: Value,
) -> (StatusCode, Value) {
    send(
        router,
        "POST",
        &format!("/v1/runs/{run}/resume"),
        token,
        Some(json!({ "results": results })),
    )
    .await
}

/// The one approval a suspended run waits on.
fn approval(body: &Value) -> (String, Value) {
    assert_eq!(body["status"], "suspended", "{body}");
    let calls = body["calls"].as_array().unwrap();
    assert_eq!(calls.len(), 1, "{body}");
    assert_eq!(calls[0]["kind"], "approval", "{body}");
    assert!(calls[0].get("tool").is_none(), "{body}");
    (
        calls[0]["call_id"].as_str().unwrap().to_owned(),
        calls[0]["approval"].clone(),
    )
}

fn run_id(body: &Value) -> String {
    body["execution_id"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn an_ask_human_call_suspends_and_an_approval_runs_it() {
    let router = router();
    let session = open(&router, None, json!({ "blueprint": "refunds" })).await;
    let suspended = execute(&router, None, &session, REFUND).await;
    let run = run_id(&suspended);
    let (call_id, approval) = approval(&suspended);
    assert!(call_id.starts_with(&format!("{run}:")), "{call_id}");
    assert_eq!(approval["approver"], "human");
    assert_eq!(approval["caller"], "main");
    assert_eq!(approval["capability"], "test.refund");
    assert_eq!(approval["context"], json!({ "amount": 250 }));
    assert_eq!(
        approval["rule"],
        json!({ "caller": "main", "index": 0, "name": "large-refunds", "filter": "amount > 100" })
    );
    let requested = approval["requested_at_unix_ms"].as_u64().unwrap();
    let expires = approval["expires_at_unix_ms"].as_u64().unwrap();
    assert_eq!(expires - requested, 2000);

    // The run waits where it is.
    let (_, current) = send(&router, "GET", &format!("/v1/runs/{run}"), None, None).await;
    assert_eq!(current, suspended);

    let (status, finished) = resume(
        &router,
        None,
        &run,
        json!([{ "call_id": call_id, "decision": "approve" }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{finished}");
    assert_eq!(finished["status"], "finished");
    assert_eq!(finished["result"], "refunded");

    // A repeated decision changes nothing.
    let (status, again) = resume(
        &router,
        None,
        &run,
        json!([{ "call_id": call_id, "decision": "deny" }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again, finished);
}

#[tokio::test]
async fn a_denial_throws_in_the_program_with_its_reason_and_source() {
    let router = router();
    let session = open(&router, None, json!({ "blueprint": "refunds" })).await;

    let suspended = execute(&router, None, &session, REFUND_OR_REASON).await;
    let (call_id, _) = approval(&suspended);
    let (_, finished) = resume(
        &router,
        None,
        &run_id(&suspended),
        json!([{ "call_id": call_id, "decision": "deny", "reason": "Not this customer" }]),
    )
    .await;
    assert_eq!(
        finished["result"],
        "denied: the person asked declined the call (ask-human): Not this customer"
    );

    // Uncaught, the run reports the approver as the denial's source.
    let suspended = execute(&router, None, &session, REFUND).await;
    let (call_id, _) = approval(&suspended);
    let (_, finished) = resume(
        &router,
        None,
        &run_id(&suspended),
        json!([{ "call_id": call_id, "decision": "deny" }]),
    )
    .await;
    assert_eq!(finished["error"]["kind"], "permission_denied", "{finished}");
    assert_eq!(finished["error"]["source"], "human", "{finished}");
}

#[tokio::test]
async fn a_call_under_the_threshold_runs_without_asking() {
    let router = router();
    let session = open(&router, None, json!({ "blueprint": "refunds" })).await;
    let code = r#"
import { check } from "submilli:security";
function main(): string { check("test.refund", { amount: 50 }); return "small"; }
"#;
    let finished = execute(&router, None, &session, code).await;
    assert_eq!(finished["status"], "finished", "{finished}");
    assert_eq!(finished["result"], "small");
}

#[tokio::test]
async fn an_approval_answered_like_a_tool_call_is_denied_by_policy() {
    let router = router();
    let session = open(&router, None, json!({ "blueprint": "refunds" })).await;
    let suspended = execute(&router, None, &session, REFUND).await;
    let (call_id, _) = approval(&suspended);
    let (status, finished) = resume(
        &router,
        None,
        &run_id(&suspended),
        json!([{ "call_id": call_id, "ok": true, "value": "yes" }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{finished}");
    assert_eq!(finished["error"]["kind"], "permission_denied", "{finished}");
    assert_eq!(finished["error"]["source"], "policy", "{finished}");
}

#[tokio::test]
async fn malformed_answers_are_refused_and_deliver_nothing() {
    let router = router();
    let session = open(&router, None, json!({ "blueprint": "refunds" })).await;
    let suspended = execute(&router, None, &session, REFUND).await;
    let run = run_id(&suspended);
    let (call_id, _) = approval(&suspended);
    for result in [
        json!({ "call_id": call_id, "ok": true, "decision": "approve" }),
        json!({ "call_id": call_id }),
        json!({ "call_id": call_id, "decision": "approve", "value": 1 }),
        json!({ "call_id": call_id, "decision": "maybe" }),
        json!({ "call_id": call_id, "decision": "deny", "reason": "x".repeat(1025) }),
        json!({ "call_id": call_id, "ok": false, "error": { "kind": "denied", "message": "no" }, "reason": "no" }),
    ] {
        let (status, body) = resume(&router, None, &run, json!([result])).await;
        assert!(
            matches!(
                status,
                StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
            ),
            "{status} {body}"
        );
    }
    // Still waiting on the same call.
    let (_, current) = send(&router, "GET", &format!("/v1/runs/{run}"), None, None).await;
    assert_eq!(current, suspended);
}

#[tokio::test]
async fn a_decision_cannot_answer_a_tool_call_and_tool_calls_name_their_kind() {
    let router = router();
    let session = open(
        &router,
        None,
        json!({
            "blueprint": "refunds",
            "tools": [{ "name": "lookup", "input_schema": { "type": "object" } }]
        }),
    )
    .await;
    let code = r#"
import tools from "@tools";
function main(): string { tools.lookup(); return "looked up"; }
"#;
    let suspended = execute(&router, None, &session, code).await;
    let call = &suspended["calls"][0];
    assert_eq!(call["kind"], "tool", "{suspended}");
    assert_eq!(call["tool"], "lookup");
    let call_id = call["call_id"].as_str().unwrap();
    let run = run_id(&suspended);
    let (status, body) = resume(
        &router,
        None,
        &run,
        json!([{ "call_id": call_id, "decision": "approve" }]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "invalid_request");
    // The tool answer, unchanged from before approvals, still finishes it.
    let (status, finished) = resume(
        &router,
        None,
        &run,
        json!([{ "call_id": call_id, "ok": true, "value": null }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{finished}");
    assert_eq!(finished["result"], "looked up");
}

#[tokio::test]
async fn an_unanswered_approval_expires_into_a_denial() {
    let router = router();
    let session = open(&router, None, json!({ "blueprint": "refunds" })).await;
    let suspended = execute(&router, None, &session, REFUND).await;
    let run = run_id(&suspended);
    let (call_id, _) = approval(&suspended);
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
    let (_, finished) = send(&router, "GET", &format!("/v1/runs/{run}"), None, None).await;
    assert_eq!(finished["status"], "finished", "{finished}");
    assert_eq!(finished["error"]["source"], "human", "{finished}");
    let (status, late) = resume(
        &router,
        None,
        &run,
        json!([{ "call_id": call_id, "decision": "approve" }]),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{late}");
    assert_eq!(late["error"], "run_not_suspended");
}

#[tokio::test]
async fn a_late_decision_is_refused_while_the_run_waits_on_another_call() {
    let router = router();
    let session = open(&router, None, json!({ "blueprint": "refunds" })).await;
    let code = r#"
import { check } from "submilli:security";
function main(): string {
    try { check("test.refund", { amount: 250 }); } catch (e) {}
    check("test.refund", { amount: 300 });
    return "refunded the second";
}
"#;
    let suspended = execute(&router, None, &session, code).await;
    let run = run_id(&suspended);
    let (first, _) = approval(&suspended);
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
    let (_, waiting) = send(&router, "GET", &format!("/v1/runs/{run}"), None, None).await;
    let (second, asked) = approval(&waiting);
    assert_ne!(second, first);
    assert_eq!(asked["context"], json!({ "amount": 300 }));

    let (status, late) = resume(
        &router,
        None,
        &run,
        json!([{ "call_id": first, "decision": "approve" }]),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{late}");
    assert_eq!(late["error"], "run_not_suspended");

    let (status, finished) = resume(
        &router,
        None,
        &run,
        json!([{ "call_id": second, "decision": "approve" }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{finished}");
    assert_eq!(finished["result"], "refunded the second");
}

#[tokio::test]
async fn only_an_admin_decides_an_ask_admin_call() {
    let router = router_with_tokens();
    let session = open(&router, Some(USER), json!({ "blueprint": "refunds" })).await;
    let suspended = execute(&router, Some(USER), &session, CREDIT).await;
    let run = run_id(&suspended);
    let (call_id, approval) = approval(&suspended);
    assert_eq!(approval["approver"], "admin");

    // The harness's token can neither approve nor deny it.
    for decision in ["approve", "deny"] {
        let (status, body) = resume(
            &router,
            Some(USER),
            &run,
            json!([{ "call_id": call_id, "decision": decision }]),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(body["error"], "approval_requires_admin");
    }
    let (status, _) = send(&router, "GET", "/v1/approvals", Some(USER), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // An administrator sees it and decides it.
    let (status, listed) = send(&router, "GET", "/v1/approvals", Some(ADMIN), None).await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    let approvals = listed["approvals"].as_array().unwrap();
    assert_eq!(approvals.len(), 1, "{listed}");
    assert_eq!(approvals[0]["call_id"], call_id.as_str());
    assert_eq!(approvals[0]["session_id"], session.as_str());
    assert_eq!(approvals[0]["approval"]["capability"], "test.credit");
    let (_, filtered) = send(
        &router,
        "GET",
        "/v1/approvals?approver=human",
        Some(ADMIN),
        None,
    )
    .await;
    assert_eq!(filtered["approvals"], json!([]));
    let (status, shown) = send(
        &router,
        "GET",
        &format!("/v1/approvals/{call_id}"),
        Some(ADMIN),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{shown}");
    assert_eq!(shown["execution_id"], run.as_str());

    let (status, finished) = send(
        &router,
        "POST",
        &format!("/v1/approvals/{call_id}"),
        Some(ADMIN),
        Some(json!({ "decision": "approve", "reason": "checked with finance" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{finished}");
    assert_eq!(finished["result"], "credited");
    let (status, _) = send(
        &router,
        "GET",
        &format!("/v1/approvals/{call_id}"),
        Some(ADMIN),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_admin_token_may_also_decide_through_resume() {
    let router = router_with_tokens();
    let session = open(&router, Some(USER), json!({ "blueprint": "refunds" })).await;
    let suspended = execute(&router, Some(USER), &session, CREDIT).await;
    let (call_id, _) = approval(&suspended);
    let (status, finished) = resume(
        &router,
        Some(ADMIN),
        &run_id(&suspended),
        json!([{ "call_id": call_id, "decision": "deny" }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{finished}");
    assert_eq!(finished["error"]["source"], "admin", "{finished}");
}

#[tokio::test]
async fn two_sessions_waiting_at_once_are_decided_independently() {
    let router = router();
    let first = open(&router, None, json!({ "blueprint": "refunds" })).await;
    let second = open(&router, None, json!({ "blueprint": "refunds" })).await;
    let one = execute(&router, None, &first, REFUND).await;
    let two = execute(&router, None, &second, REFUND).await;
    let (one_call, _) = approval(&one);
    let (two_call, _) = approval(&two);
    let (_, denied) = resume(
        &router,
        None,
        &run_id(&two),
        json!([{ "call_id": two_call, "decision": "deny" }]),
    )
    .await;
    assert_eq!(denied["error"]["source"], "human");
    let (_, approved) = resume(
        &router,
        None,
        &run_id(&one),
        json!([{ "call_id": one_call, "decision": "approve" }]),
    )
    .await;
    assert_eq!(approved["result"], "refunded");
}

#[tokio::test]
async fn closing_the_session_cancels_its_waiting_approval() {
    let router = router();
    let session = open(&router, None, json!({ "blueprint": "refunds" })).await;
    let suspended = execute(&router, None, &session, REFUND).await;
    let (call_id, _) = approval(&suspended);
    let (status, _) = send(
        &router,
        "DELETE",
        &format!("/v1/sessions/{session}"),
        None,
        None,
    )
    .await;
    assert!(status.is_success(), "{status}");
    let (status, _) = send(
        &router,
        "GET",
        &format!("/v1/approvals/{call_id}"),
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_one_shot_run_cannot_ask_and_is_denied_by_policy() {
    let router = router();
    let (status, body) = send(
        &router,
        "POST",
        "/v1/execute",
        None,
        Some(json!({ "blueprint": "refunds", "code": REFUND })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["error"]["kind"], "permission_denied", "{body}");
    assert_eq!(body["error"]["source"], "policy", "{body}");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("approval can't be asked here"),
        "{body}"
    );
}

#[tokio::test]
async fn an_asked_tool_call_reaches_the_harness_only_once_approved() {
    let blueprint = "\
name: gated-tools
default: deny
permissions:
  main:
    - capability: tools.call
      filter: tool == \"refund\"
      action: ask-human
";
    let config = ServerConfig {
        blueprints: Some(Arc::new(
            InMemoryBlueprintStore::seed([submilli_blueprint::parse(blueprint).unwrap()])
                .expect("seed blueprints"),
        )),
        ..in_memory_config::config()
    };
    let router = app(AppState::new(config).await.expect("build AppState"));
    let session = open(
        &router,
        None,
        json!({
            "blueprint": "gated-tools",
            "tools": [{ "name": "refund", "input_schema": { "type": "object" } }]
        }),
    )
    .await;
    let code = r#"
import tools from "@tools";
function main(): string { tools.refund(); return "refunded"; }
"#;
    let suspended = execute(&router, None, &session, code).await;
    let run = run_id(&suspended);
    let (call_id, approval) = approval(&suspended);
    assert_eq!(approval["capability"], "tools.call");
    assert_eq!(approval["context"], json!({ "tool": "refund" }));
    let (status, tool) = resume(
        &router,
        None,
        &run,
        json!([{ "call_id": call_id, "decision": "approve" }]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{tool}");
    assert_eq!(tool["calls"][0]["kind"], "tool", "{tool}");
    assert_eq!(tool["calls"][0]["tool"], "refund");
    let tool_call = tool["calls"][0]["call_id"].as_str().unwrap();
    let (_, finished) = resume(
        &router,
        None,
        &run,
        json!([{ "call_id": tool_call, "ok": true, "value": null }]),
    )
    .await;
    assert_eq!(finished["result"], "refunded", "{finished}");
}
