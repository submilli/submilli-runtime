//! The store on its own, fed the server's callbacks directly; `server` below runs real
//! programs through an in-process server.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use interpreter::runtime::limits::ExecutionUsage;
use interpreter::runtime::{
    BodyCopy, CallOutcome, CallRecord, DecisionAction, DecisionCause, DecisionLogOutput,
    DecisionRecord, EntryPath, ModelUsage, PayloadRecord, RuleCitation, SourceLine,
};
use serde_json::{Value, json};
use submilli_blueprint::{Blueprint, VarBindings};
use submilli_server::error::{ErrorKind, ExecuteError};
use submilli_server::record::{
    EVENT_SCHEMA, EventKind, FinishedRun, McpCatalog, RecordedRun, RunEntry, RunRecorder,
    RunRecorderFactory, RunStart, SessionEvent,
};

use super::changes::{NewVersion, WindowStart};
use super::events::{EventBody, LogPosition, StoredEvent};
use super::recorder::{FINISHED_RUN_MEMORY, PLAYGROUND_LOG_CONFIG};
use super::run::{DecisionRef, StoredRun};
use super::{FORMAT, KnownSecrets, Recorder, Store, StoreError};

mod server;

const SECRET: &str = "sk_test_51HxQ2eLkdIwHu7ix";

struct World {
    dir: tempfile::TempDir,
    store: Arc<Store>,
    recorder: Recorder,
    secrets: KnownSecrets,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(&dir.path().join("store")).unwrap());
        let secrets = KnownSecrets::default();
        let recorder = Recorder::new(Arc::clone(&store), secrets.clone());
        Self {
            dir,
            store,
            recorder,
            secrets,
        }
    }

    fn root(&self) -> &Path {
        self.store.root()
    }

    /// The store as a playground started again over the same directory would open it.
    fn reopen(&self) -> Store {
        Store::open(self.root()).unwrap()
    }

    fn start(&self, start: RunStart) -> Arc<dyn RunRecorder> {
        self.recorder
            .start(start)
            .expect("the store records every run")
    }

    /// Every byte the store wrote, file by file.
    fn files(&self) -> Vec<(std::path::PathBuf, Vec<u8>)> {
        let mut files = Vec::new();
        walk(self.root(), &mut |path| {
            files.push((path.to_path_buf(), std::fs::read(path).unwrap()));
        });
        files
    }
}

fn walk(dir: &Path, visit: &mut dyn FnMut(&Path)) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(&path, visit);
        } else {
            visit(&path);
        }
    }
}

fn run_start(execution_id: &str, session: Option<&str>) -> RunStart {
    RunStart {
        execution_id: execution_id.into(),
        label: "app".into(),
        entry: RunEntry::Session,
        test_of: None,
        client: Some("langchain-mcp-adapters".into()),
        tool_call_id: Some("toolu_1".into()),
        session_id: session.map(str::to_owned),
        idempotency_key: Some("idem-1".into()),
        blueprint_name: "demo".into(),
        blueprint: Arc::new(Blueprint::default()),
        blueprint_hash: Some("bphash".into()),
        blueprint_version: None,
        variables: Arc::new(VarBindings::from([(
            "customerId".to_owned(),
            "cus_northwind".to_owned(),
        )])),
        harness_secrets: Arc::default(),
        code: Some(Arc::from("function main(): string { return \"x\"; }")),
    }
}

fn decision(call_index: u64, capability: &str, context: Value, allowed: bool) -> DecisionRecord {
    DecisionRecord {
        call_index,
        seq: 1,
        at_micros: 100 * call_index + 10,
        caller: "main".into(),
        capability: capability.into(),
        context,
        context_truncated: false,
        context_digest: 42,
        allowed,
        action: if allowed {
            DecisionAction::Allow
        } else {
            DecisionAction::Deny
        },
        cause: DecisionCause::Rule(RuleCitation {
            caller: "main".into(),
            index: 0,
            name: Some("reads".into()),
        }),
        near_misses: Vec::new(),
        source: "policy".into(),
        rule: Some(0),
        reason: None,
        entry_path: EntryPath::GatedOp,
        line: Some(SourceLine {
            line: 3,
            column: Some(5),
        }),
        filtered: false,
        payload_dropped: false,
    }
}

fn payload(meta: Value, body: Option<&str>) -> Box<PayloadRecord> {
    Box::new(PayloadRecord {
        meta,
        body: body.map(|body| BodyCopy::Text(body.to_owned())),
        digest: "d".repeat(64),
        bytes: body.map_or(0, str::len) as u64,
        truncated: false,
        masked_headers: Vec::new(),
    })
}

fn call(call_index: u64, capability: &str, response: Option<&str>) -> CallRecord {
    CallRecord {
        call_index,
        caller: "main".into(),
        capability: capability.into(),
        started_micros: 100 * call_index,
        ended_micros: Some(100 * call_index + 50),
        outcome: Some(CallOutcome::Returned),
        line: Some(SourceLine {
            line: 3,
            column: None,
        }),
        request: Some(payload(
            json!({ "method": "GET", "url": format!("https://api.test/{call_index}") }),
            None,
        )),
        response: response.map(|body| payload(json!({ "status": 200 }), Some(body))),
        usage: Some(ModelUsage {
            input_tokens: Some(3),
            output_tokens: None,
        }),
    }
}

fn finished(decisions: Vec<DecisionRecord>, calls: Vec<CallRecord>) -> FinishedRun {
    FinishedRun {
        dispatched: true,
        error: None,
        result: Some("done".into()),
        console: "hello\n".into(),
        usage: ExecutionUsage {
            fuel: 10,
            wasm_fuel: 7,
            host_fuel: 3,
            memory_peak: 4096,
        },
        log: DecisionLogOutput {
            records: decisions,
            truncated: false,
            dropped: 0,
            line_frames: 9,
            calls,
            calls_dropped: 0,
        },
        mcp_catalog: Some(Arc::new(McpCatalog::empty())),
        wall: Duration::from_millis(12),
    }
}

fn event(seq: u64, session: &str, run: &str, kind: EventKind) -> SessionEvent {
    SessionEvent {
        schema: EVENT_SCHEMA,
        event_id: format!("srv-{seq}"),
        seq,
        // Arrived well after the run's start, as a real event would.
        at_micros: super::now_micros() + 10_000_000 + seq,
        session_id: Some(session.into()),
        run_id: Some(run.into()),
        tool_call_id: None,
        kind,
    }
}

fn run_started() -> EventKind {
    EventKind::RunStarted {
        label: "app".into(),
        entry: "session".into(),
        client: None,
        blueprint: "demo".into(),
        blueprint_hash: None,
        code_hash: None,
    }
}

fn call_started(call_index: u64) -> EventKind {
    EventKind::CallStarted {
        call_index,
        caller: "main".into(),
        capability: "http.get".into(),
        started_micros: 100 * call_index,
        line: None,
    }
}

fn call_finished(call: &CallRecord) -> EventKind {
    EventKind::CallFinished {
        call_index: call.call_index,
        capability: call.capability.clone(),
        started_micros: call.started_micros,
        ended_micros: call.ended_micros,
        outcome: call.outcome,
        sent_bytes: None,
        result_bytes: call.response.as_ref().map(|r| r.bytes),
        usage: None,
    }
}

fn run_finished(events_dropped: u64) -> EventKind {
    EventKind::RunFinished {
        dispatched: true,
        error: None,
        wall_ms: 12,
        console_bytes: 6,
        events_dropped,
    }
}

