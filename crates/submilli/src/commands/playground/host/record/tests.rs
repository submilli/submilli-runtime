//! Recording and Test on the host: events streamed from real runs, a recorded run
//! re-checked and tested again, and the policy's records as a run keeps them.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use interpreter::runtime::NetworkPolicy;
use submilli_blueprint::{HarnessSecretBindings, VarBindings};
use submilli_build::PackageStore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::super::super::state::StateDir;
use super::super::blueprints::AppliedBlueprint;
use super::super::run::{
    self, ErrorKind, FinishedRun, RunEntry, RunEvent, RunHost, RunHostConfig, RunRecorder,
    RunRequest, RunStart, VfsSource,
};
use super::super::sessions::Sessions;
use super::super::volumes::{VolumeTable, Volumes};
use super::RecordedRun;
use super::events::{EVENT_SCHEMA, EventHub, EventKind, EventSink, RunEvents, SessionEvent};
use super::test_run::{TestError, TestHost, TestMode, TestOutcome, TestRun, test_program};

mod policy_records;
mod recheck;

// ---- a host that records ---------------------------------------------------------

/// What the test recorder kept: each run's start and recording, and every event.
#[derive(Default)]
struct Kept {
    starts: Mutex<Vec<RunStart>>,
    runs: Mutex<Vec<RecordedRun>>,
    events: Mutex<Vec<SessionEvent>>,
}

impl EventSink for Kept {
    fn event(&self, event: SessionEvent) {
        self.events.lock().unwrap().push(event);
    }
}

struct Recorder {
    kept: Arc<Kept>,
    hub: Arc<EventHub>,
}

impl RunRecorder for Recorder {
    fn start(&self, run: RunStart) -> Option<Arc<dyn run::RecordedRun>> {
        self.kept.starts.lock().unwrap().push(run.clone());
        Some(Arc::new(Recording {
            events: RunEvents::start(&self.hub, &run),
            start: run,
            kept: Arc::clone(&self.kept),
        }))
    }
}

struct Recording {
    start: RunStart,
    events: Arc<RunEvents>,
    kept: Arc<Kept>,
}

impl run::RecordedRun for Recording {
    fn event(&self, event: RunEvent<'_>) {
        self.events.observe(event);
    }

    fn finish(&self, run: FinishedRun) {
        self.events.finished(
            run.dispatched,
            run.error.as_ref().map(|error| error.kind),
            run.wall,
            run.console.len() as u64,
        );
        self.kept
            .runs
            .lock()
            .unwrap()
            .push(RecordedRun::from_parts(&self.start, &run));
    }

    fn returned(&self, bytes: u64) {
        self.events.returned(bytes);
    }
}

struct Host {
    _dir: tempfile::TempDir,
    sessions: Arc<Sessions>,
    volumes: Arc<Volumes>,
    runs: RunHost,
    kept: Arc<Kept>,
}

async fn host() -> Host {
    host_recording(true).await
}

async fn host_recording(records: bool) -> Host {
    let dir = tempfile::tempdir().unwrap();
    let state = StateDir::for_project(dir.path());
    let sessions = Arc::new(Sessions::boot(&state).await.unwrap().sessions);
    let volumes = Arc::new(Volumes::boot(&state, VolumeTable::new()));
    let kept = Arc::new(Kept::default());
    let store_root = dir.path().join("store");
    std::fs::create_dir_all(&store_root).unwrap();
    let mut config = RunHostConfig::new(PackageStore::new(store_root));
    config.network_policy = NetworkPolicy::deny_private().allow_localhost(true);
    if records {
        config.recorder = Some(Arc::new(Recorder {
            kept: Arc::clone(&kept),
            hub: EventHub::new(Arc::clone(&kept) as Arc<dyn EventSink>),
        }));
    }
    let runs = RunHost::new(config, Arc::clone(&sessions), Arc::clone(&volumes)).unwrap();
    Host {
        _dir: dir,
        sessions,
        volumes,
        runs,
        kept,
    }
}

