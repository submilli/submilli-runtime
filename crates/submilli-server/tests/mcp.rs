//! End-to-end tests for the MCP streamable-HTTP transport, driven in-process
//! through `app(state).oneshot(...)` (no real port), mirroring the REST tests.
//!
//! Every blueprint runs stateful, so each interaction does the `initialize` /
//! `notifications/initialized` handshake and echoes the `MCP-Session-Id`.
//! Responses are SSE, so the helper parses the `data:` frame.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use interpreter::mangle::package_symbol;
use interpreter::{FileId, PackageDeclaration, Param, Span, Type, ValueKind, ValueSymbol};
use serde_json::{Value, json};
use submilli_blueprint::{Action, Blueprint, McpServer, PermissionRule, VfsConfig};
use submilli_build::{
    ArtifactMetadata, CapabilitySchema, PackageStore, write_package_artifact_with_docs,
};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::config::{VolumeSpec, VolumeTable};
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

#[path = "common/last_run_store.rs"]
mod last_run_store;

const EPH: &str = "eph";
const SESS: &str = "sess";
const MCP: &str = "mcp-bp";
const NO_VFS: &str = "novfs";

/// Grant the fs capabilities the WRITE/READ scripts use. The server is
/// deny-by-default, so without these the writes/reads would be denied.
fn allow_fs() -> BTreeMap<String, Vec<PermissionRule>> {
    let rules = ["fs.read", "fs.write", "fs.mkdir", "fs.list"]
        .into_iter()
        .map(|cap| PermissionRule {
            name: None,
            capability: cap.into(),
            filter: None,
            action: Action::Allow,
        })
        .collect();
    BTreeMap::from([("main".to_string(), rules)])
}

struct Harness {
    state: AppState,
    session_root: std::path::PathBuf,
    _owned_session_root: Option<tempfile::TempDir>,
    _package_store_root: Option<tempfile::TempDir>,
}

impl Harness {
    /// Build a harness over an arbitrary blueprint set (used by tests that point
    /// an `mcp:` server at a mock upstream on a dynamic port).
    fn from_blueprints(bps: Vec<Blueprint>) -> Self {
        Self::from_blueprints_with_sessions(bps, None)
    }

    fn from_blueprints_with_sessions(
        bps: Vec<Blueprint>,
        sessions: Option<Arc<dyn submilli_server::session::SessionStore>>,
    ) -> Self {
        let session_root = tempfile::tempdir().expect("session root");
        let session_root_path = session_root.path().to_path_buf();
        let blueprints = Arc::new(InMemoryBlueprintStore::seed(bps).expect("seed blueprints"));
        let config = ServerConfig {
            blueprints: Some(blueprints),
            sessions,
            session_storage_root: Some(session_root_path.clone()),
            ..in_memory_config::config()
        };
        Self {
            state: AppState::new(config).expect("AppState"),
            session_root: session_root_path,
            _owned_session_root: Some(session_root),
            _package_store_root: None,
        }
    }

    /// Like [`Harness::from_blueprints`] but with a declared volume table, for
    /// the sessionless mount route.
    fn from_blueprints_with_volumes(bps: Vec<Blueprint>, volumes: VolumeTable) -> Self {
        let session_root = tempfile::tempdir().expect("session root");
        let session_root_path = session_root.path().to_path_buf();
        let blueprints = Arc::new(InMemoryBlueprintStore::seed(bps).expect("seed blueprints"));
        let config = ServerConfig {
            blueprints: Some(blueprints),
            session_storage_root: Some(session_root_path.clone()),
            volumes,
            ..in_memory_config::config()
        };
        Self {
            state: AppState::new(config).expect("AppState"),
            session_root: session_root_path,
            _owned_session_root: Some(session_root),
            _package_store_root: None,
        }
    }

    fn from_blueprints_with_session_paths(
        bps: Vec<Blueprint>,
        session_root: std::path::PathBuf,
        session_store_dir: std::path::PathBuf,
    ) -> Self {
        let blueprints = Arc::new(InMemoryBlueprintStore::seed(bps).expect("seed blueprints"));
        let config = ServerConfig {
            blueprints: Some(blueprints),
            session_storage_root: Some(session_root.clone()),
            session_store_dir: Some(session_store_dir),
            ..in_memory_config::config()
        };
        Self {
            state: AppState::new(config).expect("AppState"),
            session_root,
            _owned_session_root: None,
            _package_store_root: None,
        }
    }

    fn from_blueprints_and_packages(
        bps: Vec<Blueprint>,
        package_store_root: tempfile::TempDir,
    ) -> Self {
        let session_root = tempfile::tempdir().expect("session root");
        let session_root_path = session_root.path().to_path_buf();
        let blueprints = Arc::new(InMemoryBlueprintStore::seed(bps).expect("seed blueprints"));
        let config = ServerConfig {
            blueprints: Some(blueprints),
            session_storage_root: Some(session_root_path.clone()),
            package_store_root: Some(package_store_root.path().to_path_buf()),
            ..in_memory_config::config()
        };
        Self {
            state: AppState::new(config).expect("AppState"),
            session_root: session_root_path,
            _owned_session_root: Some(session_root),
            _package_store_root: Some(package_store_root),
        }
    }

    fn new() -> Self {
        let session_root = tempfile::tempdir().expect("session root");
        let session_root_path = session_root.path().to_path_buf();
        let blueprints = Arc::new(
            InMemoryBlueprintStore::seed([
                Blueprint {
                    name: EPH.into(),
                    vfs: VfsConfig::Ephemeral {
                        size_limit: None,
                        mounts: Default::default(),
                        cwd: None,
                    },
                    permissions: allow_fs(),
                    ..Default::default()
                },
                Blueprint {
                    name: SESS.into(),
                    vfs: VfsConfig::PerSession {
                        size_limit: None,
                        mounts: Default::default(),
                        cwd: None,
                    },
                    permissions: allow_fs(),
                    ..Default::default()
                },
                Blueprint {
                    name: NO_VFS.into(),
                    vfs: VfsConfig::None,
                    permissions: allow_fs(),
                    ..Default::default()
                },
                Blueprint {
                    name: MCP.into(),
                    mcp: BTreeMap::from([(
                        "linear".to_string(),
                        McpServer {
                            transport: "streamable_http".into(),
                            url: "https://mcp.linear.app/mcp".into(),
                            headers: BTreeMap::new(),
                            auth: None,
                        },
                    )]),
                    ..Default::default()
                },
            ])
            .expect("seed blueprints"),
        );
        let config = ServerConfig {
            blueprints: Some(blueprints),
            session_storage_root: Some(session_root_path.clone()),
            ..in_memory_config::config()
        };
        Self {
            state: AppState::new(config).expect("AppState"),
            session_root: session_root_path,
            _owned_session_root: Some(session_root),
            _package_store_root: None,
        }
    }

    /// POST a JSON-RPC message to `/mcp/{blueprint}`, returning the status, the
    /// response headers, and the parsed JSON-RPC body (`Null` for an empty
    /// 202).
    async fn post(
        &self,
        blueprint: &str,
        body: Value,
        session: Option<&str>,
    ) -> (StatusCode, HeaderMap, Value) {
        let mut builder = Request::builder()
            .method("POST")
            .uri(format!("/mcp/{blueprint}"))
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        if let Some(s) = session {
            builder = builder.header("mcp-session-id", s);
        }
        self.send(builder.body(Body::from(body.to_string())).unwrap())
            .await
    }

    async fn delete(&self, blueprint: &str, session: &str) -> StatusCode {
        let req = Request::builder()
            .method("DELETE")
            .uri(format!("/mcp/{blueprint}"))
            .header("host", "localhost")
            .header("mcp-session-id", session)
            .body(Body::empty())
            .unwrap();
        self.send(req).await.0
    }

    /// `PUT /v1/blueprints/{name}` — create-or-replace a blueprint from YAML.
    async fn put_blueprint(&self, name: &str, yaml: &str) -> StatusCode {
        let req = Request::builder()
            .method("PUT")
            .uri(format!("/v1/blueprints/{name}"))
            .header("host", "localhost")
            .header("content-type", "application/json")
            .body(Body::from(json!({ "yaml": yaml }).to_string()))
            .unwrap();
        self.send(req).await.0
    }

    /// `DELETE /v1/blueprints/{name}` — unregister a blueprint.
    async fn delete_blueprint(&self, name: &str) -> StatusCode {
        let req = Request::builder()
            .method("DELETE")
            .uri(format!("/v1/blueprints/{name}"))
            .header("host", "localhost")
            .body(Body::empty())
            .unwrap();
        self.send(req).await.0
    }

    async fn send(&self, req: Request<Body>) -> (StatusCode, HeaderMap, Value) {
        let resp = app(self.state.clone()).oneshot(req).await.unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let content_type = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body = parse_body(&content_type, &bytes);
        (status, headers, body)
    }

    /// Stateful handshake: `initialize` then `notifications/initialized`,
    /// returning the assigned `MCP-Session-Id`.
    async fn handshake(&self, blueprint: &str) -> String {
        let (status, headers, _) = self.post(blueprint, initialize(), None).await;
        assert_eq!(status, StatusCode::OK, "initialize failed");
        let session = headers
            .get("mcp-session-id")
            .expect("per_session initialize must return a session id")
            .to_str()
            .unwrap()
            .to_string();
        let (status, _, _) = self.post(blueprint, initialized(), Some(&session)).await;
        assert_eq!(status, StatusCode::ACCEPTED, "initialized notification");
        session
    }
}

/// Parse a streamable-HTTP response body: `application/json` directly, or the
/// last `data:` payload of an SSE frame.
fn parse_body(content_type: &str, bytes: &[u8]) -> Value {
    if bytes.is_empty() {
        return Value::Null;
    }
    if content_type.starts_with("text/event-stream") {
        let text = String::from_utf8_lossy(bytes);
        let data = text
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .next_back()
            .unwrap_or("")
            .trim();
        return serde_json::from_str(data).unwrap_or(Value::Null);
    }
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

fn initialize() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "0" }
        }
    })
}

fn initialized() -> Value {
    json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })
}

fn rpc_call(id: u32, name: &str, arguments: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": name, "arguments": arguments }
    })
}

fn tools_list(id: u32) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/list" })
}

fn tools_call(id: u32, code: &str) -> Value {
    rpc_call(id, EXECUTE, json!({ "code": code }))
}

const SUM: &str = "function main(): number { return 1 + 1; }";
const WRITE: &str = r#"import { writeText } from "submilli:fs"; function main(): void { writeText("/a.txt", "hi"); }"#;
const READ: &str = r#"import { readText } from "submilli:fs"; function main(): string | null { return readText("/a.txt"); }"#;

/// The `{ result, console, error }` payload our tool puts in `structuredContent`.
fn output(rpc: &Value) -> &Value {
    &rpc["result"]["structuredContent"]
}

fn text_output(rpc: &Value) -> &str {
    rpc["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("missing text content in {rpc}"))
}

fn installed_test_package(name: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("package store root");
    let store = PackageStore::new(root.path());
    let dir = store.package_dir(name).expect("package dir");
    let metadata = ArtifactMetadata::with_description(
        name,
        "0.1.0",
        "Jina AI readers and search helpers for LLM web workflows.",
        vec![
            "reader".to_string(),
            "search".to_string(),
            "web".to_string(),
        ],
        Vec::new(),
    );
    write_package_artifact_with_docs(
        dir,
        &[],
        &interpreter::TypeInfoTable {
            package_name: name.to_string(),
            types: Vec::new(),
        },
        &CapabilitySchema::default(),
        &test_package_declaration(name),
        &metadata,
        "# Jina\n\nUse Jina to read and search web content for LLM workflows.\n",
    )
    .expect("write package artifact");
    root
}

fn test_package_declaration(name: &str) -> PackageDeclaration {
    let mut defs = PackageDeclaration::with_package(name);
    defs.values.insert(
        "read".to_string(),
        ValueSymbol {
            name: "read".to_string(),
            mangled_name: package_symbol(name, "read"),
            declaration_span: Span::at(FileId(0)),
            kind: ValueKind::Function {
                generics: Vec::new(),
                params: vec![Param::new("url", Type::String)],
                ret: Type::String,
                type_predicate: None,
                doc: None,
            },
        },
    );
    defs
}

/// The description of a named tool from a `tools/list` response (the tool order
/// isn't guaranteed, so look it up by name).
fn tool_desc<'a>(rpc: &'a Value, name: &str) -> &'a str {
    rpc["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .find(|t| t["name"] == json!(name))
        .and_then(|t| t["description"].as_str())
        .unwrap_or_else(|| panic!("no description for {name} in {rpc}"))
}

const EXECUTE: &str = "submilli__typescript__execute";

#[tokio::test]
async fn execute_returns_result() {
    let h = Harness::new();
    let session = h.handshake(EPH).await;
    let (status, _, rpc) = h.post(EPH, tools_call(1, SUM), Some(&session)).await;
    assert_eq!(status, StatusCode::OK);
    let out = output(&rpc);
    assert_eq!(out["result"], json!("2"), "got: {rpc}");
    assert_eq!(out["console"], json!([]));
    assert!(out["error"].is_null());
}

