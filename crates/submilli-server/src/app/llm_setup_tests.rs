use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;
use crate::idempotency_store::EntryState;

const CODE: &str = "function main(): number { console.log(\"executed\"); return 42; }";

struct Harness {
    state: AppState,
    fail: Arc<AtomicBool>,
    attempts: Arc<AtomicUsize>,
    root: tempfile::TempDir,
    ledger: Arc<InMemoryIdempotencyStore>,
}

impl Harness {
    fn new(installed: Option<Arc<dyn ModelDispatch>>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let fail = Arc::new(AtomicBool::new(true));
        let attempts = Arc::new(AtomicUsize::new(0));
        let factory = {
            let fail = fail.clone();
            let attempts = attempts.clone();
            Arc::new(move |blueprint, secrets, policy| {
                attempts.fetch_add(1, Ordering::SeqCst);
                if fail.load(Ordering::SeqCst) {
                    let error = reqwest::Client::builder()
                        .user_agent("injected\ninvalid-header")
                        .build()
                        .err()
                        .unwrap();
                    return Err(HttpModelDispatchError::from(error));
                }
                HttpModelDispatch::new(blueprint, secrets, policy)
            }) as LlmDispatchFactory
        };
        let blueprints = Arc::new(
            InMemoryBlueprintStore::seed([Blueprint {
                name: "test".into(),
                ..Default::default()
            }])
            .unwrap(),
        );
        let ledger = Arc::new(InMemoryIdempotencyStore::default());
        let state = AppState::with_llm_dispatch_factory(
            ServerConfig {
                blueprints: Some(blueprints),
                session_storage_root: Some(root.path().join("sessions")),
                ephemeral_storage_root: Some(root.path().join("ephemeral")),
                llm_dispatch: installed,
                idempotency_store: Some(ledger.clone()),
                ..crate::config::test_config()
            },
            factory,
        )
        .unwrap();
        Self {
            state,
            fail,
            attempts,
            root,
            ledger,
        }
    }

    async fn post(
        &self,
        path: &str,
        body: Value,
        headers: &[(&str, &str)],
    ) -> (StatusCode, HeaderMap, Value) {
        let mut request = Request::builder()
            .method("POST")
            .uri(path)
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = app(self.state.clone())
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        let payload = text
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .next_back()
            .unwrap_or(&text)
            .trim();
        let body = if payload.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(payload).unwrap()
        };
        (status, headers, body)
    }

    fn assert_no_ephemeral_vfs(&self) {
        let path = self.root.path().join("ephemeral");
        assert!(!path.exists() || std::fs::read_dir(path).unwrap().next().is_none());
    }
}

#[tokio::test]
async fn rest_setup_failure_is_undispatched_and_same_key_can_execute_after_recovery() {
    let harness = Harness::new(None);
    let (status, _, body) = harness
        .post("/v1/sessions", json!({"blueprint": "test"}), &[])
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let session = body["session_id"].as_str().unwrap();
    let path = format!("/v1/sessions/{session}/execute");
    let headers = [("idempotency-key", "setup-retry")];
    let (status, _, body) = harness.post(&path, json!({"code": CODE}), &headers).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["error"]["kind"], "runtime_error");
    assert_eq!(
        body["error"]["message"],
        "LLM dispatch initialization failed"
    );
    assert!(body["result"].is_null());
    assert!(
        harness
            .state
            .sessions()
            .get(session)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        harness
            .ledger
            .load(session, "setup-retry")
            .await
            .unwrap()
            .is_none()
    );
    harness.assert_no_ephemeral_vfs();

    harness.fail.store(false, Ordering::SeqCst);
    let (_, _, healthy) = harness.post(&path, json!({"code": CODE}), &headers).await;
    assert_eq!(healthy["result"], "42", "{healthy}");
    let record = harness
        .state
        .sessions()
        .get(session)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.console.len(), 1);
    assert!(matches!(
        harness
            .ledger
            .load(session, "setup-retry")
            .await
            .unwrap()
            .unwrap()
            .state,
        EntryState::Completed(_)
    ));
    let (_, _, replay) = harness.post(&path, json!({"code": CODE}), &headers).await;
    assert_eq!(replay, healthy);
    assert_eq!(
        harness.attempts.load(Ordering::SeqCst),
        2,
        "replay must not build a provider"
    );
    harness.assert_no_ephemeral_vfs();
}

#[tokio::test]
async fn one_shot_setup_failure_has_no_last_run_or_vfs_and_recovers() {
    let harness = Harness::new(None);
    let (status, headers, body) = harness
        .post(
            "/v1/execute",
            json!({"blueprint": "test", "code": CODE}),
            &[],
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["error"]["message"],
        "LLM dispatch initialization failed"
    );
    let session = headers.get("mcp-session-id").unwrap().to_str().unwrap();
    assert!(
        harness
            .state
            .sessions()
            .get(session)
            .await
            .unwrap()
            .is_none()
    );
    harness.assert_no_ephemeral_vfs();
    harness.fail.store(false, Ordering::SeqCst);
    let (_, _, body) = harness
        .post(
            "/v1/execute",
            json!({"blueprint": "test", "code": CODE}),
            &[],
        )
        .await;
    assert_eq!(body["result"], "42", "{body}");
    harness.assert_no_ephemeral_vfs();
}

#[tokio::test]
async fn mcp_setup_failure_is_internal_before_vfs_and_allows_follow_up() {
    let harness = Harness::new(None);
    let initialize = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "setup-test", "version": "0"}
    }});
    let (status, headers, body) = harness.post("/mcp/test", initialize, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let session = headers.get("mcp-session-id").unwrap().to_str().unwrap();
    let headers = [("mcp-session-id", session)];
    let (status, _, _) = harness
        .post(
            "/mcp/test",
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            &headers,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let call = |id| {
        json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {
            "name": "submilli__typescript__execute", "arguments": {"code": CODE}
        }})
    };
    let (_, _, body) = harness.post("/mcp/test", call(2), &headers).await;
    assert_eq!(body["error"]["code"], -32603, "{body}");
    assert_eq!(
        body["error"]["message"],
        "LLM dispatch initialization failed"
    );
    assert!(
        harness
            .state
            .sessions()
            .get(session)
            .await
            .unwrap()
            .is_none()
    );
    harness.assert_no_ephemeral_vfs();
    harness.fail.store(false, Ordering::SeqCst);
    let (_, _, body) = harness.post("/mcp/test", call(3), &headers).await;
    assert!(body.get("error").is_none(), "{body}");
    assert_eq!(
        harness
            .state
            .sessions()
            .get(session)
            .await
            .unwrap()
            .unwrap()
            .console
            .len(),
        1
    );
    harness.assert_no_ephemeral_vfs();
}

#[tokio::test]
async fn installed_dispatch_skips_failing_factory() {
    let dispatch = Arc::new(
        HttpModelDispatch::new(
            Arc::new(Blueprint::default()),
            None,
            Arc::new(interpreter::runtime::NetworkPolicy::allow_all()),
        )
        .unwrap(),
    );
    let harness = Harness::new(Some(dispatch));
    let (_, _, body) = harness
        .post(
            "/v1/execute",
            json!({"blueprint": "test", "code": CODE}),
            &[],
        )
        .await;
    assert_eq!(body["result"], "42", "{body}");
    assert_eq!(harness.attempts.load(Ordering::SeqCst), 0);
}
