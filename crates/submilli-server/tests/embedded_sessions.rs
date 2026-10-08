//! Sessions an embedder starts, runs in, and ends from inside the process, and the
//! stop control over every run an embedder starts.

#[path = "common/in_memory_config.rs"]
mod in_memory_config;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use interpreter::RuntimeConfig;
use submilli_blueprint::VarBindings;
use submilli_server::audit::AuditConfig;
use submilli_server::blueprint::InMemoryBlueprintStore;
use submilli_server::error::ErrorKind;
use submilli_server::handlers::execute::ExecuteResponse;
use submilli_server::record::{
    FinishedRun, ProgramRun, RecordedRun, RunEntry, RunRecorder, RunRecorderFactory, RunStart,
    SessionProgram, SessionRunError, SessionStart, SessionStartError, TestMode, TestRun,
    end_session, run_in_session, run_program, session_variables, start_session, test_program,
};
use submilli_server::session_store::{DurableSessionStore, InMemoryDurableSessionStore};
use submilli_server::{AppState, ServerConfig};

/// `test.com/op` is allowed only for the user the session's `tenant` names.
const TENANT_BLUEPRINT: &str = "\
name: tenant
default: deny
variables:
  tenant:
    required: true
  region:
    default: eu
vfs:
  mode: ephemeral
permissions:
  main:
    - capability: test.com/op
      filter: userId == ${vars.tenant}
      action: allow
";

const SECRET_BLUEPRINT: &str = "\
name: needs-secret
default: deny
vfs: none
secrets:
  TOKEN:
    harness:
      required: true
";

const SPIN_BLUEPRINT: &str = "\
name: spin
default: deny
vfs:
  mode: ephemeral
permissions:
  main:
    - capability: test.com/ok
      action: allow
";

/// Asks for `test.com/op` on behalf of `userId`.
fn check_for(user: &str) -> String {
    format!(
        r#"import {{ check }} from "submilli:security";
function main(): number {{ check("test.com/op", {{ userId: "{user}" }}); return 1; }}"#
    )
}

const SPIN: &str = r#"
import { check } from "submilli:security";
function main(): number {
  check("test.com/ok", {});
  let spins = 0;
  while (true) { spins = spins + 1; }
}
"#;

// ---- the recorder ----------------------------------------------------------------------

#[derive(Default)]
struct Runs {
    started: Mutex<Vec<RunStart>>,
    finished: Mutex<Vec<(RunStart, Option<ErrorKind>, RecordedRun)>>,
}

impl Runs {
    fn started(&self) -> Vec<RunStart> {
        self.started.lock().unwrap().clone()
    }

    fn recorded(&self, execution_id: &str) -> RecordedRun {
        self.finished
            .lock()
            .unwrap()
            .iter()
            .find(|(start, _, _)| start.execution_id == execution_id)
            .unwrap_or_else(|| panic!("run {execution_id} was recorded"))
            .2
            .clone()
    }

    fn error_of(&self, execution_id: &str) -> Option<ErrorKind> {
        self.finished
            .lock()
            .unwrap()
            .iter()
            .find(|(start, _, _)| start.execution_id == execution_id)
            .unwrap_or_else(|| panic!("run {execution_id} finished"))
            .1
    }

