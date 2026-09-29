//! End-to-end tests for the VFS session lifecycle over the HTTP API.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_blueprint::{Action, Blueprint, PermissionRule, VfsConfig};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::config::VolumeTable;
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

const BLUEPRINT: &str = "bp";

/// Grant the fs capabilities the lifecycle scripts use. The server is
/// deny-by-default, so the writes/reads would otherwise be denied before the
/// VFS behaviour under test is reached.
fn allow_fs() -> BTreeMap<String, Vec<PermissionRule>> {
    let rules = ["fs.read", "fs.write"]
        .into_iter()
        .map(|cap| PermissionRule {
            capability: cap.into(),
            filter: None,
            action: Action::Allow,
        })
        .collect();
    BTreeMap::from([("main".to_string(), rules)])
}

struct Harness {
    state: AppState,
    _vfs_root: tempfile::TempDir,
}

impl Harness {
    fn with_vfs(vfs: VfsConfig) -> Self {
        Self::with_vfs_and_volumes(vfs, VolumeTable::new())
    }

    fn with_vfs_and_volumes(vfs: VfsConfig, volumes: VolumeTable) -> Self {
        let vfs_root = tempfile::tempdir().expect("vfs root");
        let blueprints = Arc::new(InMemoryBlueprintStore::seed([Blueprint {
            name: BLUEPRINT.into(),
            vfs,
            permissions: allow_fs(),
            ..Default::default()
        }]));
        let config = ServerConfig {
            blueprints: Some(blueprints),
            session_storage_root: Some(vfs_root.path().to_path_buf()),
            volumes,
            ..ServerConfig::default()
        };
        Self {
            state: AppState::new(config).expect("AppState"),
            _vfs_root: vfs_root,
        }
    }

    async fn send(&self, req: Request<Body>) -> (StatusCode, Value) {
        let resp = app(self.state.clone()).oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, body)
    }

    async fn post(&self, uri: &str, body: Value, session: Option<&str>) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(s) = session {
            builder = builder.header("mcp-session-id", s);
        }
        self.send(builder.body(Body::from(body.to_string())).unwrap())
            .await
    }

    async fn delete(&self, uri: &str) -> StatusCode {
        let req = Request::builder()
            .method("DELETE")
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        self.send(req).await.0
    }

    /// Run code either one-shot (`None`) or against an existing session
    /// (`Some(id)` → `/v1/sessions/{id}/execute`). Session state (a `per_session`
    /// VFS) only persists on the session path.
    async fn execute(&self, code: &str, session: Option<&str>) -> (StatusCode, Value) {
        match session {
            Some(id) => {
                self.post(
                    &format!("/v1/sessions/{id}/execute"),
                    json!({ "code": code }),
                    None,
                )
                .await
            }
            None => {
                self.post(
                    "/v1/execute",
                    json!({ "code": code, "blueprint": BLUEPRINT }),
                    None,
                )
                .await
            }
        }
    }
}

fn per_session() -> VfsConfig {
    VfsConfig::PerSession { size_limit: None }
}

const WRITE: &str = r#"import { writeText } from "submilli:fs"; function main(): void { writeText("/a.txt", "hi"); }"#;
const READ: &str = r#"import { readText } from "submilli:fs"; function main(): string | null { return readText("/a.txt"); }"#;

