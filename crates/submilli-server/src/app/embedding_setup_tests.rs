//! Embedding client construction failures are audited like model ones: the
//! execution fails with `runtime_error`, and the audit record carries that
//! error class under the execution id the caller was given.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;

const CODE: &str = "function main(): number { return 42; }";
const MESSAGE: &str = "embedding dispatch initialization failed";

struct Harness {
    state: AppState,
    audit_path: std::path::PathBuf,
    _root: tempfile::TempDir,
}

impl Harness {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let audit_path = root.path().join("audit.log");
        let factory = Arc::new(|_, _, _| {
            let error = reqwest::Client::builder()
                .user_agent("injected\ninvalid-header")
                .build()
                .err()
                .unwrap();
            Err(HttpEmbeddingDispatchError::from(error))
        }) as EmbeddingDispatchFactory;
        let blueprint = submilli_blueprint::parse(
            "name: test\n\
             embedding:\n  providers:\n    hf:\n      type: huggingface\n      base_url: https://hf.example.com\n  models:\n    docs:\n      provider: hf\n      model: bge\n      dimensions: 4\n",
        )
        .unwrap();
        let state = AppState::with_embedding_dispatch_factory(
            ServerConfig {
                blueprints: Some(Arc::new(InMemoryBlueprintStore::seed([blueprint]).unwrap())),
                session_storage_root: Some(root.path().join("sessions")),
                ephemeral_storage_root: Some(root.path().join("ephemeral")),
                audit: crate::audit::AuditConfig {
                    file: Some(audit_path.clone()),
                    ..Default::default()
                },
                ..crate::config::test_config()
            },
            factory,
        )
        .unwrap();
        Self {
            state,
            audit_path,
            _root: root,
        }
    }

    async fn post(
        &self,
        path: &str,
        body: Value,
        headers: &[(&str, &str)],
    ) -> (StatusCode, axum::http::HeaderMap, Value) {
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

    /// The audit line that finished the execution `id`.
    fn finished_record(&self, id: &str) -> String {
        let log = std::fs::read_to_string(&self.audit_path).unwrap();
        let lines: Vec<_> = log
            .lines()
            .filter(|line| {
                line.contains("type=execution")
                    && line.contains("event=finished")
                    && line.contains(&format!("execution_id={id}"))
            })
            .collect();
        assert_eq!(lines.len(), 1, "one finished record for {id}: {log}");
        lines[0].to_owned()
    }

    fn assert_audited(&self, id: &str) {
        uuid::Uuid::parse_str(id).unwrap();
        let record = self.finished_record(id);
        assert!(record.contains("outcome=error"), "{record}");
        assert!(record.contains("error_class=runtime_error"), "{record}");
    }
}

#[tokio::test]
async fn rest_setup_failure_is_a_runtime_error_audited_under_the_response_id() {
    let harness = Harness::new();
    let (status, _, body) = harness
        .post(
            "/v1/execute",
            json!({"blueprint": "test", "code": CODE}),
            &[],
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["error"]["kind"], "runtime_error", "{body}");
    assert_eq!(body["error"]["message"], MESSAGE);
    assert!(body["result"].is_null());
    harness.assert_audited(body["execution_id"].as_str().unwrap());
}

#[tokio::test]
async fn mcp_setup_failure_is_internal_and_audited_under_the_error_id() {
    let harness = Harness::new();
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
    let (_, _, body) = harness
        .post(
            "/mcp/test",
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
                "name": "submilli__typescript__execute", "arguments": {"code": CODE}
            }}),
            &headers,
        )
        .await;
    assert_eq!(body["error"]["code"], -32603, "{body}");
    assert_eq!(body["error"]["message"], MESSAGE);
    harness.assert_audited(body["error"]["data"]["execution_id"].as_str().unwrap());
}
