//! Applying a blueprint from a trusted local file: validated as registration
//! validates it, registered with a version tag every run records, declaring new
//! volumes under the managed root, and evicting the caches a run reads.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use interpreter::{ModulePath, PackageSourceModule, compile_package};
use serde_json::{Value, json};
use submilli_build::{
    ArtifactMetadata, ArtifactSource, write_package_artifact_with_docs_and_sources,
};
use submilli_server::blueprint::{
    BlueprintStore, InMemoryBlueprintStore, StoreError, StoredBlueprint,
};
use submilli_server::record::{FinishedRun, RunRecorder, RunRecorderFactory, RunStart};
use submilli_server::{ApiToken, AppState, AuthConfig, Role, ServerConfig, app};
use tower::ServiceExt;

const APP_TOKEN: &str = "app-token-value-0123456789abcdef0123456789abcdef";
const ADMIN_TOKEN: &str = "admin-token-value-0123456789abcdef0123456789abcd";

const V1: &str = "\
name: demo
permissions:
  main:
    - name: ok
      capability: test.com/ok
      action: allow
";

const ALLOWED: &str = r#"
import { check } from "submilli:security";
function main(): string { check("test.com/ok", {}); return "done"; }
"#;

/// What each finished run started as, plus its decisions.
#[derive(Default)]
struct Runs(Mutex<Vec<(RunStart, Vec<interpreter::runtime::DecisionRecord>)>>);

struct Factory(Arc<Runs>);

impl RunRecorderFactory for Factory {
    fn start(&self, run: RunStart) -> Option<Arc<dyn RunRecorder>> {
        Some(Arc::new(Recorder {
            runs: Arc::clone(&self.0),
            start: run,
        }))
    }
}

struct Recorder {
    runs: Arc<Runs>,
    start: RunStart,
}

impl RunRecorder for Recorder {
    fn finish(&self, run: FinishedRun) {
        self.runs
            .0
            .lock()
            .unwrap()
            .push((self.start.clone(), run.log.records));
    }
}

struct Server {
    state: AppState,
    runs: Arc<Runs>,
    dirs: tempfile::TempDir,
}

/// An in-memory blueprint store whose writes can be made to fail.
#[derive(Default)]
struct Flaky {
    inner: InMemoryBlueprintStore,
    fail: AtomicBool,
}

#[async_trait::async_trait]
impl BlueprintStore for Flaky {
    async fn add_yaml(&self, stored: StoredBlueprint) -> Result<(), StoreError> {
        self.inner.add_yaml(stored).await
    }
    async fn upsert_yaml(&self, stored: StoredBlueprint) -> Result<bool, StoreError> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(StoreError::Io("disk full".into()));
        }
        self.inner.upsert_yaml(stored).await
    }
    async fn list(&self) -> Result<Vec<String>, StoreError> {
        self.inner.list().await
    }
    async fn list_blueprints(&self) -> Result<Vec<submilli_blueprint::Blueprint>, StoreError> {
        self.inner.list_blueprints().await
    }
    async fn get(&self, name: &str) -> Result<Option<submilli_blueprint::Blueprint>, StoreError> {
        self.inner.get(name).await
    }
    async fn get_yaml(&self, name: &str) -> Result<Option<String>, StoreError> {
        self.inner.get_yaml(name).await
    }
    async fn remove(&self, name: &str) -> Result<bool, StoreError> {
        self.inner.remove(name).await
    }
}

impl Server {
    fn new() -> Self {
        Self::with_blueprints(None)
    }

    fn with_blueprints(blueprints: Option<Arc<dyn BlueprintStore>>) -> Self {
        let dirs = tempfile::tempdir().expect("dirs");
        let runs = Arc::new(Runs::default());
        let config = ServerConfig {
            auth: AuthConfig::Tokens(vec![
                ApiToken::new("app", Role::User, APP_TOKEN).expect("token"),
                ApiToken::new("operator", Role::Admin, ADMIN_TOKEN).expect("token"),
            ]),
            session_storage_root: Some(dirs.path().join("sessions")),
            session_store_dir: Some(dirs.path().join("store")),
            package_store_root: Some(dirs.path().join("packages")),
            managed_volume_root: Some(dirs.path().join("volumes")),
            run_recorder: Some(Arc::new(Factory(Arc::clone(&runs)))),
            ..in_memory_config::config()
        };
        let config = match blueprints {
            Some(blueprints) => ServerConfig {
                blueprints: Some(blueprints),
                ..config
            },
            None => config,
        };
        Self {
            state: AppState::new(config).expect("state"),
            runs,
            dirs,
        }
    }