fn kinds(events: &[&StoredEvent]) -> Vec<String> {
    events
        .iter()
        .map(|event| match &event.body {
            EventBody::Gap(_) => "gap".to_owned(),
            EventBody::Event(event) => match &event.kind {
                EventKind::RunStarted { .. } => "started".to_owned(),
                EventKind::CallStarted { call_index, .. } => format!("call {call_index}"),
                EventKind::Decision { record } => format!("decision {}", record.call_index),
                EventKind::CallFinished { call_index, .. } => format!("finished {call_index}"),
                EventKind::RunFinished { .. } => "end".to_owned(),
                EventKind::Returned { .. } => "returned".to_owned(),
                EventKind::ToolCall { tool, .. } => format!("tool {tool}"),
            },
        })
        .collect()
}

// ---- runs ------------------------------------------------------------------------------

#[test]
fn a_recorded_run_round_trips_with_every_field() {
    let world = World::new();
    let start = run_start("exec-1", Some("sess-1"));
    let mut run = finished(
        vec![
            decision(0, "http.get", json!({ "host": "api.test" }), true),
            decision(1, "http.post", json!({ "host": "api.test" }), false),
        ],
        vec![
            call(0, "http.get", Some("{\"items\":[]}")),
            call(1, "http.post", None),
        ],
    );
    run.error = Some(ExecuteError {
        kind: ErrorKind::PermissionDenied,
        message: "denied".into(),
        diagnostics: Vec::new(),
        denial: Some(submilli_server::error::DenialDetails {
            caller: "main".into(),
            capability: "http.post".into(),
            source: "policy",
        }),
    });
    let expected = serde_json::to_value(StoredRun::from_parts(1, &start, &run, None, 0)).unwrap();
    world.start(start).finish(run);

    let loaded = world
        .reopen()
        .load_run(1)
        .unwrap()
        .expect("run 1 is stored");
    let mut actual = serde_json::to_value(&loaded).unwrap();
    // Only the start time is the store's own.
    actual["started_at_micros"] = json!(0);
    assert_eq!(actual, expected);
    assert_eq!(loaded.recording.decisions.len(), 2);
    assert_eq!(
        loaded.recording.calls[0].usage.unwrap().input_tokens,
        Some(3)
    );
    assert_eq!(
        loaded.error.as_ref().unwrap().capability.as_deref(),
        Some("http.post")
    );
    assert!(loaded.recording.mcp_catalog.is_some());
    assert_eq!(
        loaded.decision(2).map(|d| d.capability.as_str()),
        Some("http.post")
    );
    assert!(loaded.decision(0).is_none());

    let summary = &world.store.list_runs().unwrap()[0];
    assert_eq!(
        (summary.id, summary.label.as_str(), summary.denied),
        (1, "app", 1)
    );
    assert_eq!(summary.session_id.as_deref(), Some("sess-1"));
    assert_eq!(summary.variables["customerId"], "cus_northwind");
}

#[test]
fn decision_references_read_run_dot_position() {
    let reference: DecisionRef = "12.3".parse().unwrap();
    assert_eq!(reference, DecisionRef { run: 12, n: 3 });
    assert_eq!(reference.to_string(), "12.3");
    for bad in ["12", "0.1", "1.0", "a.b", "1.2.3", ""] {
        assert!(bad.parse::<DecisionRef>().is_err(), "{bad}");
    }
}

#[test]
fn a_reader_listing_runs_during_writes_never_sees_a_partial_file() {
    let world = World::new();
    let store = Arc::clone(&world.store);
    let recorder = world.recorder.clone();
    let writing = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let writer = {
        let writing = Arc::clone(&writing);
        std::thread::spawn(move || {
            for n in 0..40 {
                let mut run = finished(Vec::new(), Vec::new());
                // Large enough that a write takes many syscalls.
                run.console = "x".repeat(1024 * 1024 + n);
                recorder
                    .start(run_start(&format!("exec-{n}"), None))
                    .unwrap()
                    .finish(run);
            }
            writing.store(false, std::sync::atomic::Ordering::Release);
        })
    };
    let readers: Vec<_> = (0..3)
        .map(|_| {
            let store = Arc::clone(&store);
            let writing = Arc::clone(&writing);
            std::thread::spawn(move || {
                let mut read = 0;
                while writing.load(std::sync::atomic::Ordering::Acquire) {
                    for summary in store.list_runs().unwrap() {
                        let run = store
                            .load_run(summary.id)
                            .unwrap()
                            .expect("listed runs load");
                        assert_eq!(run.console.len(), 1024 * 1024 + summary.id as usize - 1);
                    }
                    // Reading the directory directly finds only whole run files too.
                    for entry in std::fs::read_dir(store.root().join("runs")).unwrap() {
                        let path = entry.unwrap().path();
                        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
                        if name.ends_with(".json") && !name.starts_with('.') {
                            let Ok(bytes) = std::fs::read(&path) else {
                                continue;
                            };
                            serde_json::from_slice::<Value>(&bytes).expect("a whole file");
                            read += 1;
                        }
                    }
                }
                read
            })
        })
        .collect();
    writer.join().unwrap();
    let read: usize = readers
        .into_iter()
        .map(|reader| reader.join().unwrap())
        .sum();
    assert!(read > 0, "the readers ran while runs were written");
    assert_eq!(world.store.list_runs().unwrap().len(), 40);
}

#[test]
fn a_known_secret_appears_nowhere_in_the_store_in_any_form() {
    let world = World::new();
    let mut start = run_start("exec-1", Some("sess-1"));
    start.harness_secrets = Arc::new([("API_KEY".to_owned(), SECRET.to_owned())].into());
    let base64 = base64::engine::general_purpose::STANDARD.encode(SECRET);
    let url: String =
        url::form_urlencoded::byte_serialize(format!("{SECRET}/x").as_bytes()).collect();
    let echoed = format!("token={SECRET} b64={base64} url={url}");
    let mut run = finished(
        vec![decision(
            0,
            "http.post",
            json!({ "body": echoed, "n": 1 }),
            true,
        )],
        vec![call(0, "http.post", Some(&echoed))],
    );
    // A binary response holding the secret is kept as base64 at an arbitrary alignment.
    let mut binary = vec![0xff, 0x00];
    binary.extend_from_slice(SECRET.as_bytes());
    run.log.calls[0].request.as_mut().unwrap().body = Some(BodyCopy::Base64(
        base64::engine::general_purpose::STANDARD.encode(&binary),
    ));
    run.console = format!("{echoed}\n");
    run.result = Some(echoed.clone());
    let recorder = world.start(start);
    world
        .recorder
        .event(event(1, "sess-1", "exec-1", run_started()));
    world.recorder.event(event(
        2,
        "sess-1",
        "exec-1",
        EventKind::Decision {
            record: Box::new(run.log.records[0].clone()),
        },
    ));
    recorder.finish(run);
    world
        .recorder
        .event(event(3, "sess-1", "exec-1", run_finished(0)));

    let forms = [
        SECRET.to_owned(),
        base64.clone(),
        base64.trim_end_matches('=').to_owned(),
        url::form_urlencoded::byte_serialize(SECRET.as_bytes()).collect(),
    ];
    let files = world.files();
    assert!(files.len() >= 4, "{files:?}");
    for (path, bytes) in &files {
        let text = String::from_utf8_lossy(bytes);
        for form in &forms {
            assert!(
                !text.contains(form.as_str()),
                "{} holds {form}",
                path.display()
            );
        }
        // Nor inside a base64 body copy.
        for copy in text.split("\"data\":\"").skip(1) {
            let data = copy.split('"').next().unwrap();
            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(data) {
                assert!(
                    !decoded
                        .windows(SECRET.len())
                        .any(|w| w == SECRET.as_bytes()),
                    "{} holds the secret in a body copy",
                    path.display()
                );
            }
        }
    }
    let stored = world.store.load_run(1).unwrap().unwrap();
    assert!(stored.console.contains(super::redact::REDACTED));
    assert_eq!(
        stored.recording.decisions.len(),
        1,
        "the record still loads"
    );
    assert!(!world.secrets.redact_text(SECRET).contains(SECRET));
}

