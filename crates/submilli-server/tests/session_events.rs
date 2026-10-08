//! Session events: each run and MCP tool call streams ordered events while it happens,
//! with the sizes and timing a page's summary and log need.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use interpreter::runtime::{CallOutcome, DecisionLogConfig, ModelUsage};
use serde_json::{Value, json};
use submilli_blueprint::{
    Action, Blueprint, LlmConfig, LlmModelDecl, LlmProviderDecl, PermissionRule, VfsConfig,
};
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::record::{
    EventKind, FinishedRun, RunRecorder, RunRecorderFactory, RunStart, SessionEvent,
};
use submilli_server::{AppState, ServerConfig, app};
use submilli_shared::llm::{
    ModelDispatch, ModelRequest, ProviderFailure, ProviderResponse, ProviderUsage, StopReason,
};
use tower::ServiceExt;

const BLUEPRINT: &str = "events";
const MODEL: &str = "test-model";

fn allow(capabilities: &[&str]) -> BTreeMap<String, Vec<PermissionRule>> {
    BTreeMap::from([(
        "main".to_string(),
        capabilities
            .iter()
            .map(|capability| PermissionRule {
                name: None,
                capability: (*capability).into(),
                filter: None,
                action: Action::Allow,
            })
            .collect(),
    )])
}

fn blueprint() -> Blueprint {
    Blueprint {
        name: BLUEPRINT.into(),
        vfs: VfsConfig::Ephemeral {
            size_limit: None,
            mounts: Default::default(),
            cwd: None,
        },
        permissions: allow(&[
            "fs.read",
            "fs.write",
            "session.read",
            "session.write",
            "llm.call",
        ]),
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
                MODEL.to_string(),
                LlmModelDecl {
                    provider: "fake".into(),
                    context_window: None,
                    output_reserve: Some(1_000),
                    description: None,
                },
            )]),
        },
        ..Default::default()
    }
}

/// Answers every prompt, reporting usage only when told to.
struct Model {
    usage: ProviderUsage,
}

impl ModelDispatch for Model {
    fn dispatch<'a>(
        &'a self,
        _request: ModelRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<ProviderResponse, ProviderFailure>> + Send + 'a>> {
        let usage = self.usage;
        Box::pin(async move {
            Ok(ProviderResponse {
                text: Some("answer".to_string()),
                stop_reason: StopReason::Stop,
                usage,
            })
        })
    }
}

// ---- the collecting recorder -----------------------------------------------------------

#[derive(Default)]
struct Collected {
    events: Mutex<Vec<SessionEvent>>,
    runs: Mutex<Vec<FinishedRun>>,
    changed: tokio::sync::Notify,
}

struct Factory {
    collected: Arc<Collected>,
    log: DecisionLogConfig,
    events: bool,
}

impl RunRecorderFactory for Factory {
    fn start(&self, _run: RunStart) -> Option<Arc<dyn RunRecorder>> {
        Some(Arc::new(Recorder {
            collected: self.collected.clone(),
            log: self.log.clone(),
        }))
    }

    fn wants_events(&self) -> bool {
        self.events
    }

    fn event(&self, event: SessionEvent) {
        self.collected.events.lock().unwrap().push(event);
        self.collected.changed.notify_waiters();
    }
}

struct Recorder {
    collected: Arc<Collected>,
    log: DecisionLogConfig,
}

impl RunRecorder for Recorder {
    fn log_config(&self) -> DecisionLogConfig {
        self.log.clone()
    }

    fn finish(&self, run: FinishedRun) {
        self.collected.runs.lock().unwrap().push(run);
    }
}

impl Collected {
    /// Waits until `done` holds for the events delivered so far, and returns them.
    async fn until(&self, done: impl Fn(&[SessionEvent]) -> bool) -> Vec<SessionEvent> {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                let notified = self.changed.notified();
                {
                    let events = self.events.lock().unwrap();
                    if done(&events) {
                        return events.clone();
                    }
                }
                notified.await;
            }
        })
        .await
        .expect("the events arrive")
    }

    /// The events through the first run's `returned`.
    async fn first_run(&self) -> Vec<SessionEvent> {
        self.until(|events| {
            events
                .iter()
                .any(|event| matches!(event.kind, EventKind::Returned { .. }))
        })
        .await
    }
}

struct Server {
    state: AppState,
    collected: Arc<Collected>,
    _dirs: tempfile::TempDir,
}

fn server_with(log: DecisionLogConfig, usage: ProviderUsage) -> Server {
    server_built(log, usage, true)
}