    /// Cancels the first run of `entry` once it has started, and returns its id.
    async fn cancel_first(&self, state: &AppState, entry: RunEntry) -> String {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let id = self
                    .started()
                    .into_iter()
                    .find(|start| start.entry == entry)
                    .map(|start| start.execution_id);
                if let Some(id) = id
                    && state.cancel_run(&id)
                {
                    return id;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the run was registered for cancelling")
    }
}

struct Factory(Arc<Runs>);

impl RunRecorderFactory for Factory {
    fn start(&self, run: RunStart) -> Option<Arc<dyn RunRecorder>> {
        self.0.started.lock().unwrap().push(run.clone());
        Some(Arc::new(Recorder {
            runs: self.0.clone(),
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
        let recorded = RecordedRun::from_parts(&self.start, &run);
        self.runs.finished.lock().unwrap().push((
            self.start.clone(),
            run.error.map(|error| error.kind),
            recorded,
        ));
    }
}

// ---- the server ------------------------------------------------------------------------

struct Server {
    state: AppState,
    runs: Arc<Runs>,
    _dirs: tempfile::TempDir,
}

/// An embedded server as the playground runs it: recorded, with its audit log off.
fn server(runtime: RuntimeConfig) -> Server {
    let blueprints = [TENANT_BLUEPRINT, SECRET_BLUEPRINT, SPIN_BLUEPRINT]
        .map(|yaml| submilli_blueprint::parse(yaml).expect("blueprint"));
    let dirs = tempfile::tempdir().expect("dirs");
    let runs = Arc::new(Runs::default());
    let config = ServerConfig {
        blueprints: Some(Arc::new(
            InMemoryBlueprintStore::seed(blueprints).expect("seed"),
        )),
        session_storage_root: Some(dirs.path().join("sessions")),
        runtime,
        audit: AuditConfig {
            enabled: false,
            ..AuditConfig::default()
        },
        run_recorder: Some(Arc::new(Factory(runs.clone()))),
        ..in_memory_config::config()
    };
    Server {
        state: AppState::new(config).expect("state"),
        runs,
        _dirs: dirs,
    }
}

fn plain() -> Server {
    server(RuntimeConfig::default())
}

/// Programs run until stopped.
fn spinning() -> Server {
    server(RuntimeConfig {
        fuel: u64::MAX,
        timeout: Some(Duration::from_secs(30)),
        ..RuntimeConfig::default()
    })
}

fn vars(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
        .collect()
}

async fn start(state: &AppState, blueprint: &str, variables: &[(&str, &str)]) -> String {
    start_session(
        state,
        SessionStart {
            blueprint: blueprint.into(),
            variables: vars(variables),
            secrets: Default::default(),
        },
    )
    .await
    .expect("the session starts")
}

async fn run_in(
    state: &AppState,
    session_id: &str,
    code: &str,
) -> Result<ExecuteResponse, SessionRunError> {
    run_in_session(
        state,
        SessionProgram {
            label: "assistant".into(),
            session_id: session_id.into(),
            code: code.into(),
        },
    )
    .await
}

// ---- sessions --------------------------------------------------------------------------

#[tokio::test]
async fn every_run_in_a_session_sees_the_variables_it_was_started_with() {
    let server = plain();
    let session = start(&server.state, "tenant", &[("tenant", "u_42")]).await;
    assert_eq!(
        session_variables(&server.state, &session).await.unwrap(),
        Some(vars(&[("region", "eu"), ("tenant", "u_42")])),
        "defaults are filled at start"
    );

    for _ in 0..2 {
        let response = run_in(&server.state, &session, &check_for("u_42"))
            .await
            .expect("the session runs");
        assert!(response.error.is_none(), "{:?}", response.error);
        assert_eq!(response.result.as_deref(), Some("1"));
        assert_eq!(response.session_id, session);
    }
    // The tenant the session was started with is the only one it may act for.
    let other = run_in(&server.state, &session, &check_for("u_43"))
        .await
        .expect("the session runs");
    assert!(other.error.is_some(), "another tenant is denied");

    let started = server.runs.started();
    assert_eq!(started.len(), 3);
    for run in &started {
        assert_eq!(run.entry, RunEntry::Session);
        assert_eq!(run.label, "assistant");
        assert_eq!(run.session_id.as_deref(), Some(session.as_str()));
        assert_eq!(
            run.variables.get("tenant").map(String::as_str),
            Some("u_42")
        );
    }
}

#[tokio::test]
async fn an_unknown_session_is_refused() {
    let server = plain();
    let error = run_in(&server.state, "no-such-session", &check_for("u_42"))
        .await
        .expect_err("an unknown session is refused");
    assert!(
        matches!(&error, SessionRunError::UnknownSession { session_id } if session_id == "no-such-session"),
        "{error}"
    );
    assert_eq!(
        session_variables(&server.state, "no-such-session")
            .await
            .unwrap(),
        None
    );
    assert!(!end_session(&server.state, "no-such-session").await.unwrap());
    assert!(server.runs.started().is_empty(), "a refusal starts no run");
}

#[tokio::test]
async fn an_ended_session_refuses_runs() {
    let server = plain();
    let session = start(&server.state, "tenant", &[("tenant", "u_42")]).await;
    run_in(&server.state, &session, &check_for("u_42"))
        .await
        .expect("the session runs");

    assert!(end_session(&server.state, &session).await.unwrap());
    let error = run_in(&server.state, &session, &check_for("u_42"))
        .await
        .expect_err("an ended session is refused");
    assert!(
        matches!(error, SessionRunError::UnknownSession { .. }),
        "{error}"
    );
    assert_eq!(
        session_variables(&server.state, &session).await.unwrap(),
        None
    );
    assert!(
        !end_session(&server.state, &session).await.unwrap(),
        "already ended"
    );
    assert_eq!(server.runs.started().len(), 1, "only the first run started");
}

#[tokio::test]
async fn invalid_variables_or_an_unknown_blueprint_are_refused_at_start() {
    let server = plain();
    let attempt = |blueprint: &str, variables: &[(&str, &str)]| {
        start_session(
            &server.state,
            SessionStart {
                blueprint: blueprint.into(),
                variables: vars(variables),
                secrets: Default::default(),
            },
        )
    };

    let missing = attempt("tenant", &[])
        .await
        .expect_err("tenant is required");
    assert!(
        matches!(missing, SessionStartError::InvalidVariables(_)),
        "{missing}"
    );
    assert!(
        missing.to_string().starts_with("invalid variables:"),
        "{missing}"
    );

    let undeclared = attempt("tenant", &[("tenant", "u_42"), ("color", "red")])
        .await
        .expect_err("an undeclared variable is refused");
    assert!(
        matches!(undeclared, SessionStartError::InvalidVariables(_)),
        "{undeclared}"
    );

    let unknown = attempt("nope", &[]).await.expect_err("no such blueprint");
    assert!(
        matches!(&unknown, SessionStartError::UnknownBlueprint { name, .. } if name == "nope"),
        "{unknown}"
    );
}

#[tokio::test]
async fn a_required_secret_missing_at_start_is_refused_naming_it() {
    let server = plain();
    let error = start_session(
        &server.state,
        SessionStart {
            blueprint: "needs-secret".into(),
            variables: BTreeMap::new(),
            secrets: Default::default(),
        },
    )
    .await
    .expect_err("the secret is required");
    assert!(
        matches!(error, SessionStartError::InvalidSecrets(_)),
        "{error}"
    );
    let message = error.to_string();
    assert!(
        message.contains("invalid secrets") && message.contains("TOKEN"),
        "{message}"
    );
}

#[tokio::test]
async fn a_session_that_lost_its_required_secret_is_refused_naming_it() {
    let blueprints = Arc::new(
        InMemoryBlueprintStore::seed([submilli_blueprint::parse(SECRET_BLUEPRINT).unwrap()])
            .expect("seed"),
    );
    let sessions: Arc<dyn DurableSessionStore> = Arc::new(InMemoryDurableSessionStore::default());
    let build = || {
        AppState::new(ServerConfig {
            blueprints: Some(blueprints.clone()),
            session_store: Some(Arc::clone(&sessions)),
            ..in_memory_config::config()
        })
        .expect("state")
    };
    let state = build();
    let session = start_session(
        &state,
        SessionStart {
            blueprint: "needs-secret".into(),
            variables: BTreeMap::new(),
            secrets: [("TOKEN".to_owned(), "t".to_owned())].into(),
        },
    )
    .await
    .expect("the session starts");

    // As a session imported from before secrets were kept: it survives, its secret
    // does not.
    drop(state);
    let mut record = sessions.load(&session).await.unwrap().unwrap();
    record.ephemeral_bindings = None;
    sessions.put(record).await.unwrap();
    let restarted = build();
    restarted.boot().await.expect("boot");

    let error = run_in(&restarted, &session, "function main(): void {}")
        .await
        .expect_err("the session cannot run without its secret");
    match &error {
        SessionRunError::SecretsRequired {
            session_id,
            required,
        } => {
            assert_eq!(session_id, &session);
            assert_eq!(required, &["TOKEN".to_owned()]);
        }
        other => panic!("expected the missing secret, got {other}"),
    }
    assert!(error.to_string().contains("TOKEN"), "{error}");
}

// ---- cancelling ------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn cancel_run_stops_a_program_run() {
    let server = spinning();
    let state = server.state.clone();
    let running = tokio::spawn(async move {
        run_program(
            &state,
            ProgramRun {
                label: "assistant".into(),
                blueprint: "spin".into(),
                code: SPIN.into(),
                variables: BTreeMap::new(),
                secrets: Default::default(),
            },
        )
        .await
    });
    let cancelled = server
        .runs
        .cancel_first(&server.state, RunEntry::Program)
        .await;
    let response = tokio::time::timeout(Duration::from_secs(20), running)
        .await
        .expect("the cancelled run returns")
        .unwrap();
    assert_eq!(response.execution_id, cancelled);
    assert_eq!(response.error.map(|e| e.kind), Some(ErrorKind::Cancelled));
    assert_eq!(server.runs.error_of(&cancelled), Some(ErrorKind::Cancelled));
    assert!(!server.state.cancel_run(&cancelled), "no longer running");
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_run_stops_a_run_in_a_session() {
    let server = spinning();
    let session = start(&server.state, "spin", &[]).await;
    let state = server.state.clone();
    let in_session = session.clone();
    let running = tokio::spawn(async move { run_in(&state, &in_session, SPIN).await });
    let cancelled = server
        .runs
        .cancel_first(&server.state, RunEntry::Session)
        .await;
    let response = tokio::time::timeout(Duration::from_secs(20), running)
        .await
        .expect("the cancelled run returns")
        .unwrap()
        .expect("the session ran");
    assert_eq!(response.execution_id, cancelled);
    assert_eq!(response.error.map(|e| e.kind), Some(ErrorKind::Cancelled));
    assert_eq!(server.runs.error_of(&cancelled), Some(ErrorKind::Cancelled));
    assert!(!server.state.cancel_run(&cancelled), "no longer running");

    // The session outlives a cancelled run.
    let next = run_in(
        &server.state,
        &session,
        "function main(): number { return 2; }",
    )
    .await
    .expect("the session runs again");
    assert_eq!(next.result.as_deref(), Some("2"), "{:?}", next.error);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_run_stops_a_test_run() {
    let server = spinning();
    // A source run to test, itself stopped from outside.
    let state = server.state.clone();
    let source = tokio::spawn(async move {
        run_program(
            &state,
            ProgramRun {
                label: "source".into(),
                blueprint: "spin".into(),
                code: SPIN.into(),
                variables: BTreeMap::new(),
                secrets: Default::default(),
            },
        )
        .await
    });
    let source_id = server
        .runs
        .cancel_first(&server.state, RunEntry::Program)
        .await;
    tokio::time::timeout(Duration::from_secs(20), source)
        .await
        .expect("the source run returns")
        .unwrap();
    let recorded = server.runs.recorded(&source_id);

    let state = server.state.clone();
    let running = tokio::spawn(async move {
        test_program(
            &state,
            TestRun {
                label: "tester".into(),
                recorded,
                bindings: VarBindings::new(),
                mode: TestMode::Recorded,
                secrets: None,
            },
        )
        .await
    });
    let cancelled = server
        .runs
        .cancel_first(&server.state, RunEntry::Test)
        .await;
    let outcome = tokio::time::timeout(Duration::from_secs(20), running)
        .await
        .expect("the cancelled test run returns")
        .unwrap()
        .expect("the test run started");
    assert_eq!(outcome.response.execution_id, cancelled);
    assert_eq!(
        outcome.response.error.map(|e| e.kind),
        Some(ErrorKind::Cancelled)
    );
    assert_eq!(server.runs.error_of(&cancelled), Some(ErrorKind::Cancelled));
    assert!(!server.state.cancel_run(&cancelled), "no longer running");
}
