//! End-to-end tests for named volumes: as a root and mounted below one, shared
//! between blueprints, under the server's access ceiling and size limits, and
//! across a restart.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::config::{Access, SizeLimit, VolumeSpec, VolumeTable};
use submilli_server::{AppState, ServerConfig, app};
use tower::ServiceExt;

const FS_PERMISSIONS: &str = "\
permissions:
  main:
    - capability: fs.read
      action: allow
    - capability: fs.write
      action: allow
    - capability: fs.list
      action: allow
    - capability: fs.stat
      action: allow
";

struct Server {
    state: AppState,
}

impl Server {
    /// A server over `blueprints` (YAML bodies, each given the fs permissions)
    /// and `volumes`, keeping its state under `data`.
    fn new(data: &Path, blueprints: &[&str], volumes: VolumeTable) -> Self {
        let blueprints = blueprints
            .iter()
            .map(|yaml| {
                submilli_blueprint::parse(&format!("{yaml}{FS_PERMISSIONS}"))
                    .expect("blueprint parses")
            })
            .collect::<Vec<_>>();
        let config = ServerConfig {
            blueprints: Some(Arc::new(
                InMemoryBlueprintStore::seed(blueprints).expect("seed blueprints"),
            )),
            session_storage_root: Some(data.join("sessions")),
            managed_volume_root: Some(data.join("volumes")),
            volumes,
            ..in_memory_config::config()
        };
        Self {
            state: futures::executor::block_on(AppState::new(config)).expect("AppState"),
        }
    }