#[test]
fn files_are_owner_only_and_directories_0700() {
    let world = World::new();
    let recorder = world.start(run_start("exec-1", Some("sess-1")));
    world
        .recorder
        .event(event(1, "sess-1", "exec-1", run_started()));
    recorder.finish(finished(Vec::new(), Vec::new()));
    world
        .store
        .append_version(NewVersion {
            hash: "h1".into(),
            bytes: "name: demo\n".into(),
            classification: json!("widening"),
            summary: "first".into(),
        })
        .unwrap();
    world.store.clear().unwrap();
    // A late event of the run starts its session's log again.
    world
        .recorder
        .event(event(2, "sess-1", "exec-1", call_started(0)));
    // A temporary file is 0600 before it is renamed into place.
    let staged = super::stage(&world.store.run_path(99), b"{}").unwrap();
    let mode = std::fs::metadata(staged.path())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    drop(staged);

    let mut checked = 0;
    walk(world.root(), &mut |path| {
        let mode = std::fs::metadata(path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "{}", path.display());
        checked += 1;
    });
    assert!(checked >= 5, "store, sequence, index, changes, events");
    for dir in [
        world.root().to_path_buf(),
        world.root().join("runs"),
        world.root().join("events"),
    ] {
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "{}", dir.display());
    }
}

#[test]
fn clear_removes_runs_resets_the_audit_window_and_ids_keep_counting() {
    let world = World::new();
    for n in 1..=2 {
        world
            .start(run_start(&format!("exec-{n}"), None))
            .finish(finished(Vec::new(), Vec::new()));
    }
    let version = world
        .store
        .append_version(NewVersion {
            hash: "h".into(),
            bytes: "name: demo\n".into(),
            classification: Value::Null,
            summary: String::new(),
        })
        .unwrap();
    assert_eq!(
        world.store.changes().unwrap().audit_window(),
        (2, WindowStart::Version(version))
    );
    // A run in progress while the store is cleared.
    let in_flight = world.start(run_start("exec-3", None));
    assert_eq!(world.store.clear().unwrap(), 2);
    assert!(world.store.list_runs().unwrap().is_empty());
    assert!(world.store.load_run(1).unwrap().is_none());
    assert_eq!(
        world.store.changes().unwrap().audit_window(),
        (3, WindowStart::Clear)
    );

    in_flight.finish(finished(Vec::new(), Vec::new()));
    let after = world.start(run_start("exec-4", None));
    after.finish(finished(Vec::new(), Vec::new()));
    let ids: Vec<u64> = world
        .store
        .list_runs()
        .unwrap()
        .iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(ids, [3, 4], "the run in progress is kept, and ids continue");

    // A restart continues the sequence too, even with the sequence file lost.
    std::fs::remove_file(world.root().join("sequence.json")).unwrap();
    for id in [3, 4] {
        std::fs::remove_file(world.store.run_path(id)).unwrap();
    }
    let reopened = world.reopen();
    assert_eq!(
        reopened.next_run_id().unwrap(),
        4,
        "past the clear's high-water mark"
    );
    let again = Store::open(world.root()).unwrap();
    assert_eq!(again.next_run_id().unwrap(), 5);
}

#[test]
fn clear_removes_the_event_logs_and_their_numbering_carries_on() {
    let world = World::new();
    world.start(run_start("exec-1", Some("sess")));
    world
        .store
        .append_session(super::sessions::SessionEntry::Ended {
            session_id: "sess".into(),
        })
        .unwrap();
    let send = |seq, kind| world.recorder.event(event(seq, "sess", "exec-1", kind));
    send(1, run_started());
    send(2, call_started(0));
    send(3, call_started(1));
    let (events, position) = world
        .store
        .read_events_from(Some("sess"), LogPosition::default())
        .unwrap();
    assert_eq!(events.len(), 3);

    world.store.clear().unwrap();
    assert!(!world.store.events_path(Some("sess")).exists());
    assert!(
        world
            .store
            .read_events(Some("sess"))
            .unwrap()
            .events
            .is_empty()
    );
    assert_eq!(world.store.session_log().unwrap().len(), 1, "sessions stay");

    // A reader that followed the old log reads the new one from its start.
    send(4, call_started(2));
    send(5, call_started(3));
    send(6, call_started(4));
    let (events, _) = world
        .store
        .read_events_from(Some("sess"), position)
        .unwrap();
    let seqs: Vec<u64> = events.iter().map(|event| event.session_seq).collect();
    assert_eq!(seqs, [4, 5, 6]);
    // So does a new process, which numbers on from what is left.
    let reopened = Arc::new(world.reopen());
    let recorder = Recorder::new(Arc::clone(&reopened), KnownSecrets::default());
    recorder.start(run_start("exec-2", Some("sess")));
    recorder.event(event(1, "sess", "exec-2", run_started()));
    let last = reopened
        .read_events(Some("sess"))
        .unwrap()
        .events
        .last()
        .cloned();
    assert_eq!(last.map(|event| event.session_seq), Some(7));
}

#[test]
fn a_run_dropped_without_finishing_is_no_longer_in_flight() {
    let world = World::new();
    let dropped = world.start(run_start("exec-1", None));
    assert_eq!(world.recorder.in_flight(1).as_deref(), Some("exec-1"));
    drop(dropped);
    assert_eq!(world.recorder.in_flight(1), None);
    let finished_run = world.start(run_start("exec-2", None));
    finished_run.finish(finished(Vec::new(), Vec::new()));
    assert_eq!(world.recorder.in_flight(2), None);
}

#[test]
fn a_run_finished_or_dropped_long_ago_is_no_longer_followed() {
    let world = World::new();
    let dropped = world.start(run_start("exec-1", None));
    let finished_run = world.start(run_start("exec-2", None));
    assert!(world.recorder.follows("exec-1"));
    drop(dropped);
    finished_run.finish(finished(Vec::new(), Vec::new()));
    // Their late events may still come for a while, with no other run started since.
    let later = Instant::now() + FINISHED_RUN_MEMORY;
    for run in ["exec-1", "exec-2"] {
        assert!(world.recorder.follows(run), "{run}");
        assert!(!world.recorder.follows_at(run, later), "{run}");
    }
    assert!(!world.recorder.follows("exec-never-started"));
}

#[test]
fn a_run_the_store_could_not_take_is_followed_until_its_end_arrives() {
    let world = World::new();
    // The run counter cannot be written, so the run goes unrecorded.
    let counter = world.root().join("sequence.json");
    let _ = std::fs::remove_file(&counter);
    std::fs::create_dir(&counter).unwrap();
    assert!(
        world
            .recorder
            .start(run_start("exec-1", Some("sess")))
            .is_none()
    );
    world
        .recorder
        .event(event(1, "sess", "exec-1", run_started()));
    let later = Instant::now() + FINISHED_RUN_MEMORY;
    assert!(world.recorder.follows("exec-1"));
    assert!(world.recorder.follows_at("exec-1", later), "still running");
    world
        .recorder
        .event(event(2, "sess", "exec-1", run_finished(0)));
    assert!(world.recorder.follows("exec-1"));
    let later = Instant::now() + FINISHED_RUN_MEMORY;
    assert!(!world.recorder.follows_at("exec-1", later));
}