/// Reaching the memory cap is reported under its own kind, which the program's
/// `catch` cannot turn into an ordinary result, and leaves the session usable.
#[tokio::test]
async fn execute_reports_memory_exhaustion_as_its_own_kind() {
    const OVER_THE_CAP: &str = r#"export function main(): string {
        let s = "x";
        try {
            for (let i = 0; i < 25; i++) { s = s + s; }
        } catch (e) {
            return "caught";
        }
        return `len ${s.length}`;
    }"#;
    let h = Harness::new();
    let session = h.handshake(EPH).await;

    let (status, _, rpc) = h
        .post(EPH, tools_call(1, OVER_THE_CAP), Some(&session))
        .await;
    assert_eq!(status, StatusCode::OK);
    let out = output(&rpc);
    assert!(out["result"].is_null(), "got: {rpc}");
    assert_eq!(
        out["error"]["kind"],
        json!("memory_exhausted"),
        "got: {rpc}"
    );

    let (_, _, rpc) = h.post(EPH, tools_call(2, SUM), Some(&session)).await;
    assert!(output(&rpc)["error"].is_null(), "got: {rpc}");
    assert_eq!(output(&rpc)["result"], json!("2"), "got: {rpc}");
}

/// A URL with a dot segment is refused as an ordinary `TypeError`, not as an
/// internal failure, and leaves the session usable.
#[tokio::test]
async fn execute_reports_http_dot_segment_as_a_runtime_error() {
    const DOT_SEGMENT: &str = r#"import { get } from "submilli:http";
        function main(): void { get("https://example.com/customers/../admin"); }"#;
    let h = Harness::new();
    let session = h.handshake(EPH).await;

    let (status, _, rpc) = h
        .post(EPH, tools_call(1, DOT_SEGMENT), Some(&session))
        .await;
    assert_eq!(status, StatusCode::OK);
    let out = output(&rpc);
    assert_eq!(out["error"]["kind"], json!("runtime_error"), "got: {rpc}");
    let message = out["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("TypeError") && message.contains("dot segment \"..\""),
        "got: {rpc}"
    );

    let (_, _, rpc) = h.post(EPH, tools_call(2, SUM), Some(&session)).await;
    assert!(output(&rpc)["error"].is_null(), "got: {rpc}");
    assert_eq!(output(&rpc)["result"], json!("2"), "got: {rpc}");
}

// ---- Session variables (`${vars.NAME}` bound at initialize) --------------

const VARBP: &str = "varbp";

fn var_blueprint() -> Blueprint {
    submilli_blueprint::parse(
        "name: varbp\n\
         default: deny\n\
         variables:\n  tenant:\n    required: true\n\
         permissions:\n  main:\n    - capability: test.com/op\n      \
         filter: userId == ${vars.tenant}\n      action: allow\n",
    )
    .expect("valid var blueprint")
}

/// Asks for `test.com/op` carrying a `userId` the `${vars.tenant}` filter scopes.
const VAR_CHECK: &str = r#"import { check } from "submilli:security";
function main(): number { check("test.com/op", { userId: "u_42" }); return 1; }"#;

fn initialize_with_vars(variables: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "test", "version": "0" },
            "_meta": { "variables": variables }
        }
    })
}

async fn handshake_with_vars(h: &Harness, blueprint: &str, variables: Value) -> String {
    let (status, headers, _) = h
        .post(blueprint, initialize_with_vars(variables), None)
        .await;
    assert_eq!(status, StatusCode::OK, "initialize failed");
    let session = headers
        .get("mcp-session-id")
        .expect("initialize must return a session id")
        .to_str()
        .unwrap()
        .to_string();
    let (status, _, _) = h.post(blueprint, initialized(), Some(&session)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "initialized notification");
    session
}

#[tokio::test]
async fn mcp_variable_bound_at_initialize_scopes_filter() {
    let h = Harness::from_blueprints(vec![var_blueprint()]);
    let session = handshake_with_vars(&h, VARBP, json!({ "tenant": "u_42" })).await;
    let (status, _, rpc) = h
        .post(VARBP, tools_call(1, VAR_CHECK), Some(&session))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(output(&rpc)["result"], json!("1"), "got: {rpc}");
    assert!(output(&rpc)["error"].is_null(), "got: {rpc}");
}

/// Handshake binding variables via the `Submilli-Variables` header (`key=value`)
/// instead of `_meta` — the channel for clients (langchain, the stock Python SDK)
/// that can't set `initialize` `_meta`.
async fn handshake_with_header(h: &Harness, blueprint: &str, vars_header: &str) -> String {
    let req = Request::builder()
        .method("POST")
        .uri(format!("/mcp/{blueprint}"))
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("submilli-variables", vars_header)
        .body(Body::from(initialize().to_string()))
        .unwrap();
    let (status, headers, _) = h.send(req).await;
    assert_eq!(status, StatusCode::OK, "header initialize failed");
    let session = headers
        .get("mcp-session-id")
        .expect("initialize must return a session id")
        .to_str()
        .unwrap()
        .to_string();
    let (status, _, _) = h.post(blueprint, initialized(), Some(&session)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "initialized notification");
    session
}

#[tokio::test]
async fn mcp_variable_bound_via_header() {
    let h = Harness::from_blueprints(vec![var_blueprint()]);
    // No `_meta`; the binding rides the Submilli-Variables header.
    let session = handshake_with_header(&h, VARBP, "tenant=u_42").await;
    let (status, _, rpc) = h
        .post(VARBP, tools_call(1, VAR_CHECK), Some(&session))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(output(&rpc)["result"], json!("1"), "got: {rpc}");
    assert!(output(&rpc)["error"].is_null(), "got: {rpc}");
}

#[tokio::test]
async fn mcp_mismatched_variable_denies_check() {
    let h = Harness::from_blueprints(vec![var_blueprint()]);
    let session = handshake_with_vars(&h, VARBP, json!({ "tenant": "u_99" })).await;
    let (status, _, rpc) = h
        .post(VARBP, tools_call(1, VAR_CHECK), Some(&session))
        .await;
    assert_eq!(status, StatusCode::OK);
    // Bound tenant differs from the script's userId, so the check is denied.
    assert!(output(&rpc)["result"].is_null(), "got: {rpc}");
    assert!(
        !output(&rpc)["error"].is_null(),
        "denied check should error: {rpc}"
    );
}

fn user_dir_blueprint() -> Blueprint {
    submilli_blueprint::parse(
        "name: userdir\n\
         vfs: per_session\n\
         default: deny\n\
         variables:\n  user_id:\n    required: true\n\
         permissions:\n  main:\n    - capability: fs.stat\n      \
         filter: path glob \"/users/${vars.user_id}/*\"\n      action: allow\n",
    )
    .expect("valid user-dir blueprint")
}

fn exists_script(path: &str) -> String {
    format!(
        r#"import {{ exists }} from "submilli:fs"; function main(): boolean {{ return exists("{path}"); }}"#
    )
}

#[tokio::test]
async fn mcp_per_user_directory_scoping() {
    // The killer fs case: a glob pattern interpolating the session's user id, so
    // the code can only touch its own subtree. `exists` is enough to show the
    // capability check (no file needs to be there) — it returns false, not a trap.
    let h = Harness::from_blueprints(vec![user_dir_blueprint()]);
    let session = handshake_with_vars(&h, "userdir", json!({ "user_id": "u_42" })).await;

    // Own directory: the capability check passes (result is the bool, no error).
    let (_, _, own) = h
        .post(
            "userdir",
            tools_call(1, &exists_script("/users/u_42/note.txt")),
            Some(&session),
        )
        .await;
    assert!(
        output(&own)["error"].is_null(),
        "own-directory access must be allowed: {own}"
    );
    assert_eq!(output(&own)["result"], json!("false"), "got: {own}");

    // Another user's directory: denied → the check traps.
    let (_, _, other) = h
        .post(
            "userdir",
            tools_call(2, &exists_script("/users/u_99/note.txt")),
            Some(&session),
        )
        .await;
    assert!(
        !output(&other)["error"].is_null(),
        "cross-user access must be denied: {other}"
    );
}

/// Post an `initialize` binding variables via the `Submilli-Variables` header,
/// returning the raw status/headers/body (unlike `handshake_with_header`, which
/// asserts success). Used to check the rejection path.
async fn init_with_header(
    h: &Harness,
    blueprint: &str,
    vars_header: &str,
) -> (StatusCode, HeaderMap, String) {
    let req = Request::builder()
        .method("POST")
        .uri(format!("/mcp/{blueprint}"))
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("submilli-variables", vars_header)
        .body(Body::from(initialize().to_string()))
        .unwrap();
    let resp = app(h.state.clone()).oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        headers,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

#[tokio::test]
async fn mcp_missing_required_variable_fails_initialize() {
    let h = Harness::from_blueprints(vec![var_blueprint()]);
    // No `_meta.variables`: the required `tenant` is unmet. The handler rejects it
    // with a 400 (not rmcp's unlogged 500) and opens no session.
    let (status, headers, _) = h.post(VARBP, initialize(), None).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a missing required variable must be a 400"
    );
    assert!(
        headers.get("mcp-session-id").is_none(),
        "a rejected initialize must not open a session"
    );
}

#[tokio::test]
async fn mcp_undeclared_variable_rejected_with_diagnostic() {
    let h = Harness::from_blueprints(vec![var_blueprint()]);
    // `var_blueprint` declares `tenant`, not `user_id`. Supplying `user_id`
    // (the field-demo failure) is a 400 whose body names the offending variable.
    let (status, headers, body) = init_with_header(&h, VARBP, "tenant=u_42;user_id=demo").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        headers.get("mcp-session-id").is_none(),
        "a rejected initialize must not open a session"
    );
    assert!(
        body.contains("user_id") && body.contains("not declared"),
        "body should name the undeclared variable: {body:?}"
    );
}

fn reload_blueprint_v1() -> Blueprint {
    submilli_blueprint::parse(
        "name: reloadbp\n\
         default: deny\n\
         permissions:\n  main:\n    - capability: test.com/op\n      action: allow\n",
    )
    .expect("valid reload blueprint")
}

const RELOAD_BP_V2: &str = "name: reloadbp\n\
     default: deny\n\
     variables:\n  user_id:\n    default: demo\n\
     permissions:\n  main:\n    - capability: test.com/op\n      action: allow\n";

#[tokio::test]
async fn mcp_apply_variable_change_seen_without_eviction() {
    // `apply` deliberately keeps the live MCP service (and its sessions) alive, so
    // the fix must re-fetch the blueprint at `initialize` rather than trust the
    // snapshot the service was built with — otherwise a freshly-declared variable
    // stays unrecognised until the service is evicted.
    let h = Harness::from_blueprints(vec![reload_blueprint_v1()]);

    // Open a session so the service is built and cached under `reloadbp`.
    let _ = h.handshake("reloadbp").await;

    // V1 declares no variables: binding `user_id` is rejected.
    let (before, _, _) = init_with_header(&h, "reloadbp", "user_id=demo").await;
    assert_eq!(before, StatusCode::BAD_REQUEST, "user_id undeclared in v1");

    // Apply a version that declares `user_id`. The cached service is NOT evicted.
    assert_eq!(
        h.put_blueprint("reloadbp", RELOAD_BP_V2).await,
        StatusCode::OK
    );

    // The same cached service now accepts `user_id` — the edit was reloaded.
    let (after, headers, _) = init_with_header(&h, "reloadbp", "user_id=demo").await;
    assert_eq!(after, StatusCode::OK, "user_id declared after apply");
    assert!(
        headers.get("mcp-session-id").is_some(),
        "a successful initialize must open a session"
    );
}

#[tokio::test]
async fn execute_rejects_blueprint_field() {
    // The blueprint is bound per connection (the `/mcp/{blueprint}` endpoint),
    // so the execute tool's schema has no `blueprint` field. Passing one is
    // rejected rather than silently ignored.
    let h = Harness::new();
    let session = h.handshake(EPH).await;
    let args = json!({ "code": SUM, "blueprint": "sess" });
    let (status, _, rpc) = h
        .post(EPH, rpc_call(1, EXECUTE, args), Some(&session))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !rpc["error"].is_null() || rpc["result"]["isError"] == json!(true),
        "passing a blueprint field should be rejected: {rpc}"
    );
}

