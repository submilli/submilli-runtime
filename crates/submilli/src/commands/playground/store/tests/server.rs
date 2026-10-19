//! Real programs through the host, whose run recorder is the store.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use interpreter::runtime::{BodyCopy, DecisionLogConfig, NetworkPolicy};
use serde_json::Value;
use submilli_blueprint::HarnessSecretBindings;
use submilli_build::PackageStore;

use super::super::events::EventBody;
use super::super::{KnownSecrets, Recorder, Store};
use super::SECRET;
use crate::commands::playground::host::app::{App, ProgramRun};
use crate::commands::playground::host::record::RecordedRun;
use crate::commands::playground::host::record::events::EventKind;
use crate::commands::playground::host::record::test_run::{TestMode, TestReport, TestRun};
use crate::commands::playground::host::run::{
    FinishedRun, RecordedRun as RunRecording, RunEvent, RunHostConfig, RunRecorder, RunStart,
};
use crate::commands::playground::labels;

/// A loopback origin that answers each request with what `reply` makes of its head.
fn origin(reply: impl Fn(&str) -> (String, Vec<u8>) + Send + 'static) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut head = Vec::new();
            let mut byte = [0_u8; 1];
            while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                head.push(byte[0]);
            }
            let (headers, body) = reply(&String::from_utf8_lossy(&head));
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
    port
}

/// Keeps each run's in-memory recording beside the store's, as a test needs to compare
/// the two.
struct Tee {
    store: Recorder,
    recordings: Arc<Mutex<HashMap<String, RecordedRun>>>,
}

struct TeeRun {
    inner: Arc<dyn RunRecording>,
    start: RunStart,
    recordings: Arc<Mutex<HashMap<String, RecordedRun>>>,
}

impl RunRecorder for Tee {
    fn start(&self, run: RunStart) -> Option<Arc<dyn RunRecording>> {
        let inner = self.store.start(run.clone())?;
        Some(Arc::new(TeeRun {
            inner,
            start: run,
            recordings: Arc::clone(&self.recordings),
        }))
    }
}

impl RunRecording for TeeRun {
    fn log_config(&self) -> DecisionLogConfig {
        self.inner.log_config()
    }
    fn event(&self, event: RunEvent<'_>) {
        self.inner.event(event);
    }
    fn finish(&self, run: FinishedRun) {
        self.recordings.lock().unwrap().insert(
            self.start.execution_id.clone(),
            RecordedRun::from_parts(&self.start, &run),
        );
        self.inner.finish(run);
    }
    fn returned(&self, bytes: u64) {
        self.inner.returned(bytes);
    }
}

/// A playground's host over the store in `dir`: what a start, or a restart, builds.
struct Playground {
    app: App,
    store: Arc<Store>,
    recordings: Arc<Mutex<HashMap<String, RecordedRun>>>,
}

impl Playground {
    async fn start(dir: &std::path::Path, yaml: &str) -> Self {
        let store = Arc::new(Store::open(&dir.join("store")).unwrap());
        let recordings = Arc::new(Mutex::new(HashMap::new()));
        let tee = Tee {
            store: Recorder::new(Arc::clone(&store), KnownSecrets::default()),
            recordings: Arc::clone(&recordings),
        };
        let mut config = RunHostConfig::new(PackageStore::new(dir.join("packages")));
        config.network_policy = NetworkPolicy::deny_private().allow_localhost(true);
        config.recorder = Some(Arc::new(tee));
        let app = App::for_tests(&dir.join("project"), "demo", config).await;
        app.blueprints.apply(yaml, "1").await.unwrap();
        Self {
            app,
            store,
            recordings,
        }
    }

    /// Stops it as a playground stops, so the next start opens its database.
    async fn stop(self) {
        self.app.sessions.close_database().await.unwrap();
    }

    async fn run(&self, code: &str, secrets: HarnessSecretBindings) -> String {
        let response = self
            .app
            .run_program(ProgramRun {
                label: labels::ASSISTANT.into(),
                blueprint: "demo".into(),
                code: code.into(),
                variables: [("customerId".to_owned(), "cus_northwind".to_owned())].into(),
                secrets,
            })
            .await;
        assert!(response.error.is_none(), "{:?}", response.error);
        response.execution_id
    }

