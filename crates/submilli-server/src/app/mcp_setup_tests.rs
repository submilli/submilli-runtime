use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;
use crate::domain::idempotent_request::RequestState as EntryState;
use crate::request_records::RequestRecords;

const CODE: &str = "function main(): number { console.log(\"executed\"); return 42; }";

struct Harness {
    state: AppState,
    fail: Arc<AtomicBool>,
    attempts: Arc<AtomicUsize>,
    root: tempfile::TempDir,
    ledger: Arc<RequestRecords>,
}

impl Harness {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let fail = Arc::new(AtomicBool::new(false));
        let attempts = Arc::new(AtomicUsize::new(0));
        let setup = {
            let fail = fail.clone();
            let attempts = attempts.clone();
            Arc::new(move || {
                attempts.fetch_add(1, Ordering::SeqCst);
                if fail.load(Ordering::SeqCst) {
                    Err(DiscoveryError::Internal {
                        message: "injected MCP setup failure",
                    })
                } else {
                    Ok(())
                }
            }) as McpSetup
        };
        let blueprints = Arc::new(
            InMemoryBlueprintStore::seed([Blueprint {
                name: "test".into(),
                ..Default::default()
            }])
            .unwrap(),
        );
        let ledger = Arc::new(RequestRecords::ephemeral());
        let mut state = AppState::with_llm_dispatch_factory(
            ServerConfig {
                blueprints: Some(blueprints),
                session_storage_root: Some(root.path().join("sessions")),
                ephemeral_storage_root: Some(root.path().join("ephemeral")),
                database: Some(ledger.database.clone()),
                ..ServerConfig::default()
            },
            Arc::new(HttpModelDispatch::new),
        )
        .await
        .unwrap();
        Arc::get_mut(&mut state.inner).unwrap().mcp_setup = setup;
        fail.store(true, Ordering::SeqCst);
        attempts.store(0, Ordering::SeqCst);
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
    let harness = Harness::new().await;
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
        "MCP discovery initialization failed"
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
        "replay must not repeat discovery"
    );
    harness.assert_no_ephemeral_vfs();
}

#[tokio::test]
async fn one_shot_setup_failure_has_no_last_run_or_vfs_and_recovers() {
    let harness = Harness::new().await;
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
        "MCP discovery initialization failed"
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
    let harness = Harness::new().await;
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
        "MCP discovery initialization failed"
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
    for (name, arguments) in [
        (
            "submilli__typescript__packages__search",
            json!({"query": ""}),
        ),
        (
            "submilli__typescript__packages__docs",
            json!({"name": "@mcp/test"}),
        ),
        (
            "submilli__typescript__builtins__docs",
            json!({"names": ["console"]}),
        ),
    ] {
        let (_, _, body) = harness
            .post(
                "/mcp/test",
                json!({
                    "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                    "params": {"name": name, "arguments": arguments}
                }),
                &headers,
            )
            .await;
        assert_eq!(body["error"]["code"], -32603, "{name}: {body}");
        assert_eq!(
            body["error"]["message"],
            "MCP discovery initialization failed"
        );
    }
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
async fn failed_catalogs_are_not_cached_and_all_lookup_routes_fail_closed() {
    let harness = Harness::new().await;
    let blueprint = harness
        .state
        .blueprints()
        .get("test")
        .await
        .unwrap()
        .unwrap();
    assert!(harness.state.mcp_catalog("test", &blueprint).await.is_err());
    assert!(
        harness
            .state
            .cached_mcp_catalog(&mcp_catalog_cache_key("test", None))
            .is_none()
    );
    for path in [
        "/v1/blueprints/test/packages/search?q=",
        "/v1/blueprints/test/packages/docs?name=@mcp/test",
        "/v1/blueprints/test/builtins",
        "/v1/blueprints/test/builtins/docs?name=console",
    ] {
        let response = app(harness.state.clone())
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "{path}"
        );
        let body: Value =
            serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                .unwrap();
        assert_eq!(body["error"], "internal_error");
        assert_eq!(body["message"], "MCP discovery initialization failed");
    }
    harness.fail.store(false, Ordering::SeqCst);
    assert!(harness.state.mcp_catalog("test", &blueprint).await.is_ok());
}