#[test]
fn a_drop_while_runs_overlap_leaves_every_open_runs_later_decisions_unnumbered() {
    // The server's sequence says that an event was dropped, not whose: each run whose
    // end has not arrived may have lost it, so none of them numbers its decisions
    // from the count any more.
    let world = World::new();
    let first = world.start(run_start("exec-1", Some("sess")));
    let second = world.start(run_start("exec-2", Some("sess")));
    let send = |seq, run, kind| world.recorder.event(event(seq, "sess", run, kind));
    let decided = |n: u64| EventKind::Decision {
        record: Box::new(decision(n, "http.get", json!({ "n": n }), true)),
    };
    send(1, "exec-1", run_started());
    send(2, "exec-2", run_started());
    send(3, "exec-2", decided(0));
    // Event 4, one of the first run's, was dropped.
    send(5, "exec-2", decided(1));
    send(6, "exec-2", run_finished(0));
    second.finish(finished(Vec::new(), Vec::new()));
    send(7, "exec-1", run_finished(1));
    first.finish(finished(Vec::new(), Vec::new()));
    let log = world.store.read_events(Some("sess")).unwrap();
    let second_run: Vec<Option<u64>> = log
        .events
        .iter()
        .filter(|event| event.run == Some(2))
        .filter_map(|event| match &event.body {
            EventBody::Event(e) => {
                matches!(e.kind, EventKind::Decision { .. }).then_some(event.decision)
            }
            EventBody::Gap(_) => None,
        })
        .collect();
    assert_eq!(second_run, [Some(1), None]);
}

#[test]
fn rewriting_a_run_cleared_since_it_was_loaded_does_not_bring_it_back() {
    let world = World::new();
    world
        .start(run_start("exec-1", None))
        .finish(finished(Vec::new(), Vec::new()));
    let loaded = world.store.load_run(1).unwrap().expect("stored");
    world.store.clear().unwrap();
    world.store.rewrite_run(&loaded).unwrap();
    assert!(world.store.load_run(1).unwrap().is_none());
    assert!(world.store.list_runs().unwrap().is_empty());
    // A run still there is rewritten.
    world
        .start(run_start("exec-2", None))
        .finish(finished(Vec::new(), Vec::new()));
    let mut kept = world.store.load_run(2).unwrap().expect("stored");
    kept.label = "kept".into();
    world.store.rewrite_run(&kept).unwrap();
    assert_eq!(world.store.load_run(2).unwrap().unwrap().label, "kept");
}

#[test]
fn a_bytes_updated_entry_supersedes_its_versions_bytes_and_a_failed_apply_voids_one() {
    let world = World::new();
    let new = |bytes: &str| NewVersion {
        hash: format!("hash-{bytes}"),
        bytes: bytes.into(),
        classification: json!({ "kind": "widening" }),
        summary: "added a rule".into(),
    };
    let first = world.store.append_version(new("v1")).unwrap();
    world
        .start(run_start("exec-1", None))
        .finish(finished(Vec::new(), Vec::new()));
    let second = world.store.append_version(new("v2")).unwrap();
    world
        .store
        .append_bytes_updated(second, "v2 # a comment".into())
        .unwrap();
    let third = world.store.append_version(new("v3")).unwrap();
    world
        .store
        .append_apply_failed(third, "volume missing".into())
        .unwrap();
    assert_eq!((first, second, third), (1, 2, 3));

    let changes = world.store.changes().unwrap();
    let versions: Vec<(u64, &str)> = changes
        .versions
        .iter()
        .map(|v| (v.version, v.bytes.as_str()))
        .collect();
    assert_eq!(versions, [(1, "v1"), (2, "v2 # a comment")]);
    assert_eq!(changes.voided, [3]);
    assert_eq!(changes.current().unwrap().version, 2);
    // The bytes-updated entry and the voided version leave the window at version 2.
    assert_eq!(changes.audit_window(), (1, WindowStart::Version(2)));
    // A voided number is never reused.
    assert_eq!(world.store.append_version(new("v4")).unwrap(), 4);
}

fn assert_newer(error: StoreError) {
    assert!(
        matches!(error, StoreError::NewerFormat { found: 2, .. }),
        "{error}"
    );
    let message = error.to_string();
    assert!(message.contains("upgrade submilli"), "{message}");
}

#[test]
fn a_store_or_record_in_a_newer_format_is_refused_with_an_upgrade_message() {
    let world = World::new();
    world
        .start(run_start("exec-1", Some("s")))
        .finish(finished(Vec::new(), Vec::new()));
    world.recorder.event(event(1, "s", "exec-1", run_started()));

    // A run file.
    let path = world.store.run_path(1);
    let mut run: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    run["format"] = json!(FORMAT + 1);
    std::fs::write(&path, run.to_string()).unwrap();
    assert_newer(world.store.load_run(1).err().unwrap());

    // A change-log line.
    std::fs::write(
        world.store.changes_path(),
        format!(
            "{{\"format\":{},\"at_micros\":0,\"kind\":\"clear\",\"high_water\":1}}\n",
            FORMAT + 1
        ),
    )
    .unwrap();
    assert_newer(world.store.changes().err().unwrap());

    // An event-log line.
    std::fs::write(
        world.store.events_path(Some("s")),
        format!("{{\"format\":{}}}\n", FORMAT + 1),
    )
    .unwrap();
    assert_newer(world.store.read_events(Some("s")).err().unwrap());

    // The store itself.
    std::fs::write(
        world.root().join("store.json"),
        format!("{{\"format\":{}}}", FORMAT + 1),
    )
    .unwrap();
    assert_newer(Store::open(world.root()).err().unwrap());
}

#[test]
fn the_playground_raises_the_recording_limits() {
    let world = World::new();
    let recorder = world.start(run_start("exec-1", None));
    let config = recorder.log_config();
    let default = interpreter::runtime::DecisionLogConfig::default();
    assert_eq!(config.max_payload_bytes, 4 * 1024 * 1024);
    assert_eq!(config.max_recorder_bytes, 64 * 1024 * 1024);
    assert_eq!(config.max_decisions, 50_000);
    assert_eq!(config.max_calls, 50_000);
    assert_eq!(config.max_context_value_bytes, 4 * 1024);
    assert_eq!(config.max_line_capture_frames, 4_000_000);
    assert!(config.max_payload_bytes > default.max_payload_bytes);
    assert!(config.max_recorder_bytes > default.max_recorder_bytes);
    assert_eq!(
        PLAYGROUND_LOG_CONFIG.max_payload_bytes,
        config.max_payload_bytes
    );
}

#[test]
fn a_test_run_is_stored_like_any_run_with_its_link_to_the_source() {
    let world = World::new();
    world
        .start(run_start("source-exec", None))
        .finish(finished(Vec::new(), Vec::new()));
    // The source run was stored before a restart.
    let world = World {
        recorder: Recorder::new(Arc::new(world.reopen()), KnownSecrets::default()),
        ..world
    };
    let mut start = run_start("test-exec", None);
    start.entry = RunEntry::Test;
    start.label = crate::commands::playground::labels::TEST.into();
    start.test_of = Some("source-exec".into());
    world.start(start).finish(finished(Vec::new(), Vec::new()));
    let store = world.reopen();
    let test = store.load_run(2).unwrap().unwrap();
    assert_eq!(test.entry, "test");
    assert_eq!(test.label, "test");
    let link = test.test_of.expect("linked to its source");
    assert_eq!(
        (link.run, link.execution_id.as_str()),
        (Some(1), "source-exec")
    );
    assert_eq!(store.list_runs().unwrap()[1].test_of, Some(link));
}

