//! Test runs end to end: a recorded run's program run again through the server's own
//! path under the current blueprint, answered from the recording, over throwaway local
//! state, and recorded as a run of its own.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use httpmock::{Method, MockServer};
use serde_json::{Value, json};
use submilli_blueprint::{
    Action, Blueprint, LlmConfig, LlmModelDecl, LlmProviderDecl, McpServer, PermissionRule,
    VarBindings, VfsConfig,
};
use submilli_server::blueprint::{BlueprintStore, InMemoryBlueprintStore};
use submilli_server::config::{VolumeSpec, VolumeTable};
use submilli_server::error::ErrorKind;
use submilli_server::handlers::execute::ExecuteResponse;
use submilli_server::record::{
    FinishedRun, MissReason, ProgramRun, RecordedRun, RunEntry, RunRecorder, RunRecorderFactory,
    RunStart, TestError, TestMode, TestOutcome, TestRun, run_program, test_program,
};
use submilli_server::{AppState, ServerConfig, app};
use submilli_shared::embedding::{
    DispatchFailure, DispatchResponse, DispatchRow, EmbeddingDispatch, EmbeddingRequest,
};
use submilli_shared::llm::{
    ModelDispatch, ModelRequest, ProviderFailure, ProviderResponse, ProviderUsage, StopReason,
};
use tower::ServiceExt;

// ---- the world -------------------------------------------------------------------------

/// Every run the server recorded, as an embedder would keep them.
#[derive(Default)]
struct Recordings {
    started: Mutex<Vec<RunStart>>,
    runs: Mutex<HashMap<String, RecordedRun>>,
}

impl Recordings {
    fn run(&self, execution_id: &str) -> RecordedRun {
        self.runs
            .lock()
            .unwrap()
            .get(execution_id)
            .unwrap_or_else(|| panic!("run {execution_id} was recorded"))
            .clone()
    }

    fn start(&self, execution_id: &str) -> RunStart {
        self.started
            .lock()
            .unwrap()
            .iter()
            .find(|start| start.execution_id == execution_id)
            .unwrap_or_else(|| panic!("run {execution_id} started"))
            .clone()
    }
}

struct Factory(Arc<Recordings>);

impl RunRecorderFactory for Factory {
    fn start(&self, run: RunStart) -> Option<Arc<dyn RunRecorder>> {
        self.0.started.lock().unwrap().push(run.clone());
        Some(Arc::new(Recorder {
            recordings: self.0.clone(),
            start: run,
        }))
    }
}

struct Recorder {
    recordings: Arc<Recordings>,
    start: RunStart,
}

impl RunRecorder for Recorder {
    fn finish(&self, run: FinishedRun) {
        self.recordings.runs.lock().unwrap().insert(
            self.start.execution_id.clone(),
            RecordedRun::from_parts(&self.start, &run),
        );
    }
}

struct World {
    state: AppState,
    blueprints: Arc<InMemoryBlueprintStore>,
    recordings: Arc<Recordings>,
    dirs: tempfile::TempDir,
}

impl World {
    fn new(blueprints: Vec<Blueprint>) -> Self {
        Self::with(blueprints, |config| config)
    }

    fn with(blueprints: Vec<Blueprint>, tweak: impl FnOnce(ServerConfig) -> ServerConfig) -> Self {
        let store = Arc::new(InMemoryBlueprintStore::seed(blueprints).expect("seed"));
        let dirs = tempfile::tempdir().expect("dirs");
        let recordings = Arc::new(Recordings::default());
        let config = ServerConfig {
            blueprints: Some(store.clone()),
            session_storage_root: Some(dirs.path().join("sessions")),
            run_recorder: Some(Arc::new(Factory(recordings.clone()))),
            ..in_memory_config::config()
        };
        Self {
            state: AppState::new(tweak(config)).expect("state"),
            blueprints: store,
            recordings,
            dirs,
        }
    }

    /// Runs `code` as a program and returns its response and recording.
    async fn run(&self, blueprint: &str, code: &str, variables: &[(&str, &str)]) -> Recorded {
        let response = run_program(
            &self.state,
            ProgramRun {
                label: "source".into(),
                blueprint: blueprint.into(),
                code: code.into(),
                variables: variables
                    .iter()
                    .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                    .collect(),
                secrets: Default::default(),
            },
        )
        .await;
        let recorded = self.recordings.run(&response.execution_id);
        Recorded { response, recorded }
    }

    async fn test(&self, recorded: &RecordedRun, mode: TestMode) -> TestOutcome {
        self.test_with(recorded, &[], mode).await
    }

    async fn test_with(
        &self,
        recorded: &RecordedRun,
        bindings: &[(&str, &str)],
        mode: TestMode,
    ) -> TestOutcome {
        test_program(
            &self.state,
            TestRun {
                label: "tester".into(),
                recorded: recorded.clone(),
                bindings: bindings
                    .iter()
                    .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                    .collect::<VarBindings>(),
                mode,
                secrets: None,
            },
        )
        .await
        .expect("the test run starts")
    }

