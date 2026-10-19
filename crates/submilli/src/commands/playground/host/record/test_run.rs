//! A test run: a recorded run's program run again under the current blueprint, with its
//! outside calls answered from the recording and its local state copied from today.
//!
//! Copied from `submilli-server`'s `record/test_run.rs`. [`test_program`] runs it
//! through the host's run path in a session of its own, with the
//! [recorded-world connectors](super::replay) and a [`Throwaway`] standing in for the
//! run's world. It is recorded as a run of its own, as [`RunEntry::Test`] with
//! [`RunStart::test_of`](super::super::run::RunStart::test_of) naming its source, so
//! it streams events like any other run. The run stops at the first call with nothing
//! recorded; the [`TestReport`] says which call, why, and what was served before it.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use interpreter::runtime::{CallOutcome, CallRecord, EmbeddingProvider, LlmProvider, SourceLine};
use serde::Serialize;
use submilli_blueprint::{Blueprint, HarnessSecretBindings, VarBindings, resolve_harness_secrets};
use tokio::sync::oneshot;

use interpreter::runtime::{DecisionLogConfig, SessionKvStore};
use submilli_shared::mcp::McpCatalog;

use super::super::blueprints::AppliedBlueprint;
use super::super::finalizers::ReplayFinalizers;
use super::super::run::{
    self, Connectors, ExecuteResponse, FinishedRun, RunEntry, RunEvent, RunHost, RunRequest,
    RunWorld, VfsSource, parse, vfs_info,
};
use super::super::sessions::Sessions;
use super::super::volumes::Volumes;
use super::RecordedRun;
use super::recheck::{VariableReport, reconcile_variables};
use super::replay::{
    Cassette, LiveReach, Miss, MissReason, Nearest, RecordedEmbeddingProvider, RecordedHttpClient,
    RecordedLlmProvider, RecordedMcpTransport, Served, call_key,
};
use super::throwaway::{LocalState, Throwaway, ThrowawayError};

/// How a test run answers a call the recording cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum TestMode {
    /// The run stops there.
    Recorded,
    /// An unrecorded `GET` or `HEAD` request goes live, through the same auth proxy,
    /// transport policy and redirect guard as in any run, and is recorded in the test
    /// run's own log. Writes, MCP calls, model and embedding calls and downloads still stop the run.
    ReadsLive,
    /// Every call the recording cannot answer goes live. Recorded calls are still served.
    Live,
}

/// A recorded run to run again.
pub(crate) struct TestRun {
    /// Who the test run is recorded as, in place of an API token's name.
    pub(crate) label: String,
    pub(crate) recorded: RecordedRun,
    /// The current values for variables the current blueprint declares and the recorded
    /// run did not bind; a recorded value is kept where the blueprint still declares it.
    pub(crate) bindings: VarBindings,
    pub(crate) mode: TestMode,
    /// Trusted harness credentials for this test run. Never persisted: the caller supplies
    /// them as it did for the source run, and recordings never hold them.
    pub(crate) secrets: Option<HarnessSecretBindings>,
}