#[test]
fn an_idempotent_retry_is_kept_as_a_link_not_a_run() {
    let world = World::new();
    world
        .start(run_start("exec-1", Some("s")))
        .finish(finished(Vec::new(), Vec::new()));
    world.recorder.retried(submilli_server::record::RetryLink {
        session_id: "s".into(),
        idempotency_key: "idem-1".into(),
        original_execution_id: Some("exec-1".into()),
    });
    assert_eq!(world.store.list_runs().unwrap().len(), 1);
    let retries = world.store.retries().unwrap();
    assert_eq!(retries[0].original.as_ref().unwrap().run, Some(1));
}

// ---- events ----------------------------------------------------------------------------

#[test]
fn events_of_two_concurrent_runs_in_a_session_are_numbered_without_gaps_and_tailed_once() {
    let world = World::new();
    let runs = ["exec-a", "exec-b"];
    for run in runs {
        world.start(run_start(run, Some("sess")));
    }
    let per_run = 200_u64;
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let tail = {
        let store = Arc::clone(&world.store);
        let stop = Arc::clone(&stop);
        std::thread::spawn(move || {
            let mut position = LogPosition::default();
            let mut seen = Vec::new();
            loop {
                let done = stop.load(std::sync::atomic::Ordering::Acquire);
                let (events, next) = store.read_events_from(Some("sess"), position).unwrap();
                position = next;
                seen.extend(events.into_iter().map(|event| event.event_id));
                if done {
                    return seen;
                }
            }
        })
    };
    let writers: Vec<_> = runs
        .iter()
        .enumerate()
        .map(|(n, run)| {
            let recorder = world.recorder.clone();
            let run = (*run).to_owned();
            std::thread::spawn(move || {
                for index in 0..per_run {
                    let seq = 1 + index * 2 + n as u64;
                    recorder.event(event(seq, "sess", &run, call_started(index)));
                }
            })
        })
        .collect();
    for writer in writers {
        writer.join().unwrap();
    }
    stop.store(true, std::sync::atomic::Ordering::Release);
    let seen = tail.join().unwrap();

    let log = world.store.read_events(Some("sess")).unwrap();
    let seqs: Vec<u64> = log.events.iter().map(|event| event.session_seq).collect();
    assert_eq!(seqs, (1..=2 * per_run).collect::<Vec<_>>());
    assert_eq!(seen.len(), 2 * per_run as usize, "each event once");
    let unique: std::collections::HashSet<_> = seen.iter().collect();
    assert_eq!(unique.len(), seen.len());
    assert!(!log.incomplete());
    // Each run's events keep their order within the session.
    for run in [1, 2] {
        let calls: Vec<u64> = log
            .events
            .iter()
            .filter(|event| event.run == Some(run))
            .filter_map(|event| event.position.call_index)
            .collect();
        assert_eq!(calls, (0..per_run).collect::<Vec<_>>());
    }
}

#[test]
fn numbering_continues_after_a_restart() {
    let world = World::new();
    world.start(run_start("exec-1", Some("sess")));
    world
        .recorder
        .event(event(1, "sess", "exec-1", run_started()));
    world
        .recorder
        .event(event(2, "sess", "exec-1", call_started(0)));
    let reopened = Arc::new(world.reopen());
    let recorder = Recorder::new(Arc::clone(&reopened), KnownSecrets::default());
    recorder.start(run_start("exec-2", Some("sess")));
    recorder.event(event(1, "sess", "exec-2", run_started()));
    let seqs: Vec<u64> = reopened
        .read_events(Some("sess"))
        .unwrap()
        .events
        .iter()
        .map(|event| event.session_seq)
        .collect();
    assert_eq!(seqs, [1, 2, 3]);
}

#[test]
fn a_partly_written_last_line_is_left_for_the_next_read() {
    let world = World::new();
    world.start(run_start("exec-1", Some("sess")));
    world
        .recorder
        .event(event(1, "sess", "exec-1", run_started()));
    let path = world.store.events_path(Some("sess"));
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    std::io::Write::write_all(&mut file, b"{\"format\":1,\"session_seq\":2,\"ev").unwrap();
    let log = world.store.read_events(Some("sess")).unwrap();
    assert_eq!(log.events.len(), 1);

    // A follower reads up to the partial line and picks up from there.
    let (events, position) = world
        .store
        .read_events_from(Some("sess"), LogPosition::default())
        .unwrap();
    assert_eq!(events.len(), 1);
    let whole = std::fs::metadata(&path).unwrap().len();
    assert!(position.offset < whole);
    std::io::Write::write_all(&mut file, b"\n").unwrap();
    let (events, next) = world
        .store
        .read_events_from(Some("sess"), position)
        .unwrap();
    assert!(events.is_empty(), "a line that does not parse is skipped");
    assert_eq!(next.offset, whole + 1);
}

#[test]
fn an_event_log_with_dropped_events_says_so() {
    let world = World::new();
    let recorder = world.start(run_start("exec-1", Some("sess")));
    world
        .recorder
        .event(event(1, "sess", "exec-1", run_started()));
    world
        .recorder
        .event(event(5, "sess", "exec-1", run_finished(3)));
    recorder.finish(finished(Vec::new(), Vec::new()));
    let log = world.store.read_events(Some("sess")).unwrap();
    assert!(log.incomplete());
    let gap = log.gaps().next().unwrap();
    assert_eq!(gap.dropped, Some(3));
    assert!(!gap.lost);
    assert!(!log.lost());
}

/// A run with three calls whose second call's events the server dropped: its call
/// start, decision, and end.
fn run_with_a_gap(world: &World) -> (Arc<dyn RunRecorder>, FinishedRun) {
    let recorder = world.start(run_start("exec-1", Some("sess")));
    let calls: Vec<CallRecord> = (0..3).map(|n| call(n, "http.get", Some("ok"))).collect();
    let decisions: Vec<DecisionRecord> = (0..3)
        .map(|n| decision(n, "http.get", json!({ "n": n }), n != 1))
        .collect();
    let mut seq = 0;
    let mut send = |kind| {
        seq += 1;
        world.recorder.event(event(seq, "sess", "exec-1", kind));
    };
    send(run_started());
    for n in [0, 2] {
        send(call_started(n));
        send(EventKind::Decision {
            record: Box::new(decisions[n as usize].clone()),
        });
        send(call_finished(&calls[n as usize]));
    }
    (recorder, finished(decisions, calls))
}

fn assert_backfilled_in_place(world: &World) {
    let log = world.store.read_events(Some("sess")).unwrap();
    assert!(log.incomplete());
    let causal = log.causal();
    let order: Vec<String> = kinds(&causal).into_iter().filter(|k| k != "gap").collect();
    assert_eq!(
        order,
        [
            "started",
            "call 0",
            "decision 0",
            "finished 0",
            "decision 1",
            "finished 1",
            "call 2",
            "decision 2",
            "finished 2",
            "end",
        ]
    );
    let recovered: Vec<_> = causal
        .iter()
        .filter(|event| event.backfilled)
        .map(|event| event.position.call_index)
        .collect();
    assert_eq!(recovered.len(), 2);
    assert!(recovered.iter().all(|index| *index == Some(1)));
    // Every decision of the record is in the log, once.
    let decisions = causal
        .iter()
        .filter(|event| {
            matches!(&event.body, EventBody::Event(e) if matches!(e.kind, EventKind::Decision { .. }))
        })
        .count();
    assert_eq!(decisions, 3);
}

