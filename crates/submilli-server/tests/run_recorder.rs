//! The run recorder: every program the server runs is recorded once, labeled by the
//! token or entry point that started it, whoever sent it and however it ended.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use interpreter::RuntimeConfig;
use interpreter::runtime::{CallOutcome, DecisionRecord, EntryPath};
use serde_json::{Value, json};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::error::ErrorKind;
use submilli_server::record::{
    FinishedRun, ProgramRun, RetryLink, RunEntry, RunRecorder, RunRecorderFactory, RunStart,
    run_program,
};
use submilli_server::{ApiToken, AppState, AuthConfig, Role, ServerConfig, app};
use tower::ServiceExt;

const BLUEPRINT: &str = "rec";
const APP_TOKEN: &str = "app-token-value-0123456789abcdef0123456789abcdef";

const BLUEPRINT_YAML: &str = "\
name: rec
default: deny
variables:
  customerId:
    required: false
vfs:
  mode: ephemeral
permissions:
  main:
    - name: ok
      capability: test.com/ok
      action: allow
    - capability: fs.write
      action: allow
";

// ---- the recording factory -----------------------------------------------------------

/// What a finished run handed its recorder, kept for assertions.
struct Finished {
    start: RunStart,
    dispatched: bool,
    error: Option<ErrorKind>,
    result: Option<String>,
    decisions: Vec<DecisionRecord>,
    calls: Vec<interpreter::runtime::CallRecord>,
    /// The `@mcp/<server>` packages the run compiled against, by name.
    mcp_packages: Option<Vec<String>>,
    returned: Option<u64>,
}

#[derive(Default)]
struct Runs {
    started: Mutex<Vec<RunStart>>,
    finished: Mutex<Vec<Finished>>,
    retried: Mutex<Vec<RetryLink>>,
    changed: tokio::sync::Notify,
}

impl Runs {
    fn finished(&self) -> std::sync::MutexGuard<'_, Vec<Finished>> {
        self.finished.lock().unwrap()
    }

    fn only(&self) -> std::sync::MutexGuard<'_, Vec<Finished>> {
        let finished = self.finished();
        assert_eq!(finished.len(), 1, "one run is recorded");
        finished
    }

    async fn wait_for(&self, count: usize) {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let notified = self.changed.notified();
                if self.finished().len() >= count {
                    return;
                }
                notified.await;
            }
        })
        .await
        .expect("the run finishes");
    }

    async fn wait_for_start(&self) -> RunStart {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let notified = self.changed.notified();
                if let Some(start) = self.started.lock().unwrap().first().cloned() {
                    return start;
                }
                notified.await;
            }
        })
        .await
        .expect("the run starts")
    }
}

struct Factory(Arc<Runs>);

impl RunRecorderFactory for Factory {
    fn start(&self, run: RunStart) -> Option<Arc<dyn RunRecorder>> {
        self.0.started.lock().unwrap().push(run.clone());
        self.0.changed.notify_waiters();
        Some(Arc::new(Recorder {
            runs: self.0.clone(),
            start: run,
        }))
    }

    fn retried(&self, retry: RetryLink) {
        self.0.retried.lock().unwrap().push(retry);
    }
}

struct Recorder {
    runs: Arc<Runs>,
    start: RunStart,
}

impl RunRecorder for Recorder {
    fn finish(&self, run: FinishedRun) {
        self.runs.finished().push(Finished {
            start: self.start.clone(),
            dispatched: run.dispatched,
            error: run.error.map(|error| error.kind),
            result: run.result,
            decisions: run.log.records,
            calls: run.log.calls,
            mcp_packages: run.mcp_catalog.map(|catalog| {
                catalog
                    .defs_refs()
                    .iter()
                    .map(|defs| defs.package_name.clone())
                    .collect()
            }),
            returned: None,
        });
        self.runs.changed.notify_waiters();
    }

    fn returned(&self, bytes: u64) {
        let mut finished = self.runs.finished();
        if let Some(run) = finished
            .iter_mut()
            .rev()
            .find(|run| run.start.execution_id == self.start.execution_id)
        {
            run.returned = Some(bytes);
        }
    }
}

// ---- the server ------------------------------------------------------------------------