/// Why a recorded run cannot be tested.
#[derive(Debug)]
pub(crate) enum TestError {
    /// The recording holds no program: a file-tool run, or a retried request that never
    /// ran one.
    NoProgram { source_run: String },
    /// The program calls the session's application tools (`@tools`), whose
    /// answers a test run cannot replay yet.
    UsesTools { source_run: String },
    /// The blueprint the run used is not the one the playground serves now.
    BlueprintNotFound(String),
    /// The test run's session could not be opened, or its files could not be.
    Session(String),
    /// The variables do not satisfy the current blueprint's declarations.
    InvalidVariables(String),
    /// The harness secrets do not satisfy the current blueprint's declarations.
    InvalidSecrets(String),
    /// The host records no runs, or its recorder declined this one, and a test run is
    /// reported from its own record.
    NoRecorder,
    /// The local state could not be copied; over the cap, the message says so.
    LocalState(ThrowawayError),
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoProgram { source_run } => write!(
                f,
                "run {source_run} recorded no program (it was a file tool, or a retry of a \
                 request answered earlier), so there is nothing to run; test the run that \
                 executed the program"
            ),
            Self::UsesTools { source_run } => write!(
                f,
                "run {source_run} calls application tools (`@tools`), and a test run cannot \
                 replay their answers yet"
            ),
            Self::BlueprintNotFound(name) => write!(f, "blueprint '{name}' is not registered"),
            Self::Session(message) => f.write_str(message),
            Self::InvalidVariables(message) => write!(f, "invalid variables: {message}"),
            Self::InvalidSecrets(message) => write!(f, "invalid secrets: {message}"),
            Self::NoRecorder => f.write_str(
                "this playground has no run recorder, or its recorder declined this run, and \
                 a test run is recorded as a run of its own and reported from that record",
            ),
            Self::LocalState(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for TestError {}

/// What a test run returned, and what it took from the recording.
#[derive(Debug, Serialize)]
pub(crate) struct TestOutcome {
    /// As MCP execute answers; a run that stopped ends `cancelled`.
    pub(crate) response: ExecuteResponse,
    pub(crate) report: TestReport,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TestReport {
    pub(crate) mode: TestMode,
    /// The recorded run that was tested.
    pub(crate) source_run: String,
    /// The test run, which is recorded as a run of its own.
    pub(crate) test_run: String,
    /// Calls answered from the recording, in the order they were answered.
    pub(crate) served: Vec<ServedCall>,
    /// Calls with nothing recorded that went live instead, in the order they were made.
    pub(crate) went_live: Vec<LiveCall>,
    /// The call the run stopped at. A run that stopped is reported stopped even when the
    /// program caught the error it saw there.
    pub(crate) stopped: Option<Stop>,
    /// Recorded variables kept, filled from the current bindings, or dropped.
    pub(crate) variables: VariableReport,
    /// Local files and session data came from today, not from the recorded run.
    pub(crate) local_state: LocalState,
}

/// A call answered from the recording.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ServedCall {
    pub(crate) source_call_index: u64,
    /// The test run's own index for the same call, when its call log kept it.
    pub(crate) test_call_index: Option<u64>,
    pub(crate) capability: String,
    pub(crate) key: Option<String>,
}

/// A call that went live because the recording could not answer it.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct LiveCall {
    /// `http GET <url>`, `mcp <server>.<tool>`, `llm <model>`, or `embedding <model>`.
    pub(crate) key: String,
    /// Why the recording could not answer it.
    pub(crate) reason: MissReason,
    /// The test run's own index for the call; its call log holds the live response.
    pub(crate) test_call_index: Option<u64>,
}

/// The call a test run stopped at, from the test run's own call log and the recording.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Stop {
    /// `http GET <url>`, `mcp <server>.<tool>`, `llm <model>`, or `embedding <model>`.
    pub(crate) key: String,
    pub(crate) reason: MissReason,
    pub(crate) detail: String,
    /// The recording nearest to the call.
    pub(crate) nearest: Option<Nearest>,
    /// The call's index in the test run, when its call log kept it.
    pub(crate) test_call_index: Option<u64>,
    pub(crate) caller: Option<String>,
    pub(crate) capability: Option<String>,
    /// The program line that led to the call.
    pub(crate) line: Option<SourceLine>,
}

/// What a test run runs on: the host's run path, sessions and volumes, and the
/// blueprint it serves now.
pub(crate) struct TestHost<'a> {
    pub(crate) runs: &'a RunHost,
    pub(crate) sessions: &'a Sessions,
    pub(crate) volumes: &'a Volumes,
    /// `None` while no blueprint applies.
    pub(crate) blueprint: Option<AppliedBlueprint>,
}