#[tokio::test]
async fn per_session_persists_across_executes() {
    let h = Harness::with_vfs(per_session());
    let (status, created) = h
        .post("/v1/sessions", json!({ "blueprint": BLUEPRINT }), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let session = created["session_id"].as_str().unwrap().to_string();

    let (_, w) = h.execute(WRITE, Some(&session)).await;
    assert!(w["error"].is_null(), "write failed: {w}");

    let (_, r) = h.execute(READ, Some(&session)).await;
    assert_eq!(r["result"], json!("hi"), "file should persist across calls");
}

/// Each execute measures what the session's directory already holds, so the
/// limit spans the whole session rather than resetting with every program.
#[tokio::test]
async fn per_session_size_limit_spans_the_session() {
    let h = Harness::with_vfs(VfsConfig::PerSession {
        size_limit: Some(100),
    });
    let (_, created) = h
        .post("/v1/sessions", json!({ "blueprint": BLUEPRINT }), None)
        .await;
    let session = created["session_id"].as_str().unwrap().to_string();
    let write = |name: &str| {
        format!(
            r#"import {{ writeText }} from "submilli:fs"; function main(): void {{ writeText("/{name}", "x".repeat(60)); }}"#
        )
    };

    let (_, first) = h.execute(&write("a.txt"), Some(&session)).await;
    assert!(first["error"].is_null(), "first write failed: {first}");

    let (_, second) = h.execute(&write("b.txt"), Some(&session)).await;
    let message = second["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("RangeError") && message.contains("size limit of 100 bytes"),
        "the second program starts from the first one's 60 bytes: {second}"
    );
}

#[tokio::test]
async fn ephemeral_does_not_persist_across_executes() {
    let h = Harness::with_vfs(VfsConfig::Ephemeral { size_limit: None });
    let (_, created) = h
        .post("/v1/sessions", json!({ "blueprint": BLUEPRINT }), None)
        .await;
    let session = created["session_id"].as_str().unwrap().to_string();

    let (_, w) = h.execute(WRITE, Some(&session)).await;
    assert!(w["error"].is_null(), "write failed: {w}");

    let (_, r) = h.execute(READ, Some(&session)).await;
    assert_eq!(
        r["result"],
        Value::Null,
        "each ephemeral execute is isolated, even within a session"
    );
}

#[tokio::test]
async fn none_mode_disables_fs() {
    // The trap reason ("filesystem is disabled") is asserted at the interpreter
    // unit level; the server renders traps as a backtrace, so here we only
    // confirm the write fails end-to-end under a `none` blueprint.
    let h = Harness::with_vfs(VfsConfig::None);
    let (_, r) = h.execute(WRITE, None).await;
    assert_eq!(r["error"]["kind"], json!("runtime_error"), "got: {r}");
    assert_eq!(r["result"], Value::Null);
}

#[tokio::test]
async fn create_unknown_blueprint_is_404() {
    let h = Harness::with_vfs(per_session());
    let (status, _) = h
        .post("/v1/sessions", json!({ "blueprint": "nope" }), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn disconnect_wipes_known_204_unknown_404() {
    let h = Harness::with_vfs(per_session());
    let (_, created) = h
        .post("/v1/sessions", json!({ "blueprint": BLUEPRINT }), None)
        .await;
    let session = created["session_id"].as_str().unwrap();

    assert_eq!(
        h.delete(&format!("/v1/sessions/{session}")).await,
        StatusCode::NO_CONTENT
    );
    // Terminated: the session is gone, so a second delete 404s.
    assert_eq!(
        h.delete(&format!("/v1/sessions/{session}")).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        h.delete("/v1/sessions/does-not-exist").await,
        StatusCode::NOT_FOUND
    );
}

/// Build a server over explicit durable dirs so two instances can share them
/// across a simulated restart.
fn restartable_state(vfs_root: &Path, store_dir: &Path) -> AppState {
    let blueprints = Arc::new(InMemoryBlueprintStore::seed([Blueprint {
        name: BLUEPRINT.into(),
        vfs: per_session(),
        permissions: allow_fs(),
        ..Default::default()
    }]));
    let config = ServerConfig {
        blueprints: Some(blueprints),
        session_storage_root: Some(vfs_root.to_path_buf()),
        session_store_dir: Some(store_dir.to_path_buf()),
        ..ServerConfig::default()
    };
    AppState::new(config).expect("AppState")
}

async fn send_to(state: &AppState, req: Request<Body>) -> (StatusCode, Value) {
    let resp = app(state.clone()).oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

async fn active_sessions(state: &AppState) -> u64 {
    let req = Request::builder()
        .method("GET")
        .uri("/v1/status")
        .body(Body::empty())
        .unwrap();
    send_to(state, req).await.1["active_sessions"]
        .as_u64()
        .expect("active_sessions count")
}

async fn execute_on(state: &AppState, code: &str, session: &str) -> Value {
    let req = Request::builder()
        .method("POST")
        .uri(format!("/v1/sessions/{session}/execute"))
        .header("content-type", "application/json")
        .body(Body::from(json!({ "code": code }).to_string()))
        .unwrap();
    send_to(state, req).await.1
}

#[tokio::test]
async fn per_session_resumes_after_restart() {
    let vfs_root = tempfile::tempdir().expect("vfs root");
    let store_dir = tempfile::tempdir().expect("session store");

    // First boot: create a session and write into its per_session VFS.
    let state = restartable_state(vfs_root.path(), store_dir.path());
    let create = Request::builder()
        .method("POST")
        .uri("/v1/sessions")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "blueprint": BLUEPRINT }).to_string()))
        .unwrap();
    let (status, created) = send_to(&state, create).await;
    assert_eq!(status, StatusCode::OK);
    let session = created["session_id"].as_str().unwrap().to_string();
    let w = execute_on(&state, WRITE, &session).await;
    assert!(w["error"].is_null(), "write failed: {w}");
    drop(state);

    // Restart: a brand-new server over the same durable dirs, empty cache.
    let restarted = restartable_state(vfs_root.path(), store_dir.path());
    assert_eq!(
        active_sessions(&restarted).await,
        0,
        "a fresh process knows nothing until boot"
    );
    restarted.boot().await;
    assert_eq!(
        active_sessions(&restarted).await,
        1,
        "boot rehydrates the persisted session"
    );

    let r = execute_on(&restarted, READ, &session).await;
    assert_eq!(r["result"], json!("hi"), "file should survive the restart");
}

#[tokio::test]
async fn restart_sweeps_orphan_session_dir() {
    let vfs_root = tempfile::tempdir().expect("vfs root");
    let store_dir = tempfile::tempdir().expect("session store");
    // A leftover per_session dir whose session was never persisted.
    let orphan = vfs_root.path().join("ghost-session");
    std::fs::create_dir_all(&orphan).unwrap();

    let state = restartable_state(vfs_root.path(), store_dir.path());
    state.boot().await;
    assert!(
        !orphan.exists(),
        "boot must reclaim a per_session dir with no live session"
    );
}

