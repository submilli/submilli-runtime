//! A test run: a recorded run's program run again under the current blueprint, with its
//! outside calls answered from the recording and its local state copied from today.
//!
//! [`test_program`] runs it through [`one_shot`](crate::handlers::execute::one_shot)'s path
//! with the [recorded-world connectors](super::replay) and a [`Throwaway`] swapped into the
//! run's services. It is recorded as a run of its own, as [`RunEntry::Test`](super::RunEntry)
//! with [`RunStart::test_of`](super::RunStart::test_of) naming its source, so it streams
//! events like any other run. The run stops at the first call with nothing recorded; the
//! [`TestReport`] says which call, why, and what was served before it.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, PoisonError};

use interpreter::runtime::{
    CallOutcome, CallRecord, LlmProvider, McpTransport, SessionKvStore, SourceLine, Vfs, VfsInfo,
};
use interpreter::stdlib::http::HttpClient;
use serde::Serialize;
use submilli_blueprint::{VarBindings, resolve_variables};
use tokio::sync::oneshot;

use super::recheck::{VariableReport, reconcile_variables};
use super::replay::{
    Cassette, LiveReach, Miss, MissReason, Nearest, RecordedHttpClient, RecordedLlmProvider,
    RecordedMcpTransport, call_key,
};
use super::throwaway::{LocalState, Throwaway, ThrowawayError};
use super::{FinishedRun, McpCatalog, RecordedRun, RunEntry, RunRecorder};
use crate::app::AppState;
use crate::handlers::execute::{ExecuteRequest, ExecuteResponse, one_shot_with};
use crate::session_manager::{SessionError, SessionManager};

/// How a test run answers a call the recording cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TestMode {
    /// The run stops there.
    Recorded,
    /// An unrecorded `GET` or `HEAD` request goes live, through the same auth proxy,
    /// transport policy and redirect guard as in any run, and is recorded in the test
    /// run's own log. Writes, MCP calls, model calls and downloads still stop the run.
    ReadsLive,
    /// Every call the recording cannot answer goes live. Recorded calls are still served.
    Live,
}

/// A recorded run to run again.
pub struct TestRun {
    /// Who the test run is recorded as, in place of an API token's name.
    pub label: String,
    pub recorded: RecordedRun,
    /// The current values for variables the current blueprint declares and the recorded
    /// run did not bind; a recorded value is kept where the blueprint still declares it.
    pub bindings: VarBindings,
    pub mode: TestMode,
}