/// Runs `test.recorded`'s program again under the blueprint the host serves now,
/// answering its outside calls from the recording and giving it a throwaway copy of
/// today's local state, in a session of its own that closes when it ends. It is
/// recorded as [`RunEntry::Test`], and never touches the network, the MCP servers,
/// a model provider, or the source session's files or data.
pub(crate) async fn test_program(
    host: TestHost<'_>,
    test: TestRun,
) -> Result<TestOutcome, TestError> {
    let TestRun {
        label,
        recorded,
        bindings,
        mode,
        secrets,
    } = test;
    if !host.runs.records() {
        return Err(TestError::NoRecorder);
    }
    let source_run = recorded.execution_id.clone();
    let Some(code) = recorded.code.clone() else {
        return Err(TestError::NoProgram { source_run });
    };
    if parse(&code)
        .ok()
        .and_then(|parsed| parsed.imports().ok())
        .is_some_and(|imports| imports.tools)
    {
        return Err(TestError::UsesTools { source_run });
    }
    // Read once, with its version tag: the variables, the local-state copy, and the
    // run itself all use this blueprint, whatever applies meanwhile.
    let Some(applied) = host
        .blueprint
        .filter(|applied| applied.blueprint.name == recorded.blueprint_name)
    else {
        return Err(TestError::BlueprintNotFound(recorded.blueprint_name));
    };
    let blueprint = Arc::clone(&applied.blueprint);
    let variables = reconcile_variables(&blueprint, &bindings, &recorded.variables);
    let supplied = variables.supplied_bindings(&blueprint);
    let resolved = variables
        .resolve(&blueprint)
        .map_err(|error| TestError::InvalidVariables(error.to_string()))?;
    // Refused before anything is copied, as the run would refuse them; the run
    // gets them as resolved, without the empty ones.
    let secrets = resolve_harness_secrets(&blueprint.secrets, &secrets.unwrap_or_default())
        .map_err(|error| TestError::InvalidSecrets(error.to_string()))?;
    let local = Throwaway::copy(
        host.sessions,
        host.volumes,
        recorded.session_id.as_deref(),
        &blueprint,
        &resolved,
    )
    .await
    .map_err(TestError::LocalState)?;
    let local_state = local.report.clone();
    let vfs = local
        .volumes
        .build_vfs(&blueprint, &resolved, local.session_root.as_deref())
        .map_err(|error| TestError::Session(format!("internal: vfs init failed: {error}")))?;
    let vfs = local
        .volumes
        .attach_limits(vfs, &blueprint)
        .await
        .map_err(|error| TestError::Session(format!("internal: vfs init failed: {error}")))?;
    let session = host
        .sessions
        .open(&blueprint, &supplied)
        .await
        .map_err(|error| TestError::Session(format!("internal: session init failed: {error}")))?;

    let (cancel, cancel_requested) = oneshot::channel();
    let cassette = Cassette::new(&recorded, cancel);
    let tap = Arc::new(CallTap::default());
    // Only the servers today's blueprint declares can be imported, as a normal run's
    // discovery would have it.
    let mcp_catalog = Arc::new(
        recorded
            .mcp_catalog
            .as_deref()
            .map_or_else(McpCatalog::empty, |catalog| {
                catalog.restricted_to(|server| blueprint.mcp.contains_key(server))
            }),
    );
    let local = Arc::new(local);
    let world = TestWorld {
        mode,
        cassette: Arc::clone(&cassette),
        cancel: Some(cancel_requested),
        mcp_catalog,
        local: Arc::clone(&local),
        tap: Arc::clone(&tap),
        blueprint: Arc::clone(&blueprint),
    };
    let output = host
        .runs
        .run(RunRequest {
            session_id: session.id.clone(),
            code,
            label,
            entry: RunEntry::Test,
            blueprint: applied,
            variables: Arc::new(session.variables.clone()),
            harness_secrets: Arc::new(secrets),
            client: None,
            tool_call_id: None,
            test_of: Some(source_run.clone()),
            vfs: VfsSource::Provided {
                vfs,
                vfs_info: vfs_info(&blueprint),
            },
            finalizers: Some(Arc::new(ReplayFinalizers::new(local.kv.limits()))),
            world: Some(Box::new(world)),
        })
        .await;
    if let Err(error) = host.sessions.close(&session.id).await {
        super::super::super::log::warn(&format!(
            "closing the test run's session failed: {error:#}"
        ));
    }
    drop(local);
    // Refused before it ran, so there is no run to report.
    if !tap.was_recorded() {
        return Err(TestError::NoRecorder);
    }

    let response = output.response;
    let replay = cassette.report();
    let calls = tap.calls();
    let (served, taken) = pair_served(&replay.served, &calls);
    let report = TestReport {
        mode,
        source_run,
        test_run: response.execution_id.clone(),
        served,
        went_live: pair_live(replay.went_live, &calls, taken),
        stopped: replay.miss.map(|miss| stop_of(miss, &calls)),
        variables,
        local_state,
    };
    Ok(TestOutcome { response, report })
}

/// A test run's world in place of a normal run's: the catalog its source compiled
/// against (no MCP server is contacted to discover one), the throwaway copy of its
/// session data, and the recorded-world connectors with the live ones behind them as
/// far as the mode lets a call go live.
struct TestWorld {
    mode: TestMode,
    cassette: Arc<Cassette>,
    /// The cassette's own cancel signal: a stop reaches the runner with no hop in
    /// between. Taken when the run starts.
    cancel: Option<oneshot::Receiver<()>>,
    mcp_catalog: Arc<McpCatalog>,
    /// Kept until the run ends, so its copies outlive it.
    local: Arc<Throwaway>,
    tap: Arc<CallTap>,
    blueprint: Arc<Blueprint>,
}