    async fn post(&self, uri: &str, body: Value) -> Value {
        let request = Request::builder()
            .method("POST")
            .uri(uri)
            .header("host", "localhost")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app(self.state.clone()).oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    /// A session of `blueprint`, which keeps its files and data between executes.
    async fn open_session(&self, blueprint: &str) -> String {
        let body = self
            .post("/v1/sessions", json!({ "blueprint": blueprint }))
            .await;
        body["session_id"].as_str().expect("session id").to_owned()
    }

    /// Runs `code` in `session`, and returns the response and the run's recording.
    async fn session_run(&self, session: &str, code: &str) -> Recorded {
        let body = self
            .post(
                &format!("/v1/sessions/{session}/execute"),
                json!({ "code": code }),
            )
            .await;
        let id = body["execution_id"]
            .as_str()
            .unwrap_or_else(|| panic!("an execution id: {body}"));
        assert!(body["error"].is_null(), "{body}");
        Recorded {
            response: ExecuteResponse {
                execution_id: id.to_owned(),
                session_id: session.to_owned(),
                result: body["result"].as_str().map(str::to_owned),
                console: Vec::new(),
                error: None,
                discovery_warnings: Vec::new(),
            },
            recorded: self.recordings.run(id),
        }
    }
}

struct Recorded {
    response: ExecuteResponse,
    recorded: RecordedRun,
}

fn blueprint(yaml: &str) -> Blueprint {
    submilli_blueprint::parse(yaml).expect("blueprint")
}

fn result(outcome: &TestOutcome) -> Option<&str> {
    outcome.response.result.as_deref()
}

fn error_kind(outcome: &TestOutcome) -> Option<ErrorKind> {
    outcome.response.error.as_ref().map(|error| error.kind)
}

/// The caller, capability and allowed flag of each decision, in order.
fn decisions(run: &RecordedRun) -> Vec<(String, String, bool)> {
    run.decisions
        .iter()
        .map(|record| {
            (
                record.caller.clone(),
                record.capability.clone(),
                record.allowed,
            )
        })
        .collect()
}

// ---- over HTTP -------------------------------------------------------------------------

/// A blueprint that denies `http.get` of `/b`.
const DENY_B: &str = "\
name: bp
default: allow
allow_insecure_http: true
vfs:
  mode: none
permissions:
  main:
    - capability: http.get
      filter: path == \"/b\"
      action: deny
";

const ALLOW_ALL: &str = "name: bp\ndefault: allow\nallow_insecure_http: true\nvfs:\n  mode: none\n";

/// Reads `/a`, then `/b`, which the first blueprint denies and the program survives.
fn fetch_both(server: &MockServer) -> String {
    format!(
        r#"import {{ get }} from "submilli:http";
function main(): string {{
  const a = get("{}").body;
  let b = "";
  try {{
    b = get("{}").body;
  }} catch (e: Error) {{
    b = "denied";
  }}
  return a + "|" + b;
}}"#,
        server.url("/a"),
        server.url("/b"),
    )
}

/// Mocks the two pages `fetch_both` reads: `/a` and `/b`.
async fn site(server: &MockServer) -> (httpmock::Mock<'_>, httpmock::Mock<'_>) {
    let a = server
        .mock_async(|when, then| {
            when.method(Method::GET).path("/a");
            then.status(200).body("alpha");
        })
        .await;
    let b = server
        .mock_async(|when, then| {
            when.method(Method::GET).path("/b");
            then.status(200).body("bravo");
        })
        .await;
    (a, b)
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_newly_allowed_call_with_nothing_recorded_stops_the_run_and_the_report_names_it() {
    let server = MockServer::start_async().await;
    let (a, b) = site(&server).await;
    let world = World::new(vec![blueprint(DENY_B)]);
    let code = fetch_both(&server);
    let source = world.run("bp", &code, &[]).await;
    assert_eq!(source.response.result.as_deref(), Some("alpha|denied"));
    a.assert_hits_async(1).await;
    b.assert_hits_async(0).await;

    // A rule now allows the call the recorded run was denied.
    world.blueprints.upsert(blueprint(ALLOW_ALL)).await.unwrap();
    let outcome = world.test(&source.recorded, TestMode::Recorded).await;

    assert_eq!(error_kind(&outcome), Some(ErrorKind::Cancelled));
    assert_eq!(result(&outcome), None);
    let report = &outcome.report;
    assert_eq!(report.source_run, source.recorded.execution_id);
    assert_eq!(report.test_run, outcome.response.execution_id);
    assert_eq!(report.served.len(), 1);
    assert_eq!(report.served[0].source_call_index, 0);
    assert_eq!(report.served[0].test_call_index, Some(0));
    let stop = report.stopped.as_ref().expect("the run stopped");
    assert_eq!(stop.reason, MissReason::NoRecording);
    assert!(
        stop.key.starts_with("http GET http://127.0.0.1:"),
        "{stop:?}"
    );
    assert!(stop.key.ends_with("/b"), "{stop:?}");
    assert_eq!(stop.capability.as_deref(), Some("http.get"));
    assert_eq!(stop.caller.as_deref(), Some("main"));
    assert_eq!(stop.test_call_index, Some(1));
    let line = code
        .lines()
        .position(|line| line.contains("/b"))
        .map(|at| at as u32 + 1);
    assert_eq!(stop.line.map(|line| line.line), line);
    // Nothing went out: the pages were read once, by the source run.
    a.assert_hits_async(1).await;
    b.assert_hits_async(0).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_guest_cannot_catch_the_stop() {
    let server = MockServer::start_async().await;
    let (_a, b) = site(&server).await;
    let world = World::new(vec![blueprint(DENY_B)]);
    // The program catches whatever the second read throws, and reports it handled.
    let source = world.run("bp", &fetch_both(&server), &[]).await;
    world.blueprints.upsert(blueprint(ALLOW_ALL)).await.unwrap();
    let outcome = world.test(&source.recorded, TestMode::Recorded).await;
    assert_eq!(result(&outcome), None, "the program did not finish");
    assert_eq!(error_kind(&outcome), Some(ErrorKind::Cancelled));
    assert!(outcome.report.stopped.is_some());
    b.assert_hits_async(0).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unchanged_blueprint_decides_alike_and_reaches_nowhere() {
    let server = MockServer::start_async().await;
    let (a, b) = site(&server).await;
    let world = World::new(vec![blueprint(DENY_B)]);
    let source = world.run("bp", &fetch_both(&server), &[]).await;

    let outcome = world.test(&source.recorded, TestMode::Recorded).await;
    assert_eq!(outcome.response.error.as_ref().map(|e| e.kind), None);
    assert_eq!(result(&outcome), Some("alpha|denied"));
    assert!(outcome.report.stopped.is_none());
    assert_eq!(outcome.report.served.len(), 1);
    let test_run = world.recordings.run(&outcome.report.test_run);
    assert_eq!(decisions(&test_run), decisions(&source.recorded));
    // No outbound request: the source run's one read is all the server saw.
    a.assert_hits_async(1).await;
    b.assert_hits_async(0).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_test_run_is_recorded_as_a_run_of_its_own_linked_to_its_source() {
    let server = MockServer::start_async().await;
    let (_a, _b) = site(&server).await;
    let world = World::new(vec![blueprint(DENY_B)]);
    let source = world.run("bp", &fetch_both(&server), &[]).await;
    let outcome = world.test(&source.recorded, TestMode::Recorded).await;

    let source_start = world.recordings.start(&source.recorded.execution_id);
    assert_eq!(source_start.test_of, None);
    assert_eq!(source_start.entry, RunEntry::Program);
    let start = world.recordings.start(&outcome.report.test_run);
    assert_eq!(start.entry, RunEntry::Test);
    assert_eq!(
        start.test_of.as_deref(),
        Some(source.recorded.execution_id.as_str())
    );
    assert_eq!(start.label, "tester");
    assert_eq!(start.code.as_deref(), source_start.code.as_deref());
    assert_ne!(start.execution_id, source_start.execution_id);
}

#[tokio::test]
async fn a_recording_with_no_program_is_refused_naming_its_run() {
    let world = World::new(vec![blueprint(ALLOW_ALL)]);
    let mut recorded = world
        .run("bp", "function main(): string { return \"x\"; }", &[])
        .await
        .recorded;
    recorded.code = None;
    let error = test_program(
        &world.state,
        TestRun {
            label: "tester".into(),
            recorded: recorded.clone(),
            bindings: VarBindings::new(),
            mode: TestMode::Recorded,
            secrets: None,
        },
    )
    .await
    .expect_err("refused");
    assert!(matches!(error, TestError::NoProgram { .. }));
    assert!(
        error.to_string().contains(&recorded.execution_id),
        "{error}"
    );
    assert_eq!(
        world.recordings.started.lock().unwrap().len(),
        1,
        "a refusal starts no run"
    );
}

#[tokio::test]
async fn a_recording_whose_blueprint_is_gone_is_refused() {
    let world = World::new(vec![blueprint(ALLOW_ALL)]);
    let recorded = world
        .run("bp", "function main(): string { return \"x\"; }", &[])
        .await
        .recorded;
    world.blueprints.remove("bp").await.unwrap();
    let error = test_program(
        &world.state,
        TestRun {
            label: "tester".into(),
            recorded,
            bindings: VarBindings::new(),
            mode: TestMode::Recorded,
            secrets: None,
        },
    )
    .await
    .expect_err("refused");
    assert!(matches!(error, TestError::BlueprintNotFound(name) if name == "bp"));
}

// ---- variables -------------------------------------------------------------------------

const CHECK: &str = r#"import { check } from "submilli:security";
function main(): string { check("test.com/op", { userId: "u_42" }); return "done"; }"#;

fn scoped(variables: &str) -> Blueprint {
    blueprint(&format!(
        "name: scoped\ndefault: deny\nvariables:\n{variables}permissions:\n  main:\n    - capability: test.com/op\n      filter: userId == ${{vars.tenant}}\n      action: allow\n"
    ))
}

#[tokio::test]
async fn variables_are_kept_filled_or_dropped_and_the_filled_binding_is_used() {
    let world = World::new(vec![blueprint(
        "name: scoped\ndefault: allow\nvariables:\n  customerId:\n    required: false\n  legacy:\n    required: false\n",
    )]);
    let source = world
        .run(
            "scoped",
            CHECK,
            &[("customerId", "cus_northwind"), ("legacy", "x")],
        )
        .await;
    assert_eq!(source.response.result.as_deref(), Some("done"));

    // The blueprint now scopes the check to a `tenant` it newly declares, and no longer
    // declares `legacy`.
    world
        .blueprints
        .upsert(scoped(
            "  customerId:\n    required: false\n  tenant:\n    required: false\n",
        ))
        .await
        .unwrap();
    let with = world
        .test_with(
            &source.recorded,
            &[("tenant", "u_42"), ("customerId", "cus_other")],
            TestMode::Recorded,
        )
        .await;
    assert_eq!(result(&with), Some("done"), "{:?}", with.response.error);
    let variables = &with.report.variables;
    assert_eq!(variables.kept["customerId"], "cus_northwind");
    assert_eq!(variables.filled["tenant"], "u_42");
    assert_eq!(variables.dropped, ["legacy"]);

    let without = world.test(&source.recorded, TestMode::Recorded).await;
    assert_eq!(error_kind(&without), Some(ErrorKind::PermissionDenied));
}

// ---- local state -----------------------------------------------------------------------

const VOLUME_BLUEPRINT: &str = "\
name: local
default: allow
vfs:
  mode: per_session
  mounts:
    /data: {mode: named, volume: shared}
";

fn volumes(dir: &std::path::Path) -> VolumeTable {
    VolumeTable::from([("shared".to_owned(), VolumeSpec::local_path(dir))])
}

#[tokio::test]
async fn a_test_run_works_on_copies_and_leaves_the_session_and_volume_alone() {
    let shared = tempfile::tempdir().expect("volume");
    std::fs::write(shared.path().join("log.txt"), "one\n").unwrap();
    let world = World::with(vec![blueprint(VOLUME_BLUEPRINT)], |config| ServerConfig {
        volumes: volumes(shared.path()),
        ..config
    });
    let session = world.open_session("local").await;
    let source = world
        .session_run(
            &session,
            r#"import { writeText } from "submilli:fs";
import session from "submilli:session";
function main(): string {
  writeText("/notes.txt", "written by the source run");
  session.set("k", "from the source");
  return "wrote";
}"#,
        )
        .await;
    assert_eq!(
        source.recorded.session_id.as_deref(),
        Some(session.as_str())
    );

    // The test run reads what the source run wrote, as it is now, and then writes to the
    // file, the volume, and the session's data.
    let program = r#"import { readText, writeText, appendText } from "submilli:fs";
import session from "submilli:session";
function main(): string {
  const seen = String(readText("/notes.txt")) + "/" + String(session.get("k"));
  writeText("/notes.txt", "changed by the test run");
  appendText("/data/log.txt", "two\n");
  session.set("k", "from the test run");
  session.set("only-here", "yes");
  return seen;
}"#;
    let mut recorded = source.recorded.clone();
    recorded.code = Some(program.to_owned());
    let outcome = world.test(&recorded, TestMode::Recorded).await;
    assert_eq!(
        result(&outcome),
        Some("written by the source run/from the source"),
        "{:?}",
        outcome.response.error
    );
    let local = &outcome.report.local_state;
    assert!(local.as_of_now && local.session_found);
    assert_eq!(local.volumes_copied, ["shared"]);

    // The volume is byte-identical, and the session's file and data are as they were.
    assert_eq!(
        std::fs::read(shared.path().join("log.txt")).unwrap(),
        b"one\n"
    );
    let after = world
        .session_run(
            &session,
            r#"import { readText } from "submilli:fs";
import session from "submilli:session";
function main(): string {
  const only = session.get("only-here");
  return String(readText("/notes.txt")) + "/" + String(session.get("k")) + "/" + (only === null ? "none" : "leaked");
}"#,
        )
        .await;
    assert_eq!(
        after.response.result.as_deref(),
        Some("written by the source run/from the source/none")
    );
    let _ = &world.dirs;
}

#[tokio::test]
async fn a_volume_over_the_cap_is_refused_with_the_cap_in_the_message() {
    let shared = tempfile::tempdir().expect("volume");
    // A sparse file: it counts for its length and takes no disk.
    std::fs::File::create(shared.path().join("big.bin"))
        .unwrap()
        .set_len(submilli_server::record::LOCAL_STATE_CAP_BYTES + 1)
        .unwrap();
    let world = World::with(vec![blueprint(VOLUME_BLUEPRINT)], |config| ServerConfig {
        volumes: volumes(shared.path()),
        ..config
    });
    let session = world.open_session("local").await;
    let source = world
        .session_run(&session, "function main(): string { return \"x\"; }")
        .await;
    let error = test_program(
        &world.state,
        TestRun {
            label: "tester".into(),
            recorded: source.recorded,
            bindings: VarBindings::new(),
            mode: TestMode::Recorded,
            secrets: None,
        },
    )
    .await
    .expect_err("refused");
    assert!(error.to_string().contains("256 MiB"), "{error}");
    assert_eq!(
        world.recordings.started.lock().unwrap().len(),
        1,
        "the refused test started no run"
    );
}

// ---- model calls ----------------------------------------------------------------------

struct Dispatch(Arc<AtomicUsize>);

impl ModelDispatch for Dispatch {
    fn dispatch<'a>(
        &'a self,
        _request: ModelRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderFailure>> + Send + 'a>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Ok(ProviderResponse {
                text: Some("answer".to_owned()),
                stop_reason: StopReason::Stop,
                usage: ProviderUsage::reported(7.0, 5.0),
            })
        })
    }
}