#[test]
fn decisions_are_numbered_as_the_run_keeps_them_and_left_unnumbered_after_a_drop() {
    let world = World::new();
    let recorder = world.start(run_start("exec-1", Some("sess")));
    let decisions: Vec<DecisionRecord> = (0..4)
        .map(|n| decision(n, "http.get", json!({ "n": n }), true))
        .collect();
    let send = |seq, kind| world.recorder.event(event(seq, "sess", "exec-1", kind));
    let decided = |n: usize| EventKind::Decision {
        record: Box::new(decisions[n].clone()),
    };
    send(1, run_started());
    send(2, decided(0));
    send(3, decided(1));
    // The server dropped event 4, the third decision: what follows has no known place.
    send(5, decided(3));
    send(6, run_finished(1));
    recorder.finish(finished(decisions.clone(), Vec::new()));
    let log = world.store.read_events(Some("sess")).unwrap();
    let numbered: Vec<(u64, Option<u64>, bool)> = log
        .events
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::Event(e) => match &e.kind {
                EventKind::Decision { record } => {
                    Some((record.call_index, event.decision, event.backfilled))
                }
                _ => None,
            },
            EventBody::Gap(_) => None,
        })
        .collect();
    assert_eq!(
        numbered,
        [
            (0, Some(1), false),
            (1, Some(2), false),
            (3, None, false),
            // Recovered from the record, where its place is known.
            (2, Some(3), true),
        ]
    );
}

#[test]
fn after_an_overflow_missing_decisions_and_calls_are_backfilled_where_they_happened() {
    let world = World::new();
    let (recorder, run) = run_with_a_gap(&world);
    // The end event reports the drops before the record arrives.
    world
        .recorder
        .event(event(20, "sess", "exec-1", run_finished(3)));
    recorder.finish(run);
    assert_backfilled_in_place(&world);
}

#[test]
fn a_backfill_also_waits_for_a_record_that_finishes_after_the_end_event() {
    let world = World::new();
    let (recorder, run) = run_with_a_gap(&world);
    recorder.finish(run);
    // Finished outside a runtime, with no end event yet: the end is presumed lost and
    // backfilled at once; the real one arriving late is not written twice.
    world
        .recorder
        .event(event(20, "sess", "exec-1", run_finished(3)));
    let log = world.store.read_events(Some("sess")).unwrap();
    let ends = kinds(&log.causal()).iter().filter(|k| *k == "end").count();
    assert_eq!(ends, 1);
    assert!(log.gaps().next().unwrap().dropped.is_none());
    assert!(
        log.lost(),
        "with the end lost, the log cannot say how much was dropped"
    );
}

#[test]
fn a_record_cut_at_its_cap_says_the_rest_is_lost() {
    let world = World::new();
    let (recorder, mut run) = run_with_a_gap(&world);
    run.log.truncated = true;
    world
        .recorder
        .event(event(20, "sess", "exec-1", run_finished(3)));
    recorder.finish(run);
    let log = world.store.read_events(Some("sess")).unwrap();
    assert!(log.lost());
}

#[test]
fn every_event_is_redacted_and_tool_calls_outside_runs_are_kept() {
    let world = World::new();
    world.secrets.add(SECRET);
    world.recorder.event(SessionEvent {
        schema: EVENT_SCHEMA,
        event_id: "srv-1".into(),
        seq: 1,
        at_micros: 1,
        session_id: Some("sess".into()),
        run_id: None,
        tool_call_id: Some(SECRET.into()),
        kind: EventKind::ToolCall {
            tool: "docs".into(),
            ok: true,
            result_bytes: 10,
            wall_ms: 1,
        },
    });
    let bytes = std::fs::read(world.store.events_path(Some("sess"))).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains(SECRET));
    let log = world.store.read_events(Some("sess")).unwrap();
    assert_eq!(kinds(&log.causal()), ["tool docs"]);
}

#[test]
fn session_ids_that_are_not_plain_never_name_a_path() {
    use super::events::session_file_name;
    assert_eq!(session_file_name(Some("3f2a-11")), "3f2a-11.jsonl");
    assert_eq!(session_file_name(None), "_sessionless.jsonl");
    for odd in ["../escape", "a/b", "", "x.y"] {
        let name = session_file_name(Some(odd));
        assert!(name.starts_with("_h-") && !name.contains('/'), "{name}");
    }
}

#[test]
fn recorded_runs_also_load_back_through_the_server_type() {
    // The stored recording is the server's own `RecordedRun`, serialized as the server
    // serializes it.
    let world = World::new();
    let start = run_start("exec-1", None);
    let run = finished(
        vec![decision(0, "http.get", json!({}), true)],
        vec![call(0, "http.get", Some("b"))],
    );
    let expected = serde_json::to_value(RecordedRun::from_parts(&start, &run)).unwrap();
    world.start(start).finish(run);
    let bytes = std::fs::read(world.store.run_path(1)).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["recording"], expected);
    let recorded: RecordedRun = serde_json::from_value(value["recording"].clone()).unwrap();
    assert_eq!(serde_json::to_value(&recorded).unwrap(), expected);
    let _ = &world.dir;
}

// ---- torn tails ------------------------------------------------------------------------

/// Appends the start of a line that a crash cut short.
fn tear(path: &Path) {
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    std::io::Write::write_all(&mut file, b"{\"format\":1,\"id\":9").unwrap();
}

fn new_version(hash: &str) -> NewVersion {
    NewVersion {
        hash: hash.into(),
        bytes: "name: demo\n".into(),
        classification: Value::Null,
        summary: String::new(),
    }
}

#[test]
fn a_torn_index_line_neither_breaks_the_next_append_nor_the_next_open() {
    let world = World::new();
    world
        .start(run_start("exec-1", None))
        .finish(finished(Vec::new(), Vec::new()));
    tear(&world.root().join("index.jsonl"));
    world
        .start(run_start("exec-2", None))
        .finish(finished(Vec::new(), Vec::new()));
    let ids = |store: &Store| -> Vec<u64> {
        store
            .list_runs()
            .unwrap()
            .iter()
            .map(|run| run.id)
            .collect()
    };
    assert_eq!(ids(&world.store), [1, 2]);
    let reopened = world.reopen();
    assert_eq!(ids(&reopened), [1, 2]);
    // Opening rewrote the index whole.
    let index = std::fs::read_to_string(world.root().join("index.jsonl")).unwrap();
    assert_eq!(index.lines().count(), 2, "{index}");
}

#[test]
fn an_unreadable_index_is_rebuilt_from_the_runs() {
    let world = World::new();
    for n in 1..=2 {
        world
            .start(run_start(&format!("exec-{n}"), None))
            .finish(finished(Vec::new(), Vec::new()));
    }
    std::fs::write(world.root().join("index.jsonl"), b"not json\n{]\n").unwrap();
    let reopened = world.reopen();
    let ids: Vec<u64> = reopened.list_runs().unwrap().iter().map(|r| r.id).collect();
    assert_eq!(ids, [1, 2]);
}

#[test]
fn a_torn_change_log_line_does_not_block_the_next_version() {
    let world = World::new();
    world.store.append_version(new_version("h1")).unwrap();
    tear(&world.store.changes_path());
    assert_eq!(world.store.append_version(new_version("h2")).unwrap(), 2);
    let changes = world.store.changes().unwrap();
    assert_eq!(changes.versions.len(), 2);
    let reopened = world.reopen();
    assert_eq!(reopened.changes().unwrap().versions.len(), 2);
    assert_eq!(reopened.append_version(new_version("h3")).unwrap(), 3);
}

#[test]
fn a_torn_event_line_does_not_stop_the_session_numbering() {
    let world = World::new();
    world.start(run_start("exec-1", Some("sess")));
    world
        .recorder
        .event(event(1, "sess", "exec-1", run_started()));
    tear(&world.store.events_path(Some("sess")));
    world
        .recorder
        .event(event(2, "sess", "exec-1", call_started(0)));
    let seqs = |store: &Store| -> Vec<u64> {
        store
            .read_events(Some("sess"))
            .unwrap()
            .events
            .iter()
            .map(|event| event.session_seq)
            .collect()
    };
    assert_eq!(seqs(&world.store), [1, 2]);
    // A restart reads the numbering back past the bad line.
    let reopened = Arc::new(world.reopen());
    let recorder = Recorder::new(Arc::clone(&reopened), KnownSecrets::default());
    recorder.start(run_start("exec-2", Some("sess")));
    recorder.event(event(3, "sess", "exec-2", run_started()));
    assert_eq!(seqs(&reopened), [1, 2, 3]);
}