impl RunWorld for TestWorld {
    fn mcp_catalog(&self) -> Arc<McpCatalog> {
        Arc::clone(&self.mcp_catalog)
    }

    fn session_kv(&self) -> Arc<dyn SessionKvStore> {
        self.local.kv.clone()
    }

    /// The recorded connectors, with the live ones behind them as far as the mode lets
    /// a call go live. The live ones a mode does not reach are dropped unused, so such
    /// a run has no route to them.
    fn connectors(&self, live: Connectors) -> Connectors {
        let cassette = &self.cassette;
        let http = RecordedHttpClient::new(Arc::clone(cassette));
        let mcp = RecordedMcpTransport::new(Arc::clone(cassette), Arc::clone(&self.blueprint));
        let llm = live.llm.map(|declared| {
            let llm = RecordedLlmProvider::new(Arc::clone(cassette), declared);
            Arc::new(if self.mode == TestMode::Live {
                llm.with_live()
            } else {
                llm
            }) as Arc<dyn LlmProvider>
        });
        let embedding = live.embedding.map(|declared| {
            let embedding = RecordedEmbeddingProvider::new(Arc::clone(cassette), declared);
            Arc::new(if self.mode == TestMode::Live {
                embedding.with_live()
            } else {
                embedding
            }) as Arc<dyn EmbeddingProvider>
        });
        let (http, mcp) = match self.mode {
            TestMode::Recorded => (http, mcp),
            TestMode::ReadsLive => (http.with_live(live.http, LiveReach::Reads), mcp),
            TestMode::Live => (
                http.with_live(live.http, LiveReach::Everything),
                mcp.with_live(live.mcp),
            ),
        };
        Connectors {
            http: Arc::new(http),
            mcp: Arc::new(mcp),
            llm,
            embedding,
        }
    }

    fn cancel(&mut self, host: oneshot::Receiver<()>) -> oneshot::Receiver<()> {
        match self.cancel.take() {
            Some(own) => forward_cancel(own, &self.cassette, Some(host)),
            None => host,
        }
    }

    fn recording(&self, run: Arc<dyn run::RecordedRun>) -> Arc<dyn run::RecordedRun> {
        self.tap.recorded.store(true, Ordering::Release);
        Arc::new(TapRecorder {
            inner: run,
            tap: Arc::clone(&self.tap),
        })
    }
}

/// The run's cancel signal, `own` (the cassette's), with a cancel from `external`
/// (`cancel_run`) forwarded into the cassette so it ends the run the same way a stop does.
fn forward_cancel(
    own: oneshot::Receiver<()>,
    cassette: &Arc<Cassette>,
    external: Option<oneshot::Receiver<()>>,
) -> oneshot::Receiver<()> {
    if let Some(external) = external {
        let cassette = Arc::clone(cassette);
        tokio::spawn(async move {
            if external.await.is_ok() {
                cassette.cancel();
            }
        });
    }
    own
}

/// What the test run's own call log said of each call, kept when the run finished.
#[derive(Default)]
struct CallTap {
    calls: Mutex<Vec<TestCall>>,
    /// The recorder took the run; one it declined is not reported.
    recorded: AtomicBool,
}

impl CallTap {
    fn was_recorded(&self) -> bool {
        self.recorded.load(Ordering::Acquire)
    }

    fn calls(&self) -> Vec<TestCall> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[derive(Clone)]
struct TestCall {
    index: u64,
    caller: String,
    capability: String,
    line: Option<SourceLine>,
    outcome: Option<CallOutcome>,
    /// The digest of the request, as the connectors see it.
    digest: Option<String>,
    key: Option<String>,
}

impl TestCall {
    fn of(call: &CallRecord) -> Self {
        Self {
            index: call.call_index,
            caller: call.caller.clone(),
            capability: call.capability.clone(),
            line: call.line,
            outcome: call.outcome,
            digest: call
                .request
                .as_deref()
                .map(|request| request.digest.clone()),
            key: call_key(call),
        }
    }
}

/// Passes a test run's end to its recorder, after noting how its calls went.
struct TapRecorder {
    inner: Arc<dyn run::RecordedRun>,
    tap: Arc<CallTap>,
}

impl run::RecordedRun for TapRecorder {
    fn log_config(&self) -> DecisionLogConfig {
        self.inner.log_config()
    }

    fn event(&self, event: RunEvent<'_>) {
        self.inner.event(event);
    }

    fn finish(&self, run: FinishedRun) {
        *self
            .tap
            .calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner) =
            run.log.calls.iter().map(TestCall::of).collect();
        self.inner.finish(run);
    }