struct Server {
    state: AppState,
    runs: Arc<Runs>,
    _dirs: tempfile::TempDir,
}

fn server(record: bool, runtime: RuntimeConfig) -> Server {
    let blueprint = submilli_blueprint::parse(BLUEPRINT_YAML).expect("blueprint");
    // Declares a package the store does not hold.
    let with_package = submilli_blueprint::parse(
        "name: withpkg\ndefault: deny\npackages:\n  - \"@acme/missing\"\n",
    )
    .expect("blueprint");
    let blueprints =
        Arc::new(InMemoryBlueprintStore::seed([blueprint, with_package]).expect("seed"));
    let dirs = tempfile::tempdir().expect("dirs");
    let runs = Arc::new(Runs::default());
    let config = ServerConfig {
        blueprints: Some(blueprints),
        auth: AuthConfig::Tokens(vec![
            ApiToken::new("app", Role::User, APP_TOKEN).expect("token"),
        ]),
        session_storage_root: Some(dirs.path().join("sessions")),
        session_store_dir: Some(dirs.path().join("store")),
        package_store_root: Some(dirs.path().join("packages")),
        runtime,
        run_recorder: record.then(|| Arc::new(Factory(runs.clone())) as _),
        ..in_memory_config::config()
    };
    Server {
        state: AppState::new(config).expect("state"),
        runs,
        _dirs: dirs,
    }
}

fn recorded() -> Server {
    server(true, RuntimeConfig::default())
}

impl Server {
    fn router(&self) -> Router {
        app(self.state.clone())
    }

