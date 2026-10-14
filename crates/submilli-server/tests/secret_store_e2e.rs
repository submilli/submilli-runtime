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
use submilli_server::database::ServerDatabase;
use submilli_server::{AppState, FileSecretStore, KeySource, SecretStore, ServerConfig, app};
use submilli_shared::secret_store::SecretCipher;
use tower::ServiceExt;

/// A router whose secrets live in a fresh in-memory database, sealed with its
/// throwaway key.
fn router_with_store() -> Router {
    app(futures::executor::block_on(AppState::new(in_memory_config::config())).expect("AppState"))
}

fn write_key(dir: &std::path::Path, fill: u8) -> std::path::PathBuf {
    let key_path = dir.join("key.b64");
    std::fs::write(
        &key_path,
        base64::engine::general_purpose::STANDARD.encode([fill; 32]),
    )
    .unwrap();
    key_path
}

/// A server over the database file in `root`, its secrets sealed with the key
/// in `key` (or off without one).
async fn durable(
    root: &std::path::Path,
    key: Option<&std::path::Path>,
) -> (AppState, Arc<ServerDatabase>) {
    let database = Arc::new(ServerDatabase::open(&root.join("server.db")).await.unwrap());
    let state = AppState::new(ServerConfig {
        database: Some(database.clone()),
        session_storage_root: Some(root.join("workspaces")),
        session_cipher: key
            .map(|key| Arc::new(SecretCipher::new(&KeySource::File(key.to_path_buf())).unwrap())),
        secret_store_dir: Some(root.join("secrets")),
        ..in_memory_config::config()
    })
    .await
    .unwrap();
    state.boot().await.unwrap();
    (state, database)
}

/// An error and its causes, as the server binary reports a failed boot.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut chain = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        chain.push_str(": ");
        chain.push_str(&cause.to_string());
        source = cause.source();
    }
    chain
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
    // A durable database without a key has nowhere safe to keep secrets.
    let root = tempfile::tempdir().unwrap();
    let (state, database) = durable(root.path(), None).await;
    let router = app(state);
    let (status, body) = get(&router, "/v1/secrets").await;
    database.close().await.unwrap();
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

const OAUTH_BLUEPRINT: &str = "name: crm\nmcp:\n  sales:\n    url: https://sales.example.com/mcp\n    auth:\n      type: oauth2\n      client_id: cid\n";
const CANARY: &str = "secret-canary-4b1e9d27";
const GRANT_CANARY: &str = "grant-canary-8f03aa61";

/// Whether any of the database's files hold `needle` in plaintext.
fn on_disk(root: &std::path::Path, needle: &str) -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file()
            && std::fs::read(&path)
                .unwrap()
                .windows(needle.len())
                .any(|window| window == needle.as_bytes())
        {
            found.push(path.display().to_string());
        }
    }
    found
}