    fn returned(&self, bytes: u64) {
        self.inner.returned(bytes);
    }
}

/// The first call the test run finished, not yet taken, that `matches`: taken from then on.
fn claim<'a>(
    calls: &'a [TestCall],
    taken: &mut HashSet<u64>,
    matches: impl Fn(&TestCall) -> bool,
) -> Option<&'a TestCall> {
    let call = calls.iter().find(|call| {
        call.outcome != Some(CallOutcome::Unfinished)
            && !taken.contains(&call.index)
            && matches(call)
    })?;
    taken.insert(call.index);
    Some(call)
}

/// Each served call with the test run's index for it: the call whose request has one of the
/// digests the served call was accepted under, in the order the run made them.
fn pair_served(served: &[Served], calls: &[TestCall]) -> (Vec<ServedCall>, HashSet<u64>) {
    let mut taken: HashSet<u64> = HashSet::new();
    let paired = served
        .iter()
        .map(|served| {
            let test_call = claim(calls, &mut taken, |call| {
                call.digest
                    .as_ref()
                    .is_some_and(|digest| served.request_digests.contains(digest))
            });
            ServedCall {
                source_call_index: served.source_call_index,
                test_call_index: test_call.map(|call| call.index),
                capability: served.capability.clone(),
                key: served.key.clone(),
            }
        })
        .collect();
    (paired, taken)
}

/// Each call that went live with the test run's index for it: the next call of the same key
/// that no served call was paired with.
fn pair_live(went_live: Vec<Miss>, calls: &[TestCall], mut taken: HashSet<u64>) -> Vec<LiveCall> {
    went_live
        .into_iter()
        .map(|miss| {
            let call = claim(calls, &mut taken, |call| {
                call.key.as_deref() == Some(miss.key.as_str())
            });
            LiveCall {
                key: miss.key,
                reason: miss.reason,
                test_call_index: call.map(|call| call.index),
            }
        })
        .collect()
}

/// The stop, with the call the test run's log shows it ended on: the unfinished call for
/// the same key, else the only unfinished one. With several and none of the key, the log
/// does not say which, and the call is left out.
fn stop_of(miss: Miss, calls: &[TestCall]) -> Stop {
    let mut unfinished = calls
        .iter()
        .filter(|call| call.outcome == Some(CallOutcome::Unfinished));
    let call = unfinished
        .clone()
        .find(|call| call.key.as_deref() == Some(miss.key.as_str()))
        .or_else(|| match (unfinished.next(), unfinished.next()) {
            (Some(only), None) => Some(only),
            _ => None,
        });
    Stop {
        key: miss.key,
        reason: miss.reason,
        detail: miss.detail,
        nearest: miss.nearest,
        test_call_index: call.map(|call| call.index),
        caller: call.map(|call| call.caller.clone()),
        capability: call.map(|call| call.capability.clone()),
        line: call.and_then(|call| call.line),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unfinished(index: u64, key: &str) -> TestCall {
        TestCall {
            index,
            caller: "main".into(),
            capability: "http.get".into(),
            line: None,
            outcome: Some(CallOutcome::Unfinished),
            digest: None,
            key: Some(key.to_owned()),
        }
    }

    fn miss(key: &str) -> Miss {
        Miss {
            key: key.to_owned(),
            reason: MissReason::NoRecording,
            detail: String::new(),
            nearest: None,
        }
    }

    #[test]
    fn a_stop_is_attributed_to_the_unfinished_call_of_its_key() {
        let calls = [unfinished(0, "http GET a"), unfinished(1, "http GET b")];
        assert_eq!(stop_of(miss("http GET b"), &calls).test_call_index, Some(1));
    }

    #[test]
    fn a_stop_with_one_unfinished_call_of_another_key_falls_back_to_it() {
        let calls = [unfinished(4, "http GET a")];
        let stop = stop_of(miss("http GET b"), &calls);
        assert_eq!(stop.test_call_index, Some(4));
        assert_eq!(stop.caller.as_deref(), Some("main"));
    }

    #[test]
    fn a_stop_with_several_unfinished_calls_and_none_of_its_key_names_none() {
        let calls = [unfinished(0, "http GET a"), unfinished(1, "http GET c")];
        let stop = stop_of(miss("http GET b"), &calls);
        assert!(stop.test_call_index.is_none() && stop.caller.is_none());
        assert!(stop.capability.is_none() && stop.line.is_none());
    }
}