fn applied(yaml: &str) -> AppliedBlueprint {
    AppliedBlueprint {
        blueprint: Arc::new(submilli_blueprint::parse(yaml).unwrap()),
        yaml: yaml.to_owned(),
        version_tag: "v1".to_owned(),
    }
}

impl Host {
    async fn open(&self, blueprint: &AppliedBlueprint) -> String {
        self.sessions
            .open(&blueprint.blueprint, &BTreeMap::new())
            .await
            .unwrap()
            .id
    }

    /// Runs `code` in `session`, and returns its recording.
    async fn run_in(
        &self,
        session: &str,
        blueprint: &AppliedBlueprint,
        code: &str,
        tool_call_id: Option<&str>,
    ) -> (run::RunOutput, RecordedRun) {
        let output = self
            .runs
            .run(RunRequest {
                session_id: session.to_owned(),
                code: code.to_owned(),
                label: "app".to_owned(),
                entry: RunEntry::Mcp,
                blueprint: blueprint.clone(),
                variables: Arc::new(VarBindings::new()),
                harness_secrets: Arc::new(HarnessSecretBindings::new()),
                client: None,
                tool_call_id: tool_call_id.map(str::to_owned),
                test_of: None,
                vfs: VfsSource::Session,
                finalizers: None,
                world: None,
            })
            .await;
        let recorded = self
            .kept
            .runs
            .lock()
            .unwrap()
            .iter()
            .find(|run| run.execution_id == output.response.execution_id)
            .cloned()
            .expect("the run is recorded");
        (output, recorded)
    }

    async fn test(
        &self,
        blueprint: &AppliedBlueprint,
        recorded: RecordedRun,
        mode: TestMode,
    ) -> Result<TestOutcome, TestError> {
        test_program(
            TestHost {
                runs: &self.runs,
                sessions: &self.sessions,
                volumes: &self.volumes,
                blueprint: Some(blueprint.clone()),
            },
            TestRun {
                label: "tester".to_owned(),
                recorded,
                bindings: VarBindings::new(),
                mode,
                secrets: None,
            },
        )
        .await
    }

    /// Every event delivered so far, once delivery has gone quiet.
    async fn events(&self) -> Vec<SessionEvent> {
        let mut seen = 0;
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let now = self.kept.events.lock().unwrap().len();
            if now == seen && now > 0 {
                return self.kept.events.lock().unwrap().clone();
            }
            seen = now;
        }
    }
}

/// A local site: `/a` answers `alpha` and `/b` answers `bravo`. Counts the requests
/// it answers, and stops answering when dropped.
struct Site {
    url: String,
    hits: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Site {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn site() -> Site {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&hits);
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let hits = Arc::clone(&counted);
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut buffer = [0u8; 1024];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    match stream.read(&mut buffer).await {
                        Ok(0) | Err(_) => return,
                        Ok(read) => request.extend_from_slice(&buffer[..read]),
                    }
                }
                let body = if request.starts_with(b"GET /b ") {
                    "bravo"
                } else {
                    "alpha"
                };
                hits.fetch_add(1, Ordering::SeqCst);
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            });
        }
    });
    Site { url, hits, task }
}

const OPEN: &str = "name: bp\ndefault: allow\nallow_insecure_http: true\nvfs:\n  mode: none\n";

fn fetch(url: &str) -> String {
    format!(
        r#"import {{ get }} from "submilli:http";
function main(): string {{ return get("{url}/a").body; }}"#
    )
}

fn result(outcome: &TestOutcome) -> Option<&str> {
    outcome.response.result.as_deref()
}

fn error_kind(outcome: &TestOutcome) -> Option<ErrorKind> {
    outcome.response.error.as_ref().map(|error| error.kind)
}

