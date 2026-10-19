use std::path::{Path, PathBuf};

use super::super::volumes::VolumeTable;
use serde_json::{Value, json};

use super::super::super::packages::ProjectPackages;
use super::super::super::scaffold;
use super::super::super::state::StateDir;
use super::*;

/// What a test recorder saw, in order.
#[derive(Default)]
struct Seen {
    log: Mutex<Vec<String>>,
    starts: Mutex<Vec<RunStart>>,
}

impl Seen {
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    fn push(&self, line: String) {
        self.log.lock().unwrap().push(line);
    }
}

struct TestRecorder(Arc<Seen>);

impl RunRecorder for TestRecorder {
    fn start(&self, run: RunStart) -> Option<Arc<dyn RecordedRun>> {
        self.0.push(format!("start {}", run.execution_id));
        self.0.starts.lock().unwrap().push(run);
        Some(Arc::new(TestRun(Arc::clone(&self.0))))
    }
}

struct TestRun(Arc<Seen>);

impl RecordedRun for TestRun {
    fn event(&self, event: RunEvent<'_>) {
        let line = match event {
            RunEvent::CallStarted(call) => format!("call started {}", call.capability),
            RunEvent::Decision(record) => format!("decision {}", record.capability),
            RunEvent::CallFinished(call) => format!("call finished {}", call.capability),
            RunEvent::ConsoleLine(line) => format!("console {}", line.text),
        };
        self.0.push(line);
    }

    fn finish(&self, run: FinishedRun) {
        self.0.push(format!(
            "finish dispatched={} error={:?}",
            run.dispatched,
            run.error.map(|error| error.kind)
        ));
    }
}

/// A host over a fresh project state directory.
struct Host {
    _dir: tempfile::TempDir,
    sessions: Arc<Sessions>,
    host: Arc<RunHost>,
    seen: Arc<Seen>,
    store_root: PathBuf,
}

async fn host() -> Host {
    host_with(|_| {}).await
}

async fn host_with(configure: impl FnOnce(&mut RunHostConfig)) -> Host {
    let dir = tempfile::tempdir().unwrap();
    let store_root = dir.path().join("store");
    std::fs::create_dir_all(&store_root).unwrap();
    host_in(dir, store_root, configure).await
}

async fn host_in(
    dir: tempfile::TempDir,
    store_root: PathBuf,
    configure: impl FnOnce(&mut RunHostConfig),
) -> Host {
    let state = StateDir::for_project(dir.path());
    let sessions = Arc::new(Sessions::boot(&state).await.unwrap());
    let volumes = Arc::new(Volumes::boot(&state, VolumeTable::new()));
    let seen = Arc::new(Seen::default());
    let mut config = RunHostConfig::new(PackageStore::new(store_root.clone()));
    config.recorder = Some(Arc::new(TestRecorder(Arc::clone(&seen))));
    configure(&mut config);
    let host = Arc::new(RunHost::new(config, Arc::clone(&sessions), volumes).unwrap());
    Host {
        _dir: dir,
        sessions,
        host,
        seen,
        store_root,
    }
}

fn applied(yaml: &str) -> AppliedBlueprint {
    AppliedBlueprint {
        blueprint: Arc::new(submilli_blueprint::parse(yaml).unwrap()),
        version_tag: "v1".to_owned(),
    }
}

const PLAIN: &str = "name: demo\nvfs:\n  mode: per_session\n";

impl Host {
    async fn open(&self, blueprint: &AppliedBlueprint) -> String {
        self.sessions
            .open(&blueprint.blueprint, &BTreeMap::new())
            .await
            .unwrap()
            .id
    }

