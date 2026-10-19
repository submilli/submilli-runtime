use std::path::Path;
use std::sync::Mutex;

use super::super::volumes::VolumeTable;
use axum::http::{HeaderMap, StatusCode};
use base64::Engine;
use rmcp::transport::streamable_http_server::session::{SessionId, SessionManager};
use serde_json::{Value, json};

use super::super::super::state::StateDir;
use super::super::auth::Role;
use super::super::run::{FinishedRun, RecordedRun, RunEntry, RunHostConfig, RunRecorder, RunStart};
use super::*;

const TOKEN: &str = "app-token-0123456789abcdefghijklmnop";

/// What a test recorder saw start.
#[derive(Default)]
struct Starts(Mutex<Vec<RunStart>>);

struct TestRecorder(Arc<Starts>);

impl RunRecorder for TestRecorder {
    fn start(&self, run: RunStart) -> Option<Arc<dyn RecordedRun>> {
        self.0.0.lock().unwrap().push(run);
        Some(Arc::new(TestRun))
    }
}

struct TestRun;

impl RecordedRun for TestRun {
    fn finish(&self, _run: FinishedRun) {}
}

/// One host over a project state directory, serving `/mcp/fixture`.
struct Harness {
    router: Router,
    sessions: Arc<Sessions>,
    blueprints: Arc<Blueprints>,
    host: Arc<McpHost>,
    starts: Arc<Starts>,
    state: StateDir,
}

async fn harness_in(dir: &Path, yaml: &str) -> Harness {
    let state = StateDir::for_project(dir);
    let sessions = Arc::new(Sessions::boot(&state).await.unwrap());
    let volumes = Arc::new(Volumes::boot(&state, VolumeTable::new()));
    let store_root = dir.join("store");
    std::fs::create_dir_all(&store_root).unwrap();
    let package_store = PackageStore::new(store_root);
    let blueprints = Arc::new(Blueprints::new(
        "fixture".to_owned(),
        Arc::clone(&volumes) as Arc<dyn super::super::blueprints::LocalVolumes>,
        None,
        package_store.clone(),
    ));
    blueprints.apply(yaml, "v1").await.unwrap();
    let starts = Arc::new(Starts::default());
    let mut config = RunHostConfig::new(package_store.clone());
    config.recorder = Some(Arc::new(TestRecorder(Arc::clone(&starts))));
    let run = Arc::new(RunHost::new(config, Arc::clone(&sessions), Arc::clone(&volumes)).unwrap());
    let host = Arc::new(McpHost::new(
        McpParts {
            run,
            sessions: Arc::clone(&sessions),
            blueprints: Arc::clone(&blueprints),
            volumes,
            package_store,
            tokens: Arc::from([ApiToken::new("app", Role::User, TOKEN).unwrap()]),
            events: None,
        },
        "fixture",
    ));
    Harness {
        router: super::super::serve::router_with_mcp(Arc::clone(&host)),
        sessions,
        blueprints,
        host,
        starts,
        state,
    }
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
    text: String,
}

/// A request to `/mcp/fixture` from localhost with the app token.
fn request(method: &str) -> axum::http::request::Builder {
    axum::http::Request::builder()
        .method(method)
        .uri("/mcp/fixture")
        .header("host", "localhost")
        .header("authorization", format!("Bearer {TOKEN}"))
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
}

impl Harness {
    async fn send(&self, req: axum::http::Request<Body>) -> Reply {
        let resp = self.router.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        // A streamable-HTTP reply is SSE: the JSON-RPC message is the last `data:`.
        let data = text
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .next_back()
            .unwrap_or(&text)
            .trim()
            .to_owned();
        Reply {
            status,
            headers,
            body: serde_json::from_str(&data).unwrap_or(Value::Null),
            text,
        }
    }

    async fn post(&self, body: Value, session: Option<&str>) -> Reply {
        let mut builder = request("POST");
        if let Some(session) = session {
            builder = builder.header("mcp-session-id", session);
        }
        self.send(builder.body(Body::from(body.to_string())).unwrap())
            .await
    }

    /// `initialize` then `notifications/initialized`, returning the session id.
    async fn handshake_with(&self, initialize: Value, headers: &[(&str, &str)]) -> String {
        let mut builder = request("POST");
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let reply = self
            .send(builder.body(Body::from(initialize.to_string())).unwrap())
            .await;
        assert_eq!(reply.status, StatusCode::OK, "initialize: {}", reply.text);
        let session = reply.headers["mcp-session-id"].to_str().unwrap().to_owned();
        let reply = self.post(initialized(), Some(&session)).await;
        assert_eq!(reply.status, StatusCode::ACCEPTED, "{}", reply.text);
        session
    }