/// Why a recorded run cannot be tested.
#[derive(Debug)]
pub enum TestError {
    /// The recording holds no program: a file-tool run, or a retried request that never
    /// ran one.
    NoProgram { source_run: String },
    /// The blueprint the run used is no longer registered.
    BlueprintNotFound(String),
    /// The blueprint could not be read.
    Store(String),
    /// The variables do not satisfy the current blueprint's declarations.
    InvalidVariables(String),
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
            Self::BlueprintNotFound(name) => write!(f, "blueprint '{name}' is not registered"),
            Self::Store(message) => f.write_str(message),
            Self::InvalidVariables(message) => write!(f, "invalid variables: {message}"),
            Self::LocalState(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for TestError {}

/// What a test run returned, and what it took from the recording.
#[derive(Debug, Serialize)]
pub struct TestOutcome {
    /// As `POST /v1/execute` answers; a run that stopped ends `cancelled`.
    pub response: ExecuteResponse,
    pub report: TestReport,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestReport {
    pub mode: TestMode,
    /// The recorded run that was tested.
    pub source_run: String,
    /// The test run, which is recorded as a run of its own.
    pub test_run: String,
    /// Calls answered from the recording, in the order they were answered.
    pub served: Vec<ServedCall>,
    /// Calls with nothing recorded that went live instead, in the order they were made.
    pub went_live: Vec<LiveCall>,
    /// The call the run stopped at. A run that stopped is reported stopped even when the
    /// program caught the error it saw there.
    pub stopped: Option<Stop>,
    /// Recorded variables kept, filled from the current bindings, or dropped.
    pub variables: VariableReport,
    /// Local files and session data came from today, not from the recorded run.
    pub local_state: LocalState,
}

/// A call answered from the recording.
#[derive(Debug, Clone, Serialize)]
pub struct ServedCall {
    pub source_call_index: u64,
    /// The test run's own index for the same call, when its call log kept it.
    pub test_call_index: Option<u64>,
    pub capability: String,
    pub key: Option<String>,
}

/// A call that went live because the recording could not answer it.
#[derive(Debug, Clone, Serialize)]
pub struct LiveCall {
    /// `http GET <url>`, `mcp <server>.<tool>`, or `llm <model>`.
    pub key: String,
    /// Why the recording could not answer it.
    pub reason: MissReason,
    /// The test run's own index for the call; its call log holds the live response.
    pub test_call_index: Option<u64>,
}

/// The call a test run stopped at, from the test run's own call log and the recording.
#[derive(Debug, Clone, Serialize)]
pub struct Stop {
    /// `http GET <url>`, `mcp <server>.<tool>`, or `llm <model>`.
    pub key: String,
    pub reason: MissReason,
    pub detail: String,
    /// The recording nearest to the call.
    pub nearest: Option<Nearest>,
    /// The call's index in the test run, when its call log kept it.
    pub test_call_index: Option<u64>,
    pub caller: Option<String>,
    pub capability: Option<String>,
    /// The program line that led to the call.
    pub line: Option<SourceLine>,
}

/// Runs `test.recorded`'s program again under the blueprint now registered under its
/// name, answering its outside calls from the recording and giving it a throwaway copy
/// of today's local state. It is recorded as [`RunEntry::Test`], and never touches the
/// network, the MCP servers, a model provider, or the source session's files or data.
pub async fn test_program(state: &AppState, test: TestRun) -> Result<TestOutcome, TestError> {
    let TestRun {
        label,
        recorded,
        bindings,
        mode,
    } = test;
    let source_run = recorded.execution_id.clone();
    let Some(code) = recorded.code.clone() else {
        return Err(TestError::NoProgram { source_run });
    };
    let blueprint = match state.blueprints().get(&recorded.blueprint_name).await {
        Ok(Some(blueprint)) => blueprint,
        Ok(None) => return Err(TestError::BlueprintNotFound(recorded.blueprint_name)),
        Err(error) => {
            return Err(TestError::Store(
                crate::blueprint::store_failure_message(error).into(),
            ));
        }
    };
    let variables = reconcile_variables(&blueprint, &bindings, &recorded.variables);
    let supplied = variables.bindings();
    let resolved = resolve_variables(&blueprint.variables, &supplied)
        .map_err(|error| TestError::InvalidVariables(error.to_string()))?;
    let local = Throwaway::copy(
        state.session_manager(),
        recorded.session_id.as_deref(),
        &blueprint,
        &resolved,
    )
    .await
    .map_err(TestError::LocalState)?;
    let local_state = local.report.clone();

    let (cancel, cancel_requested) = oneshot::channel();
    let cassette = Cassette::new(&recorded, cancel);
    let tap = Arc::new(CallTap::default());
    let world = TestWorld {
        source_run: source_run.clone(),
        mode,
        cassette: Arc::clone(&cassette),
        cancel: Some(cancel_requested),
        mcp_catalog: recorded
            .mcp_catalog
            .clone()
            .unwrap_or_else(|| Arc::new(McpCatalog::empty())),
        local,
        tap: Arc::clone(&tap),
    };
    let audit = crate::audit::ExecutionAudit::new(state.audit().clone(), &label, "test", None);
    let request = ExecuteRequest {
        code,
        blueprint: recorded.blueprint_name,
        variables: Some(supplied),
        secrets: None,
    };
    let (_session, response) = one_shot_with(
        state,
        request,
        Some(Arc::clone(&audit)),
        RunEntry::Test,
        Some(world),
    )
    .await;
    audit.finish(response.error.is_none());

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

/// What a test run needs from its caller's world, handed to the one-shot path.
pub(crate) struct TestWorld {
    source_run: String,
    mode: TestMode,
    cassette: Arc<Cassette>,
    cancel: Option<oneshot::Receiver<()>>,
    mcp_catalog: Arc<McpCatalog>,
    local: Throwaway,
    tap: Arc<CallTap>,
}

impl TestWorld {
    pub(crate) fn source_run(&self) -> &str {
        &self.source_run
    }

    pub(crate) fn tap(&self) -> &Arc<CallTap> {
        &self.tap
    }

    pub(crate) fn mcp_catalog(&self) -> Arc<McpCatalog> {
        Arc::clone(&self.mcp_catalog)
    }

    /// The run's cancel signal: the cassette's own, so a stop reaches the runner with no
    /// hop in between. A cancel from outside (`cancel_run`) is forwarded into it.
    pub(crate) fn cancel_requested(
        &mut self,
        external: Option<oneshot::Receiver<()>>,
    ) -> oneshot::Receiver<()> {
        let own = self
            .cancel
            .take()
            .expect("a test run's cancel is taken once");
        if let Some(external) = external {
            let cassette = Arc::clone(&self.cassette);
            tokio::spawn(async move {
                if external.await.is_ok() {
                    cassette.cancel();
                }
            });
        }
        own
    }

    /// The run's filesystem, over the throwaway copies.
    pub(crate) async fn vfs(
        &self,
        manager: &SessionManager,
        blueprint: &submilli_blueprint::Blueprint,
        variables: &VarBindings,
    ) -> Result<(Vfs, VfsInfo), SessionError> {
        manager
            .vfs_over_roots(
                blueprint,
                variables,
                self.local.session_root.as_deref(),
                &self.local.volumes,
            )
            .await
    }

    pub(crate) fn session_kv(&self) -> Arc<dyn SessionKvStore> {
        self.local.kv.clone()
    }

    /// The run's outside connectors: the recorded ones, with the live ones behind them as
    /// far as the mode lets a call go live. The live ones a mode does not reach are dropped
    /// unused, so such a run has no route to them.
    pub(crate) fn connectors(
        &self,
        live_http: Arc<dyn HttpClient>,
        live_mcp: Arc<dyn McpTransport>,
        live_llm: Option<Arc<dyn LlmProvider>>,
    ) -> Connectors {
        let cassette = &self.cassette;
        let http = RecordedHttpClient::new(Arc::clone(cassette));
        let mcp = RecordedMcpTransport::new(Arc::clone(cassette));
        let llm = live_llm.map(|declared| {
            let llm = RecordedLlmProvider::new(Arc::clone(cassette), declared);
            Arc::new(if self.mode == TestMode::Live {
                llm.with_live()
            } else {
                llm
            }) as Arc<dyn LlmProvider>
        });
        let (http, mcp) = match self.mode {
            TestMode::Recorded => (http, mcp),
            TestMode::ReadsLive => (http.with_live(live_http, LiveReach::Reads), mcp),
            TestMode::Live => (
                http.with_live(live_http, LiveReach::Everything),
                mcp.with_live(live_mcp),
            ),
        };
        Connectors {
            http: Arc::new(http),
            mcp: Arc::new(mcp),
            llm,
        }
    }
}

/// The three connectors a run reaches outside through.
pub(crate) struct Connectors {
    pub http: Arc<dyn HttpClient>,
    pub mcp: Arc<dyn McpTransport>,
    pub llm: Option<Arc<dyn LlmProvider>>,
}

/// What the test run's own call log said of each call, kept when the run finished.
#[derive(Default)]
pub(crate) struct CallTap {
    calls: Mutex<Vec<TestCall>>,
}

impl CallTap {
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
pub(crate) struct TapRecorder {
    inner: Arc<dyn RunRecorder>,
    tap: Arc<CallTap>,
}

impl TapRecorder {
    pub(crate) fn new(inner: Arc<dyn RunRecorder>, tap: Arc<CallTap>) -> Self {
        Self { inner, tap }
    }
}

impl RunRecorder for TapRecorder {
    fn log_config(&self) -> interpreter::runtime::DecisionLogConfig {
        self.inner.log_config()
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

/// Each served call with the test run's index for it: the call whose request has the same
/// digest, in the order the run made them.
fn pair_served(
    served: &[super::replay::Served],
    calls: &[TestCall],
) -> (Vec<ServedCall>, HashSet<u64>) {
    let mut taken: HashSet<u64> = HashSet::new();
    let paired = served
        .iter()
        .map(|served| {
            let test_call = calls.iter().find(|call| {
                call.outcome != Some(CallOutcome::Unfinished)
                    && call.digest.as_deref() == Some(served.request_digest.as_str())
                    && !taken.contains(&call.index)
            });
            if let Some(call) = test_call {
                taken.insert(call.index);
            }
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
            let call = calls.iter().find(|call| {
                call.outcome != Some(CallOutcome::Unfinished)
                    && call.key.as_deref() == Some(miss.key.as_str())
                    && !taken.contains(&call.index)
            });
            if let Some(call) = call {
                taken.insert(call.index);
            }
            LiveCall {
                key: miss.key,
                reason: miss.reason,
                test_call_index: call.map(|call| call.index),
            }
        })
        .collect()
}

/// The stop, with the call the test run's log shows it ended on: the unfinished call for
/// the same key, or the only unfinished one.
fn stop_of(miss: Miss, calls: &[TestCall]) -> Stop {
    let mut unfinished = calls
        .iter()
        .filter(|call| call.outcome == Some(CallOutcome::Unfinished));
    let call = unfinished
        .clone()
        .find(|call| call.key.as_deref() == Some(miss.key.as_str()))
        .or_else(|| unfinished.next());
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
