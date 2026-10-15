//! End-to-end tests for `submilli:session` bound to a server session.
//!
//! The claim under test is that one session's state reaches both execution
//! routes, survives a restart, and dies with the session: REST and MCP driven
//! against the *same* session id see one store, two sessions on one blueprint
//! see two, a restarted server still holds it, and every way a session ends
//! takes it away.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_blueprint::{Action, Blueprint, PermissionRule, VfsConfig};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

const BLUEPRINT: &str = "kv";

/// The server is deny-by-default, so the session capabilities must be granted
/// before the storage behaviour under test is reached.
fn allow_session() -> BTreeMap<String, Vec<PermissionRule>> {
    let rules = [
        "session.read",
        "session.write",
        "session.remove",
        "session.list",
    ]
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

const SET: &str =
    r#"import session from "submilli:session"; function main(): void { session.set("k", "v1"); }"#;
const SET_V2: &str =
    r#"import session from "submilli:session"; function main(): void { session.set("k", "v2"); }"#;
const GET: &str = r#"import session from "submilli:session";
function main(): string { const v = session.get("k"); return v === undefined ? "MISSING" : (v as string); }"#;

fn blueprint(name: &str, vfs: VfsConfig) -> Blueprint {
    Blueprint {
        name: name.into(),
        vfs,
        permissions: allow_session(),
        ..Default::default()
    }
}

struct Harness {
    state: AppState,
    /// `None` when the caller owns the directory — the restart test keeps one
    /// root alive across two harnesses.
    _vfs_root: Option<tempfile::TempDir>,
}

impl Harness {
    fn new() -> Self {
        Self::with_blueprints(vec![blueprint(BLUEPRINT, VfsConfig::None)])
    }

    fn with_blueprints(blueprints: Vec<Blueprint>) -> Self {
        Self::with_config(blueprints, |c| c)
    }

    fn with_config(
        blueprints: Vec<Blueprint>,
        tweak: impl FnOnce(ServerConfig) -> ServerConfig,
    ) -> Self {
        let vfs_root = tempfile::tempdir().expect("vfs root");
        let config = ServerConfig {
            blueprints: Some(Arc::new(
                InMemoryBlueprintStore::seed(blueprints).expect("seed blueprints"),
            )),
            volume_root: Some(vfs_root.path().to_path_buf()),
            ..in_memory_config::config()
        };
        Self {
            state: futures::executor::block_on(AppState::new(tweak(config))).expect("AppState"),
            _vfs_root: Some(vfs_root),
        }
    }

    /// A harness over a state the caller built, for tests that need two
    /// `AppState`s over one set of durable directories.
    fn over(state: AppState) -> Self {
        Self {
            state,
            _vfs_root: None,
        }
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
        (status, headers, parse_body(&content_type, &bytes))
    }

    async fn post(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("POST")
            .uri(uri)
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, _, body) = self.send(req).await;
        (status, body)
    }

    async fn get(&self, uri: &str) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("GET")
            .uri(uri)
            .header("host", "localhost")
            .body(Body::empty())
            .unwrap();
        let (status, _, body) = self.send(req).await;
        (status, body)
    }

    /// Live sessions as the server reports them, which is the same count an
    /// operator reads from `submilli server status`.
    async fn active_sessions(&self) -> u64 {
        self.get("/v1/status").await.1["active_sessions"]
            .as_u64()
            .expect("active_sessions count")
    }

    async fn delete(&self, uri: &str) -> StatusCode {
        let req = Request::builder()
            .method("DELETE")
            .uri(uri)
            .header("host", "localhost")
            .body(Body::empty())
            .unwrap();
        self.send(req).await.0
    }

    async fn create_session(&self, name: &str) -> String {
        let (status, body) = self
            .post("/v1/sessions", json!({ "blueprint": name }))
            .await;
        assert_eq!(status, StatusCode::OK, "create session: {body}");
        body["session_id"].as_str().expect("session id").to_string()
    }

    /// Run `code` in an existing session over REST.
    async fn rest(&self, session: &str, code: &str) -> Value {
        let (status, body) = self
            .post(
                &format!("/v1/sessions/{session}/execute"),
                json!({ "code": code }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "rest execute: {body}");
        body
    }

    /// One-shot `POST /v1/execute` — a transient session minted per call.
    async fn one_shot(&self, code: &str) -> Value {
        let (status, body) = self
            .post(
                "/v1/execute",
                json!({ "code": code, "blueprint": BLUEPRINT }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "one-shot execute: {body}");
        body
    }

    /// Drive the MCP `execute` tool against an *existing* session id. rmcp
    /// rejects a non-initialize request without a known session, so the caller
    /// hands us an id the transport already knows.
    async fn mcp(&self, name: &str, session: &str, code: &str) -> Value {
        let req = Request::builder()
            .method("POST")
            .uri(format!("/mcp/{name}"))
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-session-id", session)
            .body(Body::from(
                json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "tools/call",
                    "params": {
                        "name": "submilli__typescript__execute",
                        "arguments": { "code": code },
                    }
                })
                .to_string(),
            ))
            .unwrap();
        let (status, _, body) = self.send(req).await;
        assert_eq!(status, StatusCode::OK, "mcp execute: {body}");
        let text = body["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("mcp tool result: {body}"));
        serde_json::from_str(text).unwrap_or_else(|_| json!({ "raw": text }))
    }

    /// The MCP handshake, returning the session id the transport assigned.
    async fn mcp_handshake(&self, name: &str) -> String {
        let req = Request::builder()
            .method("POST")
            .uri(format!("/mcp/{name}"))
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .body(Body::from(
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
                .to_string(),
            ))
            .unwrap();
        let (status, headers, body) = self.send(req).await;
        assert_eq!(status, StatusCode::OK, "initialize: {body}");
        let session = headers
            .get("mcp-session-id")
            .expect("initialize must assign a session id")
            .to_str()
            .unwrap()
            .to_string();
        let (status, _) = self
            .post_to(
                &format!("/mcp/{name}"),
                json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
                &session,
            )
            .await;
        assert_eq!(status, StatusCode::ACCEPTED, "initialized notification");
        session
    }

    async fn post_to(&self, uri: &str, body: Value, session: &str) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("POST")
            .uri(uri)
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-session-id", session)
            .body(Body::from(body.to_string()))
            .unwrap();
        let (status, _, body) = self.send(req).await;
        (status, body)
    }
}

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