fn server_built(log: DecisionLogConfig, usage: ProviderUsage, events: bool) -> Server {
    let dirs = tempfile::tempdir().expect("dirs");
    let collected = Arc::new(Collected::default());
    let config = ServerConfig {
        blueprints: Some(Arc::new(
            InMemoryBlueprintStore::seed([blueprint()]).expect("seed"),
        )),
        session_storage_root: Some(dirs.path().join("sessions")),
        llm_dispatch: Some(Arc::new(Model { usage })),
        run_recorder: Some(Arc::new(Factory {
            collected: collected.clone(),
            log,
            events,
        })),
        ..in_memory_config::config()
    };
    Server {
        state: futures::executor::block_on(AppState::new(config)).expect("state"),
        collected,
        _dirs: dirs,
    }
}

fn server() -> Server {
    server_with(
        DecisionLogConfig::default(),
        ProviderUsage::reported(12.0, 3.0),
    )
}

impl Server {
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
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let response = app(self.state.clone())
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
        let body = if content_type.starts_with("text/event-stream") {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            text.lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .next_back()
                .and_then(|data| serde_json::from_str(data.trim()).ok())
                .unwrap_or(Value::Null)
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, headers, body)
    }

    async fn execute(&self, code: &str) -> Value {
        let (status, _, body) = self
            .post(
                "/v1/execute",
                json!({ "code": code, "blueprint": BLUEPRINT }),
                &[],
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn mcp_session(&self) -> String {
        let initialize = json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }
        });
        let (_, headers, _) = self
            .post(&format!("/mcp/{BLUEPRINT}"), initialize, &[])
            .await;
        let session = headers["mcp-session-id"].to_str().unwrap().to_owned();
        self.post(
            &format!("/mcp/{BLUEPRINT}"),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            &[("mcp-session-id", &session)],
        )
        .await;
        session
    }

    async fn mcp_tool(&self, session: &str, tool: &str, arguments: Value, meta: Value) -> Value {
        let call = json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": tool, "arguments": arguments, "_meta": meta }
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

fn kinds(events: &[SessionEvent]) -> Vec<&'static str> {
    events
        .iter()
        .map(|event| match &event.kind {
            EventKind::RunStarted { .. } => "run-started",
            EventKind::CallStarted { .. } => "call-started",
            EventKind::Decision { .. } => "decision",
            EventKind::CallFinished { .. } => "call-finished",
            EventKind::RunFinished { .. } => "run-finished",
            EventKind::Returned { .. } => "returned",
            EventKind::ToolCall { .. } => "tool-call",
        })
        .collect()
}