    fn managed_root(&self) -> PathBuf {
        self.dirs.path().join("volumes")
    }

    fn packages(&self) -> PathBuf {
        self.dirs.path().join("packages")
    }

    async fn apply(&self, yaml: &str, tag: &str) -> submilli_server::LocalApplied {
        self.state
            .apply_local_blueprint(yaml, tag)
            .await
            .unwrap_or_else(|error| panic!("apply {tag}: {error:?}"))
    }

    async fn request(
        &self,
        method: &str,
        uri: &str,
        token: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        let response = app(self.state.clone())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header("host", "localhost")
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn execute(&self, code: &str) -> Value {
        let (status, body) = self
            .request(
                "POST",
                "/v1/execute",
                APP_TOKEN,
                json!({ "code": code, "blueprint": "demo" }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn open_session(&self, variables: Value) -> String {
        let (status, body) = self
            .request(
                "POST",
                "/v1/sessions",
                APP_TOKEN,
                json!({ "blueprint": "demo", "variables": variables }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["session_id"].as_str().unwrap().to_owned()
    }

    async fn session_execute(&self, session: &str, code: &str) -> Value {
        let (status, body) = self
            .request(
                "POST",
                &format!("/v1/sessions/{session}/execute"),
                APP_TOKEN,
                json!({ "code": code }),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    fn last_run(&self) -> (RunStart, Vec<interpreter::runtime::DecisionRecord>) {
        self.runs.0.lock().unwrap().last().cloned().expect("a run")
    }

    fn last_version(&self) -> Option<String> {
        self.last_run().0.blueprint_version
    }
}

#[tokio::test]
async fn every_run_records_the_version_tag_its_blueprint_was_applied_with() {
    let server = Server::new();
    let applied = server.apply(V1, "v1").await;
    assert_eq!(applied.name, "demo");
    assert!(applied.created);

    server.execute(ALLOWED).await;
    assert_eq!(server.last_version().as_deref(), Some("v1"));
    let session = server.open_session(json!({})).await;
    server.session_execute(&session, ALLOWED).await;
    assert_eq!(server.last_version().as_deref(), Some("v1"));

    let v2 = V1.replace("name: ok", "name: still-ok");
    assert!(!server.apply(&v2, "v2").await.created);
    server.session_execute(&session, ALLOWED).await;
    assert_eq!(server.last_version().as_deref(), Some("v2"));

    // Registered over HTTP, without a tag: runs record the blueprint's hash again.
    let (status, body) = server
        .request(
            "PUT",
            "/v1/blueprints/demo",
            ADMIN_TOKEN,
            json!({ "yaml": V1 }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    server.execute(ALLOWED).await;
    let (start, _) = server.last_run();
    assert!(start.blueprint_hash.is_some());
    assert_eq!(start.blueprint_version, start.blueprint_hash);
}

#[tokio::test]
async fn a_filter_on_a_field_the_capability_lacks_is_refused_as_registration_refuses_it() {
    let server = Server::new();
    server.apply(V1, "v1").await;
    let bad = format!(
        "{V1}    - capability: http.get\n      filter: nosuchfield == \"x\"\n      action: allow\n"
    );
    let error = server
        .state
        .apply_local_blueprint(&bad, "v2")
        .await
        .expect_err("refused");
    assert_eq!(error.code, "invalid_filter", "{error:?}");
    assert!(error.message.contains("nosuchfield"), "{error:?}");

    let (status, body) = server
        .request(
            "PUT",
            "/v1/blueprints/demo",
            ADMIN_TOKEN,
            json!({ "yaml": bad }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], error.code);
    assert_eq!(body["message"], error.message);

    // The last good version stays in force.
    server.execute(ALLOWED).await;
    assert_eq!(server.last_version().as_deref(), Some("v1"));
}

#[tokio::test]
async fn invalid_yaml_is_refused_against_its_line_and_the_last_good_version_stays() {
    let server = Server::new();
    server.apply(V1, "v1").await;
    let broken = "name: demo\npermissions:\n  main:\n    - capability: [unclosed\n";
    let error = server
        .state
        .apply_local_blueprint(broken, "v2")
        .await
        .expect_err("refused");
    assert_eq!(error.code, "parse_error");
    let line = error.diagnostics.first().and_then(|d| d.line);
    assert!(matches!(line, Some(4 | 5)), "{error:?}");
    // Checking alone changes nothing either.
    assert!(server.state.check_local_blueprint(broken).await.is_err());

    server.execute(ALLOWED).await;
    assert_eq!(server.last_version().as_deref(), Some("v1"));
}

#[tokio::test]
async fn a_newly_named_volume_is_declared_under_the_managed_root_without_a_restart() {
    let server = Server::new();
    server.apply(V1, "v1").await;
    let with_volume = format!(
        "{V1}    - capability: fs.write\n      action: allow\nvfs:\n  mode: per_session\n  mounts:\n    /notes: {{mode: named, volume: notes}}\n"
    );
    // Over HTTP the volume is not declared, so registration refuses it.
    let (status, body) = server
        .request(
            "PUT",
            "/v1/blueprints/demo",
            ADMIN_TOKEN,
            json!({ "yaml": with_volume }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "undeclared_volume");

    let applied = server.apply(&with_volume, "v2").await;
    assert_eq!(applied.declared_volumes, ["notes"]);
    let session = server.open_session(json!({})).await;
    let wrote = server
        .session_execute(
            &session,
            r#"import { writeText } from "submilli:fs"; function main(): string { writeText("/notes/a.txt", "kept"); return "ok"; }"#,
        )
        .await;
    assert!(wrote["error"].is_null(), "{wrote}");
    assert_eq!(
        std::fs::read_to_string(server.managed_root().join("notes/a.txt")).unwrap(),
        "kept"
    );
    let (status, listed) = server
        .request("GET", "/v1/volumes", ADMIN_TOKEN, Value::Null)
        .await;
    assert_eq!(status, StatusCode::OK, "{listed}");
    assert!(listed.to_string().contains("notes"), "{listed}");

    // Applying again declares nothing new.
    assert!(
        server
            .apply(&with_volume, "v3")
            .await
            .declared_volumes
            .is_empty()
    );
}

#[tokio::test]
async fn a_volume_name_that_cannot_be_a_directory_is_refused() {
    let server = Server::new();
    // The parser takes any non-empty name; a managed volume's must be a directory name.
    let yaml = "name: demo\nvfs:\n  mode: named\n  volume: '-notes'\n";
    assert!(submilli_blueprint::parse(yaml).is_ok());
    let error = server
        .state
        .apply_local_blueprint(yaml, "v1")
        .await
        .expect_err("refused");
    assert_eq!(error.code, "undeclared_volume", "{error:?}");
    assert!(!server.managed_root().join("-notes").exists());
}

fn with_volume(volume: &str) -> String {
    format!(
        "{V1}    - capability: fs.write\n      action: allow\nvfs:\n  mode: per_session\n  mounts:\n    /notes: {{mode: named, volume: {volume}}}\n"
    )
}

#[tokio::test]
async fn a_volume_is_declared_only_once_the_blueprint_is_stored() {
    let store = Arc::new(Flaky::default());
    let server = Server::with_blueprints(Some(store.clone()));
    server.apply(V1, "v1").await;
    store.fail.store(true, Ordering::SeqCst);
    let error = server
        .state
        .apply_local_blueprint(&with_volume("notes"), "v2")
        .await
        .expect_err("the store refuses the write");
    assert_eq!(error.code, "store_failed", "{error:?}");
    let (_, listed) = server
        .request("GET", "/v1/volumes", ADMIN_TOKEN, Value::Null)
        .await;
    assert!(!listed.to_string().contains("notes"), "{listed}");

    store.fail.store(false, Ordering::SeqCst);
    let applied = server.apply(&with_volume("notes"), "v3").await;
    assert_eq!(applied.declared_volumes, ["notes"]);
}

#[tokio::test]
async fn a_volume_name_differing_only_in_case_from_a_stored_one_is_refused() {
    let server = Server::new();
    server.apply(&with_volume("notes"), "v1").await;
    for yaml in [with_volume("Notes"), with_volume("NOTES")] {
        let error = server
            .state
            .check_local_blueprint(&yaml)
            .await
            .expect_err("refused by the check");
        assert_eq!(error.code, "volume_name_conflict", "{error:?}");
        assert!(error.message.contains("'notes'"), "{error:?}");
        let error = server
            .state
            .apply_local_blueprint(&yaml, "v2")
            .await
            .expect_err("refused by the apply");
        assert_eq!(error.code, "volume_name_conflict", "{error:?}");
    }

    // A directory an earlier run left under the managed root counts too.
    let fresh = Server::new();
    std::fs::create_dir_all(fresh.managed_root().join("Ledger")).unwrap();
    let error = fresh
        .state
        .apply_local_blueprint(&with_volume("ledger"), "v1")
        .await
        .expect_err("refused");
    assert_eq!(error.code, "volume_name_conflict", "{error:?}");
    assert!(error.message.contains("'Ledger'"), "{error:?}");
    // The same name in the same case is that directory's volume.
    let applied = fresh.apply(&with_volume("Ledger"), "v2").await;
    assert_eq!(applied.declared_volumes, ["Ledger"]);
}

const PING: &str =
    r#"import { ping } from "@acme/tools"; function main(): number { return ping(); }"#;

fn write_package(store_root: &Path, value: u32) {
    let source = format!("export function ping(): number {{ return {value}; }}");
    let package = compile_package(
        "@acme/tools",
        ModulePath::from("lib"),
        &[PackageSourceModule {
            path: ModulePath::from("lib"),
            source: &source,
        }],
        &[],
    )
    .expect("compile synthetic package");
    write_package_artifact_with_docs_and_sources(
        store_root.join("@acme").join("tools"),
        &package.wasm,
        &package.type_info,
        &submilli_build::derive_capability_schema(
            &package.declaration,
            &[],
            &package.required_capabilities,
        ),
        &package.declaration,
        &ArtifactMetadata::new("@acme/tools", "0.0.0-test", Vec::new()),
        "Acme tooling for tests.",
        &[ArtifactSource {
            path: ModulePath::from("lib"),
            text: source.clone(),
        }],
    )
    .expect("write synthetic package artifact");
}

#[tokio::test]
async fn after_an_edit_the_next_run_prepares_its_packages_afresh() {
    let server = Server::new();
    write_package(&server.packages(), 1);
    let with_package = format!("{V1}packages:\n  - '@acme/tools'\n");
    server.apply(&with_package, "v1").await;

    assert_eq!(server.execute(PING).await["result"], "1");

    // The package changes on disk; the prepared copy is still served.
    write_package(&server.packages(), 2);
    assert_eq!(server.execute(PING).await["result"], "1");

    let edited = with_package.replace("name: ok", "name: renamed");
    server.apply(&edited, "v2").await;
    assert_eq!(server.execute(PING).await["result"], "2");
}

#[tokio::test]
async fn a_new_required_variable_leaves_a_session_opened_before_it_unbound() {
    let server = Server::new();
    let v1 = "\
name: demo
variables:
  customerId:
    required: true
permissions:
  main:
    - name: own-charges
      capability: test.com/charges
      filter: customerId == ${vars.customerId}
      action: allow
";
    server.apply(v1, "v1").await;
    let session = server.open_session(json!({ "customerId": "cus_1" })).await;

    let v2 = v1
        .replace(
            "    required: true\n",
            "    required: true\n  region:\n    required: true\n",
        )
        .replace(
            "customerId == ${vars.customerId}",
            "customerId == ${vars.customerId} and region == ${vars.region}",
        );
    server.apply(&v2, "v2").await;

    let ran = server
        .session_execute(
            &session,
            r#"import { check } from "submilli:security";
function main(): string {
  try { check("test.com/charges", { customerId: "cus_1", region: "eu" }); return "allowed"; }
  catch (e: PermissionDeniedError) { return "denied"; }
}"#,
        )
        .await;
    assert_eq!(ran["result"], "denied", "{ran}");
    let (start, decisions) = server.last_run();
    assert_eq!(start.blueprint_version.as_deref(), Some("v2"));
    let decision = decisions.first().expect("the check was recorded");
    let recorded = serde_json::to_value(&decision.near_misses).unwrap();
    assert!(
        recorded.to_string().contains("variable-not-bound"),
        "{recorded}"
    );

    // What `explain` reads: the decision re-explained from its context, under the
    // session's bindings, names the unbound variable.
    let blueprint = submilli_blueprint::parse(&v2).unwrap();
    let resolution = blueprint.explain_permission(
        &decision.caller,
        &decision.capability,
        &decision.context,
        &start.variables,
    );
    let reasons: Vec<_> = resolution
        .near_misses
        .iter()
        .flat_map(|miss| &miss.failures)
        .map(|failure| failure.reason.clone())
        .collect();
    assert!(
        reasons.contains(&submilli_blueprint::FailureReason::VariableNotBound(
            "region".into()
        )),
        "{reasons:?}"
    );
}