#[test]
fn a_torn_retry_line_is_skipped() {
    let world = World::new();
    let retry = |key: &str| super::run::RetryRecord {
        format: FORMAT,
        at_micros: 1,
        session_id: "s".into(),
        idempotency_key: key.into(),
        original: None,
    };
    world.store.record_retry(&retry("a")).unwrap();
    tear(&world.root().join("retries.jsonl"));
    world.store.record_retry(&retry("b")).unwrap();
    assert_eq!(world.store.retries().unwrap().len(), 2);
    assert_eq!(world.reopen().retries().unwrap().len(), 2);
}

// ---- redaction of records ---------------------------------------------------------------

#[test]
fn a_secret_equal_to_a_field_name_or_tag_word_leaves_runs_and_events_readable() {
    for secret in ["started", "capability", "call-started"] {
        let world = World::new();
        world.secrets.add(secret);
        let start = run_start("exec-1", Some("sess"));
        let run = finished(
            vec![decision(
                0,
                "http.get",
                json!({ "q": format!("x {secret} y") }),
                true,
            )],
            vec![call(0, "http.get", Some(&format!("body {secret}")))],
        );
        let recorder = world.start(start);
        world
            .recorder
            .event(event(1, "sess", "exec-1", run_started()));
        world
            .recorder
            .event(event(2, "sess", "exec-1", call_started(0)));
        world
            .recorder
            .event(event(3, "sess", "exec-1", run_finished(0)));
        recorder.finish(run);
        let stored = world.store.load_run(1).unwrap().expect("the run is stored");
        assert_eq!(
            stored.recording.decisions[0].context["q"],
            json!(format!("x {} y", super::redact::REDACTED)),
            "{secret}"
        );
        let log = world.store.read_events(Some("sess")).unwrap();
        assert_eq!(
            kinds(&log.causal()),
            ["started", "call 0", "end"],
            "{secret}"
        );
    }
}

// ---- event log names ---------------------------------------------------------------------

#[test]
fn session_ids_differing_only_in_case_never_share_a_log() {
    use super::events::session_file_name;
    let pairs = [("abc-123", "ABC-123"), ("aBc", "AbC"), ("Mixed", "mixed")];
    for (a, b) in pairs {
        assert_ne!(
            session_file_name(Some(a)).to_ascii_lowercase(),
            session_file_name(Some(b)).to_ascii_lowercase(),
            "{a} and {b}"
        );
    }
}

// ---- readers in other processes ----------------------------------------------------------

#[test]
fn a_read_only_open_changes_nothing_and_still_reads_around_a_crash() {
    let world = World::new();
    for n in 1..=2 {
        world
            .start(run_start(&format!("exec-{n}"), None))
            .finish(finished(Vec::new(), Vec::new()));
    }
    // A crash: run 2's index line lost, temporary files left, a change line cut short.
    let index = world.root().join("index.jsonl");
    let first_line = std::fs::read_to_string(&index)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_owned();
    std::fs::write(&index, format!("{first_line}\n")).unwrap();
    let staged = [
        world.root().join("runs").join(".staged-run"),
        world.root().join(".staged-sequence"),
    ];
    for path in &staged {
        std::fs::write(path, b"partial").unwrap();
    }
    world.store.append_version(new_version("h1")).unwrap();
    tear(&world.store.changes_path());
    let before = world.files();

    let reader = Store::open_read_only(world.root()).unwrap();
    let ids: Vec<u64> = reader.list_runs().unwrap().iter().map(|r| r.id).collect();
    assert_eq!(
        ids,
        [1, 2],
        "a run missing from the index is read from its file"
    );
    assert_eq!(reader.changes().unwrap().versions.len(), 1);
    assert_eq!(reader.last_run_id(), 2);
    assert!(matches!(
        reader.next_run_id(),
        Err(StoreError::ReadOnly { .. })
    ));
    assert!(matches!(
        reader.append_version(new_version("h2")),
        Err(StoreError::ReadOnly { .. })
    ));
    assert!(matches!(reader.clear(), Err(StoreError::ReadOnly { .. })));
    assert_eq!(world.files(), before, "the read-only open changed a file");

    // The writer's open repairs all of it.
    let writer = world.reopen();
    for path in &staged {
        assert!(!path.exists(), "{}", path.display());
    }
    assert_eq!(std::fs::read_to_string(&index).unwrap().lines().count(), 2);
    assert!(
        std::fs::read(writer.changes_path())
            .unwrap()
            .ends_with(b"\n")
    );
    // A missing store is not created by a reader.
    let missing = world.dir.path().join("nowhere");
    assert!(Store::open_read_only(&missing).is_err());
    assert!(!missing.exists());
}

// ---- audit window -----------------------------------------------------------------------

#[test]
fn the_audit_window_holds_the_runs_decided_under_the_current_version() {
    let world = World::new();
    let run_under = |execution_id: &str, version: Option<&str>| {
        let mut start = run_start(execution_id, None);
        start.blueprint_version = version.map(str::to_owned);
        world.start(start).finish(finished(Vec::new(), Vec::new()));
    };
    let in_window = || -> Vec<u64> {
        world
            .store
            .audit_window_runs()
            .unwrap()
            .iter()
            .map(|run| run.id)
            .collect()
    };
    run_under("exec-1", None);
    assert_eq!(in_window(), [1], "before any version, every run");
    assert_eq!(world.store.append_version(new_version("h1")).unwrap(), 1);
    run_under("exec-2", Some("1"));
    // Version 2 is logged; a run starts before it is applied, under version 1.
    assert_eq!(world.store.append_version(new_version("h2")).unwrap(), 2);
    run_under("exec-3", Some("1"));
    run_under("exec-4", Some("2"));
    assert_eq!(in_window(), [4]);
    // A failed apply voids version 2: the window is version 1's again.
    world
        .store
        .append_apply_failed(2, "refused".into())
        .unwrap();
    assert_eq!(in_window(), [2, 3]);
}

// ---- redaction pinned to the record formats ----------------------------------------------

/// Words the store's record formats own: enum tags and the like, each at least
/// [`super::redact::MIN_SECRET_BYTES`] long.
const FORMAT_WORDS: [&str; 12] = [
    "run-started",
    "call-started",
    "decision",
    "call-finished",
    "run-finished",
    "returned",
    "tool-call",
    "gated-op",
    "runtime-invariant",
    "variable-not-bound",
    "base64",
    "permission_denied",
];

/// User data holding the secret and, in angle brackets, every format word.
fn tainted(field: &str) -> String {
    let words: Vec<String> = FORMAT_WORDS
        .iter()
        .map(|word| format!("<{word}>"))
        .collect();
    format!("{field} {SECRET} {}", words.join(" "))
}