    async fn handshake(&self) -> String {
        self.handshake_with(initialize(json!({})), &[]).await
    }

    async fn call(&self, session: &str, name: &str, arguments: Value) -> Value {
        let reply = self
            .post(
                json!({
                    "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                    "params": { "name": name, "arguments": arguments }
                }),
                Some(session),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
        reply.body
    }

    async fn delete(&self, session: &str) -> Reply {
        self.send(
            request("DELETE")
                .header("mcp-session-id", session)
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }

    async fn stop(self) {
        drop(self.router);
        drop(self.host);
        self.sessions.close_database().await.unwrap();
    }
}

fn initialize(meta: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "test-client", "version": "0" },
            "_meta": meta,
        }
    })
}

fn initialized() -> Value {
    json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })
}

const PER_SESSION: &str = "name: fixture\nvfs:\n  mode: per_session\npermissions:\n  main:\n    - capability: fs.read\n      action: allow\n";
const NO_VFS: &str = "name: fixture\nvfs:\n  mode: none\n";
const FILES: &str = "name: fixture\nvfs:\n  mode: per_session\npermissions:\n  main:\n    - capability: fs.read\n      action: allow\n    - capability: fs.list\n      action: allow\n";

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/commands/playground/host/mcp/fixtures")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[tokio::test]
async fn tools_list_matches_the_tools_the_server_lists() {
    for (yaml, name) in [
        (PER_SESSION, "tools_list_per_session.json"),
        (NO_VFS, "tools_list_none.json"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let h = harness_in(dir.path(), yaml).await;
        let session = h.handshake().await;
        let reply = h
            .post(
                json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
                Some(&session),
            )
            .await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
        let tools = &reply.body["result"]["tools"];
        assert_eq!(tools.as_array().unwrap().len(), 8);
        assert_eq!(*tools, fixture(name), "{name}");
    }
}

#[tokio::test]
async fn meta_variables_win_over_the_variables_header() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness_in(
        dir.path(),
        "name: fixture\nvfs:\n  mode: none\nvariables:\n  user:\n    default: ada\n  team:\n    default: core\n",
    )
    .await;
    let session = h
        .handshake_with(
            initialize(json!({ "variables": { "user": "from-meta" } })),
            &[("submilli-variables", "user=from-header;team=infra")],
        )
        .await;
    let stored = h.sessions.get(&session).await.unwrap().unwrap();
    assert_eq!(stored.variables["user"], "from-meta");
    assert_eq!(stored.variables["team"], "infra");
}

#[tokio::test]
async fn no_token_or_a_wrong_token_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness_in(dir.path(), NO_VFS).await;
    let body = initialize(json!({})).to_string();
    let none = axum::http::Request::builder()
        .method("POST")
        .uri("/mcp/fixture")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .body(Body::from(body.clone()))
        .unwrap();
    assert_eq!(h.send(none).await.status, StatusCode::UNAUTHORIZED);
    let mut wrong = request("POST").body(Body::from(body)).unwrap();
    wrong.headers_mut().insert(
        "authorization",
        "Bearer wrong-token-0123456789abcdefghijklmn"
            .parse()
            .unwrap(),
    );
    assert_eq!(h.send(wrong).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_valid_token_from_a_non_local_host_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness_in(dir.path(), NO_VFS).await;
    let mut req = request("POST")
        .body(Body::from(initialize(json!({})).to_string()))
        .unwrap();
    req.headers_mut()
        .insert("host", "evil.example".parse().unwrap());
    let reply = h.send(req).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.text);
}

#[tokio::test]
async fn another_blueprint_name_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness_in(dir.path(), NO_VFS).await;
    let mut req = request("POST")
        .body(Body::from(initialize(json!({})).to_string()))
        .unwrap();
    *req.uri_mut() = "/mcp/other".parse().unwrap();
    let reply = h.send(req).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.text, "unknown blueprint: other");
}

const SECRETS: &str = "name: fixture\nvfs:\n  mode: none\nsecrets:\n  TOKEN:\n    harness:\n      required: true\n  OTHER:\n    harness:\n      required: true\n";
const META_SECRET: &str = "meta-secret-value-4f1c";
const HEADER_SECRET: &str = "header-secret-value-9b2e";

