//! End-to-end tests for the MCP OAuth admin surface + the PENDING/ACTIVE bind
//! gate, driven in-process through `app(state).oneshot(...)`.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_blueprint::{Blueprint, McpAuth, McpServer, SecretSource};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::{AppState, FileSecretStore, KeySource, SecretStore, ServerConfig, app};
use tower::ServiceExt;

const BP: &str = "sf";
const SERVER: &str = "salesforce";

struct Harness {
    state: AppState,
    store: Arc<dyn SecretStore>,
    _tmp: tempfile::TempDir,
}

impl Harness {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let key_path = tmp.path().join("key.b64");
        std::fs::write(&key_path, STANDARD.encode([9u8; 32])).unwrap();
        let store: Arc<dyn SecretStore> = Arc::new(
            FileSecretStore::open(tmp.path().join("secrets"), &KeySource::File(key_path)).unwrap(),
        );

        let oauth_server = McpServer {
            transport: "streamable_http".into(),
            url: "https://sf.example.com/mcp".into(),
            headers: BTreeMap::new(),
            auth: Some(McpAuth::Oauth2 {
                client_id: Some("${secrets.CID}".into()),
                // Pinned so auth-config needs no network discovery in the test.
                authorization_endpoint: Some("https://idp.example.com/authorize".into()),
                token_endpoint: Some("https://idp.example.com/token".into()),
                scopes: vec!["api".into(), "refresh_token".into()],
            }),
        };
        let blueprint = Blueprint {
            name: BP.into(),
            secrets: BTreeMap::from([("CID".to_string(), SecretSource::Store("cid-key".into()))]),
            mcp: BTreeMap::from([(SERVER.to_string(), oauth_server)]),
            ..Default::default()
        };

        let config = ServerConfig {
            blueprints: Some(Arc::new(
                InMemoryBlueprintStore::seed([blueprint]).expect("seed blueprints"),
            )),
            secret_store: Some(store.clone()),
            ..in_memory_config::config()
        };
        Self {
            state: AppState::new(config).expect("AppState"),
            store,
            _tmp: tmp,
        }
    }

    async fn send(&self, req: Request<Body>) -> (StatusCode, Value) {
        let resp = app(self.state.clone()).oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, body)
    }

    async fn get(&self, uri: &str) -> (StatusCode, Value) {
        self.send(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    async fn post(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        self.send(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
    }

    async fn delete(&self, uri: &str) -> (StatusCode, Value) {
        self.send(
            Request::builder()
                .method("DELETE")
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    /// POST an MCP `initialize` to the bound endpoint; return just the status.
    async fn initialize_status(&self) -> StatusCode {
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }
        });
        self.send(
            Request::builder()
                .method("POST")
                .uri(format!("/mcp/{BP}"))
                .header("content-type", "application/json")
                .header("accept", "application/json, text/event-stream")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .0
    }
}

#[tokio::test]
async fn unauthenticated_oauth_blueprint_still_binds() {
    let h = Harness::new();
    // No credential yet: the server is omitted from discovery (with a warning),
    // but the blueprint still binds — an unauthenticated MCP no longer blocks it.
    assert_ne!(h.initialize_status().await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn authenticate_then_bind_then_deauthenticate() {
    let h = Harness::new();
    let token_uri = format!("/v1/mcp/{BP}/{SERVER}/refresh-token");

    // Deposit a credential → blueprint flips ACTIVE.
    let (status, body) = h
        .post(
            &token_uri,
            json!({
                "refresh_token": "refresh-xyz",
                "client_id": "client-123",
                "token_endpoint": "https://idp.example.com/token",
                "scopes": ["api"],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"], "active", "got {body}");

    // The whole credential round-trips in the store — refresh token plus the
    // inputs the runtime redeems it with at call time.
    let raw = h
        .store
        .get(&format!("mcp_oauth/{BP}/{SERVER}/credential"))
        .await
        .unwrap()
        .expect("credential stored");
    let stored: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(stored["refresh_token"], "refresh-xyz");
    assert_eq!(stored["client_id"], "client-123");
    assert_eq!(stored["token_endpoint"], "https://idp.example.com/token");
    assert_eq!(stored["scopes"], json!(["api"]));

    // ACTIVE → binding is allowed (initialize is no longer 403).
    assert_ne!(h.initialize_status().await, StatusCode::FORBIDDEN);

    // Deauthenticate → auth-status reports PENDING again, but binding still
    // succeeds (the now-unauthenticated server is just omitted from discovery).
    let (status, body) = h.delete(&token_uri).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["state"], "pending", "got {body}");
    assert_ne!(h.initialize_status().await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn auth_config_resolves_secret_client_id() {
    let h = Harness::new();
    h.store.put("cid-key", "client-123").await.unwrap();

    let (status, body) = h.get(&format!("/v1/mcp/{BP}/{SERVER}/auth-config")).await;
    assert_eq!(status, StatusCode::OK, "got {body}");
    assert_eq!(body["client_id"], "client-123");
    assert_eq!(body["url"], "https://sf.example.com/mcp");
    assert_eq!(body["scopes"], json!(["api", "refresh_token"]));
}

#[tokio::test]
async fn auth_status_reports_per_server_state() {
    let h = Harness::new();
    let (status, body) = h.get(&format!("/v1/mcp/{BP}/auth-status")).await;
    assert_eq!(status, StatusCode::OK, "got {body}");
    assert_eq!(body["state"], "pending");
    let server = &body["servers"][0];
    assert_eq!(server["name"], SERVER);
    assert_eq!(server["kind"], "oauth");
    assert_eq!(server["authenticated"], json!(false));
}

#[tokio::test]
async fn unknown_blueprint_or_server_is_rejected() {
    let h = Harness::new();
    let (status, _) = h.get("/v1/mcp/nope/salesforce/auth-config").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = h.get(&format!("/v1/mcp/{BP}/ghost/auth-config")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