    async fn test(&self, recorded: RecordedRun) -> TestReport {
        self.app
            .test_program(TestRun {
                label: labels::TEST.into(),
                recorded,
                bindings: Default::default(),
                mode: TestMode::Recorded,
                secrets: None,
            })
            .await
            .expect("the test run starts")
            .report
    }
}

const BLUEPRINT: &str = "\
name: demo
default: allow
allow_insecure_http: true
vfs:
  mode: none
variables:
  customerId:
    required: true
permissions:
  main:
    - capability: http.get
      filter: path == \"/b\"
      action: deny
";

fn fetch_both(port: u16) -> String {
    format!(
        r#"import {{ get }} from "submilli:http";
function main(): string {{
  const a = get("http://127.0.0.1:{port}/a").body;
  let b = "";
  try {{
    b = get("http://127.0.0.1:{port}/b").body;
  }} catch (e: Error) {{
    b = "denied";
  }}
  return a + "|" + b;
}}"#
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stored_run_loaded_after_a_restart_tests_as_the_in_memory_recording_does() {
    let port = origin(|_| (String::new(), b"alpha".to_vec()));
    let dir = tempfile::tempdir().unwrap();
    let first = Playground::start(dir.path(), BLUEPRINT).await;
    let execution_id = first
        .run(&fetch_both(port), HarnessSecretBindings::new())
        .await;
    let in_memory = first.recordings.lock().unwrap()[&execution_id].clone();
    let before = first.test(in_memory).await;
    first.stop().await;

    let second = Playground::start(dir.path(), BLUEPRINT).await;
    let id = second
        .store
        .run_id_of(&execution_id)
        .unwrap()
        .expect("stored");
    assert_eq!(id, 1);
    let stored = second.store.load_run(id).unwrap().unwrap();
    assert_eq!(stored.label, labels::ASSISTANT);
    assert_eq!(stored.recording.variables["customerId"], "cus_northwind");
    let after = second.test(stored.recording).await;

    let comparable = |report: &TestReport| {
        let mut value = serde_json::to_value(report).unwrap();
        value.as_object_mut().unwrap().remove("test_run");
        value
    };
    assert_eq!(comparable(&after), comparable(&before));
    assert_eq!(after.served.len(), 1, "{after:?}");
    assert!(after.stopped.is_none());

    // Both test runs were stored as runs of their own, linked to the source.
    let runs = second.store.list_runs().unwrap();
    let tests: Vec<_> = runs.iter().filter(|run| run.entry == "test").collect();
    assert_eq!(tests.len(), 2, "{runs:?}");
    for test in tests {
        assert_eq!(test.label, labels::TEST);
        assert_eq!(test.test_of.as_ref().unwrap().run, Some(1));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn credentials_are_masked_and_a_body_over_the_cap_is_cut_with_its_digest_kept() {
    let cap = super::PLAYGROUND_LOG_CONFIG.max_payload_bytes;
    let port = origin(move |_| {
        (
            "Set-Cookie: session=s3cr3t-cookie\r\nContent-Type: text/plain\r\n".to_owned(),
            vec![b'z'; cap + 10],
        )
    });
    let code = format!(
        r#"import {{ get }} from "submilli:http";
function main(): string {{
  const headers = new Map<string, string>([["Authorization", "Bearer hunter2-token"], ["X-Trace", "t1"]]);
  return String(get("http://127.0.0.1:{port}/big", headers).body.length);
}}"#
    );
    let dir = tempfile::tempdir().unwrap();
    let playground = Playground::start(dir.path(), BLUEPRINT).await;
    playground.run(&code, HarnessSecretBindings::new()).await;
    let run = playground.store.load_run(1).unwrap().unwrap();
    assert_eq!(run.result.as_deref(), Some((cap + 10).to_string().as_str()));
    let call = &run.recording.calls[0];

    let request = call.request.as_ref().unwrap();
    assert!(
        request
            .masked_headers
            .iter()
            .any(|name| name == "Authorization")
    );
    let response = call.response.as_ref().unwrap();
    assert!(
        response
            .masked_headers
            .iter()
            .any(|name| name.eq_ignore_ascii_case("set-cookie")),
        "{:?}",
        response.masked_headers
    );
    // The program's source names the header's value; the call log never does.
    let calls = serde_json::to_string(&run.recording.calls).unwrap();
    assert!(!calls.contains("hunter2-token"), "{calls:.400}");
    assert!(!calls.contains("s3cr3t-cookie"));
    assert!(calls.contains("t1"), "other headers are kept");
    let file = std::fs::read_to_string(playground.store.run_path(1)).unwrap();
    assert!(!file.contains("s3cr3t-cookie"));

    assert!(response.truncated, "a body over the cap is marked cut");
    assert_eq!(response.bytes, (cap + 10) as u64, "the full size is kept");
    match &response.body {
        Some(BodyCopy::Text(text)) => assert_eq!(text.len(), cap),
        other => panic!("a capped text copy, not {other:?}"),
    }
    assert_eq!(response.digest.len(), 64);
    assert!(response.digest.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(request.digest.len(), 64);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_harness_secret_an_origin_echoes_is_redacted_everywhere() {
    let port = origin(|head| {
        let token = head
            .lines()
            .find_map(|line| line.strip_prefix("authorization: Bearer "))
            .or_else(|| {
                head.lines()
                    .find_map(|line| line.strip_prefix("Authorization: Bearer "))
            })
            .unwrap_or("none")
            .trim()
            .to_owned();
        let base64 = base64::engine::general_purpose::STANDARD.encode(&token);
        let url: String = url::form_urlencoded::byte_serialize(token.as_bytes()).collect();
        (
            String::new(),
            format!("plain={token} b64={base64} url={url}").into_bytes(),
        )
    });
    let yaml = format!(
        "{BLUEPRINT}secrets:\n  API_KEY:\n    harness: {{}}\nauth_proxy:\n  - host: 127.0.0.1\n    allow_insecure_http: true\n    headers:\n      Authorization: \"Bearer ${{secrets.API_KEY}}\"\n"
    );
    let code = format!(
        r#"import {{ get }} from "submilli:http";
function main(): string {{
  const body = get("http://127.0.0.1:{port}/echo").body;
  console.log(body);
  return body;
}}"#
    );
    let dir = tempfile::tempdir().unwrap();
    let playground = Playground::start(dir.path(), &yaml).await;
    let secrets: HarnessSecretBindings = [("API_KEY".to_owned(), SECRET.to_owned())].into();
    let execution_id = playground.run(&code, secrets).await;
    // The program really saw it: the in-memory recording holds it.
    let in_memory = playground.recordings.lock().unwrap()[&execution_id].clone();
    assert!(
        serde_json::to_string(&in_memory).unwrap().contains(SECRET),
        "the origin echoed the injected secret"
    );
    // Events are delivered off the run's path; give them a moment.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let base64 = base64::engine::general_purpose::STANDARD.encode(SECRET);
    let url: String = url::form_urlencoded::byte_serialize(SECRET.as_bytes()).collect();
    let mut files = 0;
    super::walk(playground.store.root(), &mut |path| {
        let text = std::fs::read_to_string(path).unwrap();
        for form in [SECRET, base64.as_str(), url.as_str()] {
            assert!(!text.contains(form), "{} holds {form}", path.display());
        }
        files += 1;
    });
    assert!(files >= 4);
    let run = playground.store.load_run(1).unwrap().unwrap();
    assert!(
        run.console
            .contains("plain=[redacted] b64=[redacted] url=[redacted]"),
        "{}",
        run.console
    );
    let events = playground
        .store
        .read_events(run.recording.session_id.as_deref())
        .unwrap();
    assert!(!events.events.is_empty());
    let _: Value = serde_json::to_value(&events.events[0]).unwrap();
    let lines: Vec<&str> = events
        .events
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::Event(event) => match &event.kind {
                EventKind::Console { line, .. } => Some(line.as_str()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(
        lines,
        ["plain=[redacted] b64=[redacted] url=[redacted]"],
        "the console line is an event of its own, masked like the run's console"
    );
}