    fn request(&self, session_id: &str, blueprint: &AppliedBlueprint, code: &str) -> RunRequest {
        RunRequest {
            session_id: session_id.to_owned(),
            code: code.to_owned(),
            label: "test".to_owned(),
            entry: RunEntry::Mcp,
            blueprint: blueprint.clone(),
            variables: Arc::new(VarBindings::new()),
            harness_secrets: Arc::new(HarnessSecretBindings::new()),
            client: None,
            tool_call_id: None,
            test_of: None,
            vfs: VfsSource::Session,
            finalizers: None,
            world: None,
        }
    }

    /// Run `code` under `yaml` in a new session.
    async fn run(&self, yaml: &str, code: &str) -> RunOutput {
        let blueprint = applied(yaml);
        let session = self.open(&blueprint).await;
        self.host
            .run(self.request(&session, &blueprint, code))
            .await
    }
}

fn error_json(output: &RunOutput) -> Value {
    serde_json::to_value(output.response.error.as_ref().expect("the run failed")).unwrap()
}

#[tokio::test]
async fn a_returned_value_comes_back_and_console_is_kept_for_the_last_run() {
    let host = host().await;
    let blueprint = applied(PLAIN);
    let session = host.open(&blueprint).await;
    let output = host
        .host
        .run(host.request(
            &session,
            &blueprint,
            r#"function main(): number { console.log("hello"); return 42; }"#,
        ))
        .await;
    assert!(output.dispatched);
    assert_eq!(output.response.result.as_deref(), Some("42"));
    assert!(output.response.error.is_none());
    assert!(
        output.response.console.is_empty(),
        "console is hidden on success"
    );
    assert_eq!(output.response.session_id, session);
    let Some(super::super::sessions::LastRun::Kept { json, run_id, .. }) =
        host.sessions.last_run(&session).await.unwrap()
    else {
        panic!("the session keeps the run");
    };
    assert_eq!(run_id, output.response.execution_id);
    assert_eq!(
        serde_json::from_slice::<Value>(&json).unwrap(),
        json!({"result": "42", "console": ["hello"], "error": null})
    );
}

#[tokio::test]
async fn a_throw_returns_the_error_kind_and_message_with_console() {
    let host = host().await;
    let output = host
        .run(
            PLAIN,
            r#"function main(): void { console.log("before"); throw new Error("boom from throw"); }"#,
        )
        .await;
    assert_eq!(output.response.result, None);
    assert_eq!(output.response.console, ["before"]);
    let error = error_json(&output);
    assert_eq!(error["kind"], "runtime_error", "{error}");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("boom from throw"),
        "{error}"
    );
    assert!(error.get("diagnostics").is_none(), "{error}");
}

#[tokio::test]
async fn a_compile_error_returns_diagnostics_with_line_and_column() {
    let host = host().await;
    let output = host
        .run(PLAIN, "function main(): string {\n  return 42;\n}")
        .await;
    assert!(output.dispatched);
    let error = error_json(&output);
    assert_eq!(error["kind"], "compile_error", "{error}");
    assert!(
        error["message"].as_str().unwrap().contains("error:"),
        "{error}"
    );
    let diagnostic = &error["diagnostics"][0];
    assert_eq!(diagnostic["severity"], "error");
    assert_eq!(diagnostic["line"], 2, "{error}");
    assert!(diagnostic["column"].as_u64().unwrap() > 0, "{error}");
}

#[tokio::test]
async fn printing_more_than_a_mebibyte_fails_the_run() {
    let host = host().await;
    let output = host
        .run(
            PLAIN,
            r#"function main(): void {
                let line = "x";
                for (let i = 0; i < 10; i++) { line = line + line; }
                for (let i = 0; i < 1100; i++) { console.log(line); }
            }"#,
        )
        .await;
    let error = error_json(&output);
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("console output exceeds 1 MiB; print less output"),
        "{error}"
    );
    assert!(output.response.result.is_none());
}

fn with_timeout(config: &mut RunHostConfig) {
    config.runtime = RuntimeConfig {
        fuel: u64::MAX,
        timeout: Some(Duration::from_millis(500)),
        ..RuntimeConfig::default()
    };
}