/// Every form a secret could take on disk.
fn encodings(secret: &str) -> Vec<String> {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    vec![
        secret.to_owned(),
        STANDARD.encode(secret),
        STANDARD_NO_PAD.encode(secret),
        URL_SAFE.encode(secret),
        URL_SAFE_NO_PAD.encode(secret),
        percent_encoding::utf8_percent_encode(secret, percent_encoding::NON_ALPHANUMERIC)
            .to_string(),
    ]
}

#[tokio::test]
async fn secrets_from_meta_and_header_bind_but_never_reach_the_stored_state() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness_in(dir.path(), SECRETS).await;
    let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(json!({ "OTHER": HEADER_SECRET }).to_string());
    let session = h
        .handshake_with(
            initialize(json!({ "secrets": { "TOKEN": META_SECRET } })),
            &[("submilli-secrets", &header)],
        )
        .await;
    let result = h
        .call(
            &session,
            "submilli__typescript__execute",
            json!({ "code": "function main(): number { return 1; }" }),
        )
        .await;
    assert_eq!(
        result["result"]["structuredContent"]["result"], "1",
        "{result}"
    );
    {
        let starts = h.starts.0.lock().unwrap();
        let secrets = &starts.last().unwrap().harness_secrets;
        assert_eq!(secrets["TOKEN"], META_SECRET);
        assert_eq!(secrets["OTHER"], HEADER_SECRET);
    }
    let stored = h.sessions.mcp_state(&session).await.unwrap().unwrap();
    assert!(stored.contains("test-client"), "{stored}");
    let database = h.state.host_database_path();
    h.stop().await;
    let mut on_disk = Vec::new();
    for suffix in ["", "-wal", "-shm"] {
        let mut path = database.clone().into_os_string();
        path.push(suffix);
        if let Ok(bytes) = std::fs::read(&path) {
            on_disk.extend(bytes);
        }
    }
    let on_disk = String::from_utf8_lossy(&on_disk);
    for secret in [META_SECRET, HEADER_SECRET] {
        for form in encodings(secret) {
            assert!(!stored.contains(&form), "stored state holds {form}");
            assert!(!on_disk.contains(&form), "database holds {form}");
        }
    }
}

#[tokio::test]
async fn a_session_that_requires_a_secret_it_has_none_of_is_told_which() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness_in(dir.path(), NO_VFS).await;
    let session = h.handshake().await;
    h.stop().await;
    // After a restart the session's secrets are gone; the blueprint now needs one.
    let h = harness_in(dir.path(), NO_VFS).await;
    let reply = h
        .post(
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            Some(&session),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    h.blueprints
        .apply(
            "name: fixture\nvfs:\n  mode: none\nsecrets:\n  TOKEN:\n    harness:\n      required: true\n",
            "v2",
        )
        .await
        .unwrap();
    let reply = h
        .call(
            &session,
            "submilli__typescript__execute",
            json!({ "code": "function main(): number { return 1; }" }),
        )
        .await;
    assert_eq!(
        reply["error"]["message"], "session_requires_secrets",
        "{reply}"
    );
    assert_eq!(reply["error"]["data"]["required"], json!(["TOKEN"]));
}

const PERSIST: &str = "name: fixture\ndefault: allow\nvfs:\n  mode: per_session\n";

#[tokio::test]
async fn an_agent_session_resumes_with_its_files_and_values_after_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness_in(dir.path(), PERSIST).await;
    let session = h.handshake().await;
    let write = h
        .call(
            &session,
            "submilli__typescript__execute",
            json!({ "code": r#"import * as fs from "submilli:fs";
            import * as session from "submilli:session";
            function main(): void { fs.writeText("/note.txt", "kept"); session.set("count", 3); }"# }),
        )
        .await;
    assert!(
        write["result"]["structuredContent"]["error"].is_null(),
        "{write}"
    );
    h.stop().await;

    let h = harness_in(dir.path(), PERSIST).await;
    let read = h
        .call(
            &session,
            "submilli__typescript__execute",
            json!({ "code": r#"import * as fs from "submilli:fs";
            import * as session from "submilli:session";
            function main(): string { return `${fs.readText("/note.txt")}${session.get<number>("count")}`; }"# }),
        )
        .await;
    let content = &read["result"]["structuredContent"];
    assert_eq!(content["result"], "kept3", "{read}");
    assert!(content["execution_id"].is_string(), "{read}");
    let starts = h.starts.0.lock().unwrap();
    let start = starts.last().unwrap();
    assert_eq!(start.session_id, session);
    assert_eq!(start.client.as_deref(), Some("test-client"));
    assert_eq!(start.label, "app");
}

#[tokio::test]
async fn the_tool_use_id_reaches_the_runs_record() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness_in(dir.path(), NO_VFS).await;
    let session = h.handshake().await;
    let reply = h
        .post(
            json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {
                    "name": "submilli__typescript__execute",
                    "arguments": { "code": "function main(): number { return 1; }" },
                    "_meta": { "claudecode/toolUseId": "toolu_01" }
                }
            }),
            Some(&session),
        )
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.text);
    let starts = h.starts.0.lock().unwrap();
    assert_eq!(
        starts.last().unwrap().tool_call_id.as_deref(),
        Some("toolu_01")
    );
}