fn model_blueprint() -> Blueprint {
    Blueprint {
        name: "llm".into(),
        vfs: VfsConfig::None,
        permissions: BTreeMap::from([(
            "main".to_string(),
            vec![PermissionRule {
                name: None,
                capability: "llm.call".into(),
                filter: None,
                action: Action::Allow,
            }],
        )]),
        llm: LlmConfig {
            providers: BTreeMap::from([(
                "fake".to_string(),
                LlmProviderDecl {
                    provider_type: "anthropic".into(),
                    base_url: None,
                    api_key: None,
                    supports_structured_outputs: true,
                },
            )]),
            models: BTreeMap::from([(
                "test-model".to_string(),
                LlmModelDecl {
                    provider: "fake".into(),
                    context_window: None,
                    output_reserve: Some(100),
                    description: None,
                },
            )]),
        },
        ..Default::default()
    }
}

const BATCH: &str = r#"import llm from "submilli:llm";
function main(): string {
  const rs = llm.batch("test-model", ["a", "b"]);
  return String(rs[0].text) + "," + String(rs[1].text) + "," + String(rs[1].inputTokens);
}"#;

#[tokio::test]
async fn a_model_batch_is_served_whole_without_touching_the_servers_token_budget() {
    let calls = Arc::new(AtomicUsize::new(0));
    let source_world = World::with(vec![model_blueprint()], |config| ServerConfig {
        llm_dispatch: Some(Arc::new(Dispatch(calls.clone()))),
        ..config
    });
    let source = source_world.run("llm", BATCH, &[]).await;
    assert_eq!(source.response.result.as_deref(), Some("answer,answer,7"));
    assert_eq!(calls.load(Ordering::SeqCst), 2, "one dispatch per prompt");

    // A server whose whole token budget is one token: a run that charged it would be
    // refused, and one that reached the provider would be counted.
    let provider_calls = Arc::new(AtomicUsize::new(0));
    let test_world = World::with(vec![model_blueprint()], |config| ServerConfig {
        llm_dispatch: Some(Arc::new(Dispatch(provider_calls.clone()))),
        max_llm_tokens: Some(1),
        ..config
    });
    let live = test_world.run("llm", BATCH, &[]).await;
    assert!(
        live.response.error.is_some(),
        "the budget refuses a live batch"
    );
    let outcome = test_world.test(&source.recorded, TestMode::Recorded).await;
    assert_eq!(
        result(&outcome),
        Some("answer,answer,7"),
        "{:?}",
        outcome.response.error
    );
    assert!(outcome.report.stopped.is_none());
    assert_eq!(outcome.report.served.len(), 1);
    assert_eq!(provider_calls.load(Ordering::SeqCst), 0, "no tokens spent");
}

