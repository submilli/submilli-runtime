//! End-to-end coverage of the stateful REST session API: `POST /v1/sessions`
//! (bind blueprint + variables) → `POST /v1/sessions/{id}/execute` (code only) →
//! `DELETE /v1/sessions/{id}`. This is the REST analog of the MCP session model:
//! bind once at create, run many executes against the same per-session VFS, then
//! terminate.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use interpreter::{ModulePath, PackageSourceModule, compile_package};
use serde_json::{Value, json};
use submilli_build::{ArtifactMetadata, write_package_artifact};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

const BLUEPRINT: &str = "session-api";
const SECRET_BLUEPRINT: &str = "secret-api";

/// A `per_session` sandbox scoped to the caller's own files via a
/// `${vars.user_id}` filename-prefix filter — exercises both per-session VFS
/// persistence and session-bound variable scoping. (The prefix lives in the
/// filename so the parent dir, root, always exists — `writeText` doesn't create
/// directories.)
const POLICY: &str = "\
name: session-api
default: deny
vfs: per_session
variables:
  user_id:
    required: true
permissions:
  main:
    - capability: fs.write
      filter: path glob \"/${vars.user_id}-*\"
      action: allow
    - capability: fs.read
      filter: path glob \"/${vars.user_id}-*\"
      action: allow
";

fn router() -> Router {
    let blueprint = submilli_blueprint::parse(POLICY).expect("valid blueprint");
    let blueprints = Arc::new(InMemoryBlueprintStore::seed([blueprint]).expect("seed blueprints"));
    let config = ServerConfig {
        blueprints: Some(blueprints),
        session_storage_root: Some(tempfile::tempdir().expect("sessions").keep()),
        ..in_memory_config::config()
    };
    app(futures::executor::block_on(AppState::new(config)).expect("build AppState"))
}

/// Reports whether the bound `TOKEN` matches what the caller expected, without
/// handing the value back. The indirection is deliberate: a fixture that
/// returned the plaintext to `main` would sit here as a copy-paste template for
/// the exact shape the secrets carve-out exists to prevent.
const SECRET_PACKAGE_SOURCE: &str = r#"
import { get } from "submilli:secrets";

/** True when the bound TOKEN resolves to exactly `expected`. */
export function tokenMatches(expected: string): boolean {
    const value = get("TOKEN");
    return value !== null && value === expected;
}
"#;

fn write_secret_package(store_root: &Path) {
    let package = compile_package(
        "@acme/secrets",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: SECRET_PACKAGE_SOURCE,
        }],
        &[],
    )
    .expect("compile @acme/secrets");
    write_package_artifact(
        store_root.join("@acme").join("secrets"),
        &package.wasm,
        &package.type_info,
        &submilli_build::derive_capability_schema(
            &package.declaration,
            &[],
            &package.required_capabilities,
        ),
        &package.declaration,
        &ArtifactMetadata::new("@acme/secrets", "0.0.0-test", Vec::new()),
    )
    .expect("write @acme/secrets artifact");
}

fn secret_router(store_root: &Path) -> Router {
    let blueprint = submilli_blueprint::parse(
        "name: secret-api\ndefault: deny\npackages:\n  - \"@acme/secrets\"\nsecrets:\n  TOKEN:\n    harness:\n      required: true\npermissions:\n  \"@acme/secrets\":\n    - capability: secrets.get\n      filter: name == \"TOKEN\"\n      action: allow\n",
    )
    .expect("valid blueprint");
    let blueprints = Arc::new(InMemoryBlueprintStore::seed([blueprint]).expect("seed blueprints"));
    app(futures::executor::block_on(AppState::new(ServerConfig {
        blueprints: Some(blueprints),
        package_store_root: Some(store_root.to_path_buf()),
        ..in_memory_config::config()
    }))
    .expect("build AppState"))
}

fn write_to(path: &str) -> String {
    format!(
        r#"import {{ writeText }} from "submilli:fs"; function main(): void {{ writeText("{path}", "hi"); }}"#
    )
}

fn read_from(path: &str) -> String {
    format!(
        r#"import {{ readText }} from "submilli:fs"; function main(): string | null {{ return readText("{path}"); }}"#
    )
}