// ---- test runs -------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_recorded_test_serves_its_calls_from_the_recording_with_the_site_gone() {
    let host = host().await;
    let blueprint = applied(OPEN);
    let site = site().await;
    let session = host.open(&blueprint).await;
    let (output, recorded) = host
        .run_in(&session, &blueprint, &fetch(&site.url), None)
        .await;
    assert_eq!(output.response.result.as_deref(), Some("alpha"));
    assert_eq!(site.hits.load(Ordering::SeqCst), 1);
    let hits = Arc::clone(&site.hits);
    drop(site);

    let outcome = host
        .test(&blueprint, recorded.clone(), TestMode::Recorded)
        .await
        .expect("the test runs");
    assert_eq!(result(&outcome), Some("alpha"), "{:?}", outcome.response);
    assert_eq!(hits.load(Ordering::SeqCst), 1, "nothing reached the site");
    let report = &outcome.report;
    assert_eq!(report.source_run, recorded.execution_id);
    assert_eq!(report.served.len(), 1);
    assert_eq!(report.served[0].capability, "http.get");
    assert!(report.served[0].test_call_index.is_some());
    assert!(report.stopped.is_none() && report.went_live.is_empty());

    // Recorded as a run of its own, linked to the run it tested.
    let starts = host.kept.starts.lock().unwrap();
    let test = starts
        .iter()
        .find(|start| start.execution_id == report.test_run)
        .expect("the test run is recorded");
    assert_eq!(test.entry, RunEntry::Test);
    assert_eq!(
        test.test_of.as_deref(),
        Some(recorded.execution_id.as_str())
    );
    assert_eq!(test.label, "tester");
    assert_ne!(test.session_id, session, "a session of its own");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_call_the_recording_lacks_stops_the_test_and_the_report_names_it() {
    let host = host().await;
    let blueprint = applied(OPEN);
    let site = site().await;
    let session = host.open(&blueprint).await;
    let (_, mut recorded) = host
        .run_in(&session, &blueprint, &fetch(&site.url), None)
        .await;
    recorded.calls.clear();

    let outcome = host
        .test(&blueprint, recorded, TestMode::Recorded)
        .await
        .expect("the test runs");
    assert_eq!(error_kind(&outcome), Some(ErrorKind::Cancelled));
    let stop = outcome.report.stopped.as_ref().expect("the run stopped");
    assert!(stop.key.starts_with("http GET "), "{}", stop.key);
    assert!(stop.key.ends_with("/a"), "{}", stop.key);
    assert!(outcome.report.served.is_empty());
    assert_eq!(site.hits.load(Ordering::SeqCst), 1, "the stop went nowhere");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_test_run_that_writes_files_leaves_the_source_sessions_files_alone() {
    let host = host().await;
    let blueprint = applied("name: bp\ndefault: allow\nvfs:\n  mode: per_session\n");
    let session = host.open(&blueprint).await;
    let code = r#"import * as fs from "submilli:fs";
import * as session from "submilli:session";
function main(): string {
  let text = "";
  try { text = fs.readText("/note.txt") ?? ""; } catch (e: Error) {}
  text = text + "x";
  fs.writeText("/note.txt", text);
  session.set("note", text);
  return text;
}"#;
    let (output, recorded) = host.run_in(&session, &blueprint, code, None).await;
    assert_eq!(
        output.response.result.as_deref(),
        Some("x"),
        "{:?}",
        output.response.error
    );

    let outcome = host
        .test(&blueprint, recorded, TestMode::Recorded)
        .await
        .expect("the test runs");
    assert_eq!(result(&outcome), Some("xx"), "it saw today's file");
    assert!(outcome.report.local_state.session_found);
    let note = host.sessions.session_dir(&session).join("note.txt");
    assert_eq!(std::fs::read_to_string(note).unwrap(), "x");

    // The source session's values are as its own run left them.
    let (output, _) = host
        .run_in(
            &session,
            &blueprint,
            r#"import * as session from "submilli:session";
function main(): string { return session.get<string>("note") ?? "none"; }"#,
            None,
        )
        .await;
    assert_eq!(output.response.result.as_deref(), Some("x"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_test_is_refused_without_a_recorder_a_program_or_the_blueprint_it_ran_under() {
    let host = host().await;
    let blueprint = applied(OPEN);
    let session = host.open(&blueprint).await;
    let (_, recorded) = host
        .run_in(
            &session,
            &blueprint,
            "function main(): string { return \"x\"; }",
            None,
        )
        .await;

    let mut no_program = recorded.clone();
    no_program.code = None;
    let error = host
        .test(&blueprint, no_program, TestMode::Recorded)
        .await
        .expect_err("refused");
    assert!(matches!(error, TestError::NoProgram { .. }), "{error}");
    assert!(
        error.to_string().contains(&recorded.execution_id),
        "{error}"
    );

    let other = applied("name: other\nvfs:\n  mode: none\n");
    let error = host
        .test(&other, recorded.clone(), TestMode::Recorded)
        .await
        .expect_err("refused");
    assert!(
        matches!(error, TestError::BlueprintNotFound(ref name) if name == "bp"),
        "{error}"
    );

    let unrecorded = host_recording(false).await;
    let error = unrecorded
        .test(&blueprint, recorded, TestMode::Recorded)
        .await
        .expect_err("refused");
    assert!(matches!(error, TestError::NoRecorder), "{error}");
}

// ---- events ----------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_runs_events_carry_their_schema_sequence_session_run_and_tool_call() {
    let host = host().await;
    let blueprint = applied(
        "name: bp\ndefault: deny\nvfs:\n  mode: none\npermissions:\n  main:\n    - capability: test.com/op\n      action: allow\n",
    );
    let session = host.open(&blueprint).await;
    let (output, _) = host
        .run_in(
            &session,
            &blueprint,
            r#"import { check } from "submilli:security";
function main(): number { console.log("one"); check("test.com/op", {}); return 1; }"#,
            Some("toolu_1"),
        )
        .await;
    let events = host.events().await;
    let kinds: Vec<&str> = events
        .iter()
        .map(|event| match &event.kind {
            EventKind::RunStarted { .. } => "run-started",
            EventKind::CallStarted { .. } => "call-started",
            EventKind::Decision { .. } => "decision",
            EventKind::CallFinished { .. } => "call-finished",
            EventKind::RunFinished { .. } => "run-finished",
            EventKind::Returned { .. } => "returned",
            EventKind::ToolCall { .. } => "tool-call",
            EventKind::Console { .. } => "console",
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "run-started",
            "console",
            "call-started",
            "decision",
            "call-finished",
            "run-finished",
            "returned"
        ]
    );
    let seqs: Vec<u64> = events.iter().map(|event| event.seq).collect();
    assert_eq!(
        seqs,
        (1..=events.len() as u64).collect::<Vec<_>>(),
        "no gap"
    );
    for event in &events {
        assert_eq!(event.schema, EVENT_SCHEMA);
        assert_eq!(event.session_id.as_deref(), Some(session.as_str()));
        assert_eq!(
            event.run_id.as_deref(),
            Some(output.response.execution_id.as_str())
        );
        assert_eq!(event.tool_call_id.as_deref(), Some("toolu_1"));
    }
    let EventKind::RunStarted {
        entry, code_hash, ..
    } = &events[0].kind
    else {
        unreachable!();
    };
    assert_eq!(entry, "mcp");
    assert_eq!(code_hash.as_ref().map(String::len), Some(64));
    let EventKind::RunFinished { events_dropped, .. } = &events[5].kind else {
        unreachable!();
    };
    assert_eq!(*events_dropped, 0);
    let value = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(value["kind"], "run-started");
    assert_eq!(value["tool_call_id"], "toolu_1");
}