#[test]
fn no_user_data_shares_a_place_with_a_value_the_format_owns() {
    // Every format word is also a secret, so redacting it breaks each record's tags and
    // the fallback leaves those tags alone. User data must never sit where a tag does:
    // every occurrence in user data, bracketed, has to be gone everywhere.
    let world = World::new();
    world.secrets.add(SECRET);
    for word in FORMAT_WORDS {
        world.secrets.add(word);
    }
    let t = tainted;
    let session = t("session");
    let execution = t("exec");
    let start = RunStart {
        execution_id: execution.clone(),
        label: t("label"),
        entry: RunEntry::McpFileTool { tool: t("tool") },
        test_of: Some(t("tested")),
        client: Some(t("client")),
        tool_call_id: Some(t("tool-call-id")),
        session_id: Some(session.clone()),
        idempotency_key: Some(t("idem")),
        blueprint_name: t("blueprint"),
        blueprint: Arc::new(Blueprint::default()),
        blueprint_hash: Some(t("hash")),
        blueprint_version: Some(t("version")),
        variables: Arc::new(VarBindings::from([("customerId".to_owned(), t("var"))])),
        harness_secrets: Arc::default(),
        code: Some(Arc::from(t("code").as_str())),
    };
    let context = json!({
        "kind": t("context kind"),
        "list": [t("item"), { "kind": t("nested") }],
    });
    let mut first = decision(0, &t("capability"), context.clone(), false);
    first.caller = t("caller");
    first.source = t("source");
    first.reason = Some(t("reason"));
    first.cause = DecisionCause::RuntimeInvariant {
        reason: t("invariant"),
    };
    first.near_misses = vec![interpreter::runtime::NearMissRecord {
        rule: RuleCitation {
            caller: t("rule caller"),
            index: 0,
            name: Some(t("rule name")),
        },
        filter: t("filter"),
        failures: vec![interpreter::runtime::FailureRecord {
            comparison: t("comparison"),
            actual: Some(context.clone()),
            expected: Some(t("expected")),
            reason: interpreter::runtime::FailureReasonRecord::VariableNotBound(t("variable")),
            negated: false,
        }],
    }];
    let mut second = decision(1, "http.get", json!(t("plain")), true);
    second.cause = DecisionCause::Rule(RuleCitation {
        caller: t("cited caller"),
        index: 1,
        name: Some(t("cited name")),
    });
    let mut first_call = call(0, &t("capability"), Some(&t("response")));
    first_call.caller = t("caller");
    let request = first_call.request.as_mut().unwrap();
    request.meta = context.clone();
    request.body = Some(BodyCopy::Base64(
        base64::engine::general_purpose::STANDARD.encode(t("binary")),
    ));
    request.masked_headers = vec![t("header")];
    let mut run = finished(vec![first.clone(), second], vec![first_call.clone()]);
    run.error = Some(ExecuteError {
        kind: ErrorKind::PermissionDenied,
        message: t("message"),
        diagnostics: Vec::new(),
        denial: Some(submilli_server::error::DenialDetails {
            caller: t("denied caller"),
            capability: t("denied capability"),
            source: "policy",
        }),
    });
    run.result = Some(t("result"));
    run.console = t("console");

    let event = |seq: u64, kind: EventKind| SessionEvent {
        schema: EVENT_SCHEMA,
        event_id: t(&format!("event {seq}")),
        seq,
        at_micros: super::now_micros() + seq,
        session_id: Some(session.clone()),
        run_id: Some(execution.clone()),
        tool_call_id: Some(t("tool-call-id")),
        kind,
    };
    let recorder = world.start(start);
    let kinds = [
        EventKind::RunStarted {
            label: t("label"),
            entry: t("entry"),
            client: Some(t("client")),
            blueprint: t("blueprint"),
            blueprint_hash: Some(t("hash")),
            code_hash: Some(t("code hash")),
        },
        EventKind::CallStarted {
            call_index: 0,
            caller: t("caller"),
            capability: t("capability"),
            started_micros: 0,
            line: None,
        },
        EventKind::Decision {
            record: Box::new(first),
        },
        call_finished(&first_call),
        EventKind::Returned { bytes: 4 },
        EventKind::ToolCall {
            tool: t("tool"),
            ok: true,
            result_bytes: 1,
            wall_ms: 1,
        },
    ];
    let appended = kinds.len();
    for (seq, kind) in (1..).zip(kinds) {
        world.recorder.event(event(seq, kind));
    }
    recorder.finish(run);
    world.recorder.event(event(99, run_finished(0)));

    let stored = world.store.load_run(1).unwrap().expect("the run is stored");
    assert_eq!(stored.recording.decisions.len(), 2);
    let log = world.store.read_events(Some(&session)).unwrap();
    // Every event is written (and the run's record may add some it backfills).
    assert!(log.events.len() > appended, "{}", log.events.len());
    assert_eq!(log.skipped, 0);
    let files = world.files();
    for (path, bytes) in &files {
        let mut text = String::from_utf8_lossy(bytes).into_owned();
        // A base64 body copy, decoded.
        for copy in text.clone().split("\"data\":\"").skip(1) {
            let data = copy.split('"').next().unwrap();
            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(data) {
                text.push_str(&String::from_utf8_lossy(&decoded));
            }
        }
        assert!(
            !text.contains(SECRET),
            "{} holds the secret",
            path.display()
        );
        for word in FORMAT_WORDS {
            assert!(
                !text.contains(&format!("<{word}>")),
                "{} keeps {word} in user data",
                path.display()
            );
        }
    }
}

#[test]
fn a_record_whose_redaction_needs_more_probes_than_the_cap_is_not_written() {
    let world = World::new();
    // Tags of a decision event and of a decision in a run, so redacting either record
    // breaks it and its changes have to be probed one shape at a time.
    world.secrets.add("decision");
    world.secrets.add("gated-op");
    let context: serde_json::Map<String, Value> = (0..70)
        .map(|n| (format!("k{n}"), json!(format!("a decision {n} gated-op"))))
        .collect();
    let record = decision(0, "http.get", Value::Object(context), true);
    let recorder = world.start(run_start("exec-1", Some("sess")));
    world.recorder.event(event(
        1,
        "sess",
        "exec-1",
        EventKind::Decision {
            record: Box::new(record.clone()),
        },
    ));
    recorder.finish(finished(vec![record], Vec::new()));
    assert!(world.store.load_run(1).unwrap().is_none(), "run not stored");
    assert!(
        world
            .store
            .read_events(Some("sess"))
            .unwrap()
            .events
            .is_empty()
    );
    // The number the dropped event would have had is not used up.
    world
        .recorder
        .event(event(2, "sess", "exec-1", run_finished(0)));
    let log = world.store.read_events(Some("sess")).unwrap();
    let seqs: Vec<u64> = log.events.iter().map(|event| event.session_seq).collect();
    assert_eq!(seqs, [1]);
}

#[test]
fn a_write_cut_before_its_newline_does_not_reuse_its_number() {
    let world = World::new();
    world.start(run_start("exec-1", Some("sess")));
    world
        .recorder
        .event(event(1, "sess", "exec-1", run_started()));
    // A write that reached the file whole but for its newline, numbered 2.
    let path = world.store.events_path(Some("sess"));
    let written = std::fs::read_to_string(&path).unwrap();
    let cut = written
        .trim_end()
        .replace("\"session_seq\":1", "\"session_seq\":2")
        .replace("srv-1", "srv-cut");
    assert_ne!(cut, written.trim_end());
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    std::io::Write::write_all(&mut file, cut.as_bytes()).unwrap();
    drop(file);
    world
        .recorder
        .event(event(2, "sess", "exec-1", call_started(0)));
    let seqs = |store: &Store| -> Vec<u64> {
        store
            .read_events(Some("sess"))
            .unwrap()
            .events
            .iter()
            .map(|event| event.session_seq)
            .collect()
    };
    assert_eq!(seqs(&world.store), [1, 2, 3]);
    let reopened = Arc::new(world.reopen());
    let recorder = Recorder::new(Arc::clone(&reopened), KnownSecrets::default());
    recorder.start(run_start("exec-2", Some("sess")));
    recorder.event(event(3, "sess", "exec-2", run_started()));
    assert_eq!(seqs(&reopened), [1, 2, 3, 4]);
}
