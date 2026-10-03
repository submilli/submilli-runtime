//! Store failures must stay distinct from missing names at every request boundary.
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_blueprint::{Blueprint, VfsConfig};
use submilli_server::{
    AppState, ServerConfig, app,
    blueprint::{BlueprintStore, InMemoryBlueprintStore, StoreError, StoredBlueprint},
};
use tower::ServiceExt;

struct ControlledStore {
    inner: InMemoryBlueprintStore,
    reads_left: AtomicUsize,
    fail_writes: AtomicBool,
    missing: AtomicBool,
}

impl ControlledStore {
    fn new() -> Self {
        let blueprint = Blueprint {
            name: "tenant".into(),
            vfs: VfsConfig::PerSession {
                size_limit: None,
                mounts: Default::default(),
                cwd: None,
            },
            ..Blueprint::default()
        };
        Self {
            inner: InMemoryBlueprintStore::seed([blueprint]).unwrap(),
            reads_left: AtomicUsize::new(usize::MAX),
            fail_writes: AtomicBool::new(false),
            missing: AtomicBool::new(false),
        }
    }

    fn read(&self) -> Result<(), StoreError> {
        self.reads_left
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                if left == usize::MAX {
                    Some(left)
                } else {
                    left.checked_sub(1)
                }
            })
            .map(|_| ())
            .map_err(|_| StoreError::Io("injected private store detail".into()))
    }

    fn write(&self) -> Result<(), StoreError> {
        if self.fail_writes.load(Ordering::SeqCst) {
            Err(StoreError::Poisoned)
        } else {
            Ok(())
        }
    }
}

#[async_trait::async_trait]
impl BlueprintStore for ControlledStore {
    async fn add_yaml(&self, stored: StoredBlueprint) -> Result<(), StoreError> {
        self.write()?;
        self.inner.add_yaml(stored).await
    }
    async fn upsert_yaml(&self, stored: StoredBlueprint) -> Result<bool, StoreError> {
        self.write()?;
        self.inner.upsert_yaml(stored).await
    }
    async fn remove(&self, name: &str) -> Result<bool, StoreError> {
        self.write()?;
        self.inner.remove(name).await
    }
    async fn list(&self) -> Result<Vec<String>, StoreError> {
        self.read()?;
        self.inner.list().await
    }
    async fn list_blueprints(&self) -> Result<Vec<Blueprint>, StoreError> {
        self.read()?;
        self.inner.list_blueprints().await
    }
    async fn get(&self, name: &str) -> Result<Option<Blueprint>, StoreError> {
        self.read()?;
        if self.missing.load(Ordering::SeqCst) {
            Ok(None)
        } else {
            self.inner.get(name).await
        }
    }
    async fn get_yaml(&self, name: &str) -> Result<Option<String>, StoreError> {
        self.read()?;
        self.inner.get_yaml(name).await
    }
    async fn unusable_reason(&self, name: &str) -> Result<Option<String>, StoreError> {
        self.read()?;
        self.inner.unusable_reason(name).await
    }
}

struct Harness {
    store: Arc<ControlledStore>,
    router: Router,
    _root: tempfile::TempDir,
}

impl Harness {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Arc::new(ControlledStore::new());
        let state = AppState::new(ServerConfig {
            blueprints: Some(store.clone()),
            session_storage_root: Some(root.path().into()),
            ..ServerConfig::default()
        })
        .unwrap();
        Self {
            router: app(state.clone()),
            store,
            _root: root,
        }
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        body: Value,
        session: Option<&str>,
    ) -> (StatusCode, HeaderMap, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if path.starts_with("/v1/sessions/") && path.ends_with("/execute") {
            request = request.header("idempotency-key", "store-failure-key");
        }
        if let Some(session) = session {
            request = request.header("mcp-session-id", session);
        }
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let value = if bytes.is_empty() {
            Value::Null
        } else if headers
            .get("content-type")
            .is_some_and(|v| v.to_str().unwrap().starts_with("text/event-stream"))
        {
            let text = std::str::from_utf8(&bytes).unwrap();
            let frame = text
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .next_back()
                .unwrap();
            serde_json::from_str(frame.trim()).unwrap()
        } else if headers
            .get("content-type")
            .is_some_and(|v| v.to_str().unwrap().starts_with("application/json"))
        {
            serde_json::from_slice(&bytes).unwrap()
        } else {
            Value::String(String::from_utf8(bytes.to_vec()).unwrap())
        };
        (status, headers, value)
    }

    async fn initialize(&self) -> (StatusCode, HeaderMap, Value) {
        self.request("POST", "/mcp/tenant", json!({
            "jsonrpc":"2.0", "id":1, "method":"initialize",
            "params":{"protocolVersion":"2025-06-18", "capabilities":{}, "clientInfo":{"name":"test","version":"0"}}
        }), None).await
    }

    async fn handshake(&self) -> String {
        let (status, headers, body) = self.initialize().await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let session = headers
            .get("mcp-session-id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(
            self.request(
                "POST",
                "/mcp/tenant",
                json!({"jsonrpc":"2.0", "method":"notifications/initialized"}),
                Some(&session)
            )
            .await
            .0,
            StatusCode::ACCEPTED
        );
        session
    }
}