#[tokio::test]
async fn an_unknown_session_is_not_found_and_delete_closes_a_live_one() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness_in(dir.path(), PERSIST).await;
    let unknown = uuid::Uuid::new_v4().to_string();
    let reply = h
        .post(
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            Some(&unknown),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND, "{}", reply.text);
    assert_eq!(h.delete(&unknown).await.status, StatusCode::NOT_FOUND);

    let session = h.handshake().await;
    let dir_of_session = h.sessions.session_dir(&session);
    assert!(dir_of_session.exists());
    let reply = h.delete(&session).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT, "{}", reply.text);
    assert!(h.sessions.get(&session).await.unwrap().is_none());
    assert!(!dir_of_session.exists());
    assert_eq!(h.delete(&session).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_close_that_fails_fails_the_delete_with_its_error() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness_in(dir.path(), PERSIST).await;
    let session = h.handshake().await;
    h.sessions.close_database().await.unwrap();
    let id = SessionId::from(session.as_str());
    let closed = EXPLICIT_DELETE
        .scope(true, h.host.sessions.close_session(&id))
        .await;
    let error = closed.expect_err("the close fails").to_string();
    assert!(
        error.contains("closed") || error.contains("pool"),
        "{error}"
    );
}

#[tokio::test]
async fn files_read_returns_a_window_past_the_default_and_marks_long_lines() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness_in(dir.path(), FILES).await;
    let session = h.handshake().await;
    let mut text = "x".repeat(2100);
    for line in 2..=2500 {
        text.push_str(&format!("\nline {line}"));
    }
    std::fs::write(h.sessions.session_dir(&session).join("big.txt"), text).unwrap();
    let reply = h
        .call(
            &session,
            "submilli__files__read",
            json!({ "path": "/big.txt" }),
        )
        .await;
    let read = &reply["result"]["structuredContent"];
    assert_eq!(read["returned_lines"], 2000, "{reply}");
    assert_eq!(read["has_more"], true);
    assert_eq!(read["next_offset"], 2001);
    assert_eq!(read["truncated_lines"], 1);
    assert!(
        read["content"]
            .as_str()
            .unwrap()
            .contains("…[line truncated]")
    );
}

#[tokio::test]
async fn files_read_of_a_denied_path_is_permission_denied() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness_in(dir.path(), "name: fixture\nvfs:\n  mode: per_session\n").await;
    let session = h.handshake().await;
    let reply = h
        .call(
            &session,
            "submilli__files__read",
            json!({ "path": "/a.txt" }),
        )
        .await;
    let result = &reply["result"];
    assert_eq!(result["isError"], true, "{reply}");
    assert_eq!(
        result["structuredContent"]["error"]["kind"],
        "permission_denied"
    );
    let starts = h.starts.0.lock().unwrap();
    assert_eq!(
        starts.last().unwrap().entry,
        RunEntry::McpFileTool {
            tool: "submilli__files__read".into()
        }
    );
}

#[tokio::test]
async fn a_string_boolean_is_accepted_and_an_unknown_argument_is_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let h = harness_in(dir.path(), FILES).await;
    let session = h.handshake().await;
    let listed = h
        .call(
            &session,
            "submilli__files__list",
            json!({ "recursive": "true" }),
        )
        .await;
    assert_eq!(
        listed["result"]["structuredContent"]["count"], 0,
        "{listed}"
    );
    let unknown = h
        .call(&session, "submilli__files__list", json!({ "bogus": 1 }))
        .await;
    assert_eq!(unknown["result"]["isError"], true, "{unknown}");
    assert_eq!(
        unknown["result"]["structuredContent"]["error"]["kind"],
        "invalid_arguments"
    );
}

#[test]
fn the_variables_header_parses_key_value_pairs() {
    let vars = session::parse_variable_header("  user_id = u_42 ; ; token=a=b ;").unwrap();
    assert_eq!(vars["user_id"], "u_42");
    assert_eq!(vars["token"], "a=b");
    assert!(session::parse_variable_header("user_id").is_err());
    assert!(session::parse_variable_header("=value").is_err());
}