/// A blueprint naming `volume: work`, with the server declaring `work` at
/// `target`.
fn with_volume(target: &Path) -> Harness {
    Harness::with_vfs_and_volumes(
        VfsConfig::Persistent {
            volume: "work".into(),
        },
        VolumeTable::from([("work".to_string(), target.to_path_buf())]),
    )
}

#[tokio::test]
async fn persistent_rejects_path_traversal() {
    let dir = tempfile::tempdir().expect("persistent dir");
    let h = with_volume(dir.path());
    let escape = r#"import { writeText } from "submilli:fs"; function main(): void { writeText("../escape.txt", "x"); }"#;
    let (_, r) = h.execute(escape, None).await;
    // The lexical `..` escape traps; the server surfaces it as a runtime error
    // and nothing is written to the parent of the configured root.
    assert_eq!(r["error"]["kind"], json!("runtime_error"), "got: {r}");
    let escaped = dir.path().parent().unwrap().join("escape.txt");
    assert!(
        !escaped.exists(),
        "traversal must not write outside the root"
    );
}

const WRITE_NOTE: &str = r#"import { writeText } from "submilli:fs"; function main(): void { writeText("note.txt", "hi"); }"#;

#[tokio::test]
async fn persistent_mounts_the_declared_volume() {
    let dir = tempfile::tempdir().expect("volume dir");
    let h = with_volume(dir.path());
    let (_, w) = h.execute(WRITE_NOTE, None).await;
    assert!(w["error"].is_null(), "write failed: {w}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("note.txt")).unwrap(),
        "hi",
        "the write must land in the declared volume's directory"
    );
}

/// The three ways a `persistent` mount fails, and what the client is allowed to
/// learn: the volume name, never the host directory behind it.
async fn mount_failure(target: PathBuf, volumes: VolumeTable) -> String {
    let h = Harness::with_vfs_and_volumes(
        VfsConfig::Persistent {
            volume: "work".into(),
        },
        volumes,
    );
    let (_, r) = h.execute(WRITE_NOTE, None).await;
    let body = r.to_string();
    assert!(
        body.contains("work"),
        "the volume name must reach the client: {body}"
    );
    assert!(
        !body.contains(target.to_str().expect("utf-8 path")),
        "the host directory must not reach the client: {body}"
    );
    body
}

#[tokio::test]
async fn a_volume_removed_from_config_fails_by_name() {
    let dir = tempfile::tempdir().expect("volume dir");
    // Registered against a volume the operator has since dropped.
    let body = mount_failure(dir.path().to_path_buf(), VolumeTable::new()).await;
    assert!(body.contains("not declared"), "got {body}");
}

#[tokio::test]
async fn a_volume_whose_target_vanished_fails_by_name() {
    let dir = tempfile::tempdir().expect("volume parent");
    let target = dir.path().join("work");
    std::fs::create_dir(&target).unwrap();
    let volumes = VolumeTable::from([("work".to_string(), target.clone())]);
    std::fs::remove_dir(&target).unwrap();
    let body = mount_failure(target, volumes).await;
    assert!(body.contains("unavailable"), "got {body}");
}

#[tokio::test]
async fn a_volume_whose_target_became_a_file_fails_by_name() {
    let dir = tempfile::tempdir().expect("volume parent");
    let target = dir.path().join("work");
    std::fs::write(&target, b"not a directory").unwrap();
    let volumes = VolumeTable::from([("work".to_string(), target.clone())]);
    let body = mount_failure(target, volumes).await;
    assert!(body.contains("unavailable"), "got {body}");
}

/// A `MakeWriter` collecting the server's log into memory.
#[derive(Clone, Default)]
struct LogBuf(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl tracing_subscriber::fmt::MakeWriter<'_> for LogBuf {
    type Writer = LogBuf;

    fn make_writer(&self) -> Self::Writer {
        self.clone()
    }
}

/// The process-wide subscriber. Execution runs off the test's own thread, so a
/// thread-local subscriber would capture nothing.
fn server_log() -> &'static LogBuf {
    static LOG: std::sync::OnceLock<LogBuf> = std::sync::OnceLock::new();
    LOG.get_or_init(|| {
        let log = LogBuf::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(log.clone())
            .with_max_level(tracing::Level::TRACE)
            .finish();
        let _ = tracing::subscriber::set_global_default(subscriber);
        log
    })
}

#[tokio::test]
async fn a_failed_mount_logs_the_host_path_it_withheld() {
    let dir = tempfile::tempdir().expect("volume parent");
    let target = dir.path().join("work");
    let volumes = VolumeTable::from([("work".to_string(), target.clone())]);

    let log = server_log();

    let h = Harness::with_vfs_and_volumes(
        VfsConfig::Persistent {
            volume: "work".into(),
        },
        volumes,
    );
    let _ = h.execute(WRITE_NOTE, None).await;

    let text = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    assert!(
        text.contains(target.to_str().unwrap()),
        "the operator's log must carry the host path: {text}"
    );
    assert!(text.contains("work"), "and the volume name: {text}");
}