#[tokio::test]
async fn reserved_grant_names_are_refused_as_secrets() {
    let router = router_with_store();
    let (status, body) = post(
        &router,
        "/v1/secrets",
        json!({ "key": "mcp_oauth/crm/sales/credential", "value": "v" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], json!("invalid_secret_name"));
    let (status, _) = delete(&router, "/v1/secrets/mcp_oauth/crm/sales/credential").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = post(&router, "/v1/secrets", json!({ "key": "", "value": "v" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn secrets_and_grants_stay_sealed_on_disk_and_survive_a_restart() {
    let root = tempfile::tempdir().unwrap();
    let key = write_key(root.path(), 5);
    let (state, database) = durable(root.path(), Some(&key)).await;
    let router = app(state);
    let (status, _) = post(
        &router,
        "/v1/secrets",
        json!({ "key": "prod/token", "value": CANARY }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = post(
        &router,
        "/v1/blueprints",
        json!({ "yaml": OAUTH_BLUEPRINT }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = post(
        &router,
        "/v1/mcp/crm/sales/refresh-token",
        json!({
            "refresh_token": GRANT_CANARY,
            "token_endpoint": "https://auth.example.com/token",
            "client_id": "cid",
            "scopes": ["read"],
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], json!("active"));
    // Neither the database nor its WAL ever holds a value in plaintext.
    assert!(on_disk(root.path(), CANARY).is_empty());
    assert!(on_disk(root.path(), GRANT_CANARY).is_empty());
    drop(router);
    database.close().await.unwrap();
    assert!(on_disk(root.path(), CANARY).is_empty());
    assert!(on_disk(root.path(), GRANT_CANARY).is_empty());

    let (state, database) = durable(root.path(), Some(&key)).await;
    let router = app(state);
    let blueprint = "name: needs-store\nsecrets:\n  TOK:\n    store: prod/token\n";
    let (status, body) = post(&router, "/v1/blueprints", json!({ "yaml": blueprint })).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the secret resolves after a restart: {body}"
    );
    let (_, body) = get(&router, "/v1/mcp-grants").await;
    assert_eq!(body["grants"][0]["blueprint"], json!("crm"));
    assert_eq!(body["grants"][0]["kind"], json!("refresh"));
    assert_eq!(body["grants"][0]["scopes"], json!(["read"]));
    assert!(!body.to_string().contains(GRANT_CANARY));
    drop(router);
    database.close().await.unwrap();

    // Another key cannot open them, and says so instead of calling them corrupt.
    let other = tempfile::tempdir().unwrap();
    let wrong = write_key(other.path(), 6);
    let (state, database) = durable(root.path(), Some(&wrong)).await;
    let router = app(state);
    let blueprint = "name: needs-store-too\nsecrets:\n  TOK:\n    store: prod/token\n";
    let (status, body) = post(&router, "/v1/blueprints", json!({ "yaml": blueprint })).await;
    assert_ne!(status, StatusCode::OK);
    assert!(body.to_string().contains("was sealed with key"), "{body}");
    assert!(!body.to_string().contains(CANARY));
    drop(router);
    database.close().await.unwrap();
}

#[tokio::test]
async fn grants_are_listed_deleted_and_removed_with_their_blueprint() {
    let router = router_with_store();
    let (status, _) = post(
        &router,
        "/v1/blueprints",
        json!({ "yaml": OAUTH_BLUEPRINT }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let deposit = json!({ "access_token": "static-token" });
    let (status, _) = post(&router, "/v1/mcp/crm/sales/refresh-token", deposit.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = get(&router, "/v1/mcp-grants?blueprint=crm").await;
    assert_eq!(body["grants"][0]["kind"], json!("static"));
    let (_, body) = get(&router, "/v1/mcp-grants?blueprint=other").await;
    assert_eq!(body["grants"], json!([]));

    let (status, body) = delete(&router, "/v1/mcp-grants/crm/sales").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = delete(&router, "/v1/mcp-grants/crm/sales").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], json!("grant_not_found"));
    let (_, body) = get(&router, "/v1/mcp/crm/auth-status").await;
    assert_eq!(body["state"], json!("pending"));

    let (status, _) = post(&router, "/v1/mcp/crm/sales/refresh-token", deposit).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = delete(&router, "/v1/blueprints/crm").await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = get(&router, "/v1/mcp-grants").await;
    assert_eq!(
        body["grants"],
        json!([]),
        "a removed blueprint takes its grants"
    );
}

/// A pre-database secret-store directory holding a secret and a grant.
fn legacy_directory(root: &std::path::Path, key: &std::path::Path) {
    let legacy =
        FileSecretStore::open(root.join("secrets"), &KeySource::File(key.to_path_buf())).unwrap();
    futures::executor::block_on(async {
        legacy.put("legacy/token", CANARY).await.unwrap();
        legacy
            .put(
                "mcp_oauth/crm/sales/credential",
                &json!({
                    "refresh_token": GRANT_CANARY,
                    "token_endpoint": "https://auth.example.com/token",
                    "scopes": ["read"],
                })
                .to_string(),
            )
            .await
            .unwrap();
        legacy
            .put(
                "mcp_oauth/gone/sales/credential",
                &json!({ "access_token": "orphan" }).to_string(),
            )
            .await
            .unwrap();
    });
}

async fn seed_oauth_blueprint(root: &std::path::Path) {
    use submilli_server::blueprint::{BlueprintStore, SqliteBlueprintStore, StoredBlueprint};
    let database = Arc::new(ServerDatabase::open(&root.join("server.db")).await.unwrap());
    SqliteBlueprintStore::new(database.clone(), None)
        .add_yaml(StoredBlueprint::new(
            submilli_blueprint::parse(OAUTH_BLUEPRINT).unwrap(),
            OAUTH_BLUEPRINT.into(),
        ))
        .await
        .unwrap();
    database.close().await.unwrap();
}

#[tokio::test]
async fn a_legacy_directory_is_imported_and_archived() {
    let root = tempfile::tempdir().unwrap();
    let key = write_key(root.path(), 5);
    legacy_directory(root.path(), &key);
    seed_oauth_blueprint(root.path()).await;

    let (state, database) = durable(root.path(), Some(&key)).await;
    assert!(!root.path().join("secrets").exists());
    assert!(root.path().join("archive/secrets").is_dir());
    let router = app(state);
    let (_, body) = get(&router, "/v1/secrets").await;
    assert_eq!(body["keys"], json!(["legacy/token"]));
    let (_, body) = get(&router, "/v1/mcp-grants").await;
    assert_eq!(body["grants"].as_array().unwrap().len(), 1, "{body}");
    assert_eq!(body["grants"][0]["server"], json!("sales"));
    assert!(on_disk(root.path(), CANARY).is_empty());
    drop(router);
    database.close().await.unwrap();

    // A second boot has nothing left to import.
    let (_, database) = durable(root.path(), Some(&key)).await;
    database.close().await.unwrap();
}

#[tokio::test]
async fn a_legacy_directory_under_another_key_is_not_imported() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    legacy_directory(root.path(), &write_key(other.path(), 7));
    let key = write_key(root.path(), 5);
    let database = Arc::new(
        ServerDatabase::open(&root.path().join("server.db"))
            .await
            .unwrap(),
    );
    let state = AppState::new(ServerConfig {
        database: Some(database.clone()),
        session_storage_root: Some(root.path().join("workspaces")),
        session_cipher: Some(Arc::new(SecretCipher::new(&KeySource::File(key)).unwrap())),
        secret_store_dir: Some(root.path().join("secrets")),
        ..in_memory_config::config()
    })
    .await
    .unwrap();
    let error = error_chain(&state.boot().await.unwrap_err());
    assert!(error.contains("cannot be read"), "{error}");
    assert!(!error.contains(CANARY));
    assert!(root.path().join("secrets").is_dir(), "nothing is archived");
    assert!(!root.path().join("archive").exists());
    database.close().await.unwrap();
}

#[tokio::test]
async fn an_empty_legacy_directory_needs_no_key_and_is_left_alone() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("secrets")).unwrap();
    let (_, database) = durable(root.path(), None).await;
    database.close().await.unwrap();
    assert!(root.path().join("secrets").is_dir());
    assert!(!root.path().join("archive").exists());
}

#[tokio::test]
async fn a_later_import_archives_beside_an_earlier_one() {
    let root = tempfile::tempdir().unwrap();
    let key = write_key(root.path(), 5);
    legacy_directory(root.path(), &key);
    let (_, database) = durable(root.path(), Some(&key)).await;
    database.close().await.unwrap();
    // A rollback to the old server writes a new secret where it used to.
    let legacy =
        FileSecretStore::open(root.path().join("secrets"), &KeySource::File(key.clone())).unwrap();
    legacy.put("after/rollback", "v").await.unwrap();

    let (state, database) = durable(root.path(), Some(&key)).await;
    assert!(root.path().join("archive/secrets").is_dir());
    assert!(root.path().join("archive/secrets-2").is_dir());
    let (_, body) = get(&app(state), "/v1/secrets").await;
    assert_eq!(body["keys"], json!(["after/rollback", "legacy/token"]));
    database.close().await.unwrap();
}

#[tokio::test]
async fn a_legacy_entry_that_differs_from_the_database_stops_the_import() {
    let root = tempfile::tempdir().unwrap();
    let key = write_key(root.path(), 5);
    let (state, database) = durable(root.path(), Some(&key)).await;
    let (status, _) = post(
        &app(state),
        "/v1/secrets",
        json!({ "key": "legacy/token", "value": "newer" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    database.close().await.unwrap();
    legacy_directory(root.path(), &key);

    let database = Arc::new(
        ServerDatabase::open(&root.path().join("server.db"))
            .await
            .unwrap(),
    );
    let state = AppState::new(ServerConfig {
        database: Some(database.clone()),
        session_storage_root: Some(root.path().join("workspaces")),
        session_cipher: Some(Arc::new(SecretCipher::new(&KeySource::File(key)).unwrap())),
        secret_store_dir: Some(root.path().join("secrets")),
        ..in_memory_config::config()
    })
    .await
    .unwrap();
    let error = error_chain(&state.boot().await.unwrap_err());
    assert!(error.contains("different value"), "{error}");
    assert!(root.path().join("secrets").is_dir(), "nothing is archived");
    database.close().await.unwrap();
}

#[tokio::test]
async fn a_legacy_entry_the_database_cannot_hold_is_skipped_and_archived() {
    let root = tempfile::tempdir().unwrap();
    let key = write_key(root.path(), 5);
    let legacy =
        FileSecretStore::open(root.path().join("secrets"), &KeySource::File(key.clone())).unwrap();
    legacy.put("bad\nname", "v").await.unwrap();
    legacy.put("good", "v").await.unwrap();
    legacy
        .put("mcp_oauth/crm/other/credential", "not json")
        .await
        .unwrap();
    // Accepted before grants had rules: a refresh grant with no endpoint.
    legacy
        .put(
            "mcp_oauth/crm/sales/credential",
            &json!({ "refresh_token": "r", "token_endpoint": "" }).to_string(),
        )
        .await
        .unwrap();
    seed_oauth_blueprint(root.path()).await;
    let (state, database) = durable(root.path(), Some(&key)).await;
    let router = app(state);
    let (_, body) = get(&router, "/v1/secrets").await;
    assert_eq!(body["keys"], json!(["good"]));
    let (_, body) = get(&router, "/v1/mcp-grants").await;
    assert_eq!(body["grants"], json!([]));
    assert!(root.path().join("archive/secrets").is_dir());
    drop(router);
    database.close().await.unwrap();
}

#[tokio::test]
async fn a_legacy_directory_holding_the_database_is_refused_before_importing() {
    let root = tempfile::tempdir().unwrap();
    let key = write_key(root.path(), 5);
    legacy_directory(root.path(), &key);
    let database = Arc::new(
        ServerDatabase::open(&root.path().join("secrets/server.db"))
            .await
            .unwrap(),
    );
    let config = |secret_store_dir| ServerConfig {
        database: Some(database.clone()),
        session_storage_root: Some(root.path().join("workspaces")),
        session_cipher: Some(Arc::new(
            SecretCipher::new(&KeySource::File(key.clone())).unwrap(),
        )),
        secret_store_dir,
        ..in_memory_config::config()
    };
    let state = AppState::new(config(Some(root.path().join("secrets"))))
        .await
        .unwrap();
    let error = error_chain(&state.boot().await.unwrap_err());
    assert!(error.contains("contains the database"), "{error}");
    assert!(!root.path().join("archive").exists(), "nothing is archived");

    // Refused before importing: the database holds none of the directory.
    let state = AppState::new(config(None)).await.unwrap();
    state.boot().await.unwrap();
    let (_, body) = get(&app(state), "/v1/secrets").await;
    assert_eq!(body["keys"], json!([]));
    database.close().await.unwrap();
}

#[tokio::test]
async fn a_legacy_directory_with_secrets_needs_a_durable_database() {
    let root = tempfile::tempdir().unwrap();
    let key = write_key(root.path(), 5);
    legacy_directory(root.path(), &key);
    let error = AppState::new(ServerConfig {
        secret_store_dir: Some(root.path().join("secrets")),
        ..in_memory_config::config()
    })
    .await
    .err()
    .expect("an in-memory database cannot import");
    assert!(error.to_string().contains("persistent"), "{error}");
}

#[tokio::test]
async fn an_invalid_grant_is_a_bad_request() {
    let router = router_with_store();
    let (status, _) = post(
        &router,
        "/v1/blueprints",
        json!({ "yaml": OAUTH_BLUEPRINT }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = post(
        &router,
        "/v1/mcp/crm/sales/refresh-token",
        json!({ "refresh_token": "", "token_endpoint": "https://auth.example.com/token" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], json!("invalid_credential"));
}