#[tokio::test]
async fn a_loop_past_the_timeout_fails_with_the_timeout_error() {
    let host = host_with(with_timeout).await;
    let started = Instant::now();
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        host.run(PLAIN, "function main(): void { while (true) {} }"),
    )
    .await
    .expect("the loop is interrupted");
    assert!(started.elapsed() >= Duration::from_millis(500));
    assert_eq!(error_json(&output)["kind"], "timeout");
    let after = host
        .run(PLAIN, "function main(): number { return 42; }")
        .await;
    assert_eq!(after.response.result.as_deref(), Some("42"));
}

#[tokio::test]
async fn time_in_package_top_level_code_counts_against_the_timeout() {
    let host = host_with(with_timeout).await;
    packages::tests::install(
        &host.store_root,
        "@acme/util",
        "let spins = 0;\nwhile (true) { spins = spins + 1; }\nexport function answer(): number { return spins; }",
        &[],
    );
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        host.run(
            "name: demo\nvfs:\n  mode: none\npackages: ['@acme/util']\n",
            "import { answer } from \"@acme/util\";\nfunction main(): number { return answer(); }",
        ),
    )
    .await
    .expect("the package's top-level loop is interrupted");
    let error = error_json(&output);
    assert_eq!(error["kind"], "timeout", "{error}");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("at <top level> (@acme/util/lib:2:"),
        "{error}"
    );
}

#[tokio::test]
async fn a_listed_package_links_with_its_dependencies() {
    let host = host().await;
    packages::tests::install(
        &host.store_root,
        "@z/util",
        "export function forty(): number { return 40; }",
        &[],
    );
    packages::tests::install(
        &host.store_root,
        "@a/app",
        "import { forty } from \"@z/util\";\nexport function fortyTwo(): number { return forty() + 2; }",
        &["@z/util"],
    );
    let output = host
        .run(
            "name: demo\nvfs:\n  mode: none\npackages: ['@a/app']\n",
            "import { fortyTwo } from \"@a/app\";\nfunction main(): number { return fortyTwo(); }",
        )
        .await;
    assert_eq!(
        output.response.result.as_deref(),
        Some("42"),
        "{:?}",
        output.response.error
    );
}

#[tokio::test]
async fn importing_a_package_the_blueprint_does_not_list_does_not_compile() {
    let host = host().await;
    packages::tests::install(
        &host.store_root,
        "@acme/util",
        "export function answer(): number { return 41; }",
        &[],
    );
    let output = host
        .run(
            "name: demo\nvfs:\n  mode: none\n",
            "import { answer } from \"@acme/util\";\nfunction main(): number { return answer(); }",
        )
        .await;
    // As on the server: an unlisted package is not offered to the compiler.
    let error = error_json(&output);
    assert_eq!(error["kind"], "compile_error", "{error}");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("package `@acme/util` not found"),
        "{error}"
    );
}

#[tokio::test]
async fn a_listed_package_missing_from_the_store_is_a_package_resolution_error() {
    let host = host().await;
    let output = host
        .run(
            "name: demo\nvfs:\n  mode: none\npackages: ['@acme/util']\n",
            "import { answer } from \"@acme/util\";\nfunction main(): number { return answer(); }",
        )
        .await;
    assert!(!output.dispatched);
    let error = error_json(&output);
    assert_eq!(error["kind"], "package_resolution", "{error}");
    assert!(
        error["message"].as_str().unwrap().contains("@acme/util"),
        "{error}"
    );
}

#[tokio::test]
async fn an_http_call_the_network_policy_denies_is_refused_with_its_reason() {
    let host = host_with(|config| config.network_policy = NetworkPolicy::deny_private()).await;
    let output = host
        .run(
            "name: ssrf-test\nallow_insecure_http: true\nvfs:\n  mode: none\npermissions:\n  main:\n    - capability: http.get\n      action: allow\n",
            r#"import { get, Response } from "submilli:http";
            function main(): string { const r: Response = get("http://127.0.0.1:9/"); return r.body; }"#,
        )
        .await;
    assert!(output.response.result.is_none());
    let error = error_json(&output);
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("blocked by network policy"),
        "{error}"
    );
}

