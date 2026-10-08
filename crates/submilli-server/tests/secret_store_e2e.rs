//! End-to-end tests for the secret-store REST surface (`/v1/secrets`) and the
//! blueprint `store:` secret source, driven through the real router.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine as _;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_server::{AppState, FileSecretStore, KeySource, SecretStore, ServerConfig, app};
use tower::ServiceExt;

/// A router whose `AppState` has a fresh, empty file-backed secret store.
fn router_with_store() -> Router {
    let tmp = tempfile::tempdir().expect("tempdir");
    let key_path = tmp.path().join("key.b64");
    std::fs::write(
        &key_path,
        base64::engine::general_purpose::STANDARD.encode([5u8; 32]),
    )
    .unwrap();
    let store: Arc<dyn SecretStore> = Arc::new(
        FileSecretStore::open(tmp.path().join("secrets"), &KeySource::File(key_path)).unwrap(),
    );
    std::mem::forget(tmp); // outlive the store for the test process
    let config = ServerConfig {
        secret_store: Some(store),
        ..in_memory_config::config()
    };
    app(futures::executor::block_on(AppState::new(config)).expect("AppState"))
}

async fn send(router: &Router, method: &str, path: &str, body: Body) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(path);
    if method == "POST" {
        builder = builder.header("content-type", "application/json");
    }
    let resp = router
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, value)
}

async fn post(router: &Router, path: &str, body: Value) -> (StatusCode, Value) {
    send(router, "POST", path, Body::from(body.to_string())).await
}

async fn get(router: &Router, path: &str) -> (StatusCode, Value) {
    send(router, "GET", path, Body::empty()).await
}

async fn delete(router: &Router, path: &str) -> (StatusCode, Value) {
    send(router, "DELETE", path, Body::empty()).await
}

#[tokio::test]
async fn put_list_delete_round_trip() {
    let router = router_with_store();
    let key = "mcp/blueprint/server/refresh_token";

    let (status, _) = post(
        &router,
        "/v1/secrets",
        json!({ "key": key, "value": "tok" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = post(
        &router,
        "/v1/secrets",
        json!({ "key": "other/k", "value": "x" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Listing returns key names only (never values), and honors the prefix.
    let (status, body) = get(&router, "/v1/secrets?prefix=mcp/").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["keys"], json!([key]));

    let (status, _) = delete(&router, &format!("/v1/secrets/{key}")).await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = delete(&router, &format!("/v1/secrets/{key}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], json!("secret_not_found"));

    let (status, body) = get(&router, "/v1/secrets").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["keys"],
        json!(["other/k"]),
        "deleted key should be gone"
    );
}

#[tokio::test]
async fn deleting_a_missing_secret_returns_404() {
    let router = router_with_store();
    let (status, body) = delete(&router, "/v1/secrets/missing/key").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], json!("secret_not_found"));
}

#[tokio::test]
async fn secrets_cannot_be_read_back_over_the_api() {
    let router = router_with_store();
    let (status, _) = post(&router, "/v1/secrets", json!({ "key": "k", "value": "v" })).await;
    assert_eq!(status, StatusCode::OK);
    // There is no read route; GET on the per-key path is method-not-allowed.
    let (status, _) = get(&router, "/v1/secrets/k").await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn disabled_store_returns_503() {
    let router = app(AppState::new(in_memory_config::config())
        .await
        .expect("AppState"));
    let (status, body) = get(&router, "/v1/secrets").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"], json!("no_secret_store"));
}

#[tokio::test]
async fn blueprint_store_secret_verified_at_add() {
    let router = router_with_store();
    let blueprint = "name: needs-store\nsecrets:\n  TOK:\n    store: prod/token\n";

    // The key isn't in the store yet → add is rejected.
    let (status, body) = post(&router, "/v1/blueprints", json!({ "yaml": blueprint })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "got: {body}");
    assert_eq!(body["error"], json!("unresolved_secret"));

    // Store the key, then the same add succeeds.
    let (status, _) = post(
        &router,
        "/v1/secrets",
        json!({ "key": "prod/token", "value": "v" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = post(&router, "/v1/blueprints", json!({ "yaml": blueprint })).await;
    assert_eq!(status, StatusCode::OK, "got: {body}");
}