#[tokio::test]
async fn mcp_block_surfaces_through_binding() {
    // A blueprint carrying an `mcp:` block binds per connection (the
    // `/mcp/{blueprint}` endpoint) and still serves the execute tool — the
    // parsed block reaches the binding through the bound blueprint without
    // breaking it. `@mcp/<server>` enumeration in the description lands in a
    // later slice.
    let h = Harness::new();
    let session = h.handshake(MCP).await;
    let (status, _, rpc) = h.post(MCP, tools_list(1), Some(&session)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(tool_desc(&rpc, EXECUTE).contains("strict TypeScript subset"));
}

#[tokio::test]
async fn every_blueprint_issues_session_id() {
    let h = Harness::new();
    // All blueprints run stateful now, so even an ephemeral one assigns a
    // session id at initialize — the key `lastRun` stores its run under.
    for bp in [EPH, SESS] {
        let (status, headers, _) = h.post(bp, initialize(), None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            headers.get("mcp-session-id").is_some(),
            "{bp} should issue a session id"
        );
    }
}

#[tokio::test]
async fn per_session_rejects_missing_session_id() {
    let h = Harness::new();
    // A tools/call with no session id under a stateful endpoint is rejected:
    // rmcp treats the first session-less message as a failed handshake (it
    // isn't `initialize`) and returns 422 Unprocessable Entity. Either way the
    // call cannot run without a session.
    let (status, _, _) = h.post(SESS, tools_call(2, SUM), None).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn per_session_unknown_session_is_404() {
    let h = Harness::new();
    let (status, _, _) = h
        .post(SESS, tools_call(2, SUM), Some("not-a-real-session"))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unknown_blueprint_is_404() {
    let h = Harness::new();
    let (status, _, _) = h.post("does-not-exist", initialize(), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn per_session_persists_across_calls() {
    let h = Harness::new();
    let session = h.handshake(SESS).await;

    let (_, _, w) = h.post(SESS, tools_call(2, WRITE), Some(&session)).await;
    assert!(output(&w)["error"].is_null(), "write failed: {w}");

    let (_, _, r) = h.post(SESS, tools_call(3, READ), Some(&session)).await;
    assert_eq!(output(&r)["result"], json!("hi"), "file should persist");
}

#[tokio::test]
async fn mcp_session_restores_after_server_restart() {
    let session_root = tempfile::tempdir().expect("session root");
    let session_store = tempfile::tempdir().expect("session store");
    let blueprints = || {
        vec![Blueprint {
            name: SESS.into(),
            vfs: VfsConfig::PerSession {
                size_limit: None,
                mounts: Default::default(),
                cwd: None,
            },
            permissions: allow_fs(),
            ..Default::default()
        }]
    };

    let first = Harness::from_blueprints_with_session_paths(
        blueprints(),
        session_root.path().to_path_buf(),
        session_store.path().to_path_buf(),
    );
    let session = first.handshake(SESS).await;
    let (_, _, w) = first.post(SESS, tools_call(2, WRITE), Some(&session)).await;
    assert!(output(&w)["error"].is_null(), "write failed: {w}");
    drop(first);

    let restarted = Harness::from_blueprints_with_session_paths(
        blueprints(),
        session_root.path().to_path_buf(),
        session_store.path().to_path_buf(),
    );
    restarted.state.boot().await.expect("boot");

    let (status, _, r) = restarted
        .post(SESS, tools_call(3, READ), Some(&session))
        .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "old MCP session id should restore after restart: {r}"
    );
    assert_eq!(
        output(&r)["result"],
        json!("hi"),
        "workspace must survive restart: {r}"
    );
}

/// `sess` (per_session + fs) re-applied with an added `http.get` allow rule —
/// keeps the same vfs mode so its workspace survives, but flips `{http_access}`
/// in the execute tool description from omitted to "GET → any host".
const SESS_PLUS_HTTP: &str = "name: sess\n\
    vfs: per_session\n\
    permissions:\n  \
    main:\n    \
    - capability: fs.read\n      action: allow\n    \
    - capability: fs.write\n      action: allow\n    \
    - capability: fs.mkdir\n      action: allow\n    \
    - capability: http.get\n      action: allow\n";

#[tokio::test]
async fn session_survives_blueprint_update() {
    let h = Harness::new();
    let session = h.handshake(SESS).await;

    // Before the update the bound blueprint blocks http.
    let (_, _, before) = h.post(SESS, tools_list(1), Some(&session)).await;
    assert!(
        !tool_desc(&before, EXECUTE).contains("submilli:http"),
        "baseline description should omit http: {before}"
    );

    // Write a file into the per_session workspace, then update the blueprint.
    let (_, _, w) = h.post(SESS, tools_call(2, WRITE), Some(&session)).await;
    assert!(output(&w)["error"].is_null(), "write failed: {w}");
    assert_eq!(h.put_blueprint(SESS, SESS_PLUS_HTTP).await, StatusCode::OK);

    // The same session id is still live (no re-handshake) and its workspace file
    // survived — the service was not torn down.
    let (status, _, r) = h.post(SESS, tools_call(3, READ), Some(&session)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "session must survive the update: {r}"
    );
    assert_eq!(
        output(&r)["result"],
        json!("hi"),
        "workspace must persist: {r}"
    );

    // ...and the tool description now reflects the updated policy.
    let (_, _, after) = h.post(SESS, tools_list(4), Some(&session)).await;
    assert!(
        tool_desc(&after, EXECUTE).contains("GET → any host"),
        "description should reflect the updated http policy: {after}"
    );
}

#[tokio::test]
async fn blueprint_removal_drops_sessions() {
    let h = Harness::new();
    let session = h.handshake(SESS).await;

    // Confirm the session works before removal.
    let (status, _, _) = h.post(SESS, tools_call(1, SUM), Some(&session)).await;
    assert_eq!(status, StatusCode::OK);

    // Delete then re-create the blueprint by the same name. Removal drops the MCP
    // service (and its session map), so a rebuilt service no longer knows the id.
    assert_eq!(h.delete_blueprint(SESS).await, StatusCode::OK);
    assert_eq!(h.put_blueprint(SESS, SESS_PLUS_HTTP).await, StatusCode::OK);

    let (status, _, _) = h.post(SESS, tools_call(2, SUM), Some(&session)).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the old session must not survive blueprint removal"
    );

    // A fresh handshake against the re-created blueprint still works.
    let fresh = h.handshake(SESS).await;
    let (status, _, _) = h.post(SESS, tools_call(3, SUM), Some(&fresh)).await;
    assert_eq!(status, StatusCode::OK);
}

// Write a 5-line file, then page through it with `files.read`.
const WRITE_LINES: &str = r#"import { writeText } from "submilli:fs";
function main(): void { writeText("/lines.txt", "l1\nl2\nl3\nl4\nl5\n"); }"#;

#[tokio::test]
async fn files_read_windows_a_persisted_file() {
    let h = Harness::new();
    let session = h.handshake(SESS).await;

    let (_, _, w) = h
        .post(SESS, tools_call(2, WRITE_LINES), Some(&session))
        .await;
    assert!(output(&w)["error"].is_null(), "write failed: {w}");

    // First window: lines 2-3.
    let call = rpc_call(
        3,
        "submilli__files__read",
        json!({ "path": "/lines.txt", "offset": 2, "limit": 2 }),
    );
    let (status, _, rpc) = h.post(SESS, call, Some(&session)).await;
    assert_eq!(status, StatusCode::OK);
    let out = output(&rpc);
    assert_eq!(out["content"], json!("l2\nl3"), "got: {rpc}");
    assert_eq!(out["line_start"], json!(2));
    assert_eq!(out["line_end"], json!(3));
    assert_eq!(out["returned_lines"], json!(2));
    assert_eq!(out["has_more"], json!(true));
    assert_eq!(out["next_offset"], json!(4), "paginates onward");

    // Resume from next_offset: lines 4-5, end of file.
    let call = rpc_call(
        4,
        "submilli__files__read",
        json!({ "path": "/lines.txt", "offset": 4, "limit": 2 }),
    );
    let (_, _, rpc) = h.post(SESS, call, Some(&session)).await;
    let out = output(&rpc);
    assert_eq!(out["content"], json!("l4\nl5"), "got: {rpc}");
    assert_eq!(out["has_more"], json!(false), "no more after the last line");
    assert!(out["next_offset"].is_null());
}

#[tokio::test]
async fn files_read_rejects_path_escape() {
    let h = Harness::new();
    let session = h.handshake(SESS).await;
    let call = rpc_call(
        2,
        "submilli__files__read",
        json!({ "path": "/../../etc/passwd" }),
    );
    let (_, _, rpc) = h.post(SESS, call, Some(&session)).await;
    assert!(
        refuses_with(&rpc, ESCAPE_DIAGNOSTIC),
        "path escape must be rejected: {rpc}"
    );
}

const WRITE_TREE: &str = r#"import { writeText, mkdir } from "submilli:fs";
function main(): void {
    writeText("/a.txt", "aaa");
    mkdir("/sub", true);
    writeText("/sub/b.txt", "bb");
}"#;

#[tokio::test]
async fn files_list_enumerates_the_workspace() {
    let h = Harness::new();
    let session = h.handshake(SESS).await;

    let (_, _, w) = h
        .post(SESS, tools_call(2, WRITE_TREE), Some(&session))
        .await;
    assert!(output(&w)["error"].is_null(), "write failed: {w}");

    // Non-recursive: top level only — `/a.txt` and the `/sub` dir, not `/sub/b.txt`.
    let call = rpc_call(3, "submilli__files__list", json!({ "path": "/" }));
    let (status, _, rpc) = h.post(SESS, call, Some(&session)).await;
    assert_eq!(status, StatusCode::OK);
    let out = output(&rpc);
    let paths: Vec<&str> = out["entries"]
        .as_array()
        .expect("entries array")
        .iter()
        .map(|e| e["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, vec!["/a.txt", "/sub"], "got: {rpc}");
    assert_eq!(out["entries"][1]["kind"], json!("directory"));
    assert_eq!(out["entries"][0]["bytes"], json!(3));

    // Recursive: the nested file shows up too.
    let call = rpc_call(4, "submilli__files__list", json!({ "recursive": true }));
    let (_, _, rpc) = h.post(SESS, call, Some(&session)).await;
    let paths: Vec<&str> = output(&rpc)["entries"]
        .as_array()
        .expect("entries array")
        .iter()
        .map(|e| e["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, vec!["/a.txt", "/sub", "/sub/b.txt"], "got: {rpc}");

    // Listing a subdirectory still names entries by their path from the root, not
    // relative to the directory asked for.
    let call = rpc_call(5, "submilli__files__list", json!({ "path": "/sub/" }));
    let (_, _, rpc) = h.post(SESS, call, Some(&session)).await;
    let out = output(&rpc);
    assert_eq!(out["entries"][0]["path"], json!("/sub/b.txt"), "got: {rpc}");
    assert_eq!(out["count"], json!(1));
}

#[tokio::test]
async fn tool_arguments_accept_string_scalars_and_report_invalid_arguments() {
    let h = Harness::new();
    let session = h.handshake(SESS).await;
    let (_, _, written) = h
        .post(SESS, tools_call(2, WRITE_TREE), Some(&session))
        .await;
    assert!(output(&written)["error"].is_null(), "{written}");

    for (value, count) in [
        (json!("true"), 3),
        (json!("false"), 2),
        (json!(true), 3),
        (json!(null), 2),
    ] {
        let (_, _, rpc) = h
            .post(
                SESS,
                rpc_call(3, "submilli__files__list", json!({"recursive": value})),
                Some(&session),
            )
            .await;
        assert_eq!(output(&rpc)["count"], count, "{rpc}");
    }

    let (_, _, written) = h
        .post(SESS, tools_call(4, WRITE_LINES), Some(&session))
        .await;
    assert!(output(&written)["error"].is_null(), "{written}");
    let (_, _, rpc) = h
        .post(
            SESS,
            rpc_call(
                5,
                "submilli__files__read",
                json!({"path": "/lines.txt", "offset": "2", "limit": "2"}),
            ),
            Some(&session),
        )
        .await;
    assert_eq!(output(&rpc)["content"], "l2\nl3", "{rpc}");

    for (tool, args, message) in [
        (
            "submilli__files__list",
            json!({"recursive": "yes"}),
            "boolean",
        ),
        ("submilli__files__list", json!({"recursive": 1}), "boolean"),
        ("submilli__files__read", json!({}), "missing field"),
        ("submilli__files__read", json!({"path": 1}), "string"),
        (
            "submilli__files__read",
            json!({"path": "/", "limit": "-1"}),
            "u32",
        ),
        (
            "submilli__files__read",
            json!({"path": "/", "limit": "4294967296"}),
            "u32",
        ),
        (
            "submilli__files__read",
            json!({"path": "/", "limit": "1.5"}),
            "u32",
        ),
        ("submilli__typescript__execute", json!({}), "missing field"),
        (
            "submilli__typescript__execute",
            json!({"code": "", "extra": true}),
            "unknown field",
        ),
        (
            "submilli__typescript__packages__docs",
            json!({}),
            "missing field",
        ),
        (
            "submilli__typescript__builtins__docs",
            json!({"names": "Array"}),
            "sequence",
        ),
    ] {
        let (_, _, rpc) = h.post(SESS, rpc_call(6, tool, args), Some(&session)).await;
        assert!(refuses_with(&rpc, message), "{tool}: {rpc}");
        assert_eq!(output(&rpc)["error"]["kind"], "invalid_arguments", "{rpc}");
        assert!(text_output(&rpc).contains(message), "{rpc}");
    }

    let (_, _, rpc) = h
        .post(SESS, rpc_call(7, "missing_tool", json!({})), Some(&session))
        .await;
    assert_eq!(rpc["error"]["code"], -32602, "{rpc}");
    let (_, _, rpc) = h
        .post(
            SESS,
            rpc_call(8, "submilli__files__list", json!({})),
            Some(&session),
        )
        .await;
    assert_eq!(output(&rpc)["count"], 3, "session remains usable: {rpc}");
}

/// A file tool's per-call failure: an `isError` result whose `error.message` the
/// model reads, never a JSON-RPC error. Clients such as `langchain-mcp-adapters`
/// raise on a JSON-RPC error, which crashes the agent instead of telling the model.
fn tool_error_message(rpc: &Value) -> Option<&str> {
    if !rpc["error"].is_null() || rpc["result"]["isError"] != json!(true) {
        return None;
    }
    output(rpc)["error"]["message"].as_str()
}

/// A refusal that names `reason`, not merely any error.
///
/// A transport-level error such as "tool not found" is a JSON-RPC error, not a
/// tool result, so a containment test resting on this cannot pass when the call
/// never reaches the tool — which is how a renamed tool once left the escape
/// assertions green while proving nothing.
fn refuses_with(rpc: &Value, reason: &str) -> bool {
    tool_error_message(rpc).is_some_and(|message| message.contains(reason))
}

/// A per-call failure reaches the model as a tool result carrying a
/// `{ kind, message }` error like execute's: a policy denial as
/// `permission_denied`; a path that is missing, a directory, or outside the VFS
/// as `file_error`. A JSON-RPC error stays for protocol problems.
#[tokio::test]
async fn files_tools_answer_failures_as_tool_results() {
    let h = Harness::from_blueprints(vec![
        submilli_blueprint::parse(
            "name: eph\npermissions:\n  main:\n    - capability: fs.read\n      filter: 'path == \"/missing.txt\" or path == \"/\"'\n      action: allow\n    - capability: fs.list\n      filter: 'path == \"/missing\"'\n      action: allow\n",
        )
        .unwrap(),
    ]);
    let session = h.handshake(EPH).await;
    for (id, tool, args, kind, message) in [
        (
            2,
            "submilli__files__list",
            json!({ "path": "/" }),
            "permission_denied",
            "permission denied: caller=main capability=fs.list",
        ),
        (
            3,
            "submilli__files__read",
            json!({ "path": "/a.txt" }),
            "permission_denied",
            "permission denied: caller=main capability=fs.read",
        ),
        (
            4,
            "submilli__files__read",
            json!({ "path": "/missing.txt" }),
            "file_error",
            "/missing.txt: ",
        ),
        (
            5,
            "submilli__files__list",
            json!({ "path": "/missing" }),
            "file_error",
            "/missing: ",
        ),
        (
            6,
            "submilli__files__read",
            json!({ "path": "/" }),
            "file_error",
            "/: ",
        ),
        (
            7,
            "submilli__files__read",
            json!({ "path": "/../etc/passwd" }),
            "file_error",
            ESCAPE_DIAGNOSTIC,
        ),
        (
            8,
            "submilli__files__list",
            json!({ "path": ".." }),
            "file_error",
            ESCAPE_DIAGNOSTIC,
        ),
    ] {
        let (status, _, rpc) = h.post(EPH, rpc_call(id, tool, args), Some(&session)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(refuses_with(&rpc, message), "{tool}: {rpc}");
        assert_eq!(output(&rpc)["error"]["kind"], json!(kind), "{rpc}");
        let text = text_output(&rpc);
        assert!(text.contains(message), "unstructured clients see: {text}");
    }

    // Invalid tool arguments reach the model as a correctable tool failure.
    let call = rpc_call(9, "submilli__files__read", json!({ "path": 1 }));
    let (_, _, rpc) = h.post(EPH, call, Some(&session)).await;
    assert!(refuses_with(&rpc, "expected a string"), "{rpc}");
}

const ESCAPE_DIAGNOSTIC: &str = "path escapes the VFS root";
const DISABLED_DIAGNOSTIC: &str = "filesystem is disabled";

/// Content that exists only outside the VFS root, so seeing it in a response is
/// proof the containment boundary was crossed.
const HOST_ONLY: &str = "SUBMILLI-HOST-ONLY-MARKER";

/// Plant `escape -> <a directory outside the root>` inside the session workspace,
/// with a marked file behind it. The returned handle owns the outside directory.
///
/// Not gated to Unix, and deliberately not skipped when the link cannot be created:
/// `cap-std` resolves paths differently on each platform, so the MCP file tools are
/// exactly as unproven on Windows as the interpreter surface was, and a test that
/// compiled to nothing there would say otherwise. A failure here is a real signal,
/// including a Windows runner without symlink privilege.
fn plant_escaping_link(session_dir: &std::path::Path) -> tempfile::TempDir {
    let outside = tempfile::tempdir().expect("outside dir");
    std::fs::write(outside.path().join("secret.txt"), HOST_ONLY).expect("host file");
    let at = session_dir.join("escape");
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), &at).expect("symlink");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(outside.path(), &at).expect("symlink_dir");
    outside
}

/// Set up a `per_session` workspace with a tree written and an escaping link
/// planted, returning the session id and the outside-directory handle.
async fn session_with_escaping_link(h: &Harness) -> (String, tempfile::TempDir) {
    let session = h.handshake(SESS).await;
    let (_, _, w) = h
        .post(SESS, tools_call(2, WRITE_TREE), Some(&session))
        .await;
    assert!(output(&w)["error"].is_null(), "write failed: {w}");
    let outside = plant_escaping_link(&h.session_root.join(&session));
    (session, outside)
}

#[tokio::test]
async fn files_read_through_escaping_symlink_refuses() {
    let h = Harness::new();
    let (session, _outside) = session_with_escaping_link(&h).await;

    let call = rpc_call(
        3,
        "submilli__files__read",
        json!({ "path": "/escape/secret.txt" }),
    );
    let (_, _, rpc) = h.post(SESS, call, Some(&session)).await;
    assert!(
        refuses_with(&rpc, ESCAPE_DIAGNOSTIC),
        "escaping read must refuse as an escape: {rpc}"
    );
    assert!(
        !rpc.to_string().contains(HOST_ONLY),
        "host file content reached the caller: {rpc}"
    );
}

#[tokio::test]
async fn files_list_through_escaping_symlink_refuses() {
    let h = Harness::new();
    let (session, outside) = session_with_escaping_link(&h).await;

    let call = rpc_call(3, "submilli__files__list", json!({ "path": "/escape" }));
    let (_, _, rpc) = h.post(SESS, call, Some(&session)).await;
    assert!(
        refuses_with(&rpc, ESCAPE_DIAGNOSTIC),
        "escaping listing must refuse as an escape: {rpc}"
    );
    assert!(
        !rpc.to_string().contains("secret.txt"),
        "enumerated outside the root: {rpc}"
    );
    assert!(
        !rpc.to_string().contains(&*outside.path().to_string_lossy()),
        "host path echoed into the response: {rpc}"
    );
}

/// A link is its own kind, and it has no content of its own. Calling one a `"file"`
/// sends a client that trusts `kind` to read a directory, and a symlink's `len()` is
/// the byte length of its *target*, so reporting it measures paths the caller is not
/// allowed to see. `submilli:fs` already answers `"symlink"` and 0 here; the two
/// surfaces describe the same bytes, so they have to agree.
#[tokio::test]
async fn files_list_reports_a_symlink_as_a_symlink_without_measuring_its_target() {
    let h = Harness::new();
    let (session, outside) = session_with_escaping_link(&h).await;

    let call = rpc_call(3, "submilli__files__list", json!({ "path": "/" }));
    let (_, _, rpc) = h.post(SESS, call, Some(&session)).await;
    let out = output(&rpc);
    let link = out["entries"]
        .as_array()
        .expect("entries array")
        .iter()
        .find(|e| e["path"] == json!("/escape"))
        .unwrap_or_else(|| panic!("the link is not listed at all: {rpc}"));

    assert_eq!(link["kind"], json!("symlink"), "got: {rpc}");
    assert_eq!(
        link["bytes"],
        json!(0),
        "a link's size is its target's length — {} bytes here: {rpc}",
        outside.path().as_os_str().len(),
    );
}

#[tokio::test]
async fn files_responses_never_echo_host_paths() {
    let h = Harness::new();
    let session = h.handshake(SESS).await;
    let (_, _, w) = h
        .post(SESS, tools_call(2, WRITE_TREE), Some(&session))
        .await;
    assert!(output(&w)["error"].is_null(), "write failed: {w}");

    let host_root = h.session_root.join(&session).to_string_lossy().into_owned();
    let calls = [
        rpc_call(3, "submilli__files__list", json!({ "recursive": true })),
        rpc_call(4, "submilli__files__read", json!({ "path": "/a.txt" })),
        rpc_call(
            5,
            "submilli__files__read",
            json!({ "path": "/missing.txt" }),
        ),
        rpc_call(6, "submilli__files__list", json!({ "path": "/nope" })),
    ];
    for call in calls {
        let (_, _, rpc) = h.post(SESS, call, Some(&session)).await;
        assert!(
            !rpc.to_string().contains(&host_root),
            "host path leaked into an MCP response: {rpc}"
        );
    }
}

#[tokio::test]
async fn files_tools_refuse_under_vfs_none() {
    // With no VFS backing the blueprint there is nothing to resolve against, so
    // both tools must refuse before touching the filesystem. Cargo runs an
    // integration test with the package root as its working directory, which is
    // what an unrooted relative path would otherwise resolve against.
    let h = Harness::new();
    let session = h.handshake(NO_VFS).await;

    let call = rpc_call(2, "submilli__files__read", json!({ "path": "/Cargo.toml" }));
    let (_, _, rpc) = h.post(NO_VFS, call, Some(&session)).await;
    assert!(
        refuses_with(&rpc, DISABLED_DIAGNOSTIC),
        "vfs: none must refuse a read as disabled: {rpc}"
    );
    assert!(
        !rpc.to_string().contains("[package]"),
        "read the server crate's own manifest: {rpc}"
    );

    let call = rpc_call(3, "submilli__files__list", json!({ "path": "/src" }));
    let (_, _, rpc) = h.post(NO_VFS, call, Some(&session)).await;
    assert!(
        refuses_with(&rpc, DISABLED_DIAGNOSTIC),
        "vfs: none must refuse a listing as disabled: {rpc}"
    );
    assert!(
        !rpc.to_string().contains("lib.rs"),
        "enumerated the server crate's own source tree: {rpc}"
    );
}

#[tokio::test]
async fn tool_description_is_resolved_prompt() {
    let h = Harness::new();
    let session = h.handshake(EPH).await;
    let (status, _, rpc) = h.post(EPH, tools_list(1), Some(&session)).await;
    assert_eq!(status, StatusCode::OK);
    let desc = tool_desc(&rpc, EXECUTE);
    for name in [
        "submilli__typescript__packages__search",
        "submilli__typescript__packages__docs",
        "submilli__typescript__builtins__docs",
    ] {
        assert!(desc.contains(&format!("`{name}`")), "{desc}");
        assert!(
            rpc["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == name)
        );
    }
    assert!(!desc.contains("{t_"));

    // Body of llm-prompt.md's `## The prompt` section is present...
    assert!(desc.contains("strict TypeScript subset"), "got: {desc}");
    // ...with `{vfs_mode}` resolved for this blueprint...
    assert!(desc.contains("ephemeral"), "vfs_mode unresolved: {desc}");
    assert!(!desc.contains("{vfs_mode}"), "placeholder left in: {desc}");
    // ...and `{builtins}` resolved to the prelude catalog.
    assert!(
        desc.contains("Array") && desc.contains("Temporal"),
        "builtins unresolved: {desc}"
    );
    assert!(!desc.contains("{builtins}"), "placeholder left in: {desc}");
    // HTTP is omitted because this blueprint grants only FS capabilities.
    assert!(
        !desc.contains("submilli:http"),
        "http_access unresolved: {desc}"
    );
    assert!(
        !desc.contains("{http_access}"),
        "placeholder left in: {desc}"
    );
    // ...and the doc's front matter / trailing sections excluded.
    assert!(
        !desc.contains("Editing principles"),
        "front matter leaked: {desc}"
    );
    assert!(
        !desc.contains("Server-injected placeholders"),
        "tail leaked: {desc}"
    );
}

/// Every tool the server exposes must be callable by the major model APIs and
/// must carry a description. OpenAI rejects any tool name outside
/// `^[a-zA-Z0-9_-]+$` with a 400 before the model gets a turn, so the name
/// check covers all eight and any tool added later.
///
/// The description check is narrower than it looks: only the five table-fed
/// tools reach a client via a `list_tools` lookup arm, so only they go blank
/// when an arm stops matching its `#[tool(name = ...)]` attribute. `execute`,
/// `files__read` and `files__list` carry inline `description = ...` and have no
/// arm to drift from. Neither check reads description *text*, so a description
/// naming a tool that is not registered still passes.
#[tokio::test]
async fn every_tool_name_is_provider_safe_and_described() {
    let h = Harness::new();
    let session = h.handshake(EPH).await;
    let (status, _, rpc) = h.post(EPH, tools_list(1), Some(&session)).await;
    assert_eq!(status, StatusCode::OK);

    let tools = rpc["result"]["tools"].as_array().expect("tools array");

    // Pin the set, not just non-emptiness: a loop over a surface that silently
    // shrank would pass while checking nothing.
    let exposed: std::collections::BTreeSet<&str> =
        tools.iter().filter_map(|t| t["name"].as_str()).collect();
    let expected: std::collections::BTreeSet<&str> = [
        "submilli__files__list",
        "submilli__files__read",
        "submilli__typescript__builtins__docs",
        "submilli__typescript__builtins__list",
        "submilli__typescript__execute",
        "submilli__typescript__last_run",
        "submilli__typescript__packages__docs",
        "submilli__typescript__packages__search",
    ]
    .into_iter()
    .collect();
    assert_eq!(exposed, expected, "exposed tool set changed");

    for tool in tools {
        let name = tool["name"]
            .as_str()
            .unwrap_or_else(|| panic!("unnamed tool in {rpc}"));
        assert!(
            (1..=64).contains(&name.len())
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
            "tool name `{name}` is not 1-64 chars of [A-Za-z0-9_-]; OpenAI rejects it"
        );
        assert!(
            tool["description"].as_str().is_some_and(|d| !d.is_empty()),
            "tool `{name}` has no description — a list_tools lookup arm likely stopped \
             matching its #[tool(name = ...)] attribute"
        );
    }
}

#[tokio::test]
async fn per_session_description_reports_session_mode() {
    let h = Harness::new();
    let session = h.handshake(SESS).await;
    let (status, _, rpc) = h.post(SESS, tools_list(9), Some(&session)).await;
    assert_eq!(status, StatusCode::OK);
    let desc = tool_desc(&rpc, EXECUTE);
    assert!(desc.contains("per_session"), "got: {desc}");
}

#[tokio::test]
async fn delete_wipes_session_vfs() {
    let h = Harness::new();
    let session = h.handshake(SESS).await;

    let (_, _, w) = h.post(SESS, tools_call(2, WRITE), Some(&session)).await;
    assert!(output(&w)["error"].is_null(), "write failed: {w}");

    let dir = h.session_root.join(&session);
    assert!(dir.is_dir(), "per_session dir should exist after a write");

    assert_eq!(h.delete(SESS, &session).await, StatusCode::NO_CONTENT);
    assert!(!dir.exists(), "DELETE must wipe the session VFS");
    assert_eq!(h.delete(SESS, &session).await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_unknown_or_other_blueprint_session_is_404() {
    let h = Harness::new();
    assert_eq!(
        h.delete(SESS, "unknown-session").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.delete(SESS, &"x".repeat(1024)).await,
        StatusCode::NOT_FOUND
    );
    let session = h.handshake(SESS).await;
    assert_eq!(h.delete(EPH, &session).await, StatusCode::NOT_FOUND);
    let (status, _, _) = h.post(SESS, tools_list(2), Some(&session)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "wrong-blueprint DELETE must preserve the session"
    );
    assert_eq!(h.delete(SESS, &session).await, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn concurrent_deletes_return_one_204_and_one_404() {
    let h = Harness::new();
    let session = h.handshake(SESS).await;
    let (first, second) = tokio::join!(h.delete(SESS, &session), h.delete(SESS, &session));
    assert!(
        (first == StatusCode::NO_CONTENT && second == StatusCode::NOT_FOUND)
            || (second == StatusCode::NO_CONTENT && first == StatusCode::NOT_FOUND)
    );
}

#[tokio::test]
async fn delete_persisted_mcp_session_before_restore() {
    let session_root = tempfile::tempdir().expect("session root");
    let session_store = tempfile::tempdir().expect("session store");
    let blueprints = || {
        vec![Blueprint {
            name: SESS.into(),
            vfs: VfsConfig::PerSession {
                size_limit: None,
                mounts: Default::default(),
                cwd: None,
            },
            permissions: allow_fs(),
            ..Default::default()
        }]
    };
    let first = Harness::from_blueprints_with_session_paths(
        blueprints(),
        session_root.path().to_path_buf(),
        session_store.path().to_path_buf(),
    );
    let session = first.handshake(SESS).await;
    let (_, _, result) = first.post(SESS, tools_call(2, WRITE), Some(&session)).await;
    assert!(output(&result)["error"].is_null(), "write failed: {result}");
    drop(first);
    let restarted = Harness::from_blueprints_with_session_paths(
        blueprints(),
        session_root.path().to_path_buf(),
        session_store.path().to_path_buf(),
    );
    restarted.state.boot().await.expect("boot");
    assert_eq!(
        restarted.delete(SESS, &"x".repeat(1024)).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        restarted.delete(SESS, &session).await,
        StatusCode::NO_CONTENT
    );
    assert!(!session_root.path().join(&session).exists());
    assert_eq!(
        restarted.delete(SESS, &session).await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn last_run_recovers_full_console() {
    let h = Harness::new();
    let session = h.handshake(EPH).await;

    // A successful run: execute suppresses the console; lastRun recovers it.
    let code = r#"function main(): number { console.log("hello from submilli"); return 7; }"#;
    let (_, _, run) = h.post(EPH, tools_call(1, code), Some(&session)).await;
    assert_eq!(output(&run)["result"], json!("7"));
    assert_eq!(
        output(&run)["console"],
        json!([]),
        "console suppressed: {run}"
    );

    let (status, _, last) = h
        .post(
            EPH,
            rpc_call(2, "submilli__typescript__last_run", json!({})),
            Some(&session),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(output(&last)["result"], json!("7"));
    assert_eq!(output(&last)["console"], json!(["hello from submilli"]));
}

/// Its description comes from the shared table, not an inline literal — the
/// HTTP client harness publishes the same tool and reads its text from the
/// REST prompt endpoint, which serves that same constant.
#[tokio::test]
async fn last_run_description_comes_from_the_shared_table() {
    let h = Harness::new();
    let session = h.handshake(EPH).await;

    let (status, _, tools) = h.post(EPH, tools_list(1), Some(&session)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        tool_desc(&tools, "submilli__typescript__last_run"),
        submilli_shared::prompt::tools::LAST_RUN,
    );
}

#[tokio::test]
async fn last_run_without_prior_execute_errors() {
    let h = Harness::new();
    let session = h.handshake(EPH).await;
    let (status, _, rpc) = h
        .post(
            EPH,
            rpc_call(1, "submilli__typescript__last_run", json!({})),
            Some(&session),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        rpc.to_string().contains("no previous run"),
        "expected a tool error: {rpc}"
    );
}

#[tokio::test]
async fn packages_docs_host_module() {
    let h =
        Harness::from_blueprints(vec![submilli_blueprint::parse(
        "name: eph\npermissions:\n  main:\n    - capability: http.get\n      action: allow\n"
    ).unwrap()]);
    let session = h.handshake(EPH).await;
    let (status, _, rpc) = h
        .post(
            EPH,
            rpc_call(
                1,
                "submilli__typescript__packages__docs",
                json!({"name": "submilli:http"}),
            ),
            Some(&session),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let out = output(&rpc);
    assert_eq!(out["source"], json!("host"));
    assert!(!out["description"].as_str().unwrap().is_empty());
    let decls = out["declarations"].as_str().unwrap_or("");
    assert!(
        decls.contains("get") && decls.contains("post"),
        "declarations missing http verbs: {decls}"
    );
}

#[tokio::test]
async fn packages_docs_structured_errors() {
    let h = Harness::new();
    let session = h.handshake(EPH).await;
    let docs = |id: u32, name: &str| {
        rpc_call(
            id,
            "submilli__typescript__packages__docs",
            json!({ "name": name }),
        )
    };

    // Unknown name → structured error, HTTP 200 (not a trap).
    let (status, _, r) = h.post(EPH, docs(1, "nope"), Some(&session)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(output(&r)["error"], json!("unknown_package"));

    // Internal `submilli:security` is excluded → unknown.
    let (_, _, r2) = h
        .post(EPH, docs(2, "submilli:security"), Some(&session))
        .await;
    assert_eq!(output(&r2)["error"], json!("unknown_package"));

    // `@mcp/<server>` for a server this blueprint doesn't declare → unknown.
    let (_, _, r3) = h.post(EPH, docs(3, "@mcp/linear"), Some(&session)).await;
    assert_eq!(output(&r3)["error"], json!("unknown_mcp_server"));
}

#[tokio::test]
async fn packages_search_by_symbol_and_list_all() {
    let h = Harness::new();
    let session = h.handshake(EPH).await;
    let search = |id: u32, q: &str| {
        rpc_call(
            id,
            "submilli__typescript__packages__search",
            json!({ "query": q }),
        )
    };

    // Matching an exported symbol name surfaces the module.
    let (_, _, r) = h.post(EPH, search(1, "sha256"), Some(&session)).await;
    let names: Vec<&str> = output(&r)["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"submilli:crypto"), "got: {names:?}");

    // Empty query lists every module the blueprint permits (security excluded).
    let (_, _, all) = h.post(EPH, search(2, ""), Some(&session)).await;
    let count = output(&all)["results"].as_array().unwrap().len();
    assert_eq!(count, 6, "expected 6 permitted stdlib modules: {all}");
    let listed = |name: &str| {
        output(&all)["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == name)
    };
    assert!(listed("submilli:code"));
    // The blueprint grants FS only, so session state is hidden.
    assert!(!listed("submilli:session"));
    let (_, _, code) = h.post(EPH, search(3, "applyPatch"), Some(&session)).await;
    assert_eq!(output(&code)["results"][0]["name"], "submilli:code");
}

#[tokio::test]
async fn packages_docs_and_search_include_blueprint_registry_packages() {
    const PKG: &str = "@submilli/jina";
    let package_store = installed_test_package(PKG);
    let h = Harness::from_blueprints_and_packages(
        vec![Blueprint {
            name: EPH.into(),
            packages: [PKG.to_string()].into_iter().collect(),
            ..Default::default()
        }],
        package_store,
    );
    let session = h.handshake(EPH).await;

    let docs = rpc_call(
        1,
        "submilli__typescript__packages__docs",
        json!({ "name": PKG }),
    );
    let (_, _, docs_response) = h.post(EPH, docs, Some(&session)).await;
    let docs = text_output(&docs_response);
    assert!(
        docs.contains("# Jina") && docs.contains("Use Jina") && docs.contains("function read("),
        "got: {docs}"
    );

    let search = rpc_call(
        2,
        "submilli__typescript__packages__search",
        json!({ "query": "read" }),
    );
    let (_, _, search_response) = h.post(EPH, search, Some(&session)).await;
    let names: Vec<&str> = output(&search_response)["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&PKG), "got: {names:?}");
    let jina = output(&search_response)["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["name"] == PKG)
        .expect("jina result");
    assert_eq!(
        jina["description"],
        json!("Jina AI readers and search helpers for LLM web workflows.")
    );
}

#[tokio::test]
async fn builtins_docs_batch_and_unknown() {
    let h = Harness::new();
    let session = h.handshake(EPH).await;
    let (status, _, r) = h
        .post(
            EPH,
            rpc_call(
                1,
                "submilli__typescript__builtins__docs",
                json!({ "names": ["Array", "Temporal", "JSON", "Bogus"] }),
            ),
            Some(&session),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let results = output(&r)["results"].as_array().unwrap().clone();
    assert_eq!(results.len(), 4, "one entry per requested name: {r}");
    let by_name = |name: &str| {
        results
            .iter()
            .find(|x| x["name"] == json!(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .clone()
    };

    let array = by_name("Array");
    let decls = array["declarations"].as_str().unwrap();
    assert!(decls.contains("interface Array<") && decls.contains("ArrayConstructor"));
    assert!(
        by_name("Temporal")["declarations"]
            .as_str()
            .unwrap()
            .contains("namespace Temporal")
    );
    assert!(
        by_name("JSON")["declarations"]
            .as_str()
            .unwrap()
            .contains("stringify")
    );

    // Unknown name → inline error entry, batch still HTTP 200.
    assert_eq!(by_name("Bogus")["error"], json!("unknown_builtin"));
}

#[tokio::test]
async fn builtins_docs_names_the_packages_docs_call_for_a_package_name() {
    let h = Harness::new();
    let session = h.handshake(EPH).await;
    let (status, _, r) = h
        .post(
            EPH,
            rpc_call(
                1,
                "submilli__typescript__builtins__docs",
                json!({ "names": ["submilli:crypto", "Temporel", "Array"] }),
            ),
            Some(&session),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let results = output(&r)["results"].as_array().unwrap().clone();
    assert_eq!(results.len(), 3, "one entry per requested name: {r}");
    let by_name = |name: &str| {
        results
            .iter()
            .find(|x| x["name"] == json!(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .clone()
    };

    // A package name gets the call to make, not the declarations — the
    // `import` is the step that distinguishes the two namespaces.
    let package = by_name("submilli:crypto");
    assert_eq!(
        package["error"],
        json!("not_a_builtin"),
        "a correcting call needs its own code, distinguishable from a real miss: {package}"
    );
    let message = package["message"].as_str().unwrap_or("");
    assert!(
        message.contains("submilli__typescript__packages__docs(\"submilli:crypto\")"),
        "got: {message}"
    );
    assert!(message.contains("import"), "got: {message}");
    assert!(package["declarations"].is_null(), "got: {package}");

    // A name in neither catalog still gets a suggestion.
    let miss = by_name("Temporel");
    assert_eq!(miss["error"], json!("unknown_builtin"));
    assert_eq!(miss["did_you_mean"], json!("Temporal"), "got: {miss}");

    // A real built-in in the same batch is untouched.
    assert!(
        by_name("Array")["declarations"]
            .as_str()
            .unwrap_or("")
            .contains("interface Array<")
    );
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn builtins_docs_recognizes_only_the_mcp_servers_the_blueprint_declares() {
    let url = upstream::spawn().await;
    let bp = Blueprint {
        name: "up-bp".into(),
        mcp: BTreeMap::from([(
            "up".to_string(),
            McpServer {
                transport: "streamable_http".into(),
                url,
                headers: BTreeMap::new(),
                auth: None,
            },
        )]),
        ..Default::default()
    };
    let h = Harness::from_blueprints(vec![bp]);
    let session = h.handshake("up-bp").await;
    let (status, _, r) = h
        .post(
            "up-bp",
            rpc_call(
                1,
                "submilli__typescript__builtins__docs",
                json!({ "names": ["@mcp/up", "@mcp/bogus"] }),
            ),
            Some(&session),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let results = output(&r)["results"].as_array().unwrap().clone();
    let by_name = |name: &str| {
        results
            .iter()
            .find(|x| x["name"] == json!(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .clone()
    };

    // A server this blueprint declares is a package: name the call to make.
    let declared = by_name("@mcp/up");
    assert_eq!(declared["error"], json!("not_a_builtin"), "got: {declared}");
    assert!(
        declared["message"]
            .as_str()
            .unwrap_or("")
            .contains("submilli__typescript__packages__docs(\"@mcp/up\")"),
        "got: {declared}"
    );

    // One it does not declare is not a package, so it keeps the plain miss
    // rather than being redirected into a second error.
    let undeclared = by_name("@mcp/bogus");
    assert_eq!(
        undeclared["error"],
        json!("unknown_builtin"),
        "got: {undeclared}"
    );
}

#[tokio::test]
async fn builtins_docs_resolves_a_dotted_member_path() {
    let h = Harness::new();
    let session = h.handshake(EPH).await;
    let (status, _, r) = h
        .post(
            EPH,
            rpc_call(
                1,
                "submilli__typescript__builtins__docs",
                json!({ "names": ["Temporal.Instant", "Temporal.Foo"] }),
            ),
            Some(&session),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let results = output(&r)["results"].as_array().unwrap().clone();
    let by_name = |name: &str| {
        results
            .iter()
            .find(|x| x["name"] == json!(name))
            .unwrap_or_else(|| panic!("missing {name}"))
            .clone()
    };

    let slice = by_name("Temporal.Instant");
    let decls = slice["declarations"].as_str().unwrap_or("");
    assert!(
        decls.contains("interface InstantConstructor {"),
        "got: {decls}"
    );
    assert!(!decls.contains("interface ZonedDateTime {"), "got: {decls}");

    // An unresolvable member names the members that do exist.
    let miss = by_name("Temporal.Foo");
    assert_eq!(miss["error"], json!("unknown_builtin"));
    let message = miss["message"].as_str().unwrap_or("");
    assert!(message.contains("Instant"), "got: {message}");
}

#[tokio::test]
async fn builtins_list_returns_catalog_and_is_discoverable() {
    let h = Harness::new();
    let session = h.handshake(EPH).await;

    let (status, _, tools) = h.post(EPH, tools_list(1), Some(&session)).await;
    assert_eq!(status, StatusCode::OK);
    let desc = tool_desc(&tools, "submilli__typescript__builtins__list");
    assert!(desc.contains("always in scope without"));
    assert!(desc.contains("not imported"));

    let (status, _, r) = h
        .post(
            EPH,
            rpc_call(2, "submilli__typescript__builtins__list", json!({})),
            Some(&session),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let out = output(&r);
    let types = out["types"].as_array().unwrap();
    let namespaces = out["namespaces"].as_array().unwrap();
    assert!(types.iter().any(|x| x == "Array"), "got: {out}");
    assert!(types.iter().any(|x| x == "Map"), "got: {out}");
    assert!(namespaces.iter().any(|x| x == "Temporal"), "got: {out}");
    assert!(namespaces.iter().any(|x| x == "JSON"), "got: {out}");
}

/// A minimal upstream MCP server, served over streamable HTTP, used to exercise
/// real `@mcp/<server>` discovery end-to-end.
mod upstream {
    use rmcp::handler::server::router::tool::ToolRouter;
    use rmcp::handler::server::wrapper::Parameters;
    use rmcp::model::{ServerCapabilities, ServerInfo};
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService,
    };
    use rmcp::{Json, ServerHandler, schemars, tool, tool_handler, tool_router};

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    pub struct CreateIssueRequest {
        /// Issue title.
        #[allow(dead_code)]
        pub title: String,
        /// Owning team key.
        #[serde(default)]
        #[allow(dead_code)]
        pub team: Option<String>,
    }

    #[derive(serde::Serialize, schemars::JsonSchema)]
    pub struct UserResponse {
        pub id: String,
        pub name: String,
    }

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    pub struct EchoRequest {
        pub value: serde_json::Value,
        pub label: String,
    }

    #[derive(Clone)]
    pub struct Upstream {
        tool_router: ToolRouter<Self>,
        /// State that lives in one upstream session: a handler is built per
        /// session, so a count above one means calls shared a session.
        calls: std::sync::Arc<std::sync::atomic::AtomicU32>,
    }

    impl Upstream {
        fn new() -> Self {
            Self {
                tool_router: Self::tool_router(),
                calls: Default::default(),
            }
        }
    }

    #[tool_router]
    impl Upstream {
        #[tool(name = "echo", description = "Echo a JSON value")]
        fn echo(&self, Parameters(req): Parameters<EchoRequest>) -> String {
            serde_json::json!({"value": req.value, "label": req.label}).to_string()
        }

        #[tool(name = "createIssue", description = "Create an issue")]
        fn create_issue(&self, Parameters(_req): Parameters<CreateIssueRequest>) -> String {
            "ok".to_string()
        }

        #[tool(name = "countCalls", description = "Count this session's calls")]
        fn count_calls(&self) -> String {
            let count = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            format!("call {count}")
        }

        #[tool(name = "getUser", description = "Get a user")]
        fn get_user(&self) -> Json<UserResponse> {
            Json(UserResponse {
                id: "U1".to_string(),
                name: "Ada".to_string(),
            })
        }
    }

    #[tool_handler(router = self.tool_router)]
    impl ServerHandler for Upstream {
        fn get_info(&self) -> ServerInfo {
            ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        }
    }

    /// Bind a mock upstream on an ephemeral port and return its `/mcp` URL.
    pub async fn spawn() -> String {
        let service: StreamableHttpService<Upstream, LocalSessionManager> =
            StreamableHttpService::new(
                || Ok(Upstream::new()),
                Default::default(),
                StreamableHttpServerConfig::default(),
            );
        let router = axum::Router::new().nest_service("/mcp", service);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        format!("http://{addr}/mcp")
    }
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn mcp_virtual_package_discovers_typechecks_and_calls() {
    let url = upstream::spawn().await;
    let bp = Blueprint {
        name: "up-bp".into(),
        mcp: BTreeMap::from([(
            "up".to_string(),
            McpServer {
                transport: "streamable_http".into(),
                url,
                headers: BTreeMap::new(),
                auth: None,
            },
        )]),
        // The server is deny-by-default; grant the one MCP tool this test calls.
        permissions: BTreeMap::from([(
            "main".to_string(),
            vec![PermissionRule {
                name: None,
                capability: "mcp.up".into(),
                filter: None,
                action: Action::Allow,
            }],
        )]),
        ..Default::default()
    };
    let h = Harness::from_blueprints(vec![bp]);
    let session = h.handshake("up-bp").await;

    // Discovery renders the upstream tool surface through `packages.docs`.
    let docs = rpc_call(
        1,
        "submilli__typescript__packages__docs",
        json!({ "name": "@mcp/up" }),
    );
    let (status, _, r) = h.post("up-bp", docs, Some(&session)).await;
    assert_eq!(status, StatusCode::OK);
    // `@mcp/<server>` docs render as a markdown string, like registry packages.
    let decls = text_output(&r);
    assert!(decls.contains("function createIssue"), "docs: {r}");
    assert!(
        decls.contains("did not publish an outputSchema"),
        "docs should tell agents when response shape metadata is absent: {r}"
    );
    assert!(
        decls.contains("function getUser(): { id: string; name: string };"),
        "published outputSchema should keep the existing typed return: {decls}"
    );
    assert!(
        decls.contains("Typed — use the result directly"),
        "a published outputSchema should read as typed use-directly: {decls}"
    );
    assert!(
        !decls.contains("Published outputSchema shape:"),
        "the doc comment must not repeat the type body: {decls}"
    );

    // Search surfaces the discovered server too — matched here by its tool name
    // and tagged `source: "mcp"`, so an agent can find MCP tooling, not just read
    // its docs once the server name is known.
    let search = rpc_call(
        3,
        "submilli__typescript__packages__search",
        json!({ "query": "createIssue" }),
    );
    let (status, _, rs) = h.post("up-bp", search, Some(&session)).await;
    assert_eq!(status, StatusCode::OK);
    let up = output(&rs)["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["name"] == json!("@mcp/up"))
        .unwrap_or_else(|| panic!("@mcp/up missing from search: {rs}"));
    assert_eq!(up["source"], json!("mcp"), "got: {rs}");

    // The typed import resolves, the object arg typechecks, and the call
    // dispatches to the upstream server — proving discovery + codegen + transport
    // all line up. `createIssue` publishes no outputSchema, so it returns
    // `unknown`; the script validates the upstream JSON string with `as string`.
    let code = r#"import up from "@mcp/up"; function main(): string { return up.createIssue({ title: "x" }) as string; }"#;
    let (status, _, r2) = h.post("up-bp", tools_call(2, code), Some(&session)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        output(&r2)["error"].is_null(),
        "call must succeed, got: {r2}"
    );
    assert!(
        output(&r2)["result"].as_str().unwrap_or("").contains("ok"),
        "expected the upstream tool result, got: {r2}",
    );

    assert!(decls.contains("label: string; value: unknown"), "{decls}");
    let code = include_str!("fixtures/mcp_unknown_arguments.ts");
    let (_, _, response) = h.post("up-bp", tools_call(4, code), Some(&session)).await;
    let result = output(&response);
    assert!(result["error"].is_null(), "{result}");
    let actual: Value = serde_json::from_str(result["result"].as_str().unwrap()).unwrap();
    assert_eq!(
        actual,
        json!([
            {"label": "check", "value": {"nested": [1, 2], "flag": true}},
            {"label": "check", "value": [1, 2]},
            {"label": "check", "value": null},
            {"label": "check", "value": 42},
            {"label": "check", "value": "hello"},
        ])
    );
    for args in [r#"{ label: 42, value: null }"#, r#"{ value: null }"#] {
        let code = format!(
            r#"import up from "@mcp/up"; function main(): unknown {{ return up.echo({args}); }}"#
        );
        let (_, _, response) = h.post("up-bp", tools_call(5, &code), Some(&session)).await;
        assert!(
            !output(&response)["error"].is_null(),
            "typed required fields must remain checked: {response}"
        );
    }
}

/// A server that keeps state in its session (a browser page, a cursor) only
/// works if one program's calls reach it as one session.
#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test]
async fn a_program_calls_an_mcp_server_over_one_session() {
    let url = upstream::spawn().await;
    let bp = Blueprint {
        name: "up-bp".into(),
        mcp: BTreeMap::from([(
            "up".to_string(),
            McpServer {
                transport: "streamable_http".into(),
                url,
                headers: BTreeMap::new(),
                auth: None,
            },
        )]),
        permissions: BTreeMap::from([(
            "main".to_string(),
            vec![PermissionRule {
                name: None,
                capability: "mcp.up".into(),
                filter: None,
                action: Action::Allow,
            }],
        )]),
        ..Default::default()
    };
    let h = Harness::from_blueprints(vec![bp]);
    let session = h.handshake("up-bp").await;

    let code = r#"import up from "@mcp/up"; function main(): string { up.countCalls(); up.countCalls(); return up.countCalls() as string; }"#;
    let (status, _, run) = h.post("up-bp", tools_call(1, code), Some(&session)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(output(&run)["result"], json!("call 3"), "got: {run}");

    // The next program starts a session of its own.
    let (_, _, next) = h.post("up-bp", tools_call(2, code), Some(&session)).await;
    assert_eq!(output(&next)["result"], json!("call 3"), "got: {next}");
}

const VOL: &str = "vol";

fn volume_blueprint(volume: &str) -> Blueprint {
    Blueprint {
        name: VOL.into(),
        vfs: VfsConfig::Named {
            volume: volume.into(),
            access: None,
            mounts: Default::default(),
            cwd: None,
            sub_path: None,
        },
        permissions: allow_fs(),
        ..Default::default()
    }
}

/// The MCP route builds its own VFS for every non-`per_session` blueprint, so
/// volume resolution has to hold there too — not only on the session path.
#[tokio::test]
async fn mcp_named_root_mounts_the_declared_volume() {
    let dir = tempfile::tempdir().expect("volume dir");
    let h = Harness::from_blueprints_with_volumes(
        vec![volume_blueprint("work")],
        VolumeTable::from([(
            "work".to_string(),
            VolumeSpec::local_path(dir.path().to_path_buf()),
        )]),
    );
    let session = h.handshake(VOL).await;
    let (_, _, rpc) = h.post(VOL, tools_call(2, WRITE), Some(&session)).await;
    assert!(output(&rpc)["error"].is_null(), "write failed: {rpc}");
    assert!(dir.path().join("a.txt").exists(), "{rpc}");
}

/// The file tools describe themselves as a way to read what an `execute` run wrote,
/// and name the modes where that survives. A named volume is one of them — an agent
/// told otherwise would page a large payload back through a single result instead.
#[tokio::test]
async fn files_tools_see_what_execute_wrote_to_a_named_volume() {
    let dir = tempfile::tempdir().expect("volume dir");
    let h = Harness::from_blueprints_with_volumes(
        vec![volume_blueprint("work")],
        VolumeTable::from([(
            "work".to_string(),
            VolumeSpec::local_path(dir.path().to_path_buf()),
        )]),
    );
    let session = h.handshake(VOL).await;
    let (_, _, w) = h.post(VOL, tools_call(2, WRITE), Some(&session)).await;
    assert!(output(&w)["error"].is_null(), "write failed: {w}");

    let call = rpc_call(3, "submilli__files__read", json!({ "path": "/a.txt" }));
    let (_, _, read) = h.post(VOL, call, Some(&session)).await;
    assert_eq!(output(&read)["content"], json!("hi"), "got: {read}");

    let call = rpc_call(4, "submilli__files__list", json!({}));
    let (_, _, list) = h.post(VOL, call, Some(&session)).await;
    let paths: Vec<&str> = output(&list)["entries"]
        .as_array()
        .expect("entries array")
        .iter()
        .filter_map(|e| e["path"].as_str())
        .collect();
    assert!(paths.contains(&"/a.txt"), "got: {list}");
}

/// A recursive listing reaches a mounted volume's files through the volume, not
/// the empty directory the root holds at its mount point.
#[tokio::test]
async fn files_list_descends_into_a_mounted_volume() {
    let dir = tempfile::tempdir().expect("volume dir");
    std::fs::create_dir(dir.path().join("notes")).unwrap();
    std::fs::write(dir.path().join("notes/a.md"), "a").unwrap();
    let mut blueprint = submilli_blueprint::parse(
        "name: vol\nvfs:\n  mounts:\n    /data/memory: {mode: named, volume: work}\n",
    )
    .unwrap();
    blueprint.permissions = allow_fs();
    let h = Harness::from_blueprints_with_volumes(
        vec![blueprint],
        VolumeTable::from([(
            "work".to_string(),
            VolumeSpec::local_path(dir.path().to_path_buf()),
        )]),
    );
    let session = h.handshake(VOL).await;
    let call = rpc_call(
        2,
        "submilli__files__list",
        json!({ "path": "/data", "recursive": true }),
    );
    let (_, _, list) = h.post(VOL, call, Some(&session)).await;
    let paths: Vec<&str> = output(&list)["entries"]
        .as_array()
        .expect("entries array")
        .iter()
        .filter_map(|e| e["path"].as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "/data/memory",
            "/data/memory/notes",
            "/data/memory/notes/a.md"
        ],
        "got: {list}"
    );
}

#[tokio::test]
async fn mcp_undeclared_volume_fails_by_name_without_a_host_path() {
    let dir = tempfile::tempdir().expect("volume dir");
    let h =
        Harness::from_blueprints_with_volumes(vec![volume_blueprint("gone")], VolumeTable::new());
    let (status, headers, body) = init_with_header(&h, VOL, "").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(headers.get("mcp-session-id").is_none());
    assert!(
        body.contains("gone"),
        "the volume name must reach the client: {body}"
    );
    assert!(body.contains("not declared"), "got {body}");
    assert!(
        !body.contains(dir.path().to_str().unwrap()),
        "no host path may reach the client: {body}"
    );
}

#[tokio::test]
async fn policy_hides_libraries_from_mcp_discovery() {
    for default in ["deny", "allow", "ask-human"] {
        let h = Harness::from_blueprints(vec![
            submilli_blueprint::parse(&format!("name: eph\ndefault: {default}\n")).unwrap(),
        ]);
        let session = h.handshake(EPH).await;
        let (_, _, tools) = h.post(EPH, tools_list(1), Some(&session)).await;
        let visible = default != "deny";
        for name in [
            "submilli:http",
            "submilli:fs",
            "submilli:code",
            "submilli:session",
        ] {
            assert_eq!(tool_desc(&tools, EXECUTE).contains(name), visible);
            let typo = format!("{name}x");
            let (_, _, builtins) = h
                .post(
                    EPH,
                    rpc_call(
                        4,
                        "submilli__typescript__builtins__docs",
                        json!({"names": [name, typo]}),
                    ),
                    Some(&session),
                )
                .await;
            let entries = output(&builtins)["results"].as_array().unwrap().clone();
            assert_eq!(
                entries[0]["error"] == "not_a_builtin",
                visible,
                "{builtins}"
            );
            assert_eq!(entries[1]["did_you_mean"] == name, visible, "{builtins}");
            for query in [name, "", "nothingmatchesthis"] {
                let (_, _, response) = h
                    .post(
                        EPH,
                        rpc_call(
                            2,
                            "submilli__typescript__packages__search",
                            json!({"query": query}),
                        ),
                        Some(&session),
                    )
                    .await;
                assert_eq!(
                    output(&response).to_string().contains(name),
                    visible,
                    "{response}"
                );
            }
            let (_, _, response) = h
                .post(
                    EPH,
                    rpc_call(
                        3,
                        "submilli__typescript__packages__docs",
                        json!({"name": name}),
                    ),
                    Some(&session),
                )
                .await;
            assert_eq!(output(&response)["source"] == "host", visible, "{response}");
            if !visible {
                assert_eq!(output(&response)["error"], "unknown_package");
            }
        }
    }
}

#[tokio::test]
async fn llm_discovery_requires_models_and_permission() {
    let config = "llm:\n  providers:\n    test:\n      type: anthropic\n  models:\n    test-model:\n      provider: test\n";
    for (configuration, policy, visible) in [
        ("", "default: allow\n", false),
        (
            "llm:\n  providers:\n    test:\n      type: anthropic\n",
            "default: allow\n",
            false,
        ),
        (config, "default: deny\n", false),
        (config, "default: allow\n", true),
        (
            config,
            "permissions:\n  main:\n    - capability: llm.call\n      action: ask-human\n",
            true,
        ),
    ] {
        let blueprint =
            submilli_blueprint::parse(&format!("name: eph\n{configuration}{policy}")).unwrap();
        let h = Harness::from_blueprints(vec![blueprint]);
        let session = h.handshake(EPH).await;
        let (_, _, tools) = h.post(EPH, tools_list(1), Some(&session)).await;
        let prompt = tool_desc(&tools, EXECUTE);
        assert_eq!(prompt.contains("submilli:llm"), visible);
        assert_eq!(prompt.contains("Model calls"), visible);
        for query in ["", "submilli:llm", "models", "nothingmatchesthis"] {
            let (_, _, response) = h
                .post(
                    EPH,
                    rpc_call(
                        2,
                        "submilli__typescript__packages__search",
                        json!({"query": query}),
                    ),
                    Some(&session),
                )
                .await;
            assert_eq!(
                output(&response).to_string().contains("submilli:llm"),
                visible,
                "{response}"
            );
        }
        let (_, _, response) = h
            .post(
                EPH,
                rpc_call(
                    3,
                    "submilli__typescript__packages__docs",
                    json!({"name": "submilli:llm"}),
                ),
                Some(&session),
            )
            .await;
        assert_eq!(output(&response)["source"] == "host", visible, "{response}");
        if !visible {
            assert_eq!(output(&response)["error"], "unknown_package");
        }
        let (_, _, response) = h
            .post(
                EPH,
                rpc_call(
                    4,
                    "submilli__typescript__builtins__docs",
                    json!({"names": ["submilli:llm", "submilli:llmx"]}),
                ),
                Some(&session),
            )
            .await;
        let entries = output(&response)["results"].as_array().unwrap().clone();
        assert_eq!(
            entries[0]["error"] == "not_a_builtin",
            visible,
            "{response}"
        );
        assert_eq!(
            entries[1]["did_you_mean"] == "submilli:llm",
            visible,
            "{response}"
        );
    }
}

#[tokio::test]
async fn files_tools_enforce_session_policy_on_a_named_volume() {
    let dir = tempfile::tempdir().expect("volume");
    for user in ["ada", "grace"] {
        std::fs::create_dir(dir.path().join(user)).unwrap();
        std::fs::write(dir.path().join(user).join("secret.txt"), user).unwrap();
    }
    let blueprint = submilli_blueprint::parse(
        r#"
name: vol
vfs: { mode: named, volume: work }
variables:
  user: { required: true }
permissions:
  main:
    - capability: fs.read
      filter: 'path == "/${vars.user}/secret.txt"'
      action: allow
    - capability: fs.list
      filter: 'path == "/${vars.user}" and recursive == false'
      action: allow
"#,
    )
    .unwrap();
    let h = Harness::from_blueprints_with_volumes(
        vec![blueprint],
        VolumeTable::from([(
            "work".to_string(),
            VolumeSpec::local_path(dir.path().to_path_buf()),
        )]),
    );
    for user in ["ada", "grace"] {
        let session = handshake_with_vars(&h, VOL, json!({"user": user})).await;
        let other = if user == "ada" { "grace" } else { "ada" };
        for (tool, args, capability) in [
            (
                "submilli__files__read",
                json!({"path": format!("/{other}/secret.txt")}),
                "fs.read",
            ),
            (
                "submilli__files__list",
                json!({"path": format!("/{other}")}),
                "fs.list",
            ),
            (
                "submilli__files__list",
                json!({"recursive": true}),
                "fs.list",
            ),
            (
                "submilli__files__list",
                json!({"path": format!("/{user}"), "recursive": true}),
                "fs.list",
            ),
        ] {
            let (_, _, rpc) = h.post(VOL, rpc_call(2, tool, args), Some(&session)).await;
            assert!(
                refuses_with(
                    &rpc,
                    &format!("permission denied: caller=main capability={capability}")
                ),
                "{rpc}"
            );
        }
        let (_, _, read) = h
            .post(
                VOL,
                rpc_call(
                    3,
                    "submilli__files__read",
                    json!({"path": format!("/{user}/secret.txt")}),
                ),
                Some(&session),
            )
            .await;
        assert_eq!(output(&read)["content"], user, "{read}");
        let (_, _, list) = h
            .post(
                VOL,
                rpc_call(
                    4,
                    "submilli__files__list",
                    json!({"path": format!("/{user}")}),
                ),
                Some(&session),
            )
            .await;
        assert_eq!(
            output(&list)["entries"][0]["path"],
            format!("/{user}/secret.txt"),
            "{list}"
        );
    }
}

#[tokio::test]
async fn files_tools_default_deny_in_every_vfs_mode() {
    for vfs in [
        VfsConfig::Ephemeral {
            size_limit: None,
            mounts: Default::default(),
            cwd: None,
        },
        VfsConfig::PerSession {
            size_limit: None,
            mounts: Default::default(),
            cwd: None,
        },
        VfsConfig::None,
    ] {
        let h = Harness::from_blueprints(vec![Blueprint {
            name: "denied".into(),
            vfs,
            ..Default::default()
        }]);
        let session = h.handshake("denied").await;
        for (tool, args, capability) in [
            (
                "submilli__files__read",
                json!({"path": "/missing"}),
                "fs.read",
            ),
            ("submilli__files__list", json!({}), "fs.list"),
        ] {
            let (_, _, rpc) = h
                .post("denied", rpc_call(2, tool, args), Some(&session))
                .await;
            assert!(
                refuses_with(
                    &rpc,
                    &format!("permission denied: caller=main capability={capability}")
                ),
                "{rpc}"
            );
        }
    }
}

#[tokio::test]
async fn filesystem_policy_uses_normalized_paths_for_programs_and_file_tools() {
    let dir = tempfile::tempdir().unwrap();
    for user in ["ada", "grace"] {
        std::fs::create_dir(dir.path().join(user)).unwrap();
        std::fs::write(dir.path().join(user).join("secret.txt"), user).unwrap();
    }
    let blueprint = submilli_blueprint::parse(
        r#"
name: vol
vfs: { mode: named, volume: work }
variables:
  user: { required: true }
permissions:
  main:
    - capability: fs.read
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.write
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.stat
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.list
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.mkdir
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.remove
      filter: 'path == "/${vars.user}" or path glob "/${vars.user}/*"'
      action: allow
    - capability: fs.copy
      filter: 'from glob "/${vars.user}/*" and to glob "/${vars.user}/*"'
      action: allow
    - capability: fs.move
      filter: 'from glob "/${vars.user}/*" and to glob "/${vars.user}/*"'
      action: allow
"#,
    )
    .unwrap();
    let h = Harness::from_blueprints_with_volumes(
        vec![blueprint],
        VolumeTable::from([(
            "work".to_string(),
            VolumeSpec::local_path(dir.path().to_path_buf()),
        )]),
    );
    let session = handshake_with_vars(&h, VOL, json!({"user": "ada"})).await;
    for expression in [
        "fs.readText('/ada/../grace/secret.txt')",
        "fs.readBytes('/ada/../grace/secret.txt', 0, 1)",
        "fs.lines('/ada/../grace/secret.txt')",
        "fs.bytes('/ada/../grace/secret.txt', 1)",
        "fs.exists('/ada/../grace/secret.txt')",
        "fs.stat('/ada/../grace/secret.txt')",
        "fs.list('/ada/../grace', true)",
        "fs.writeText('/ada/../grace/new.txt', 'wrong')",
        "fs.mkdir('/ada/../grace/new', false)",
        "fs.remove('/ada/../grace/secret.txt', false)",
        "fs.copy('/ada/../grace/secret.txt', '/ada/copy.txt', false)",
        "fs.copy('/ada/secret.txt', '/ada/../grace/copy.txt', false)",
        "fs.move('/ada/../grace/secret.txt', '/ada/moved.txt')",
        "fs.move('/ada/secret.txt', '/ada/../grace/moved.txt')",
    ] {
        let code =
            format!("import * as fs from 'submilli:fs'; function main(): void {{ {expression}; }}");
        let (_, _, rpc) = h.post(VOL, tools_call(2, &code), Some(&session)).await;
        let error = output(&rpc)["error"].to_string();
        assert!(
            error.contains("permission denied: caller=main capability=fs."),
            "{expression}: {rpc}"
        );
    }
    for (tool, path) in [
        ("submilli__files__read", "/ada/../grace/secret.txt"),
        ("submilli__files__list", "/ada/../grace"),
    ] {
        let (_, _, rpc) = h
            .post(
                VOL,
                rpc_call(3, tool, json!({"path": path})),
                Some(&session),
            )
            .await;
        assert!(
            refuses_with(&rpc, "permission denied: caller=main capability=fs."),
            "{rpc}"
        );
    }
    for path in [
        "ada/secret.txt",
        "/ada/./secret.txt",
        "/grace/../ada/secret.txt",
    ] {
        let code = format!(
            "import {{ readText }} from 'submilli:fs'; function main(): string | null {{ return readText('{path}'); }}"
        );
        let (_, _, rpc) = h.post(VOL, tools_call(4, &code), Some(&session)).await;
        assert_eq!(output(&rpc)["result"], "ada", "{rpc}");
        let (_, _, rpc) = h
            .post(
                VOL,
                rpc_call(5, "submilli__files__read", json!({"path": path})),
                Some(&session),
            )
            .await;
        assert_eq!(output(&rpc)["content"], "ada", "{rpc}");
    }
    let (_, _, rpc) = h
        .post(
            VOL,
            rpc_call(6, "submilli__files__list", json!({"path": "ada/./"})),
            Some(&session),
        )
        .await;
    assert_eq!(
        output(&rpc)["entries"][0]["path"],
        "/ada/secret.txt",
        "{rpc}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("grace/secret.txt")).unwrap(),
        "grace"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("ada/secret.txt")).unwrap(),
        "ada"
    );
    assert_eq!(
        std::fs::read_dir(dir.path().join("grace")).unwrap().count(),
        1
    );
    assert_eq!(
        std::fs::read_dir(dir.path().join("ada")).unwrap().count(),
        1
    );
}

#[path = "common/parser_depth.rs"]
mod parser_depth;

#[test]
fn mcp_parser_depth_is_bounded() {
    parser_depth::isolated_worker("mcp_parser_depth_is_bounded", async {
        let h = Harness::new();
        let session = h.handshake(EPH).await;
        let (status, _, rpc) = h
            .post(
                EPH,
                tools_call(1, &parser_depth::nested_source()),
                Some(&session),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(output(&rpc)["error"]["kind"], "compile_error", "{rpc}");
        assert!(
            rpc.to_string().contains("parser recursion limit exceeded"),
            "{rpc}"
        );
        let (status, _, rpc) = h.post(EPH, tools_call(2, SUM), Some(&session)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(output(&rpc)["error"].is_null(), "{rpc}");
        assert_eq!(output(&rpc)["result"], "2", "{rpc}");
    });
}

#[test]
fn mcp_compiler_structure_limits_are_bounded() {
    parser_depth::isolated_worker("mcp_compiler_structure_limits_are_bounded", async {
        let h = Harness::new();
        let session = h.handshake(EPH).await;
        let (status, _, rpc) = h
            .post(
                EPH,
                tools_call(1, &parser_depth::flat_chain_source()),
                Some(&session),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(output(&rpc)["error"]["kind"], "compile_error", "{rpc}");
        assert!(
            rpc.to_string().contains(parser_depth::SYNTAX_LIMIT_MESSAGE),
            "{rpc}"
        );
        let (status, _, rpc) = h
            .post(
                EPH,
                tools_call(2, &parser_depth::near_limit_chain_source()),
                Some(&session),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert!(output(&rpc)["error"].is_null(), "{rpc}");
        assert_eq!(output(&rpc)["result"], "200", "{rpc}");
    });
}

#[test]
fn mcp_closure_arity_returns_diagnostics() {
    parser_depth::isolated_worker("mcp_closure_arity_returns_diagnostics", async {
        let h = Harness::new();
        let session = h.handshake(EPH).await;
        let (status, _, rpc) = h
            .post(
                EPH,
                tools_call(1, &parser_depth::oversized_closure_source()),
                Some(&session),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        parser_depth::assert_closure_diagnostic(output(&rpc));
        let (status, _, rpc) = h.post(EPH, tools_call(2, SUM), Some(&session)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(output(&rpc)["error"].is_null(), "{rpc}");
        assert_eq!(output(&rpc)["result"], "2", "{rpc}");
    });
}

#[tokio::test]
async fn subpath_cwd_is_shared_by_execute_file_tools_and_prompt() {
    let volume = tempfile::tempdir().unwrap();
    let blueprint = submilli_blueprint::parse(
        r#"name: cwd
variables:
  user: {required: true}
vfs:
  mode: named
  volume: notes
  subPath: users/${vars.user}
  cwd: /drafts/${vars.user}
permissions:
  main:
    - {capability: fs.read, action: allow, filter: 'path glob "/drafts/${vars.user}/*"'}
    - {capability: fs.write, action: allow, filter: 'path glob "/drafts/${vars.user}/*"'}
    - {capability: fs.list, action: allow, filter: 'path == "/drafts/${vars.user}"'}
"#,
    )
    .unwrap();
    let h = Harness::from_blueprints_with_volumes(
        vec![blueprint],
        VolumeTable::from([("notes".into(), VolumeSpec::local_path(volume.path()))]),
    );
    let session = handshake_with_vars(&h, "cwd", json!({"user":"ada"})).await;
    let (_, _, tools) = h.post("cwd", tools_list(2), Some(&session)).await;
    assert!(
        tool_desc(&tools, EXECUTE).contains("Working directory: /drafts/ada."),
        "{tools}"
    );
    let (_, _, written) = h.post("cwd", tools_call(3, r#"import { writeText, cwd } from "submilli:fs"; function main(): string { writeText("a.txt", "hello"); return cwd(); }"#), Some(&session)).await;
    assert!(output(&written)["error"].is_null(), "{written}");
    assert_eq!(output(&written)["result"], "/drafts/ada");
    let (_, _, read) = h
        .post(
            "cwd",
            rpc_call(4, "submilli__files__read", json!({"path":"a.txt"})),
            Some(&session),
        )
        .await;
    assert_eq!(output(&read)["content"], "hello", "{read}");
    let (_, _, listed) = h
        .post(
            "cwd",
            rpc_call(5, "submilli__files__list", json!({})),
            Some(&session),
        )
        .await;
    assert_eq!(output(&listed)["count"], 1, "{listed}");
    assert_eq!(
        std::fs::read_to_string(volume.path().join("users/ada/drafts/ada/a.txt")).unwrap(),
        "hello"
    );
}

#[tokio::test]
async fn execute_ids_cover_argument_validation_and_compile_errors() {
    let h = Harness::new();
    let session = h.handshake(EPH).await;
    for arguments in [json!({}), json!({"code": "function main(): missing {}"})] {
        let (_, _, rpc) = h
            .post(EPH, rpc_call(1, EXECUTE, arguments), Some(&session))
            .await;
        let id = rpc["result"]["structuredContent"]["execution_id"]
            .as_str()
            .or_else(|| rpc["error"]["data"]["execution_id"].as_str())
            .expect("execution ID on refusal");
        uuid::Uuid::parse_str(id).unwrap();
    }
}

#[tokio::test]
async fn last_run_recording_failure_preserves_execution_and_recovers() {
    use std::sync::atomic::Ordering;
    let store = Arc::new(last_run_store::FaultStore::default());
    store.fail_writes.store(true, Ordering::SeqCst);
    let h = Harness::from_blueprints_with_sessions(
        vec![Blueprint {
            name: EPH.into(),
            ..Default::default()
        }],
        Some(store.clone()),
    );
    let session = h.handshake(EPH).await;
    let (_, _, success) = h
        .post(
            EPH,
            tools_call(
                2,
                r#"function main(): number { console.log("captured"); return 7; }"#,
            ),
            Some(&session),
        )
        .await;
    assert_eq!(output(&success)["result"], "7");
    assert_eq!(output(&success)["console"], json!([]));
    assert!(output(&success)["error"].is_null());
    let (_, _, failed) = h.post(EPH, tools_call(3, r#"function main(): void { console.log("before throw"); throw new Error("original failure"); }"#), Some(&session)).await;
    assert_eq!(output(&failed)["console"], json!(["before throw"]));
    assert!(
        output(&failed)["error"]["message"]
            .as_str()
            .unwrap()
            .contains("original failure")
    );
    assert!(!failed.to_string().contains("private backend detail"));
    assert_eq!(store.writes.load(Ordering::SeqCst), 2);
    store.fail_writes.store(false, Ordering::SeqCst);
    let (_, _, healthy) = h
        .post(
            EPH,
            tools_call(4, "function main(): number { return 42; }"),
            Some(&session),
        )
        .await;
    assert_eq!(output(&healthy)["result"], "42");
    let (_, _, stored) = h
        .post(
            EPH,
            rpc_call(5, "submilli__typescript__last_run", json!({})),
            Some(&session),
        )
        .await;
    assert_eq!(output(&stored)["result"], "42");
}

#[tokio::test]
async fn last_run_read_failure_is_internal_and_hides_backend_details() {
    use std::sync::atomic::Ordering;
    let store = Arc::new(last_run_store::FaultStore::default());
    let h = Harness::from_blueprints_with_sessions(
        vec![Blueprint {
            name: EPH.into(),
            ..Default::default()
        }],
        Some(store.clone()),
    );
    let session = h.handshake(EPH).await;
    store.fail_reads.store(true, Ordering::SeqCst);
    for id in [2, 4] {
        if id == 4 {
            h.post(
                EPH,
                tools_call(3, "function main(): number { return 7; }"),
                Some(&session),
            )
            .await;
        }
        let (status, _, response) = h
            .post(
                EPH,
                rpc_call(id, "submilli__typescript__last_run", json!({})),
                Some(&session),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["error"]["code"], -32603, "{response}");
        assert_eq!(response["error"]["message"], "last-run storage unavailable");
        assert!(!response.to_string().contains("private backend detail"));
    }
    store.fail_reads.store(false, Ordering::SeqCst);
    let (_, _, stored) = h
        .post(
            EPH,
            rpc_call(5, "submilli__typescript__last_run", json!({})),
            Some(&session),
        )
        .await;
    assert_eq!(output(&stored)["result"], "7");
}

#[tokio::test]
async fn oversized_diagnostic_is_abbreviated_and_next_request_succeeds() {
    let h = Harness::new();
    let session = h.handshake(EPH).await;
    let source = format!(
        "function main(): number {{ {} return missing; }}",
        " ".repeat(100_000)
    );
    let (_, _, rpc) = h.post(EPH, tools_call(1, &source), Some(&session)).await;
    let error = &output(&rpc)["error"];
    assert_eq!(error["kind"], "compile_error");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains(interpreter::rendering::TRUNCATED));
    assert!(message.len() <= interpreter::rendering::RenderLimits::collection().bytes);
    let (_, _, healthy) = h.post(EPH, tools_call(2, SUM), Some(&session)).await;
    assert_eq!(output(&healthy)["result"], "2");
    assert!(output(&healthy)["error"].is_null());
}