#[tokio::test]
async fn a_policy_denial_carries_who_was_refused_what_and_by_which_layer() {
    let host = host().await;
    let output = host
        .run(
            "name: policy\ndefault: deny\nvfs:\n  mode: none\npermissions:\n  main:\n    - capability: test.com/op\n      filter: amount >= 500\n      action: allow\n",
            r#"import { check } from "submilli:security";
            function main(): number { check("test.com/op", { amount: 100 }); return 1; }"#,
        )
        .await;
    let error = error_json(&output);
    assert_eq!(error["kind"], "permission_denied", "{error}");
    assert_eq!(error["capability"], "test.com/op", "{error}");
    assert_eq!(error["source"], "policy", "{error}");
    assert!(error["caller"].is_string(), "{error}");
}

#[tokio::test]
async fn cancelling_a_run_in_flight_ends_it_cancelled_and_a_finished_one_refuses() {
    let host = host().await;
    let blueprint = applied(PLAIN);
    let session = host.open(&blueprint).await;
    let running = tokio::spawn({
        let host = Arc::clone(&host.host);
        let request = host_request(
            &session,
            &blueprint,
            "function main(): void { while (true) {} }",
        );
        async move { host.run(request).await }
    });
    let execution_id = loop {
        if let Some(start) = host.seen.starts.lock().unwrap().first() {
            break start.execution_id.clone();
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    // Let the program start looping.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(host.host.cancel(&execution_id), "the run took the cancel");
    let output = tokio::time::timeout(Duration::from_secs(15), running)
        .await
        .expect("the cancelled run ends")
        .unwrap();
    assert_eq!(error_json(&output)["kind"], "cancelled");
    assert!(
        !host.host.cancel(&execution_id),
        "a finished run takes no cancel"
    );
    assert!(!host.host.cancel("no-such-run"));
}

fn host_request(session_id: &str, blueprint: &AppliedBlueprint, code: &str) -> RunRequest {
    RunRequest {
        session_id: session_id.to_owned(),
        code: code.to_owned(),
        label: "test".to_owned(),
        entry: RunEntry::Mcp,
        blueprint: blueprint.clone(),
        variables: Arc::new(VarBindings::new()),
        harness_secrets: Arc::new(HarnessSecretBindings::new()),
        client: None,
        tool_call_id: None,
        test_of: None,
        vfs: VfsSource::Session,
        finalizers: None,
        world: None,
    }
}

#[tokio::test]
async fn closing_the_session_cancels_its_run() {
    let host = host().await;
    let blueprint = applied(PLAIN);
    let session = host.open(&blueprint).await;
    let running = tokio::spawn({
        let host = Arc::clone(&host.host);
        let request = host_request(
            &session,
            &blueprint,
            "function main(): void { while (true) {} }",
        );
        async move { host.run(request).await }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(host.sessions.close(&session).await.unwrap());
    let output = tokio::time::timeout(Duration::from_secs(15), running)
        .await
        .expect("the run ends when its session closes")
        .unwrap();
    assert_eq!(error_json(&output)["kind"], "cancelled");
}

#[tokio::test]
async fn the_recorder_sees_the_start_each_event_in_order_and_one_finish() {
    let host = host().await;
    let output = host
        .run(
            "name: policy\ndefault: deny\nvfs:\n  mode: none\npermissions:\n  main:\n    - capability: test.com/op\n      action: allow\n",
            r#"import { check } from "submilli:security";
            function main(): number { console.log("first"); check("test.com/op", {}); console.log("second"); return 1; }"#,
        )
        .await;
    assert_eq!(output.response.result.as_deref(), Some("1"));
    let log = host.seen.log();
    assert_eq!(
        log,
        [
            format!("start {}", output.response.execution_id),
            "console first".to_owned(),
            "call started test.com/op".to_owned(),
            "decision test.com/op".to_owned(),
            "call finished test.com/op".to_owned(),
            "console second".to_owned(),
            "finish dispatched=true error=None".to_owned(),
        ]
    );
}

#[tokio::test]
async fn a_run_refused_before_dispatch_is_finished_once_as_undispatched() {
    let host = host().await;
    let blueprint = applied(PLAIN);
    let output = host
        .host
        .run(host_request(
            "no-such-session",
            &blueprint,
            "function main(): number { return 1; }",
        ))
        .await;
    assert!(!output.dispatched);
    assert_eq!(error_json(&output)["kind"], "runtime_error");
    let log = host.seen.log();
    assert_eq!(log.len(), 2, "{log:?}");
    assert!(log[0].starts_with("start "));
    assert_eq!(log[1], "finish dispatched=false error=Some(RuntimeError)");
}

#[tokio::test]
async fn a_run_whose_caller_goes_away_is_still_finished_once() {
    let host = host().await;
    let blueprint = applied(PLAIN);
    let session = host.open(&blueprint).await;
    let run = host.host.run(host_request(
        &session,
        &blueprint,
        "function main(): void { while (true) {} }",
    ));
    let _ = tokio::time::timeout(Duration::from_millis(300), run).await;
    let finished = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let log = host.seen.log();
            if log.iter().any(|line| line.starts_with("finish")) {
                break log;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("the owner task finishes the run");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        host.seen
            .log()
            .iter()
            .filter(|line| line.starts_with("finish"))
            .count(),
        1,
        "{finished:?}"
    );
    // The session is no longer counted in flight, so it can idle out.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let closed = host
                .sessions
                .reap(std::time::SystemTime::now() + Duration::from_secs(365 * 24 * 3600))
                .await
                .unwrap();
            if closed.contains(&session) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the session idles out");
}

#[tokio::test]
async fn every_entry_point_hands_the_recorder_the_sessions_harness_secrets() {
    let host = host().await;
    let blueprint = applied(
        "name: secrets\nvfs:\n  mode: per_session\nsecrets:\n  TOKEN:\n    harness:\n      required: true\n",
    );
    let session = host.open(&blueprint).await;
    let secrets = Arc::new(HarnessSecretBindings::from([(
        "TOKEN".to_owned(),
        "tok-0123456789".to_owned(),
    )]));
    let entries = [
        RunEntry::Mcp,
        RunEntry::Program,
        RunEntry::Session,
        RunEntry::Test,
    ];
    for entry in entries.clone() {
        let mut request = host_request(
            &session,
            &blueprint,
            "function main(): number { return 1; }",
        );
        request.entry = entry;
        request.harness_secrets = Arc::clone(&secrets);
        if request.entry == RunEntry::Test {
            request.test_of = Some("recorded-run".to_owned());
        }
        let output = host.host.run(request).await;
        assert!(
            output.response.error.is_none(),
            "{:?}",
            output.response.error
        );
    }
    let starts = host.seen.starts.lock().unwrap();
    assert_eq!(
        starts
            .iter()
            .map(|start| start.entry.clone())
            .collect::<Vec<_>>(),
        entries
    );
    for start in starts.iter() {
        assert_eq!(
            start.harness_secrets.get("TOKEN").map(String::as_str),
            Some("tok-0123456789"),
            "{:?}",
            start.entry
        );
        assert_eq!(start.session_id, session);
        assert_eq!(start.blueprint_version, "v1");
    }
    assert_eq!(starts[3].test_of.as_deref(), Some("recorded-run"));
}

#[tokio::test]
async fn a_programs_files_and_values_persist_in_its_session() {
    let host = host().await;
    let blueprint = applied("name: demo\ndefault: allow\nvfs:\n  mode: per_session\n");
    let session = host.open(&blueprint).await;
    let write = host
        .host
        .run(host_request(
            &session,
            &blueprint,
            r#"import * as fs from "submilli:fs";
            import * as session from "submilli:session";
            function main(): void { fs.writeText("/note.txt", "kept"); session.set("count", 3); }"#,
        ))
        .await;
    assert!(write.response.error.is_none(), "{:?}", write.response.error);
    assert!(
        host.sessions
            .session_dir(&session)
            .join("note.txt")
            .exists()
    );
    let read = host
        .host
        .run(host_request(
            &session,
            &blueprint,
            r#"import * as fs from "submilli:fs";
            import * as session from "submilli:session";
            function main(): string { return `${fs.readText("/note.txt")}${session.get<number>("count")}`; }"#,
        ))
        .await;
    assert_eq!(
        read.response.result.as_deref(),
        Some("kept3"),
        "{:?}",
        read.response.error
    );
}

/// The starter project with a second package whose source is easy to change.
fn money_project(dir: &Path) -> (PathBuf, PathBuf) {
    scaffold::init(dir).unwrap();
    let package_dir = dir.join("submilli");
    let money = package_dir.join("packages/money");
    std::fs::create_dir_all(money.join("src")).unwrap();
    std::fs::create_dir_all(money.join("docs")).unwrap();
    std::fs::write(
        money.join("src/lib.ts"),
        "/** The release. */\nexport function release(): string { return \"first\"; }\n",
    )
    .unwrap();
    std::fs::write(money.join("docs/readme.md"), "# @acme/money\n").unwrap();
    let manifest = package_dir.join("submilli.toml");
    let mut text = std::fs::read_to_string(&manifest).unwrap();
    text.push_str("\n[[package]]\nname = \"@acme/money\"\nversion = \"0.1.0\"\ndescription = \"Money helpers.\"\npath = \"packages/money\"\n");
    std::fs::write(&manifest, text).unwrap();
    (package_dir, money.join("src/lib.ts"))
}

fn edit(path: &Path, from: &str, to: &str) {
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains(from), "{from} is in {}", path.display());
    std::fs::write(path, text.replace(from, to)).unwrap();
}

const MONEY_YAML: &str =
    "name: demo\ndefault: allow\nvfs:\n  mode: none\npackages:\n- '@acme/money'\n";
const MONEY_CODE: &str =
    "import { release } from \"@acme/money\";\nfunction main(): string { return release(); }\n";

#[tokio::test]
async fn a_stale_package_is_rebuilt_before_the_run_and_a_failed_rebuild_refuses_it() {
    let dir = tempfile::tempdir().unwrap();
    let (package_dir, lib) = money_project(dir.path());
    let store_root = dir.path().join("store");
    let freshness = Arc::new(Freshness::new(Arc::new(ProjectPackages::new(
        &package_dir,
        store_root.clone(),
    ))));
    let host = host_in(dir, store_root, |config| {
        config.freshness = Some(freshness);
    })
    .await;
    let first = host.run(MONEY_YAML, MONEY_CODE).await;
    assert_eq!(
        first.response.result.as_deref(),
        Some("first"),
        "{:?}",
        first.response.error
    );

    edit(&lib, "\"first\"", "\"second\"");
    let second = host.run(MONEY_YAML, MONEY_CODE).await;
    assert_eq!(
        second.response.result.as_deref(),
        Some("second"),
        "{:?}",
        second.response.error
    );

    edit(&lib, "return \"second\";", "return \"second\" +;");
    let refused = host.run(MONEY_YAML, MONEY_CODE).await;
    assert!(!refused.dispatched);
    let error = error_json(&refused);
    assert_eq!(error["kind"], "package_resolution", "{error}");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("no longer builds"), "{message}");
    assert!(message.contains("the run did not start"), "{message}");
}

struct CountingFinalizers {
    seen: Mutex<Vec<(String, BTreeMap<String, String>)>>,
}

impl FinalizerRegistry for CountingFinalizers {
    fn provider(
        &self,
        session_id: &str,
        package_versions: BTreeMap<String, String>,
    ) -> Result<Arc<dyn SessionFinalizers>, String> {
        self.seen
            .lock()
            .unwrap()
            .push((session_id.to_owned(), package_versions));
        Err("finalizers are not taken here".to_owned())
    }
}

#[tokio::test]
async fn the_finalizer_registry_is_asked_with_the_linked_versions_before_the_run() {
    let registry = Arc::new(CountingFinalizers {
        seen: Mutex::new(Vec::new()),
    });
    let host = host_with({
        let registry = Arc::clone(&registry);
        move |config| config.finalizers = Some(registry)
    })
    .await;
    packages::tests::install(
        &host.store_root,
        "@a/app",
        "export function finalizer(key: string, value: string): void {}",
        &[],
    );
    let blueprint = applied("name: demo\nvfs:\n  mode: none\npackages: ['@a/app']\n");
    let session = host.open(&blueprint).await;
    let output = host
        .host
        .run(host_request(
            &session,
            &blueprint,
            "import { finalizer } from \"@a/app\";\nfunction main(): void {}",
        ))
        .await;
    assert!(!output.dispatched, "a refused registry refuses the run");
    assert_eq!(
        error_json(&output)["message"],
        "finalizers are not taken here"
    );
    assert_eq!(
        *registry.seen.lock().unwrap(),
        [(
            session,
            BTreeMap::from([("@a/app".to_owned(), r#"{"@a/app":"1.0.0"}"#.to_owned())])
        )]
    );
}

#[test]
fn a_catalog_discovered_before_a_clear_is_returned_but_not_kept() {
    let blueprint = applied(PLAIN).blueprint;
    let mut catalogs = McpCatalogs::default();
    let generation = catalogs.generation;
    catalogs.entries.clear();
    catalogs.generation += 1;
    let stale = Arc::new(McpCatalog::empty());
    let returned = catalogs.keep(
        "server".to_owned(),
        &blueprint,
        generation,
        Arc::clone(&stale),
    );
    assert!(Arc::ptr_eq(&returned, &stale));
    assert!(catalogs.entries.is_empty());

    let fresh = Arc::new(McpCatalog::empty());
    let kept = catalogs.keep(
        "server".to_owned(),
        &blueprint,
        catalogs.generation,
        Arc::clone(&fresh),
    );
    assert!(Arc::ptr_eq(&kept, &fresh));
    assert!(Arc::ptr_eq(
        &catalogs.cached("server", &blueprint).unwrap(),
        &fresh
    ));
}

#[test]
fn a_catalog_from_a_blueprint_taken_before_a_clear_is_never_served_after_it() {
    let old = applied(PLAIN).blueprint;
    let new = applied(PLAIN).blueprint;
    let mut catalogs = McpCatalogs::default();
    // The run took `old`, then a save applied `new` and cleared, and only then
    // did the run read the generation and discover.
    catalogs.entries.clear();
    catalogs.generation += 1;
    let generation = catalogs.generation;
    let stale = Arc::new(McpCatalog::empty());
    let returned = catalogs.keep("server".to_owned(), &old, generation, Arc::clone(&stale));
    assert!(Arc::ptr_eq(&returned, &stale));
    assert!(catalogs.cached("server", &new).is_none());

    let fresh = Arc::new(McpCatalog::empty());
    let kept = catalogs.keep("server".to_owned(), &new, generation, Arc::clone(&fresh));
    assert!(Arc::ptr_eq(&kept, &fresh));
    assert!(Arc::ptr_eq(
        &catalogs.cached("server", &new).unwrap(),
        &fresh
    ));
}