// ---- embeddings -------------------------------------------------------------------------

/// Answers every embedding request with a unit vector per text, counting the requests.
struct EmbedDispatch(Arc<AtomicUsize>);

impl EmbeddingDispatch for EmbedDispatch {
    fn dispatch<'a>(
        &'a self,
        request: EmbeddingRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<DispatchResponse, DispatchFailure>> + Send + 'a>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        let rows = request
            .texts
            .iter()
            .map(|_| DispatchRow {
                index: None,
                values: vec![1.0, 0.0, 0.0, 0.0],
            })
            .collect();
        Box::pin(async move { Ok(DispatchResponse { rows, usage: None }) })
    }
}

fn embedding_blueprint() -> Blueprint {
    submilli_blueprint::parse(
        "name: embed\n\
         default: allow\n\
         embedding:\n  providers:\n    hf:\n      type: huggingface\n      base_url: https://hf.example.com\n  models:\n    docs:\n      provider: hf\n      model: bge\n      dimensions: 4\n",
    )
    .expect("blueprint parses")
}

const EMBED: &str = r#"import embedding from "submilli:embedding";
function main(): string {
  const r = embedding.embed("docs", ["a text of some length"], "document");
  return "OK:" + r.count.toString();
}"#;

fn embedding_world(calls: &Arc<AtomicUsize>, tokens: Option<u64>) -> World {
    World::with(vec![embedding_blueprint()], |config| ServerConfig {
        embedding_dispatch: Some(Arc::new(EmbedDispatch(calls.clone()))),
        max_embedding_tokens: tokens,
        ..config
    })
}

#[tokio::test]
async fn an_embedding_call_stops_a_recorded_or_reads_live_run_and_never_reaches_the_provider() {
    let calls = Arc::new(AtomicUsize::new(0));
    let world = embedding_world(&calls, None);
    let source = world.run("embed", EMBED, &[]).await;
    assert_eq!(source.response.result.as_deref(), Some("OK:1"));
    let before = calls.load(Ordering::SeqCst);
    assert_eq!(before, 1);

    for mode in [TestMode::Recorded, TestMode::ReadsLive] {
        let outcome = world.test(&source.recorded, mode).await;
        let stop = outcome.report.stopped.as_ref().expect("the run stopped");
        assert_eq!(stop.key, "embedding docs");
        assert_eq!(stop.reason, MissReason::NotRecorded);
        assert_eq!(stop.capability.as_deref(), Some("embedding.embed"));
        assert!(outcome.report.served.is_empty() && outcome.report.went_live.is_empty());
        assert!(outcome.response.result.is_none(), "{:?}", outcome.response);
        assert_eq!(calls.load(Ordering::SeqCst), before, "no provider call");
    }
}

#[tokio::test]
async fn a_live_test_run_sends_an_embedding_call_live_on_a_budget_of_its_own() {
    let source_calls = Arc::new(AtomicUsize::new(0));
    let source = embedding_world(&source_calls, None)
        .run("embed", EMBED, &[])
        .await;
    assert_eq!(source.response.result.as_deref(), Some("OK:1"));

    // A server whose whole embedding budget is one token: a run that charged it would be
    // refused.
    let calls = Arc::new(AtomicUsize::new(0));
    let world = embedding_world(&calls, Some(1));
    let refused = world.run("embed", EMBED, &[]).await;
    assert!(
        refused.response.error.is_some(),
        "the budget refuses a live run"
    );

    let live = world.test(&source.recorded, TestMode::Live).await;
    assert_eq!(result(&live), Some("OK:1"), "{:?}", live.response.error);
    assert!(live.report.stopped.is_none());
    assert_eq!(live.report.went_live.len(), 1);
    assert_eq!(live.report.went_live[0].key, "embedding docs");
    assert_eq!(live.report.went_live[0].reason, MissReason::NotRecorded);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the provider was called once"
    );
}

// ---- MCP ------------------------------------------------------------------------------

mod upstream {
    use rmcp::handler::server::router::tool::ToolRouter;
    use rmcp::handler::server::wrapper::Parameters;
    use rmcp::model::{ServerCapabilities, ServerInfo};
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService,
    };
    use rmcp::{ServerHandler, schemars, tool, tool_handler, tool_router};

    #[derive(serde::Deserialize, schemars::JsonSchema)]
    pub struct IssueRequest {
        /// Issue title.
        #[allow(dead_code)]
        pub title: String,
    }

    #[derive(Clone)]
    pub struct Upstream {
        tool_router: ToolRouter<Self>,
    }

    #[tool_router]
    impl Upstream {
        #[tool(name = "createIssue", description = "Create an issue")]
        fn create_issue(&self, Parameters(req): Parameters<IssueRequest>) -> String {
            format!("created {}", req.title)
        }
    }

    #[tool_handler(router = self.tool_router)]
    impl ServerHandler for Upstream {
        fn get_info(&self) -> ServerInfo {
            ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        }
    }

    /// An upstream on an ephemeral port: its `/mcp` URL and the task serving it.
    pub async fn spawn() -> (String, tokio::task::JoinHandle<()>) {
        let service: StreamableHttpService<Upstream, LocalSessionManager> =
            StreamableHttpService::new(
                || {
                    Ok(Upstream {
                        tool_router: Upstream::tool_router(),
                    })
                },
                Default::default(),
                StreamableHttpServerConfig::default(),
            );
        let router = axum::Router::new().nest_service("/mcp", service);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        (format!("http://{addr}/mcp"), task)
    }
}