async fn send(router: &Router, req: Request<Body>) -> (StatusCode, Value) {
    let resp = router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

async fn create(router: &Router, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/sessions")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    send(router, req).await
}

async fn session_execute(router: &Router, session: &str, code: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(format!("/v1/sessions/{session}/execute"))
        .header("content-type", "application/json")
        .body(Body::from(json!({ "code": code }).to_string()))
        .unwrap();
    send(router, req).await
}

async fn rebind(router: &Router, session: &str, secrets: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(format!("/v1/sessions/{session}/rebind"))
        .header("content-type", "application/json")
        .body(Body::from(json!({ "secrets": secrets }).to_string()))
        .unwrap();
    send(router, req).await
}

async fn last_run(router: &Router, session: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/sessions/{session}/last-run"))
        .body(Body::empty())
        .unwrap();
    send(router, req).await
}

async fn delete(router: &Router, session: &str) -> StatusCode {
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/v1/sessions/{session}"))
        .body(Body::empty())
        .unwrap();
    send(router, req).await.0
}

#[tokio::test]
async fn full_lifecycle_create_execute_delete() {
    let router = router();

    // Create binds the blueprint + variables once and returns an id.
    let (status, created) = create(
        &router,
        json!({ "blueprint": BLUEPRINT, "variables": { "user_id": "alice" } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create failed: {created}");
    let session = created["session_id"]
        .as_str()
        .expect("session_id")
        .to_string();

    // First execute writes into the per-session VFS, scoped by the bound variable.
    let (status, w) = session_execute(&router, &session, &write_to("/alice-a.txt")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(w["error"].is_null(), "write should be allowed: {w}");

    // Second execute reads it back — the per-session VFS persists across executes
    // (the parity guarantee with the MCP session model).
    let (_, r) = session_execute(&router, &session, &read_from("/alice-a.txt")).await;
    assert_eq!(
        r["result"],
        json!("hi"),
        "file should persist across executes"
    );

    // The full console/result is recoverable via last-run.
    let (status, lr) = last_run(&router, &session).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(lr["result"], json!("hi"));

    // Delete terminates the session; a subsequent execute 404s.
    assert_eq!(delete(&router, &session).await, StatusCode::NO_CONTENT);
    let (status, _) = session_execute(&router, &session, &read_from("/alice-a.txt")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "execute on a wiped session 404s"
    );
    assert_eq!(
        delete(&router, &session).await,
        StatusCode::NOT_FOUND,
        "a second delete 404s"
    );
}

#[tokio::test]
async fn bound_variable_scopes_the_filter() {
    let router = router();
    let (status, created) = create(
        &router,
        json!({ "blueprint": BLUEPRINT, "variables": { "user_id": "alice" } }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create failed: {created}");
    let session = created["session_id"].as_str().unwrap().to_string();

    // Writing outside the bound user's subtree misses the filter and falls
    // through to `default: deny` — the check is denied. This proves the variable
    // threaded from create into the policy, not an allow-all.
    let (status, r) = session_execute(&router, &session, &write_to("/bob-a.txt")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(r["result"], Value::Null);
    assert_eq!(r["error"]["kind"], json!("permission_denied"), "got: {r}");
}

#[tokio::test]
async fn create_missing_required_variable_is_400() {
    let router = router();
    let (status, body) = create(&router, json!({ "blueprint": BLUEPRINT })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("user_id"),
        "error should name the missing variable: {body}"
    );
}

#[tokio::test]
async fn create_unknown_variable_is_400() {
    let router = router();
    let (status, _) = create(
        &router,
        json!({ "blueprint": BLUEPRINT, "variables": { "user_id": "alice", "bogus": "x" } }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn create_unknown_blueprint_is_404() {
    let router = router();
    let (status, _) = create(&router, json!({ "blueprint": "nope" })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn execute_unknown_session_is_404() {
    let router = router();
    let (status, _) = session_execute(&router, "does-not-exist", &read_from("/alice-a.txt")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn harness_secret_create_and_rebind() {
    let store = tempfile::tempdir().expect("store tempdir");
    write_secret_package(store.path());
    let router = secret_router(store.path());

    let (status, body) = create(&router, json!({ "blueprint": SECRET_BLUEPRINT })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("TOKEN"));

    let (_, created) = create(
        &router,
        json!({ "blueprint": SECRET_BLUEPRINT, "secrets": { "TOKEN": "first" } }),
    )
    .await;
    let session = created["session_id"].as_str().unwrap();

    let (_, bound) = session_execute(&router, session, &probe("first")).await;
    assert_eq!(bound["result"], json!("true"), "body: {bound}");
    let (_, stale) = session_execute(&router, session, &probe("second")).await;
    assert_eq!(stale["result"], json!("false"), "body: {stale}");

    let (status, _) = rebind(&router, session, json!({ "TOKEN": "second" })).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (_, rebound) = session_execute(&router, session, &probe("second")).await;
    assert_eq!(rebound["result"], json!("true"), "body: {rebound}");
    let (_, previous) = session_execute(&router, session, &probe("first")).await;
    assert_eq!(previous["result"], json!("false"), "body: {previous}");

    let (status, body) = rebind(&router, session, json!({ "OTHER": "nope" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("OTHER"));
}

fn probe(expected: &str) -> String {
    format!(
        r#"import {{ tokenMatches }} from "@acme/secrets";
function main(): boolean {{ return tokenMatches("{expected}"); }}"#
    )
}