fn assert_internal(body: &Value) {
    assert_eq!(body["error"], "internal_error", "{body}");
    assert_eq!(body["message"], "blueprint store unavailable", "{body}");
    assert!(!body.to_string().contains("injected private store detail"));
}

#[tokio::test]
async fn rest_reads_report_internal_failure_and_recover() {
    let harness = Harness::new();
    harness.store.reads_left.store(0, Ordering::SeqCst);
    for route in [
        "/v1/status",
        "/v1/blueprints",
        "/v1/blueprints/tenant",
        "/v1/blueprints/tenant/prompt",
        "/v1/mcp/tenant/auth-status",
        "/v1/blueprints/tenant/packages/search",
        "/v1/blueprints/tenant/packages/docs?name=submilli:fs",
        "/v1/blueprints/tenant/builtins",
        "/v1/blueprints/tenant/builtins/docs?name=Array",
    ] {
        let (status, _, body) = harness.request("GET", route, Value::Null, None).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{route}: {body}");
        assert_internal(&body);
    }
    let (status, _, body) = harness.initialize().await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_internal(&body);
    harness.store.reads_left.store(usize::MAX, Ordering::SeqCst);
    assert_eq!(
        harness
            .request("GET", "/v1/blueprints/tenant", Value::Null, None)
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn rest_writes_report_internal_failure_without_changing_store() {
    let harness = Harness::new();
    harness.store.fail_writes.store(true, Ordering::SeqCst);
    for (method, route, body) in [
        (
            "POST",
            "/v1/blueprints",
            json!({"yaml":"name: new-tenant\n"}),
        ),
        (
            "PUT",
            "/v1/blueprints/tenant",
            json!({"yaml":"name: tenant\n"}),
        ),
        ("DELETE", "/v1/blueprints/tenant", Value::Null),
    ] {
        let (status, _, response) = harness.request(method, route, body, None).await;
        assert_eq!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{route}: {response}"
        );
        assert_internal(&response);
    }
    assert_eq!(harness.store.inner.list().await.unwrap(), vec!["tenant"]);
    harness.store.fail_writes.store(false, Ordering::SeqCst);
    assert_eq!(
        harness
            .request(
                "PUT",
                "/v1/blueprints/tenant",
                json!({"yaml":"name: tenant\n"}),
                None
            )
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn failed_execution_lookup_creates_no_session_and_keeps_runtime_envelope() {
    let harness = Harness::new();
    harness.store.reads_left.store(0, Ordering::SeqCst);
    let code = "function main(): number { return 42; }";
    let (status, headers, body) = harness
        .request(
            "POST",
            "/v1/execute",
            json!({"blueprint":"tenant", "code":code}),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.contains_key("mcp-session-id"));
    assert_eq!(body["error"]["kind"], "runtime_error");
    assert_eq!(body["error"]["message"], "blueprint store unavailable");
    assert!(body["result"].is_null());
    let (status, _, body) = harness
        .request("POST", "/v1/sessions", json!({"blueprint":"tenant"}), None)
        .await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_internal(&body);
    harness.store.reads_left.store(usize::MAX, Ordering::SeqCst);
    let (_, _, status) = harness
        .request("GET", "/v1/status", Value::Null, None)
        .await;
    assert_eq!(status["active_sessions"], 0);
    harness.store.reads_left.store(usize::MAX, Ordering::SeqCst);
    let (_, _, body) = harness
        .request(
            "POST",
            "/v1/execute",
            json!({"blueprint":"tenant", "code":code}),
            None,
        )
        .await;
    assert_eq!(body["result"], "42", "{body}");
}

#[tokio::test]
async fn missing_name_metadata_failure_is_not_reported_as_not_found() {
    let harness = Harness::new();
    harness.store.missing.store(true, Ordering::SeqCst);
    for path in ["/v1/execute", "/v1/sessions"] {
        harness.store.reads_left.store(1, Ordering::SeqCst);
        let (_, _, body) = harness
            .request(
                "POST",
                path,
                json!({"blueprint":"tenant", "code":"function main(): void {}"}),
                None,
            )
            .await;
        assert!(
            body.to_string().contains("blueprint store unavailable"),
            "{body}"
        );
        assert!(!body.to_string().contains("blueprint_not_found"), "{body}");
    }
}

#[tokio::test]
async fn mcp_initialization_failure_binds_no_session() {
    let harness = Harness::new();
    // The router sees a blueprint; the session initializer's fresh lookup fails.
    harness.store.reads_left.store(1, Ordering::SeqCst);
    let (status, _, body) = harness.initialize().await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    harness.store.reads_left.store(usize::MAX, Ordering::SeqCst);
    let (_, _, status) = harness
        .request("GET", "/v1/status", Value::Null, None)
        .await;
    assert_eq!(status["active_sessions"], 0);
    harness.store.reads_left.store(usize::MAX, Ordering::SeqCst);
    harness.handshake().await;
}

#[tokio::test]
async fn mcp_methods_propagate_store_failures_instead_of_using_fallbacks() {
    let harness = Harness::new();
    let session = harness.handshake().await;
    for (index, message) in [
        json!({"method":"tools/list"}),
        json!({"method":"tools/call", "params":{"name":"submilli__typescript__packages__search", "arguments":{"query":""}}}),
        json!({"method":"tools/call", "params":{"name":"submilli__typescript__packages__docs", "arguments":{"name":"submilli:fs"}}}),
        json!({"method":"tools/call", "params":{"name":"submilli__typescript__builtins__docs", "arguments":{"names":["Array"]}}}),
        json!({"method":"tools/call", "params":{"name":"submilli__typescript__execute", "arguments":{"code":"function main(): number { return 42; }"}}}),
    ].into_iter().enumerate() {
        let mut message = message;
        message["jsonrpc"] = json!("2.0");
        message["id"] = json!(index + 2);
        // Permit the endpoint lookup, fail the method's own lookup.
        harness.store.reads_left.store(1, Ordering::SeqCst);
        let (status, _, body) = harness.request("POST", "/mcp/tenant", message, Some(&session)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["error"]["code"], -32603, "{body}");
        assert_eq!(body["error"]["message"], "blueprint store unavailable", "{body}");
    }
    harness.store.reads_left.store(usize::MAX, Ordering::SeqCst);
    let (_, _, body) = harness
        .request(
            "POST",
            "/mcp/tenant",
            json!({"jsonrpc":"2.0","id":20,"method":"tools/list"}),
            Some(&session),
        )
        .await;
    assert!(body["result"]["tools"].is_array(), "{body}");
}

#[tokio::test]
async fn session_lookup_failure_does_not_reserve_the_idempotency_key() {
    let harness = Harness::new();
    let (status, _, created) = harness
        .request("POST", "/v1/sessions", json!({"blueprint":"tenant"}), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let session = created["session_id"].as_str().unwrap();
    harness.store.reads_left.store(0, Ordering::SeqCst);
    for (suffix, body) in [
        (
            "execute",
            json!({"code":"function main(): number { return 1; }"}),
        ),
        ("rebind", json!({"secrets":{}})),
    ] {
        let path = format!("/v1/sessions/{session}/{suffix}");
        let (status, _, response) = harness.request("POST", &path, body, None).await;
        assert_eq!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{path}: {response}"
        );
        assert_internal(&response);
    }
    harness.store.reads_left.store(usize::MAX, Ordering::SeqCst);
    // A different program under the same key must run, proving no reservation/fingerprint was stored.
    let (_, _, response) = harness
        .request(
            "POST",
            &format!("/v1/sessions/{session}/execute"),
            json!({"code":"function main(): number { return 42; }"}),
            None,
        )
        .await;
    assert_eq!(response["result"], "42", "{response}");
}