fn mcp_blueprint(url: &str) -> Blueprint {
    Blueprint {
        name: "up-bp".into(),
        vfs: VfsConfig::None,
        mcp: BTreeMap::from([(
            "up".to_string(),
            McpServer {
                transport: "streamable_http".into(),
                url: url.to_owned(),
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
    }
}

const ISSUE: &str = r#"import up from "@mcp/up";
function main(): string { return up.createIssue({ title: "x" }) as string; }"#;

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_mcp_program_compiles_from_the_recorded_catalog_and_is_answered_with_the_server_down() {
    let (url, upstream) = upstream::spawn().await;
    let source_world = World::new(vec![mcp_blueprint(&url)]);
    let source = source_world.run("up-bp", ISSUE, &[]).await;
    assert_eq!(source.response.result.as_deref(), Some("created x"));
    assert!(source.recorded.mcp_catalog.is_some());

    // The server goes away, and the test runs on a server that never discovered it.
    upstream.abort();
    let _ = upstream.await;
    let test_world = World::new(vec![mcp_blueprint(&url)]);
    let outcome = test_world.test(&source.recorded, TestMode::Recorded).await;
    assert_eq!(
        result(&outcome),
        Some("created x"),
        "{:?}",
        outcome.response.error
    );
    assert!(outcome.report.stopped.is_none());
    assert_eq!(outcome.report.served.len(), 1);
    assert_eq!(outcome.report.served[0].capability, "mcp.up");
    assert_eq!(
        outcome.report.served[0].key.as_deref(),
        Some("mcp up.createIssue")
    );

    // A call the recording never made stops the run at it.
    let mut other = source.recorded.clone();
    other.code = Some(ISSUE.replace("\"x\"", "\"y\""));
    let stopped = test_world.test(&other, TestMode::Recorded).await;
    let stop = stopped.report.stopped.as_ref().expect("the run stopped");
    assert_eq!(stop.reason, MissReason::RequestDiffers);
    assert_eq!(stop.key, "mcp up.createIssue");
    assert_eq!(error_kind(&stopped), Some(ErrorKind::Cancelled));
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_the_blueprint_no_longer_declares_cannot_be_imported_from_the_recorded_catalog() {
    let (url, _upstream) = upstream::spawn().await;
    let world = World::new(vec![mcp_blueprint(&url)]);
    let source = world.run("up-bp", ISSUE, &[]).await;
    assert_eq!(source.response.result.as_deref(), Some("created x"));
    let mut without = mcp_blueprint(&url);
    without.mcp.clear();
    world.blueprints.upsert(without).await.unwrap();

    let live = world.run("up-bp", ISSUE, &[]).await;
    let live_error = live.response.error.as_ref().expect("a normal run fails");
    let outcome = world.test(&source.recorded, TestMode::Recorded).await;
    let error = outcome.response.error.as_ref().expect("the test run fails");
    assert_eq!(error.kind, live_error.kind);
    assert_eq!(error.message, live_error.message);
    assert!(outcome.report.stopped.is_none(), "a refusal, not a stop");
    assert!(outcome.report.served.is_empty());
}

// ---- reads go live, and continue live ---------------------------------------------------

const ONLY_A: &str = r#"import { get } from "submilli:http";
function main(): string { return get("__A__").body; }"#;

/// Reads `/a`, then posts to `/b` (a denial is caught and reported).
fn read_then_post(server: &MockServer) -> String {
    format!(
        r#"import {{ get, post }} from "submilli:http";
function main(): string {{
  const a = get("{}").body;
  let b = "";
  try {{
    b = post("{}", "payload").body;
  }} catch (e: Error) {{
    b = "denied";
  }}
  return a + "|" + b;
}}"#,
        server.url("/a"),
        server.url("/b"),
    )
}

const DENY_POST: &str = "\
name: bp
default: allow
allow_insecure_http: true
vfs:
  mode: none
permissions:
  main:
    - capability: http.post
      action: deny
";

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reads_live_completes_the_newly_allowed_get_and_records_it_in_the_test_runs_own_log() {
    let server = MockServer::start_async().await;
    let (a, b) = site(&server).await;
    let world = World::new(vec![blueprint(DENY_B)]);
    let source = world.run("bp", &fetch_both(&server), &[]).await;
    world.blueprints.upsert(blueprint(ALLOW_ALL)).await.unwrap();

    let outcome = world.test(&source.recorded, TestMode::ReadsLive).await;
    assert_eq!(
        result(&outcome),
        Some("alpha|bravo"),
        "{:?}",
        outcome.response.error
    );
    let report = &outcome.report;
    assert!(report.stopped.is_none());
    assert_eq!(report.served.len(), 1);
    assert_eq!(report.went_live.len(), 1);
    assert!(report.went_live[0].key.ends_with("/b"));
    assert_eq!(report.went_live[0].reason, MissReason::NoRecording);
    assert_eq!(report.went_live[0].test_call_index, Some(1));
    // The recorded read was not sent again; the new one went out once.
    a.assert_hits_async(1).await;
    b.assert_hits_async(1).await;
    let test_run = world.recordings.run(&report.test_run);
    assert_eq!(test_run.calls.len(), 2);
    let live = &test_run.calls[1];
    assert_eq!(live.capability, "http.get");
    assert!(
        live.response.is_some(),
        "the live response is in the test run's log"
    );
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reads_live_still_stops_at_a_newly_allowed_post_and_live_sends_it() {
    let server = MockServer::start_async().await;
    let (_a, _b) = site(&server).await;
    let posted = server
        .mock_async(|when, then| {
            when.method(Method::POST).path("/b");
            then.status(200).body("posted");
        })
        .await;
    let world = World::new(vec![blueprint(DENY_POST)]);
    let source = world.run("bp", &read_then_post(&server), &[]).await;
    assert_eq!(source.response.result.as_deref(), Some("alpha|denied"));
    world.blueprints.upsert(blueprint(ALLOW_ALL)).await.unwrap();

    let reads = world.test(&source.recorded, TestMode::ReadsLive).await;
    let stop = reads.report.stopped.as_ref().expect("a write still stops");
    assert_eq!(stop.reason, MissReason::NoRecording);
    assert_eq!(stop.capability.as_deref(), Some("http.post"));
    assert!(reads.report.went_live.is_empty());
    assert_eq!(error_kind(&reads), Some(ErrorKind::Cancelled));
    posted.assert_hits_async(0).await;

    let live = world.test(&source.recorded, TestMode::Live).await;
    assert_eq!(
        result(&live),
        Some("alpha|posted"),
        "{:?}",
        live.response.error
    );
    assert!(live.report.stopped.is_none());
    assert_eq!(live.report.served.len(), 1);
    assert_eq!(live.report.went_live.len(), 1);
    posted.assert_hits_async(1).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn live_completes_the_newly_allowed_get_too() {
    let server = MockServer::start_async().await;
    let (a, b) = site(&server).await;
    let world = World::new(vec![blueprint(DENY_B)]);
    let source = world.run("bp", &fetch_both(&server), &[]).await;
    world.blueprints.upsert(blueprint(ALLOW_ALL)).await.unwrap();
    let outcome = world.test(&source.recorded, TestMode::Live).await;
    assert_eq!(result(&outcome), Some("alpha|bravo"));
    a.assert_hits_async(1).await;
    b.assert_hits_async(1).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_live_read_is_still_checked_against_the_current_blueprint() {
    let server = MockServer::start_async().await;
    let (_a, b) = site(&server).await;
    let world = World::new(vec![blueprint(ALLOW_ALL)]);
    let only_a = ONLY_A.replace("__A__", &server.url("/a"));
    let source = world.run("bp", &only_a, &[]).await;
    // The blueprint now denies `/b`, and the program being tested reads it.
    world.blueprints.upsert(blueprint(DENY_B)).await.unwrap();
    let mut recorded = source.recorded.clone();
    recorded.code = Some(fetch_both(&server));
    let outcome = world.test(&recorded, TestMode::Live).await;
    assert_eq!(
        result(&outcome),
        Some("alpha|denied"),
        "{:?}",
        outcome.response.error
    );
    assert!(outcome.report.went_live.is_empty(), "denied before it left");
    b.assert_hits_async(0).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_live_read_is_still_checked_against_the_network_policy() {
    let server = MockServer::start_async().await;
    let (_a, b) = site(&server).await;
    let recorded_on = World::new(vec![blueprint(ALLOW_ALL)]);
    let only_a = ONLY_A.replace("__A__", &server.url("/a"));
    let source = recorded_on.run("bp", &only_a, &[]).await;

    // A server that refuses loopback addresses: the recorded read is served, the new one
    // is refused by the live client's own policy.
    let strict = World::with(vec![blueprint(ALLOW_ALL)], |config| ServerConfig {
        network_policy: interpreter::runtime::NetworkPolicy::deny_private(),
        ..config
    });
    let mut recorded = source.recorded.clone();
    recorded.code = Some(fetch_both(&server));
    let outcome = strict.test(&recorded, TestMode::ReadsLive).await;
    assert_eq!(
        result(&outcome),
        Some("alpha|denied"),
        "{:?}",
        outcome.response.error
    );
    assert_eq!(outcome.report.served.len(), 1);
    assert_eq!(
        outcome.report.went_live.len(),
        1,
        "it reached the live client"
    );
    b.assert_hits_async(0).await;
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_mcp_call_with_nothing_recorded_stops_unless_the_run_continues_live() {
    let (url, _upstream) = upstream::spawn().await;
    let world = World::new(vec![mcp_blueprint(&url)]);
    let source = world.run("up-bp", ISSUE, &[]).await;
    let mut other = source.recorded.clone();
    other.code = Some(ISSUE.replace("\"x\"", "\"y\""));

    let reads = world.test(&other, TestMode::ReadsLive).await;
    let stop = reads
        .report
        .stopped
        .as_ref()
        .expect("an MCP call still stops");
    assert_eq!(stop.reason, MissReason::RequestDiffers);
    assert_eq!(result(&reads), None);

    let live = world.test(&other, TestMode::Live).await;
    assert_eq!(
        result(&live),
        Some("created y"),
        "{:?}",
        live.response.error
    );
    assert!(live.report.stopped.is_none());
    assert_eq!(live.report.went_live.len(), 1);
    assert_eq!(live.report.went_live[0].key, "mcp up.createIssue");
}

#[tokio::test]
async fn a_model_call_with_nothing_recorded_stops_unless_the_run_continues_live() {
    let calls = Arc::new(AtomicUsize::new(0));
    let world = World::with(vec![model_blueprint()], |config| ServerConfig {
        llm_dispatch: Some(Arc::new(Dispatch(calls.clone()))),
        ..config
    });
    let source = world.run("llm", BATCH, &[]).await;
    let before = calls.load(Ordering::SeqCst);
    let mut other = source.recorded.clone();
    other.code = Some(BATCH.replace("[\"a\", \"b\"]", "[\"a\", \"c\"]"));

    let reads = world.test(&other, TestMode::ReadsLive).await;
    assert!(reads.report.stopped.is_some());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        before,
        "a model call stays stopped"
    );

    let live = world.test(&other, TestMode::Live).await;
    assert_eq!(
        result(&live),
        Some("answer,answer,7"),
        "{:?}",
        live.response.error
    );
    assert_eq!(live.report.went_live.len(), 1);
    assert_eq!(calls.load(Ordering::SeqCst), before + 2);
}

// ---- harness secrets --------------------------------------------------------------------

/// Reads `/a`, then `/b`, sending the harness secret `K` as a bearer token. The first
/// blueprint denies `/b`; the second allows it.
fn secret_blueprint(deny_b: bool) -> Blueprint {
    let deny = if deny_b {
        "permissions:\n  main:\n    - capability: http.get\n      filter: path == \"/b\"\n      action: deny\n"
    } else {
        ""
    };
    blueprint(&format!(
        "name: bp\ndefault: allow\nallow_insecure_http: true\nvfs:\n  mode: none\nsecrets:\n  K:\n    harness:\n      required: true\nauth_proxy:\n  - host: 127.0.0.1\n    allow_insecure_http: true\n    auth:\n      bearer: K\n{deny}"
    ))
}

fn secrets(token: &str) -> submilli_blueprint::HarnessSecretBindings {
    [("K".to_owned(), token.to_owned())].into()
}

/// Runs `code` as a program that was given the secret `K`.
async fn run_with_secret(world: &World, code: &str) -> RecordedRun {
    let response = run_program(
        &world.state,
        ProgramRun {
            label: "source".into(),
            blueprint: "bp".into(),
            code: code.into(),
            variables: Default::default(),
            secrets: secrets("tok-source"),
        },
    )
    .await;
    world.recordings.run(&response.execution_id)
}

#[tokio::test]
async fn a_missing_required_secret_is_refused_naming_it_and_starts_no_run() {
    let world = World::new(vec![secret_blueprint(true)]);
    let recorded = run_with_secret(&world, "function main(): string { return \"x\"; }").await;
    let started = world.recordings.started.lock().unwrap().len();
    let error = test_program(
        &world.state,
        TestRun {
            label: "tester".into(),
            recorded,
            bindings: VarBindings::new(),
            mode: TestMode::Live,
            secrets: None,
        },
    )
    .await
    .expect_err("a missing secret is refused");
    assert!(matches!(error, TestError::InvalidSecrets(_)), "{error}");
    let message = error.to_string();
    assert!(
        message.contains("invalid secrets") && message.contains('K'),
        "{message}"
    );
    assert_eq!(
        world.recordings.started.lock().unwrap().len(),
        started,
        "a refusal starts no run"
    );
}

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_live_call_carries_the_secret_the_test_run_was_given() {
    let server = MockServer::start_async().await;
    let _a = server
        .mock_async(|when, then| {
            when.method(Method::GET).path("/a");
            then.status(200).body("alpha");
        })
        .await;
    let b = server
        .mock_async(|when, then| {
            when.method(Method::GET)
                .path("/b")
                .header("authorization", "Bearer tok-test");
            then.status(200).body("bravo");
        })
        .await;
    let world = World::new(vec![secret_blueprint(true)]);
    let recorded = run_with_secret(&world, &fetch_both(&server)).await;
    world
        .blueprints
        .upsert(secret_blueprint(false))
        .await
        .unwrap();
    let outcome = test_program(
        &world.state,
        TestRun {
            label: "tester".into(),
            recorded,
            bindings: VarBindings::new(),
            mode: TestMode::Live,
            secrets: Some(secrets("tok-test")),
        },
    )
    .await
    .expect("the test run starts");
    assert_eq!(
        result(&outcome),
        Some("alpha|bravo"),
        "{:?}",
        outcome.response.error
    );
    b.assert_hits_async(1).await;
}

// ---- cancelling from outside ------------------------------------------------------------

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cancel_from_outside_stops_a_test_run_waiting_on_a_live_call() {
    let server = MockServer::start_async().await;
    let _a = server
        .mock_async(|when, then| {
            when.method(Method::GET).path("/a");
            then.status(200).body("alpha");
        })
        .await;
    let world = World::new(vec![blueprint(DENY_B)]);
    let source = world.run("bp", &fetch_both(&server), &[]).await;
    world.blueprints.upsert(blueprint(ALLOW_ALL)).await.unwrap();
    // The newly allowed read now goes live, and the page is slow to answer.
    let slow = server
        .mock_async(|when, then| {
            when.method(Method::GET).path("/b");
            then.status(200)
                .body("bravo")
                .delay(std::time::Duration::from_secs(30));
        })
        .await;

    let state = world.state.clone();
    let recorded = source.recorded.clone();
    let running = tokio::spawn(async move {
        test_program(
            &state,
            TestRun {
                label: "tester".into(),
                recorded,
                bindings: VarBindings::new(),
                mode: TestMode::Live,
                secrets: None,
            },
        )
        .await
    });
    let cancelled = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        // Cancel once the live call is in flight.
        while slow.hits_async().await == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        loop {
            let id = world
                .recordings
                .started
                .lock()
                .unwrap()
                .iter()
                .find(|start| start.entry == RunEntry::Test)
                .map(|start| start.execution_id.clone());
            if let Some(id) = id
                && world.state.cancel_run(&id)
            {
                return id;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the test run was registered for cancelling");
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(20), running)
        .await
        .expect("the cancelled test run returns")
        .unwrap()
        .expect("the test run started");
    assert_eq!(outcome.response.execution_id, cancelled);
    assert_eq!(error_kind(&outcome), Some(ErrorKind::Cancelled));
    assert!(
        outcome.report.stopped.is_none(),
        "someone outside asked; no call was missed"
    );
    assert!(!world.state.cancel_run(&cancelled), "no longer running");
    slow.assert_hits_async(1).await;
}

// ---- a volume referenced more than once -------------------------------------------------

#[tokio::test]
async fn a_volume_one_mount_only_reads_and_another_writes_is_still_copied() {
    let shared = tempfile::tempdir().expect("volume");
    std::fs::write(shared.path().join("log.txt"), "one\n").unwrap();
    let twice = "name: twice\ndefault: allow\nvfs:\n  mode: per_session\n  mounts:\n    /a: {mode: named, volume: shared, access: read_only}\n    /b: {mode: named, volume: shared}\n";
    let world = World::with(vec![blueprint(twice)], |config| ServerConfig {
        volumes: volumes(shared.path()),
        ..config
    });
    let session = world.open_session("twice").await;
    let source = world
        .session_run(&session, "function main(): string { return \"x\"; }")
        .await;
    let mut recorded = source.recorded.clone();
    recorded.code = Some(
        r#"import { appendText, readText } from "submilli:fs";
function main(): string {
  appendText("/b/log.txt", "two\n");
  return String(readText("/a/log.txt"));
}"#
        .to_owned(),
    );
    let outcome = world.test(&recorded, TestMode::Recorded).await;
    assert_eq!(
        result(&outcome),
        Some("one\ntwo\n"),
        "{:?}",
        outcome.response.error
    );
    assert_eq!(outcome.report.local_state.volumes_copied, ["shared"]);
    assert_eq!(
        std::fs::read(shared.path().join("log.txt")).unwrap(),
        b"one\n",
        "the volume is byte-identical"
    );
}

// ---- a credential the auth proxy added never reaches a report ---------------------------

#[cfg_attr(skip_http_tests, ignore = "HTTP tests disabled")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_proxy_injected_query_value_appears_in_no_report_error_or_call_log() {
    let server = MockServer::start_async().await;
    let _file = server
        .mock_async(|when, then| {
            when.method(Method::GET).path("/f.bin");
            then.status(200).body("data");
        })
        .await;
    let proxied = "name: dl\ndefault: allow\nallow_insecure_http: true\nvfs:\n  mode: per_session\nsecrets:\n  K:\n    harness:\n      required: true\nauth_proxy:\n  - host: 127.0.0.1\n    allow_insecure_http: true\n    query:\n      appid: \"${secrets.K}\"\n";
    let world = World::new(vec![blueprint(proxied)]);
    let url = server.url("/f.bin");
    let code = format!(
        r#"import {{ download }} from "submilli:http";
function main(): number {{
  return download("{url}", "/f.bin").bytesWritten;
}}"#
    );
    let response = run_program(
        &world.state,
        ProgramRun {
            label: "source".into(),
            blueprint: "dl".into(),
            code,
            variables: Default::default(),
            secrets: [("K".to_owned(), "SECRET-TOKEN".to_owned())].into(),
        },
    )
    .await;
    assert_eq!(
        response.result.as_deref(),
        Some("4"),
        "{:?}",
        response.error
    );
    let recorded = world.recordings.run(&response.execution_id);

    // A download is never answered, so the test run stops at it.
    let outcome = test_program(
        &world.state,
        TestRun {
            label: "tester".into(),
            recorded,
            bindings: VarBindings::new(),
            mode: TestMode::Recorded,
            secrets: Some([("K".to_owned(), "SECRET-TOKEN".to_owned())].into()),
        },
    )
    .await
    .expect("the test run starts");
    let stop = outcome.report.stopped.as_ref().expect("the run stopped");
    assert_eq!(stop.reason, MissReason::Download);
    assert_eq!(stop.key, format!("http GET {url}"));
    assert!(
        stop.nearest.is_some(),
        "the recorded download is the nearest"
    );
    let test_log = world.recordings.run(&outcome.response.execution_id);
    for text in [
        serde_json::to_string(&outcome.report).unwrap(),
        serde_json::to_string(&outcome.response).unwrap(),
        serde_json::to_string(&test_log).unwrap(),
    ] {
        assert!(!text.contains("SECRET-TOKEN"), "{text}");
    }
}

// ---- the same model request as a call or a batch ----------------------------------------

#[tokio::test]
async fn a_batch_the_test_run_makes_as_a_call_is_still_paired_with_its_source() {
    let calls = Arc::new(AtomicUsize::new(0));
    let world = World::with(vec![model_blueprint()], |config| ServerConfig {
        llm_dispatch: Some(Arc::new(Dispatch(calls))),
        ..config
    });
    let source = world
        .run(
            "llm",
            r#"import llm from "submilli:llm";
function main(): string { return String(llm.batch("test-model", ["a"])[0].text); }"#,
            &[],
        )
        .await;
    let mut recorded = source.recorded.clone();
    recorded.code = Some(
        r#"import llm from "submilli:llm";
function main(): string { return String(llm.call("test-model", "a").text); }"#
            .to_owned(),
    );
    let outcome = world.test(&recorded, TestMode::Recorded).await;
    assert_eq!(
        result(&outcome),
        Some("answer"),
        "{:?}",
        outcome.response.error
    );
    assert_eq!(outcome.report.served.len(), 1);
    assert!(outcome.report.served[0].test_call_index.is_some());
}

// ---- a model stop cannot be caught -------------------------------------------------------

#[tokio::test]
async fn a_model_call_stop_inside_try_catch_still_ends_the_run_cancelled() {
    let calls = Arc::new(AtomicUsize::new(0));
    let world = World::with(vec![model_blueprint()], |config| ServerConfig {
        llm_dispatch: Some(Arc::new(Dispatch(calls.clone()))),
        ..config
    });
    let guarded = |prompt: &str| {
        format!(
            r#"import llm from "submilli:llm";
function main(): string {{
  try {{ return String(llm.call("test-model", "{prompt}").text); }} catch (e) {{ return "caught"; }}
}}"#
        )
    };
    let source = world.run("llm", &guarded("a"), &[]).await;
    assert_eq!(source.response.result.as_deref(), Some("answer"));
    let before = calls.load(Ordering::SeqCst);
    let mut recorded = source.recorded.clone();
    recorded.code = Some(guarded("b"));
    let outcome = world.test(&recorded, TestMode::Recorded).await;
    assert_eq!(result(&outcome), None, "the program did not finish");
    assert_eq!(error_kind(&outcome), Some(ErrorKind::Cancelled));
    assert!(outcome.report.stopped.is_some());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        before,
        "nothing reached the model"
    );
}

// ---- refusals before anything is copied or run -------------------------------------------

#[tokio::test]
async fn missing_secrets_are_refused_before_any_local_state_is_copied() {
    let shared = tempfile::tempdir().expect("volume");
    let both = "name: both\ndefault: allow\nvfs:\n  mode: per_session\n  mounts:\n    /data: {mode: named, volume: shared}\nsecrets:\n  K:\n    harness:\n      required: true\n";
    let world = World::with(vec![blueprint(both)], |config| ServerConfig {
        volumes: volumes(shared.path()),
        ..config
    });
    let response = run_program(
        &world.state,
        ProgramRun {
            label: "source".into(),
            blueprint: "both".into(),
            code: "function main(): string { return \"x\"; }".into(),
            variables: Default::default(),
            secrets: secrets("tok"),
        },
    )
    .await;
    let recorded = world.recordings.run(&response.execution_id);
    // Copying this would be refused for its size; the refusal that comes is the secret's.
    std::fs::File::create(shared.path().join("big.bin"))
        .unwrap()
        .set_len(submilli_server::record::LOCAL_STATE_CAP_BYTES + 1)
        .unwrap();
    let error = test_program(
        &world.state,
        TestRun {
            label: "tester".into(),
            recorded,
            bindings: VarBindings::new(),
            mode: TestMode::Recorded,
            secrets: None,
        },
    )
    .await
    .expect_err("refused");
    assert!(matches!(error, TestError::InvalidSecrets(_)), "{error}");
}

#[tokio::test]
async fn a_server_with_no_run_recorder_refuses_a_test_run() {
    let source_world = World::new(vec![blueprint(ALLOW_ALL)]);
    let source = source_world
        .run("bp", "function main(): string { return \"x\"; }", &[])
        .await;
    let world = World::with(vec![blueprint(ALLOW_ALL)], |config| ServerConfig {
        run_recorder: None,
        ..config
    });
    let error = test_program(
        &world.state,
        TestRun {
            label: "tester".into(),
            recorded: source.recorded,
            bindings: VarBindings::new(),
            mode: TestMode::Recorded,
            secrets: None,
        },
    )
    .await
    .expect_err("refused");
    assert!(matches!(error, TestError::NoRecorder), "{error}");
}

/// Records nothing.
struct Declines;

impl RunRecorderFactory for Declines {
    fn start(&self, _run: RunStart) -> Option<Arc<dyn RunRecorder>> {
        None
    }
}

#[tokio::test]
async fn a_recorder_that_declines_the_test_run_refuses_it_before_it_runs() {
    let server = MockServer::start_async().await;
    let (_a, b) = site(&server).await;
    let source_world = World::new(vec![blueprint(DENY_B)]);
    let source = source_world.run("bp", &fetch_both(&server), &[]).await;
    let world = World::with(vec![blueprint(ALLOW_ALL)], |config| ServerConfig {
        run_recorder: Some(Arc::new(Declines)),
        ..config
    });
    let error = test_program(
        &world.state,
        TestRun {
            label: "tester".into(),
            recorded: source.recorded,
            bindings: VarBindings::new(),
            mode: TestMode::Live,
            secrets: None,
        },
    )
    .await
    .expect_err("refused");
    assert!(matches!(error, TestError::NoRecorder), "{error}");
    assert!(error.to_string().contains("declined"), "{error}");
    b.assert_hits_async(0).await;
}

// ---- a model the blueprint no longer declares ---------------------------------------------

#[tokio::test]
async fn a_model_removed_from_the_blueprint_fails_as_it_does_in_a_normal_run() {
    let calls = Arc::new(AtomicUsize::new(0));
    let world = World::with(vec![model_blueprint()], |config| ServerConfig {
        llm_dispatch: Some(Arc::new(Dispatch(calls.clone()))),
        ..config
    });
    let source = world.run("llm", BATCH, &[]).await;
    let mut without = model_blueprint();
    without.llm.models.clear();
    world.blueprints.upsert(without).await.unwrap();

    let live = world.run("llm", BATCH, &[]).await;
    let live_error = live
        .response
        .error
        .as_ref()
        .expect("a normal run is refused");
    assert!(
        live_error.message.contains("does not serve that model"),
        "{live_error:?}"
    );
    let outcome = world.test(&source.recorded, TestMode::Recorded).await;
    let error = outcome
        .response
        .error
        .as_ref()
        .expect("the test run is refused");
    assert_eq!(error.message, live_error.message);
    assert!(outcome.report.stopped.is_none(), "a refusal, not a stop");
    assert!(outcome.report.served.is_empty());
}
