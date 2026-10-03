//! End-to-end tests for `submilli:session` bound to a server session.
//!
//! The claim under test is that one store handle reaches both execution routes
//! and dies with the session: REST and MCP driven against the *same* session id
//! see one store, two sessions on one blueprint see two, and every way a session
//! ends takes the store with it.

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
    let rules = ["session.read", "session.write", "session.remove"]
        .into_iter()
        .map(|cap| PermissionRule {
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
function main(): string { const v = session.get("k"); return v === null ? "MISSING" : (v as string); }"#;

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
            blueprints: Some(Arc::new(InMemoryBlueprintStore::seed(blueprints))),
            session_storage_root: Some(vfs_root.path().to_path_buf()),
            ..ServerConfig::default()
        };
        Self {
            state: AppState::new(tweak(config)).expect("AppState"),
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

/// Nothing in `SessionRecord` carries KV state, so a restart leaves it behind.
/// Asserted through `boot`, which is the only path that rebuilds an entry from
/// a record.
#[tokio::test]
async fn a_restored_session_starts_with_an_empty_store() {
    let store_dir = tempfile::tempdir().expect("session store dir");
    let vfs_root = tempfile::tempdir().expect("vfs root");
    let build = || {
        let config = ServerConfig {
            blueprints: Some(Arc::new(InMemoryBlueprintStore::seed(vec![blueprint(
                BLUEPRINT,
                VfsConfig::PerSession {
                    size_limit: None,
                    mounts: Default::default(),
                    cwd: None,
                },
            )]))),
            session_storage_root: Some(vfs_root.path().to_path_buf()),
            session_store_dir: Some(store_dir.path().to_path_buf()),
            ..ServerConfig::default()
        };
        AppState::new(config).expect("AppState")
    };

    let first = Harness::over(build());
    let session = first.create_session(BLUEPRINT).await;
    ok(&first.rest(&session, SET).await);

    // Restart: a fresh AppState over the same durable directories.
    let restarted = Harness::over(build());
    restarted.state.boot().await;

    let read = restarted.rest(&session, GET).await;
    assert_eq!(
        ok(&read)["result"],
        json!("MISSING"),
        "session KV is memory-only, so a restored session starts empty"
    );
}

/// The aggregate budget bounds the *sum* across live sessions. The second
/// session's write is refused rather than evicting the first session's entries,
/// and the capacity comes back when a session ends.
#[tokio::test]
async fn the_aggregate_budget_refuses_rather_than_evicting() {
    // Room for one entry of this size, not two.
    const BUDGET: u64 = 128;
    let h = Harness::with_config(vec![blueprint(BLUEPRINT, VfsConfig::None)], |c| {
        ServerConfig {
            max_session_state_memory: Some(BUDGET),
            ..c
        }
    });

    let first = h.create_session(BLUEPRINT).await;
    let second = h.create_session(BLUEPRINT).await;

    ok(&h.rest(&first, SET).await);
    let refused = h.rest(&second, SET).await;
    assert!(
        !refused["error"].is_null(),
        "the second session must be refused: {refused}"
    );
    let message = refused["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("server session-state budget"),
        "the refusal must name the exhausted budget: {message}"
    );
    // Which budget it is decides what to do about it. This session's own data
    // is not what filled the aggregate one, so a message that read like the
    // per-session limit would send the caller to shrink data that cannot help.
    assert!(
        message.contains("max-session-state-memory"),
        "the aggregate refusal must point at the operator's knob: {message}"
    );

    // Nothing was evicted to make room.
    let still_there = h.rest(&first, GET).await;
    assert_eq!(
        ok(&still_there)["result"],
        json!("v1"),
        "an exhausted budget must never silently evict another session"
    );

    // Ending the first session releases its reservation for the second.
    assert_eq!(
        h.delete(&format!("/v1/sessions/{first}")).await,
        StatusCode::NO_CONTENT
    );
    ok(&h.rest(&second, SET).await);
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
/// teardown a one-shot's entry survives to the blueprint's `idle_timeout` — a
/// day by default — holding its share of the server-wide session-state budget.
/// A service using this route as its stateless entry point then ratchets that
/// budget to the cap and every session's writes are refused, its own and other
/// callers' alike.
#[tokio::test]
async fn a_one_shot_releases_its_budget_and_its_session_when_it_returns() {
    // Room for exactly one entry of the size `SET` writes. A one-shot that held
    // its reservation would exhaust this on the first call, so every later call
    // — and the real session at the end — would be refused.
    const BUDGET: u64 = 128;
    let h = Harness::with_config(vec![blueprint(BLUEPRINT, VfsConfig::None)], |c| {
        ServerConfig {
            max_session_state_memory: Some(BUDGET),
            ..c
        }
    });

    for i in 0..8 {
        let run = h.one_shot(SET).await;
        assert!(
            run["error"].is_null(),
            "one-shot {i} was refused, so an earlier one never released its bytes: {run}"
        );
        assert_eq!(
            h.active_sessions().await,
            0,
            "one-shot {i} left its session registered"
        );
    }

    // The budget the one-shots would otherwise have exhausted is intact, so a
    // real session can still write — the failure an operator actually sees.
    let session = h.create_session(BLUEPRINT).await;
    ok(&h.rest(&session, SET).await);
    assert_eq!(ok(&h.rest(&session, GET).await)["result"], json!("v1"));
}

/// A transient session must not be resumable: its id coming back in the
/// response is for reading the run's output, not for driving it again.
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

    // The run's output is still readable, which is what the id is returned for.
    let (status, body) = h.get(&format!("/v1/sessions/{session_id}/last-run")).await;
    assert_eq!(status, StatusCode::OK, "last-run must survive: {body}");
}