    async fn post(
        &self,
        uri: &str,
        body: Value,
        headers: &[(&str, &str)],
    ) -> (StatusCode, HeaderMap, Value) {
        let mut builder = Request::builder()
            .method("POST")
            .uri(uri)
            .header("host", "localhost")
            .header("authorization", format!("Bearer {APP_TOKEN}"))
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let response = self
            .router()
            .oneshot(builder.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let content_type = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (status, headers, parse_body(&content_type, &bytes))
    }

    async fn execute(&self, code: &str) -> Value {
        self.execute_on(BLUEPRINT, code).await
    }

    async fn execute_on(&self, blueprint: &str, code: &str) -> Value {
        let (status, _, body) = self
            .post(
                "/v1/execute",
                json!({ "code": code, "blueprint": blueprint }),
                &[],
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn open_session(&self, variables: Value) -> String {
        let (status, _, body) = self
            .post(
                "/v1/sessions",
                json!({ "blueprint": BLUEPRINT, "variables": variables }),
                &[],
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["session_id"].as_str().unwrap().to_owned()
    }

    async fn session_execute(&self, session: &str, code: &str, key: Option<&str>) -> Value {
        let headers: Vec<_> = key
            .map(|key| ("idempotency-key", key))
            .into_iter()
            .collect();
        let (status, _, body) = self
            .post(
                &format!("/v1/sessions/{session}/execute"),
                json!({ "code": code }),
                &headers,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    /// The MCP handshake, binding `variables`; returns the session id.
    async fn mcp_session(&self, variables: Value) -> String {
        let initialize = json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": { "name": "langchain-mcp-adapters", "version": "0" },
                "_meta": { "variables": variables }
            }
        });
        let (status, headers, body) = self
            .post(&format!("/mcp/{BLUEPRINT}"), initialize, &[])
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let session = headers["mcp-session-id"].to_str().unwrap().to_owned();
        let initialized = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        self.post(
            &format!("/mcp/{BLUEPRINT}"),
            initialized,
            &[("mcp-session-id", &session)],
        )
        .await;
        session
    }

    async fn mcp_tool(&self, session: &str, tool: &str, arguments: Value) -> Value {
        let call = json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": tool, "arguments": arguments }
        });
        let (status, _, body) = self
            .post(
                &format!("/mcp/{BLUEPRINT}"),
                call,
                &[("mcp-session-id", session)],
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
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
            .filter_map(|line| line.strip_prefix("data:"))
            .next_back()
            .unwrap_or("")
            .trim();
        return serde_json::from_str(data).unwrap_or(Value::Null);
    }
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

const ALLOWED: &str = r#"
import { check } from "submilli:security";
function main(): string { check("test.com/ok", {}); return "done"; }
"#;

// ---- one run per execution, labeled -----------------------------------------------------

#[tokio::test]
async fn rest_session_and_mcp_execute_each_record_exactly_one_run() {
    let server = recorded();
    server.execute(ALLOWED).await;
    let session = server.open_session(json!({})).await;
    server.session_execute(&session, ALLOWED, None).await;
    let mcp = server.mcp_session(json!({})).await;
    server
        .mcp_tool(
            &mcp,
            "submilli__typescript__execute",
            json!({ "code": ALLOWED }),
        )
        .await;

    let finished = server.runs.finished();
    let entries: Vec<_> = finished.iter().map(|run| run.start.entry.clone()).collect();
    assert_eq!(entries, [RunEntry::Http, RunEntry::Session, RunEntry::Mcp]);
    for run in finished.iter() {
        assert_eq!(run.start.label, "app");
        assert_eq!(run.start.test_of, None);
        assert!(run.dispatched);
        assert_eq!(run.error, None);
        assert_eq!(run.result.as_deref(), Some("done"));
        let [decision] = run.decisions.as_slice() else {
            panic!("one decision: {:#?}", run.decisions);
        };
        assert!(decision.allowed);
        assert_eq!(run.calls.len(), 1);
        // The blueprint declares no MCP servers: the run compiled against none.
        assert_eq!(run.mcp_packages, Some(Vec::new()));
        assert!(run.start.blueprint_hash.is_some());
        assert_eq!(run.start.code.as_deref(), Some(ALLOWED));
    }
}

#[tokio::test]
async fn an_mcp_run_records_its_token_binding_and_client() {
    let server = recorded();
    let session = server
        .mcp_session(json!({ "customerId": "cus_northwind" }))
        .await;
    server
        .mcp_tool(
            &session,
            "submilli__typescript__execute",
            json!({ "code": ALLOWED }),
        )
        .await;
    let finished = server.runs.only();
    let run = &finished[0];
    assert_eq!(run.start.label, "app");
    assert_eq!(run.start.client.as_deref(), Some("langchain-mcp-adapters"));
    assert_eq!(run.start.session_id.as_deref(), Some(session.as_str()));
    assert_eq!(
        run.start.variables.get("customerId").map(String::as_str),
        Some("cus_northwind")
    );
}

#[tokio::test]
async fn the_token_itself_never_reaches_a_recorded_run() {
    let server = recorded();
    server.execute(ALLOWED).await;
    let finished = server.runs.only();
    let run = &finished[0];
    let recorded = format!(
        "{} {:?} {:?} {:?}",
        run.start.label, run.start.client, run.start.session_id, run.start.variables
    );
    assert!(!recorded.contains(APP_TOKEN), "{recorded}");
    let decisions = serde_json::to_string(&run.decisions).unwrap();
    assert!(!decisions.contains(APP_TOKEN));
}

// ---- outcomes ----------------------------------------------------------------------------

#[tokio::test]
async fn a_type_error_is_recorded_as_a_compile_failure() {
    let server = recorded();
    let body = server
        .execute("function main(): number { return \"text\"; }")
        .await;
    assert_eq!(body["error"]["kind"], "compile_error");
    let finished = server.runs.only();
    assert!(finished[0].dispatched);
    assert_eq!(finished[0].error, Some(ErrorKind::CompileError));
    assert!(finished[0].decisions.is_empty());
}

#[tokio::test]
async fn an_uninstalled_package_is_recorded_as_a_package_resolution_failure() {
    let server = recorded();
    let body = server
        .execute_on(
            "withpkg",
            "import { x } from \"@acme/missing\"; function main(): number { return x(); }",
        )
        .await;
    assert_eq!(body["error"]["kind"], "package_resolution");
    let finished = server.runs.only();
    assert!(!finished[0].dispatched);
    assert_eq!(finished[0].error, Some(ErrorKind::PackageResolution));
    assert_eq!(finished[0].mcp_packages, None);
}

#[tokio::test]
async fn top_level_statements_record_an_allowed_then_a_denied_call_and_the_denial() {
    let server = recorded();
    let body = server
        .execute(
            r#"
import { check } from "submilli:security";
check("test.com/ok", {});
check("test.com/denied", {});
function main(): string { return "unreachable"; }
"#,
        )
        .await;
    assert_eq!(body["error"]["kind"], "permission_denied");
    let finished = server.runs.only();
    let run = &finished[0];
    assert_eq!(run.error, Some(ErrorKind::PermissionDenied));
    let verdicts: Vec<_> = run
        .decisions
        .iter()
        .map(|d| (d.capability.as_str(), d.allowed))
        .collect();
    assert_eq!(
        verdicts,
        [("test.com/ok", true), ("test.com/denied", false)]
    );
}

const SPIN_AFTER_A_CALL: &str = r#"
import { check } from "submilli:security";
function main(): number {
  check("test.com/ok", {});
  let spins = 0;
  while (true) { spins = spins + 1; }
  return spins;
}
"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_run_that_times_out_is_recorded_with_its_decisions() {
    let server = server(
        true,
        RuntimeConfig {
            fuel: u64::MAX,
            timeout: Some(Duration::from_millis(300)),
            ..RuntimeConfig::default()
        },
    );
    let body = server.execute(SPIN_AFTER_A_CALL).await;
    assert_eq!(body["error"]["kind"], "timeout");
    let finished = server.runs.only();
    assert_eq!(finished[0].error, Some(ErrorKind::Timeout));
    assert_eq!(finished[0].decisions.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_dropped_mid_run_is_still_recorded_when_the_run_ends() {
    let server = server(
        true,
        RuntimeConfig {
            fuel: u64::MAX,
            timeout: Some(Duration::from_millis(1500)),
            ..RuntimeConfig::default()
        },
    );
    let request = server.execute(SPIN_AFTER_A_CALL);
    // The client gives up while the program spins; the server finishes the run.
    let gave_up = tokio::time::timeout(Duration::from_millis(300), request).await;
    assert!(gave_up.is_err(), "the run is still spinning");
    server.runs.wait_for(1).await;
    let finished = server.runs.only();
    assert_eq!(finished[0].error, Some(ErrorKind::Timeout));
    assert_eq!(
        finished[0].decisions.len(),
        1,
        "the call made before the drop"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_recorded_run_can_be_cancelled_by_id_whoever_sent_it() {
    let server = server(
        true,
        RuntimeConfig {
            fuel: u64::MAX,
            timeout: Some(Duration::from_secs(30)),
            ..RuntimeConfig::default()
        },
    );
    let state = server.state.clone();
    let running = tokio::spawn(async move {
        run_program(
            &state,
            ProgramRun {
                label: "assistant".into(),
                blueprint: BLUEPRINT.into(),
                code: SPIN_AFTER_A_CALL.into(),
                variables: BTreeMap::new(),
                secrets: Default::default(),
            },
        )
        .await
    });
    let start = server.runs.wait_for_start().await;
    let cancelled = tokio::time::timeout(Duration::from_secs(10), async {
        while !server.state.cancel_run(&start.execution_id) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(cancelled.is_ok(), "the run was registered for cancelling");
    let response = tokio::time::timeout(Duration::from_secs(20), running)
        .await
        .expect("the cancelled run returns")
        .unwrap();
    assert_eq!(response.error.map(|e| e.kind), Some(ErrorKind::Cancelled));
    let finished = server.runs.only();
    assert_eq!(finished[0].start.label, "assistant");
    assert_eq!(finished[0].start.entry, RunEntry::Program);
    assert_eq!(finished[0].error, Some(ErrorKind::Cancelled));
    assert!(
        !server.state.cancel_run(&start.execution_id),
        "no longer running"
    );
}

#[tokio::test]
async fn run_program_records_a_run_under_the_label_it_was_given() {
    let server = recorded();
    let response = run_program(
        &server.state,
        ProgramRun {
            label: "example".into(),
            blueprint: BLUEPRINT.into(),
            code: ALLOWED.into(),
            variables: BTreeMap::from([("customerId".into(), "cus_initech".into())]),
            secrets: Default::default(),
        },
    )
    .await;
    assert_eq!(response.result.as_deref(), Some("done"));
    let finished = server.runs.only();
    let run = &finished[0];
    assert_eq!(run.start.label, "example");
    assert_eq!(run.start.execution_id, response.execution_id);
    assert_eq!(
        run.start.variables.get("customerId").map(String::as_str),
        Some("cus_initech")
    );
}

// ---- retries and file tools ----------------------------------------------------------------

#[tokio::test]
async fn an_idempotent_retry_records_no_second_run_and_links_to_the_first() {
    let server = recorded();
    let session = server.open_session(json!({})).await;
    let first = server
        .session_execute(&session, ALLOWED, Some("key-1"))
        .await;
    let again = server
        .session_execute(&session, ALLOWED, Some("key-1"))
        .await;
    assert_eq!(first, again);
    assert_eq!(server.runs.finished().len(), 1);
    assert_eq!(
        server.runs.finished()[0].start.idempotency_key.as_deref(),
        Some("key-1")
    );
    let retried = server.runs.retried.lock().unwrap();
    let [retry] = retried.as_slice() else {
        panic!("one retry: {retried:?}");
    };
    assert_eq!(retry.idempotency_key, "key-1");
    assert_eq!(
        retry.original_execution_id.as_deref(),
        first["execution_id"].as_str()
    );
}

#[tokio::test]
async fn a_denied_mcp_file_read_is_a_single_decision_run() {
    let server = recorded();
    let session = server.mcp_session(json!({})).await;
    let body = server
        .mcp_tool(
            &session,
            "submilli__files__read",
            json!({ "path": "/a.txt" }),
        )
        .await;
    assert_eq!(
        body["result"]["structuredContent"]["error"]["kind"], "permission_denied",
        "{body}"
    );
    let finished = server.runs.only();
    let run = &finished[0];
    assert_eq!(
        run.start.entry,
        RunEntry::McpFileTool {
            tool: "submilli__files__read".into()
        }
    );
    assert_eq!(run.start.label, "app");
    assert_eq!(run.start.code, None);
    assert_eq!(run.mcp_packages, None, "a file tool compiles nothing");
    assert_eq!(run.error, Some(ErrorKind::PermissionDenied));
    let [decision] = run.decisions.as_slice() else {
        panic!("one decision: {:#?}", run.decisions);
    };
    assert!(!decision.allowed);
    assert_eq!(decision.capability, "fs.read");
    assert_eq!(decision.entry_path, EntryPath::FileTool);
    let [call] = run.calls.as_slice() else {
        panic!("one call");
    };
    assert_eq!(call.outcome, Some(CallOutcome::Failed));
}

// ---- what the caller received, and nothing changes without a recorder -----------------------

#[tokio::test]
async fn successful_and_failed_mcp_executes_record_what_the_agent_received() {
    let server = recorded();
    let session = server.mcp_session(json!({})).await;
    server
        .mcp_tool(
            &session,
            "submilli__typescript__execute",
            json!({ "code": ALLOWED }),
        )
        .await;
    server
        .mcp_tool(
            &session,
            "submilli__typescript__execute",
            json!({ "code": "function main(): number { throw new Error(\"boom\"); }" }),
        )
        .await;
    let finished = server.runs.finished();
    let sizes: Vec<_> = finished.iter().map(|run| run.returned).collect();
    let [Some(ok), Some(failed)] = sizes.as_slice() else {
        panic!("both report a size: {sizes:?}");
    };
    assert!(*ok > 0 && *failed > *ok, "{ok} {failed}");
}

/// Everything in a response but its per-run ids.
fn without_ids(mut body: Value) -> Value {
    if let Some(fields) = body.as_object_mut() {
        fields.remove("execution_id");
        fields.remove("session_id");
    }
    body
}

#[tokio::test]
async fn responses_are_the_same_with_and_without_a_recorder() {
    let programs = [
        ALLOWED,
        "function main(): number { return \"text\"; }",
        "import { check } from \"submilli:security\";\nfunction main(): void { check(\"test.com/denied\", {}); }",
    ];
    let plain = server(false, RuntimeConfig::default());
    let recorded = recorded();
    for program in programs {
        assert_eq!(
            without_ids(recorded.execute(program).await),
            without_ids(plain.execute(program).await),
            "{program}"
        );
    }
    assert!(plain.runs.finished().is_empty());
}
