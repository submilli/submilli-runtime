//! SQL-backed REST and MCP sessions use the same encrypted bindings after restart.
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
use submilli_server::blueprint::{BlueprintStore, SqliteBlueprintStore, StoredBlueprint};
use submilli_server::database::ServerDatabase;
use submilli_server::session_store::SessionStatus;
use submilli_server::{AppState, ServerConfig, app};
use submilli_shared::secret_store::{KeySource, SecretCipher};
use tower::ServiceExt;

const POLICY: &str = "name: durable\ndefault: deny\nvfs: per_session\nvariables:\n  tenant:\n    required: true\nsecrets:\n  TOKEN:\n    harness:\n      required: true\n";
const CANARY: &str = "durable-secret-canary-7c5d91e0";

async fn open(root: &Path, with_key: bool) -> (Router, Arc<ServerDatabase>) {
    let database = Arc::new(ServerDatabase::open(&root.join("server.db")).await.unwrap());
    let blueprints = Arc::new(SqliteBlueprintStore::new(database.clone(), None));
    if blueprints.get("durable").await.unwrap().is_none() {
        blueprints
            .add_yaml(StoredBlueprint::new(
                submilli_blueprint::parse(POLICY).unwrap(),
                POLICY.into(),
            ))
            .await
            .unwrap();
    }
    let key = root.join("key");
    if !key.exists() {
        std::fs::write(&key, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").unwrap();
    }
    let cipher = with_key.then(|| Arc::new(SecretCipher::new(&KeySource::File(key)).unwrap()));
    let state = AppState::new(ServerConfig {
        database: Some(database.clone()),
        blueprints: Some(blueprints),
        session_storage_root: Some(root.join("workspaces")),
        session_cipher: cipher,
        ..Default::default()
    })
    .unwrap();
    state.boot().await.unwrap();
    (app(state), database)
}

async fn request(
    router: &Router,
    path: &str,
    body: Value,
    session: Option<&str>,
) -> (StatusCode, Option<String>, Value) {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    if let Some(session) = session {
        request = request.header("mcp-session-id", session);
    }
    let response = router
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let id = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes);
    let value = serde_json::from_slice(&bytes)
        .ok()
        .or_else(|| {
            text.lines()
                .filter_map(|line| line.strip_prefix("data: "))
                .find_map(|line| serde_json::from_str(line).ok())
        })
        .unwrap_or(Value::Null);
    (status, id, value)
}

fn assert_no_plaintext(root: &Path) {
    for name in ["server.db", "server.db-wal", "server.db-shm"] {
        if let Ok(bytes) = std::fs::read(root.join(name)) {
            assert!(
                !bytes
                    .windows(CANARY.len())
                    .any(|window| window == CANARY.as_bytes()),
                "plaintext in {name}"
            );
        }
    }
}

#[tokio::test]
async fn rest_resumes_encrypted_bindings_and_rejects_missing_or_wrong_key() {
    let root = tempfile::tempdir().unwrap();
    let (router, database) = open(root.path(), true).await;
    let (status, _, created) = request(
        &router,
        "/v1/sessions",
        json!({"blueprint":"durable", "variables":{"tenant":"one"}, "secrets":{"TOKEN":CANARY}}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let id = created["session_id"].as_str().unwrap().to_owned();
    assert_no_plaintext(root.path());
    drop(router);
    database.close().await.unwrap();
    drop(database);
    let path = format!("/v1/sessions/{id}/execute");
    let code = json!({"code":"function main(): number { return 42; }"});
    let (router, database) = open(root.path(), true).await;
    let (status, _, output) = request(&router, &path, code.clone(), None).await;
    assert_eq!(status, StatusCode::OK, "{output}");
    assert_eq!(output["result"], "42", "{output}");
    drop(router);
    database.close().await.unwrap();
    drop(database);
    let (router, database) = open(root.path(), false).await;
    assert_eq!(
        request(&router, &path, code.clone(), None).await.0,
        StatusCode::CONFLICT
    );
    drop(router);
    database.close().await.unwrap();
    drop(database);
    std::fs::write(
        root.path().join("key"),
        "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=",
    )
    .unwrap();
    let (router, database) = open(root.path(), true).await;
    assert_eq!(
        request(&router, &path, code, None).await.0,
        StatusCode::CONFLICT
    );
    assert_no_plaintext(root.path());
    drop(router);
    database.close().await.unwrap();
}

#[tokio::test]
async fn mcp_metadata_secrets_are_sealed_and_resume_with_required_variables() {
    let root = tempfile::tempdir().unwrap();
    let (router, database) = open(root.path(), true).await;
    let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"},"_meta":{"variables":{"tenant":"one"},"secrets":{"TOKEN":CANARY}}}});
    let (status, id, output) = request(&router, "/mcp/durable", initialize, None).await;
    assert_eq!(status, StatusCode::OK, "{output}");
    let id = id.unwrap();
    request(
        &router,
        "/mcp/durable",
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        Some(&id),
    )
    .await;
    assert_no_plaintext(root.path());
    drop(router);
    database.close().await.unwrap();
    drop(database);
    let (router, database) = open(root.path(), true).await;
    let (status, _, output) = request(&router, "/mcp/durable", json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"submilli__typescript__execute","arguments":{"code":"function main(): number { return 42; }"}}}), Some(&id)).await;
    assert_eq!(status, StatusCode::OK, "{output}");
    assert!(output.get("error").is_none(), "{output}");
    assert_ne!(output["result"]["isError"], true, "{output}");
    assert_no_plaintext(root.path());
    drop(router);
    database.close().await.unwrap();
}