fn ok(body: &Value) -> &Value {
    assert!(body["error"].is_null(), "execution failed: {body}");
    body
}

#[tokio::test]
async fn two_executes_in_one_session_share_state() {
    let h = Harness::new();
    let session = h.create_session(BLUEPRINT).await;

    ok(&h.rest(&session, SET).await);
    let read = h.rest(&session, GET).await;
    assert_eq!(ok(&read)["result"], json!("v1"));
}

#[tokio::test]
async fn two_sessions_on_one_blueprint_do_not_share_state() {
    let h = Harness::new();
    let first = h.create_session(BLUEPRINT).await;
    let second = h.create_session(BLUEPRINT).await;

    ok(&h.rest(&first, SET).await);
    let read = h.rest(&second, GET).await;
    assert_eq!(
        ok(&read)["result"],
        json!("MISSING"),
        "a second session must start empty"
    );
}

/// The parity claim: one session id, both transports, one store. The write goes
/// in over MCP and comes back out over REST, and then the reverse — so neither
/// direction can pass by accident on a route-local store.
#[tokio::test]
async fn rest_and_mcp_reach_one_store_for_one_session() {
    let h = Harness::new();
    let session = h.mcp_handshake(BLUEPRINT).await;

    ok(&h.mcp(BLUEPRINT, &session, SET).await);
    let over_rest = h.rest(&session, GET).await;
    assert_eq!(
        ok(&over_rest)["result"],
        json!("v1"),
        "an MCP write must be visible to a REST execute in the same session"
    );

    ok(&h.rest(&session, SET_V2).await);
    let over_mcp = h.mcp(BLUEPRINT, &session, GET).await;
    assert_eq!(
        ok(&over_mcp)["result"],
        json!("v2"),
        "a REST write must be visible to an MCP execute in the same session"
    );
}

/// Session state is independent of the VFS: a blueprint with no filesystem at
/// all still gets one, and an `ephemeral` VFS — a fresh temp dir per execute —
/// does not make the KV store per-execute too.
#[tokio::test]
async fn state_survives_every_vfs_mode() {
    for (name, vfs) in [
        ("none", VfsConfig::None),
        (
            "ephemeral",
            VfsConfig::Ephemeral {
                size_limit: None,
                mounts: Default::default(),
                cwd: None,
            },
        ),
        (
            "per_session",
            VfsConfig::PerSession {
                size_limit: None,
                mounts: Default::default(),
                cwd: None,
            },
        ),
    ] {
        let h = Harness::with_blueprints(vec![blueprint(name, vfs)]);
        let session = h.create_session(name).await;
        ok(&h.rest(&session, SET).await);
        let read = h.rest(&session, GET).await;
        assert_eq!(
            ok(&read)["result"],
            json!("v1"),
            "session state must persist under vfs mode {name}"
        );
    }
}