fn call_finished(events: &[SessionEvent], capability: &str) -> (Option<u64>, Option<u64>) {
    events
        .iter()
        .find_map(|event| match &event.kind {
            EventKind::CallFinished {
                capability: name,
                sent_bytes,
                result_bytes,
                ..
            } if name == capability => Some((*sent_bytes, *result_bytes)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no {capability} call finished: {events:#?}"))
}

// ---- order, sizes, and timing ------------------------------------------------------------

const TWO_CALLS: &str = r#"
import * as fs from "submilli:fs";
function main(): string {
  fs.writeText("/a.txt", "hello");
  return fs.readText("/a.txt") ?? "";
}
"#;

#[tokio::test]
async fn a_run_with_two_calls_emits_its_events_in_order() {
    let server = server();
    server.execute(TWO_CALLS).await;
    let events = server.collected.first_run().await;
    assert_eq!(
        kinds(&events),
        [
            "run-started",
            "call-started",
            "decision",
            "call-finished",
            "call-started",
            "decision",
            "call-finished",
            "run-finished",
            "returned",
        ]
    );
    let seqs: Vec<_> = events.iter().map(|event| event.seq).collect();
    assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "{seqs:?}");
    let times: Vec<_> = events.iter().map(|event| event.at_micros).collect();
    assert!(times.windows(2).all(|w| w[0] <= w[1]), "{times:?}");
    let run = events[0].run_id.clone().expect("a run id");
    assert!(
        events
            .iter()
            .all(|event| event.run_id.as_ref() == Some(&run))
    );
    assert!(events.iter().all(|event| event.session_id.is_some()));
    assert_eq!(call_finished(&events, "fs.read").1, Some(5));
}

#[tokio::test]
async fn calls_carry_increasing_indexes_and_their_own_timing() {
    let server = server();
    server
        .execute(
            r#"
import * as fs from "submilli:fs";
import session from "submilli:session";
function main(): void {
  fs.writeText("/a.txt", "x");
  session.set("k", 1);
  fs.readText("/a.txt");
}
"#,
        )
        .await;
    let events = server.collected.first_run().await;
    let finished: Vec<_> = events
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::CallFinished {
                call_index,
                capability,
                started_micros,
                ended_micros,
                outcome,
                ..
            } => Some((
                *call_index,
                capability.clone(),
                *started_micros,
                *ended_micros,
                *outcome,
            )),
            _ => None,
        })
        .collect();
    let capabilities: Vec<_> = finished.iter().map(|call| call.1.as_str()).collect();
    assert_eq!(capabilities, ["fs.write", "session.write", "fs.read"]);
    assert!(
        finished
            .windows(2)
            .all(|w| w[0].0 < w[1].0 && w[0].2 <= w[1].2)
    );
    for (_, _, started, ended, outcome) in &finished {
        assert!(ended.is_some_and(|ended| ended >= *started));
        assert_eq!(*outcome, Some(CallOutcome::Returned));
    }
}

#[tokio::test]
async fn a_read_stored_truncated_still_reports_its_full_size() {
    let server = server();
    server
        .execute(
            r#"
import * as fs from "submilli:fs";
function main(): number {
  fs.writeText("/big.txt", "x".repeat(2 * 1024 * 1024));
  return (fs.readText("/big.txt") ?? "").length;
}
"#,
        )
        .await;
    let events = server.collected.first_run().await;
    assert_eq!(call_finished(&events, "fs.read").1, Some(2 * 1024 * 1024));
    let runs = server.collected.runs.lock().unwrap();
    let read = runs[0]
        .log
        .calls
        .iter()
        .find(|call| call.capability == "fs.read")
        .unwrap();
    let response = read.response.as_ref().unwrap();
    assert!(response.truncated, "the copy is capped");
    assert_eq!(response.bytes, 2 * 1024 * 1024);
}

#[tokio::test]
async fn a_copy_cut_to_the_budget_keeps_its_end_and_full_size() {
    let server = server_with(
        DecisionLogConfig {
            max_recorder_bytes: 64 * 1024,
            ..DecisionLogConfig::default()
        },
        ProviderUsage::default(),
    );
    server
        .execute(
            r#"
import * as fs from "submilli:fs";
function main(): void {
  fs.writeText("/big.txt", "x".repeat(512 * 1024));
  fs.readText("/big.txt");
}
"#,
        )
        .await;
    let events = server.collected.first_run().await;
    assert_eq!(call_finished(&events, "fs.read").1, Some(512 * 1024));
    let runs = server.collected.runs.lock().unwrap();
    let read = runs[0]
        .log
        .calls
        .iter()
        .find(|call| call.capability == "fs.read")
        .unwrap();
    assert!(read.ended_micros.is_some());
    let response = read.response.as_ref().unwrap();
    // The copy is cut to the room the budget has, never the whole body.
    assert!(response.truncated);
    let kept = match &response.body {
        Some(interpreter::runtime::BodyCopy::Text(text)) => text.len(),
        other => panic!("a cut text body is kept as text: {other:?}"),
    };
    assert!(kept > 0 && kept < 64 * 1024, "{kept}");
    assert_eq!(response.bytes, 512 * 1024);
}

// ---- model calls ---------------------------------------------------------------------------

const MODEL_CALL: &str = r#"
import llm from "submilli:llm";
function main(): string { return llm.call("test-model", "say hi").text ?? ""; }
"#;

#[tokio::test]
async fn a_model_call_records_its_input_size_and_reported_usage() {
    let server = server();
    server.execute(MODEL_CALL).await;
    let events = server.collected.first_run().await;
    let (sent, received) = call_finished(&events, "llm.call");
    assert_eq!(sent, Some("say hi".len() as u64));
    assert_eq!(received, Some("answer".len() as u64));
    let usage = events.iter().find_map(|event| match &event.kind {
        EventKind::CallFinished { usage, .. } => *usage,
        _ => None,
    });
    assert_eq!(
        usage,
        Some(ModelUsage {
            input_tokens: Some(12),
            output_tokens: Some(3),
        })
    );
}

#[tokio::test]
async fn unreported_model_usage_is_absent_not_zero() {
    let server = server_with(DecisionLogConfig::default(), ProviderUsage::default());
    server.execute(MODEL_CALL).await;
    let events = server.collected.first_run().await;
    let finished = events
        .iter()
        .find(|event| matches!(&event.kind, EventKind::CallFinished { capability, .. } if capability == "llm.call"))
        .unwrap();
    let EventKind::CallFinished { usage, .. } = &finished.kind else {
        unreachable!()
    };
    assert_eq!(
        *usage,
        Some(ModelUsage {
            input_tokens: None,
            output_tokens: None,
        })
    );
    let value = serde_json::to_value(finished).unwrap();
    assert_eq!(
        value["usage"],
        json!({}),
        "unknown counts are absent: {value}"
    );
}

// ---- MCP tool calls --------------------------------------------------------------------------

#[tokio::test]
async fn every_mcp_tool_call_on_a_session_is_an_event() {
    let server = server();
    let session = server.mcp_session().await;
    server
        .mcp_tool(
            &session,
            "submilli__typescript__packages__docs",
            json!({ "name": "submilli:fs" }),
            json!({ "claudecode/toolUseId": "toolu_docs" }),
        )
        .await;
    let events = server.collected.until(|events| !events.is_empty()).await;
    let [event] = events.as_slice() else {
        panic!("one event: {events:#?}");
    };
    let EventKind::ToolCall {
        tool,
        ok,
        result_bytes,
        ..
    } = &event.kind
    else {
        panic!("a tool call: {event:?}");
    };
    assert_eq!(tool, "submilli__typescript__packages__docs");
    assert!(*ok && *result_bytes > 0);
    assert_eq!(event.session_id.as_deref(), Some(session.as_str()));
    assert_eq!(event.tool_call_id.as_deref(), Some("toolu_docs"));
    assert_eq!(event.run_id, None);
}

#[tokio::test]
async fn an_mcp_execute_links_its_run_events_to_the_tool_call() {
    let server = server();
    let session = server.mcp_session().await;
    server
        .mcp_tool(
            &session,
            "submilli__typescript__execute",
            json!({ "code": TWO_CALLS }),
            json!({ "claudecode/toolUseId": "toolu_exec" }),
        )
        .await;
    let events = server
        .collected
        .until(|events| {
            events
                .iter()
                .any(|event| matches!(event.kind, EventKind::ToolCall { .. }))
        })
        .await;
    assert_eq!(kinds(&events).last(), Some(&"tool-call"));
    assert!(
        events
            .iter()
            .all(|event| event.tool_call_id.as_deref() == Some("toolu_exec")),
        "{events:#?}"
    );
    assert!(
        events
            .iter()
            .all(|event| event.session_id.as_deref() == Some(session.as_str()))
    );
}

// ---- the overhead of recording and streaming ----------------------------------------------

/// Not a check: prints what recording and streaming add to a call-heavy run. Run with
/// `cargo test --release -p submilli-server --test session_events -- --ignored --nocapture`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "timing measurement, not a check"]
async fn overhead_of_recording_and_events() {
    const CALLS: u32 = 2_000;
    let program = format!(
        r#"
import session from "submilli:session";
function main(): number {{
  session.set("k", 1);
  let total = 0;
  for (let i = 0; i < {CALLS}; i++) {{ total += session.get<number>("k") ?? 0; }}
  return total;
}}
"#
    );
    let plain = {
        let dirs = tempfile::tempdir().unwrap();
        let config = ServerConfig {
            blueprints: Some(Arc::new(
                InMemoryBlueprintStore::seed([blueprint()]).unwrap(),
            )),
            session_storage_root: Some(dirs.path().join("sessions")),
            ..in_memory_config::config()
        };
        (AppState::new(config).await.unwrap(), dirs)
    };
    let recorded = server();
    let without_events = server_built(
        DecisionLogConfig::default(),
        ProviderUsage::default(),
        false,
    );
    let time = |state: AppState| {
        let program = program.clone();
        async move {
            let mut best = Duration::MAX;
            for _ in 0..7 {
                let started = std::time::Instant::now();
                let response = app(state.clone())
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri("/v1/execute")
                            .header("content-type", "application/json")
                            .body(Body::from(
                                json!({ "code": program, "blueprint": BLUEPRINT }).to_string(),
                            ))
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                let body = response.into_body().collect().await.unwrap().to_bytes();
                let body: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body["result"], json!(CALLS.to_string()), "{body}");
                best = best.min(started.elapsed());
            }
            best
        }
    };
    let without = time(plain.0.clone()).await;
    let recording = time(without_events.state.clone()).await;
    let with = time(recorded.state.clone()).await;
    let per_call = |total: Duration| total.as_secs_f64() * 1e6 / f64::from(CALLS + 1);
    println!(
        "{CALLS} session reads, best of 7: unrecorded {:.2} µs/call; recorded {:.2} \
         (+{:.2}); recorded with events {:.2} (+{:.2})",
        per_call(without),
        per_call(recording),
        per_call(recording) - per_call(without),
        per_call(with),
        per_call(with) - per_call(without),
    );
}