#[tokio::test]
async fn durable_bindings_without_key_are_rejected_before_session_creation() {
    let root = tempfile::tempdir().unwrap();
    let (router, database) = open(root.path(), false).await;
    let (status, _, output) = request(
        &router,
        "/v1/sessions",
        json!({"blueprint":"durable","variables":{"tenant":"one"},"secrets":{"TOKEN":CANARY}}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{output}");
    assert_no_plaintext(root.path());
    drop(router);
    database.close().await.unwrap();
}

#[tokio::test]
async fn mcp_does_not_acknowledge_failed_handshake_commit() {
    let root = tempfile::tempdir().unwrap();
    let (router, database) = open(root.path(), true).await;
    database.transaction(|connection| Box::pin(async move {
        sqlx::query("CREATE TRIGGER fail_handshake BEFORE INSERT ON session_mcp BEGIN SELECT RAISE(FAIL, 'injected handshake failure'); END")
            .execute(connection).await?;
        Ok(())
    })).await.unwrap();
    let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"},"_meta":{"variables":{"tenant":"one"},"secrets":{"TOKEN":CANARY}}}});
    let (status, _, _) = request(&router, "/mcp/durable", initialize.clone(), None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_no_plaintext(root.path());
    database
        .transaction(|connection| {
            Box::pin(async move {
                sqlx::query("DROP TRIGGER fail_handshake")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    assert_eq!(
        request(&router, "/mcp/durable", initialize, None).await.0,
        StatusCode::OK
    );
    drop(router);
    database.close().await.unwrap();
}

#[tokio::test]
async fn mcp_delete_checks_host_and_closes_the_persisted_session() {
    use submilli_server::session_store::{DurableSessionStore, SqliteSessionStore};
    let root = tempfile::tempdir().unwrap();
    let (router, database) = open(root.path(), true).await;
    let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"},"_meta":{"variables":{"tenant":"one"},"secrets":{"TOKEN":CANARY}}}});
    let (_, id, _) = request(&router, "/mcp/durable", initialize, None).await;
    let id = id.unwrap();
    let delete = |host: &str| {
        Request::builder()
            .method("DELETE")
            .uri("/mcp/durable")
            .header("host", host)
            .header("mcp-session-id", &id)
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(
        router
            .clone()
            .oneshot(delete("untrusted.example"))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let store = SqliteSessionStore::new(database.clone(), None, root.path().join("workspaces"));
    assert!(store.load(&id).await.unwrap().unwrap().status == SessionStatus::Active);
    assert_eq!(
        router
            .clone()
            .oneshot(delete("localhost"))
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert!(store.load(&id).await.unwrap().unwrap().status == SessionStatus::Closed);
    assert_eq!(
        request(
            &router,
            "/mcp/durable",
            json!({"jsonrpc":"2.0","id":2,"method":"ping"}),
            Some(&id)
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    drop(router);
    database.close().await.unwrap();
}

#[tokio::test]
async fn failed_mcp_restore_preserves_session_for_rebinding() {
    use submilli_server::session_store::{DurableSessionStore, SqliteSessionStore};
    let root = tempfile::tempdir().unwrap();
    let (router, database) = open(root.path(), true).await;
    let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"},"_meta":{"variables":{"tenant":"one"},"secrets":{"TOKEN":CANARY}}}});
    let (_, id, _) = request(&router, "/mcp/durable", initialize, None).await;
    let id = id.unwrap();
    request(
        &router,
        "/mcp/durable",
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        Some(&id),
    )
    .await;
    drop(router);
    database.close().await.unwrap();
    drop(database);
    std::fs::write(
        root.path().join("key"),
        "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=",
    )
    .unwrap();
    let (router, database) = open(root.path(), true).await;
    let call = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"submilli__typescript__execute","arguments":{"code":"function main(): number { return 42; }"}}});
    for _ in 0..2 {
        let (status, _, _) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            request(&router, "/mcp/durable", call.clone(), Some(&id)),
        )
        .await
        .unwrap();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }
    let store = SqliteSessionStore::new(database.clone(), None, root.path().join("workspaces"));
    let record = store.load(&id).await.unwrap().unwrap();
    assert!(record.status == SessionStatus::Active);
    assert!(record.root_vfs_path.unwrap().is_dir());
    let (status, _, output) = request(
        &router,
        &format!("/v1/sessions/{id}/rebind"),
        json!({"secrets":{"TOKEN":CANARY}}),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{output}");
    let (status, _, output) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        request(&router, "/mcp/durable", call, Some(&id)),
    )
    .await
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{output}");
    assert!(output.get("error").is_none(), "{output}");
    assert_ne!(output["result"]["isError"], true, "{output}");
    assert_no_plaintext(root.path());
    drop(router);
    database.close().await.unwrap();
}