#[tokio::test]
async fn explicit_delete_clears_the_store() {
    let h = Harness::new();
    let session = h.create_session(BLUEPRINT).await;
    ok(&h.rest(&session, SET).await);

    assert_eq!(
        h.delete(&format!("/v1/sessions/{session}")).await,
        StatusCode::NO_CONTENT
    );

    // The session id is gone, so an execute against it 404s rather than
    // resurrecting the state under a rebuilt entry.
    let (status, body) = h
        .post(
            &format!("/v1/sessions/{session}/execute"),
            json!({ "code": GET }),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // And re-creating a session — a fresh id on the same blueprint — starts
    // empty rather than finding the terminated session's entries.
    let reborn = h.create_session(BLUEPRINT).await;
    let read = h.rest(&reborn, GET).await;
    assert_eq!(ok(&read)["result"], json!("MISSING"));
}

/// Unregistering a blueprint terminates the sessions bound to it, so their
/// state goes with them. (Idle expiry takes the same `remove_entry` path; it is
/// asserted in the manager's own tests, which can drive the clock.)
#[tokio::test]
async fn blueprint_removal_clears_the_store() {
    let h = Harness::new();
    let session = h.create_session(BLUEPRINT).await;
    ok(&h.rest(&session, SET).await);

    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/v1/blueprints/{BLUEPRINT}"))
        .header("host", "localhost")
        .body(Body::empty())
        .unwrap();
    let (status, _, _) = h.send(req).await;
    assert_eq!(status, StatusCode::OK, "unregister the blueprint");

    let (status, _) = h
        .post(
            &format!("/v1/sessions/{session}/execute"),
            json!({ "code": GET }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "removing the blueprint takes its sessions' state with it"
    );
}

fn persistent_config(
    store_dir: &std::path::Path,
    vfs_root: &std::path::Path,
    cipher: Option<Arc<submilli_shared::secret_store::SecretCipher>>,
) -> ServerConfig {
    ServerConfig {
        blueprints: Some(Arc::new(
            InMemoryBlueprintStore::seed(vec![blueprint(
                BLUEPRINT,
                VfsConfig::PerSession {
                    size_limit: None,
                    mounts: Default::default(),
                    cwd: None,
                },
            )])
            .expect("seed blueprints"),
        )),
        volume_root: Some(vfs_root.to_path_buf()),
        database_path: Some(store_dir.join("server.db")),
        session_cipher: cipher,
        ..in_memory_config::config()
    }
}

/// Session state is in the database, so a restarted server finds it where the
/// session left it.
#[tokio::test]
async fn a_restored_session_keeps_its_values() {
    let store_dir = tempfile::tempdir().expect("session store dir");
    let vfs_root = tempfile::tempdir().expect("vfs root");
    let build = async || {
        AppState::new(persistent_config(store_dir.path(), vfs_root.path(), None))
            .await
            .expect("AppState")
    };

    let first = Harness::over(build().await);
    let session = first.create_session(BLUEPRINT).await;
    ok(&first.rest(&session, SET).await);

    first.state.database().unwrap().close().await.unwrap();
    // Restart: a fresh AppState over the same durable directories.
    let restarted = Harness::over(build().await);
    restarted.state.boot().await.expect("boot");

    let read = restarted.rest(&session, GET).await;
    assert_eq!(ok(&read)["result"], json!("v1"));
}

const FIRST_PAGE: &str = r#"import session from "submilli:session";
function main(): string {
    session.set("a", 1); session.set("b", 2); session.set("c", 3);
    const page = session.list("", 1);
    return page.nextCursor as string;
}"#;

fn next_page(cursor: &str) -> String {
    format!(
        r#"import session from "submilli:session";
function main(): string {{
    try {{
        const page = session.list("", 1, "{cursor}");
        return page.entries[0].key;
    }} catch (e: TypeError) {{
        return "REFUSED";
    }}
}}"#
    )
}

/// A listing a client was paging through before a restart carries on after it:
/// the cursor keys are derived from the server's key, not made up per process.
/// A server restarted under another key no longer holds the one that sealed the
/// cursor, and refuses it.
#[tokio::test]
async fn a_list_cursor_survives_a_restart_under_the_same_key() {
    let key = |byte| {
        Some(Arc::new(
            submilli_shared::secret_store::SecretCipher::from_key([byte; 32]),
        ))
    };
    for (after_restart, expected) in [(key(3), "b"), (key(4), "REFUSED")] {
        let store_dir = tempfile::tempdir().expect("session store dir");
        let vfs_root = tempfile::tempdir().expect("vfs root");
        let build = async |cipher| {
            AppState::new(persistent_config(store_dir.path(), vfs_root.path(), cipher))
                .await
                .expect("AppState")
        };

        let first = Harness::over(build(key(3)).await);
        let session = first.create_session(BLUEPRINT).await;
        let page = first.rest(&session, FIRST_PAGE).await;
        let cursor = ok(&page)["result"].as_str().expect("cursor").to_owned();
        first.state.database().unwrap().close().await.unwrap();

        let restarted = Harness::over(build(after_restart).await);
        restarted.state.boot().await.expect("boot");
        let read = restarted.rest(&session, &next_page(&cursor)).await;
        assert_eq!(ok(&read)["result"], json!(expected));
    }
}

/// The no-provider refusal is what this wiring exists to prevent, and it is
/// what the red state of these tests looked like. A runtime the server built
/// must never produce it — which is a stronger claim than "the write
/// succeeded", because a silently-per-execution store would also succeed here
/// and only fail the sharing tests above.
#[tokio::test]
async fn a_server_built_runtime_never_reports_a_missing_provider() {
    const CATCH: &str = r#"import session from "submilli:session";
function main(): string {
    try {
        session.set("k", "v");
        return "WROTE";
    } catch (e: Error) {
        return e.message;
    }
}"#;
    let h = Harness::new();

    let session = h.create_session(BLUEPRINT).await;
    let over_rest = h.rest(&session, CATCH).await;
    assert_eq!(
        ok(&over_rest)["result"],
        json!("WROTE"),
        "REST: the configuration error must not be reachable"
    );

    // A provider missing on only one route is exactly the failure this wiring
    // exists to prevent, so MCP is asserted separately rather than assumed.
    let mcp_session = h.mcp_handshake(BLUEPRINT).await;
    let over_mcp = h.mcp(BLUEPRINT, &mcp_session, CATCH).await;
    assert_eq!(
        ok(&over_mcp)["result"],
        json!("WROTE"),
        "MCP: the configuration error must not be reachable"
    );
}

/// One-shot `POST /v1/execute` mints a transient session per call, so its store
/// has nothing to outlive the call.
///
/// A fresh id per call makes this pass on its own, which is why it is not the
/// whole claim — see the release and resumability tests below.
#[tokio::test]
async fn one_shot_executions_get_a_throwaway_store() {
    let h = Harness::new();
    ok(&h.one_shot(SET).await);
    let read = h.one_shot(GET).await;
    assert_eq!(
        ok(&read)["result"],
        json!("MISSING"),
        "each one-shot execute is its own session, so its state is discarded"
    );
}

/// The claim a fresh id per call cannot carry: the transient session is *torn
/// down*, not merely unreachable by a new id.
///
/// `execute_core` registers every session it runs, so without an explicit
/// teardown a one-shot's session would survive to the blueprint's
/// `idle_timeout` — a day by default — holding its state in the database.
#[tokio::test]
async fn a_one_shot_releases_its_session_when_it_returns() {
    let h = Harness::new();
    for i in 0..8 {
        let run = h.one_shot(SET).await;
        assert!(run["error"].is_null(), "one-shot {i} failed: {run}");
        assert_eq!(
            h.active_sessions().await,
            0,
            "one-shot {i} left its session registered"
        );
    }
    let session = h.create_session(BLUEPRINT).await;
    ok(&h.rest(&session, SET).await);
    assert_eq!(ok(&h.rest(&session, GET).await)["result"], json!("v1"));
}

/// A transient session must not be resumable, and its last run ends with it:
/// the response already carried the run's whole output.
#[tokio::test]
async fn a_one_shot_session_id_cannot_be_executed_against() {
    let h = Harness::new();
    let first = h.one_shot(SET).await;
    let session_id = first["session_id"]
        .as_str()
        .expect("session id")
        .to_string();

    let (status, body) = h
        .post(
            &format!("/v1/sessions/{session_id}/execute"),
            json!({ "code": GET }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a one-shot session must be gone once its run returns: {body}"
    );

    let (status, body) = h.get(&format!("/v1/sessions/{session_id}/last-run")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}