    async fn post(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        let request = Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app(self.state.clone()).oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn session(&self, blueprint: &str) -> String {
        let (status, created) = self
            .post("/v1/sessions", json!({ "blueprint": blueprint }))
            .await;
        assert_eq!(status, StatusCode::OK, "{created}");
        created["session_id"].as_str().unwrap().to_string()
    }

    /// Run `main` in a new session of `blueprint`, returning the envelope.
    async fn run(&self, blueprint: &str, code: &str) -> Value {
        let session = self.session(blueprint).await;
        self.run_in(&session, code).await
    }

    async fn run_in(&self, session: &str, code: &str) -> Value {
        self.post(
            &format!("/v1/sessions/{session}/execute"),
            json!({ "code": code }),
        )
        .await
        .1
    }
}

fn managed(limit: SizeLimit) -> VolumeSpec {
    VolumeSpec::managed(limit)
}

#[tokio::test]
async fn a_session_root_coexists_with_several_named_mounts() {
    let data = tempfile::tempdir().unwrap();
    let handbook = data.path().join("handbook");
    std::fs::create_dir(&handbook).unwrap();
    std::fs::write(handbook.join("rules.md"), "be kind").unwrap();
    let server = Server::new(
        data.path(),
        &["name: agent
vfs:
  mode: per_session
  mounts:
    /memory: {mode: named, volume: project-memory, access: read_write}
    /handbook: {mode: named, volume: company-handbook}
"],
        VolumeTable::from([
            (
                "project-memory".to_string(),
                managed(SizeLimit::Bytes(1 << 20)),
            ),
            (
                "company-handbook".to_string(),
                VolumeSpec::local_path(&handbook).with_access(Access::ReadOnly),
            ),
        ]),
    );
    let session = server.session("agent").await;
    let result = server
        .run_in(
            &session,
            r#"
import { writeText, readText, info } from "submilli:fs";
function main(): string {
    writeText("/scratch.txt", "session");
    writeText("/memory/note.md", "remember");
    let denied = "no";
    try { writeText("/handbook/rules.md", "rewrite"); } catch (e: PermissionDeniedError) { denied = e.capability; }
    const fs = info();
    return [readText("/handbook/rules.md"), denied, fs.mode, String(fs.mounts.length),
            fs.mounts[0].path, fs.mounts[0].access, String(fs.mounts[1].sizeLimit)].join("|");
}
"#,
        )
        .await;
    assert!(result["error"].is_null(), "{result}");
    assert_eq!(
        result["result"],
        json!("be kind|fs.write|per_session|2|/handbook|read_only|1048576")
    );
    assert_eq!(
        std::fs::read_to_string(data.path().join("volumes/project-memory/note.md")).unwrap(),
        "remember"
    );
    assert_eq!(
        std::fs::read_to_string(handbook.join("rules.md")).unwrap(),
        "be kind"
    );
}

#[tokio::test]
async fn two_blueprints_share_a_named_volume_across_sessions() {
    let data = tempfile::tempdir().unwrap();
    let server = Server::new(
        data.path(),
        &[
            "name: writer
vfs:
  mounts:
    /shared: {mode: named, volume: memory}
",
            "name: reader
vfs:
  mode: named
  volume: memory
  access: read_only
",
        ],
        VolumeTable::from([("memory".to_string(), managed(SizeLimit::Unlimited))]),
    );
    let wrote = server
        .run(
            "writer",
            r#"import { writeText } from "submilli:fs"; function main(): void { writeText("/shared/fact.txt", "durable"); }"#,
        )
        .await;
    assert!(wrote["error"].is_null(), "{wrote}");
    let read = server
        .run(
            "reader",
            r#"import { readText, info } from "submilli:fs"; function main(): string { return readText("/fact.txt")! + "|" + info().mode + "|" + info().volume + "|" + info().access; }"#,
        )
        .await;
    assert_eq!(
        read["result"],
        json!("durable|named|memory|read_only"),
        "{read}"
    );
}

#[tokio::test]
async fn a_volume_limit_is_shared_by_every_blueprint_and_path_using_it() {
    let data = tempfile::tempdir().unwrap();
    let server = Server::new(
        data.path(),
        &[
            "name: one
vfs:
  mounts:
    /a: {mode: named, volume: capped}
",
            "name: two
vfs:
  mounts:
    /b/c: {mode: named, volume: capped}
",
        ],
        VolumeTable::from([("capped".to_string(), managed(SizeLimit::Bytes(100)))]),
    );
    let first = server
        .run(
            "one",
            r#"import { writeText } from "submilli:fs"; function main(): void { writeText("/a/x.txt", "x".repeat(60)); }"#,
        )
        .await;
    assert!(first["error"].is_null(), "{first}");
    let second = server
        .run(
            "two",
            r#"import { writeText } from "submilli:fs";
function main(): boolean {
    try { writeText("/b/c/y.txt", "y".repeat(60)); return false; } catch (e) { return e instanceof QuotaExceededError; }
}"#,
        )
        .await;
    assert_eq!(second["result"], json!("true"), "{second}");
}

#[tokio::test]
async fn concurrent_writers_cannot_pass_a_shared_limit_together() {
    let data = tempfile::tempdir().unwrap();
    let server = Arc::new(Server::new(
        data.path(),
        &["name: racer
vfs:
  mounts:
    /v: {mode: named, volume: capped}
"],
        VolumeTable::from([("capped".to_string(), managed(SizeLimit::Bytes(1000)))]),
    ));
    let mut runs = Vec::new();
    for index in 0..8 {
        let server = Arc::clone(&server);
        runs.push(tokio::spawn(async move {
            let code = format!(
                r#"import {{ writeText }} from "submilli:fs";
function main(): boolean {{
    try {{ writeText("/v/{index}.txt", "z".repeat(300)); return true; }} catch (e) {{ return false; }}
}}"#
            );
            server.run("racer", &code).await["result"] == json!("true")
        }));
    }
    let mut written = 0;
    for run in runs {
        if run.await.unwrap() {
            written += 1;
        }
    }
    assert_eq!(
        written, 3,
        "1000 bytes hold three 300-byte files and no more"
    );
    let on_disk: u64 = std::fs::read_dir(data.path().join("volumes/capped"))
        .unwrap()
        .map(|entry| entry.unwrap().metadata().unwrap().len())
        .sum();
    assert_eq!(on_disk, 900);
}

#[tokio::test]
async fn managed_data_survives_a_restart_and_removal_from_the_config() {
    let data = tempfile::tempdir().unwrap();
    let blueprint = "name: keeper
vfs:
  mode: named
  volume: notes
";
    let volumes = || VolumeTable::from([("notes".to_string(), managed(SizeLimit::Unlimited))]);
    {
        let server = Server::new(data.path(), &[blueprint], volumes());
        let wrote = server
            .run(
                "keeper",
                r#"import { writeText } from "submilli:fs"; function main(): void { writeText("/n.txt", "kept"); }"#,
            )
            .await;
        assert!(wrote["error"].is_null(), "{wrote}");
    }
    {
        // Declared no more: the blueprint can no longer mount it, and nothing
        // removes its files.
        let server = Server::new(data.path(), &[blueprint], VolumeTable::new());
        server.state.boot().await.expect("boot");
        let (status, refused) = server
            .post("/v1/sessions", json!({"blueprint":"keeper"}))
            .await;
        assert!(!status.is_success(), "{refused}");
        assert!(refused.to_string().contains("not declared"), "{refused}");
        assert!(data.path().join("volumes/notes/n.txt").exists());
    }
    let server = Server::new(data.path(), &[blueprint], volumes());
    server.state.boot().await.expect("boot");
    let read = server
        .run(
            "keeper",
            r#"import { readText } from "submilli:fs"; function main(): string { return readText("/n.txt")!; }"#,
        )
        .await;
    assert_eq!(read["result"], json!("kept"), "{read}");
}

#[tokio::test]
async fn registration_checks_mounts_against_the_declared_volumes() {
    let data = tempfile::tempdir().unwrap();
    let handbook = data.path().join("handbook");
    let server = Server::new(
        data.path(),
        &[],
        VolumeTable::from([(
            "handbook".to_string(),
            VolumeSpec::local_path(&handbook).with_access(Access::ReadOnly),
        )]),
    );
    let (status, body) = server
        .post(
            "/v1/blueprints",
            json!({ "yaml": "name: a\nvfs:\n  mounts:\n    /h: {mode: named, volume: handbook, access: read_write}\n" }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], json!("volume_access_exceeded"), "{body}");
    assert!(body.to_string().contains("read_only"), "{body}");
    assert!(
        !body.to_string().contains(&handbook.display().to_string()),
        "no host path: {body}"
    );

    let (status, body) = server
        .post(
            "/v1/blueprints",
            json!({ "yaml": "name: b\nvfs:\n  mounts:\n    /m: {mode: named, volume: missing}\n" }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], json!("undeclared_volume"), "{body}");
    assert!(
        body.to_string().contains("declared volumes: handbook"),
        "{body}"
    );

    let (status, body) = server
        .post(
            "/v1/blueprints",
            json!({ "yaml": "name: c\nvfs:\n  mounts:\n    /h: {mode: named, volume: handbook}\n" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn subpaths_isolate_sessions_and_cwd_uses_guest_paths() {
    let data = tempfile::tempdir().unwrap();
    let server = Server::new(
        data.path(),
        &[r#"name: personal
variables:
  user: {required: true}
vfs:
  cwd: /notes
  mounts:
    /notes: {mode: named, volume: notes, subPath: 'users/${vars.user}'}
"#],
        VolumeTable::from([("notes".into(), managed(SizeLimit::Bytes(100)))]),
    );
    let mut sessions = Vec::new();
    for user in ["ada", "bob"] {
        let (status, body) = server
            .post(
                "/v1/sessions",
                json!({"blueprint":"personal", "variables":{"user":user}}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let id = body["session_id"].as_str().unwrap().to_owned();
        let code = format!(
            r#"import {{ writeText, cwd }} from "submilli:fs"; function main(): string {{ writeText("a.md", "{user}"); return cwd(); }}"#
        );
        let result = server.run_in(&id, &code).await;
        assert!(result["error"].is_null(), "{result}");
        assert_eq!(result["result"], "/notes");
        sessions.push(id);
    }
    for (session, user) in sessions.iter().zip(["ada", "bob"]) {
        let result = server.run_in(session, r#"import { readText } from "submilli:fs"; function main(): string { return readText("a.md")!; }"#).await;
        assert_eq!(result["result"], user, "{result}");
        assert_eq!(
            std::fs::read_to_string(data.path().join(format!("volumes/notes/users/{user}/a.md")))
                .unwrap(),
            user
        );
    }
    for bad in ["..", ".", "a/b", "", "a\0b"] {
        let (status, body) = server
            .post(
                "/v1/sessions",
                json!({"blueprint":"personal", "variables":{"user":bad}}),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
}

#[tokio::test]
async fn overlapping_subpaths_share_quota_and_respect_mount_access() {
    let data = tempfile::tempdir().unwrap();
    let server = Server::new(
        data.path(),
        &[r#"name: aliases
vfs:
  cwd: /personal
  mounts:
    /personal: {mode: named, volume: notes, subPath: users/ada}
    /all: {mode: named, volume: notes}
    /readonly: {mode: named, volume: notes, subPath: users/ada, access: read_only}
"#],
        VolumeTable::from([("notes".into(), managed(SizeLimit::Bytes(10)))]),
    );
    // A read-only alias requires the selected directory to exist before setup.
    std::fs::create_dir_all(data.path().join("volumes/notes/users/ada")).unwrap();
    let session = server.session("aliases").await;
    let written = server.run_in(&session, r#"import { writeText, readText } from "submilli:fs"; function main(): string { writeText("a", "12345678"); return readText("/readonly/a")!; }"#).await;
    assert_eq!(written["result"], "12345678", "{written}");
    let denied = server.run_in(&session, r#"import { writeText } from "submilli:fs"; function main(): void { writeText("/readonly/a", "x"); }"#).await;
    assert!(!denied["error"].is_null(), "{denied}");
    let full = server.run_in(&session, r#"import { writeText } from "submilli:fs"; function main(): void { writeText("/all/other", "123"); }"#).await;
    assert!(!full["error"].is_null(), "{full}");
    let rewrite = server.run_in(&session, r#"import { writeText, readText } from "submilli:fs"; function main(): string { writeText("/all/users/ada/a", "abcdefgh"); return readText("a")!; }"#).await;
    assert_eq!(rewrite["result"], "abcdefgh", "{rewrite}");
}

#[tokio::test]
async fn readonly_subpath_setup_fails_without_disclosing_host_path() {
    let data = tempfile::tempdir().unwrap();
    let server = Server::new(
        data.path(),
        &[r#"name: readonly
vfs:
  mode: named
  volume: notes
  subPath: missing
  access: read_only
"#],
        VolumeTable::from([("notes".into(), managed(SizeLimit::Unlimited))]),
    );
    let (status, body) = server
        .post("/v1/sessions", json!({"blueprint":"readonly"}))
        .await;
    assert!(!status.is_success(), "{body}");
    assert!(body.to_string().contains("notes"), "{body}");
    assert!(!body.to_string().contains(data.path().to_str().unwrap()));
    assert!(!data.path().join("volumes/notes/missing").exists());
}

#[tokio::test]
async fn one_shot_resolves_subpaths_before_execution() {
    let data = tempfile::tempdir().unwrap();
    let server = Server::new(
        data.path(),
        &[r#"name: once
variables:
  user: {required: true}
vfs:
  mode: named
  volume: notes
  subPath: users/${vars.user}
  cwd: /drafts
"#],
        VolumeTable::from([("notes".into(), managed(SizeLimit::Unlimited))]),
    );
    let (status, output) = server.post("/v1/execute", json!({"blueprint":"once", "variables":{"user":"ada"}, "code":r#"import { writeText, cwd } from "submilli:fs"; function main(): string { writeText("a", "once"); return cwd(); }"#})).await;
    assert_eq!(status, StatusCode::OK, "{output}");
    assert!(output["error"].is_null(), "{output}");
    assert_eq!(output["result"], "/drafts", "{output}");
    assert_eq!(
        std::fs::read_to_string(data.path().join("volumes/notes/users/ada/drafts/a")).unwrap(),
        "once"
    );
}
