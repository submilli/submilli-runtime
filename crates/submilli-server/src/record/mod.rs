//! Run recording: an embedder's hook into every program the server runs.
//!
//! A [`RunRecorderFactory`] on [`ServerConfig`](crate::ServerConfig) is asked about each
//! run as it starts, whoever sent it: REST, a session, MCP, an MCP file tool, or the
//! [`run_program`](crate::record::run_program) and
//! [`run_in_session`](crate::record::run_in_session) entry points. The recorder it
//! returns sees the run's decisions and calls, through the interpreter's
//! [`DecisionLog`](interpreter::runtime::DecisionLog), and is finished once, from the
//! task that owns the run, so a run is recorded even when its client has gone.
//!
//! Without a factory nothing here runs, and responses and the server audit are exactly
//! what they are without this module.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use interpreter::runtime::limits::ExecutionUsage;
use interpreter::runtime::{DecisionLogConfig, DecisionLogOutput, RecordObserver};
use submilli_blueprint::{Blueprint, HarnessSecretBindings, VarBindings};

use crate::error::ExecuteError;

pub mod events;
mod program;
pub mod recheck;
pub mod replay;
mod session;
mod test_run;
mod throwaway;
pub use events::{EVENT_SCHEMA, EventKind, SessionEvent};
pub use program::{ProgramRun, run_program};
pub use recheck::{RecheckReport, RecordedRun, VariableReport, recheck};
pub use replay::{
    Cassette, LiveReach, Miss, MissReason, RecordedEmbeddingProvider, RecordedHttpClient,
    RecordedLlmProvider, RecordedMcpTransport, ReplayReport, Served,
};
pub use session::{
    SessionProgram, SessionRunError, SessionStart, SessionStartError, end_session, run_in_session,
    session_variables, start_session,
};
pub use submilli_shared::mcp::McpCatalog;
pub(crate) use test_run::TestWorld;
pub use test_run::{
    LiveCall, ServedCall, Stop, TestError, TestMode, TestOutcome, TestReport, TestRun, test_program,
};
pub use throwaway::{
    ForkedSessionKv, LOCAL_STATE_CAP_BYTES, LocalState, Throwaway, ThrowawayError,
};

/// How a run reached the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEntry {
    /// `POST /v1/execute`.
    Http,
    /// `POST /v1/sessions/{id}/execute`, or [`run_in_session`] with the label its caller
    /// gave.
    Session,
    /// The MCP execute tool.
    Mcp,
    /// An MCP file tool (`read_file`, `list_files`); its run is the one policy decision,
    /// finished when it is made. The read's own I/O and its outcome are not part of the
    /// run.
    McpFileTool { tool: String },
    /// [`run_program`], with the label its caller gave.
    Program,
    /// A test of a recorded run under a newer blueprint; the run it tests is
    /// [`RunStart::test_of`].
    Test,
}

/// What a run is, captured as it starts.
#[derive(Clone)]
pub struct RunStart {
    /// The execution's audit id; a stored response carries it too. A recorded run is
    /// stopped by it through [`AppState::cancel_run`](crate::AppState::cancel_run) while
    /// it is in flight.
    pub execution_id: String,
    /// Who started it: the API token's name, or the label given to [`run_program`],
    /// [`run_in_session`], or [`test_program`]. Never the token itself.
    pub label: String,
    pub entry: RunEntry,
    /// The `execution_id` of the recorded run this run tests; `None` for any other run.
    pub test_of: Option<String>,
    /// The MCP client's name from its `initialize`, such as `langchain-mcp-adapters`.
    pub client: Option<String>,
    /// The MCP client's id for the tool call that started the run, when it sent one.
    pub tool_call_id: Option<String>,
    pub session_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub blueprint_name: String,
    /// The blueprint the run is decided under, read once as it starts.
    pub blueprint: Arc<Blueprint>,
    /// The audit's hash of that blueprint.
    pub blueprint_hash: Option<String>,
    /// The version the run is decided under: the version tag the blueprint was
    /// registered with, read in the same lookup as the blueprint, or
    /// [`Self::blueprint_hash`] when it was registered without one.
    pub blueprint_version: Option<String>,
    pub variables: Arc<VarBindings>,
    /// The harness secrets the request supplied for this run, so a recorder can keep
    /// their values out of what it stores. A recorder must never store or log them.
    pub harness_secrets: Arc<HarnessSecretBindings>,
    /// The program's source; `None` for a file tool.
    pub code: Option<Arc<str>>,
}

/// An idempotent retry answered from the ledger: no new run, a link to the first one.
#[derive(Debug, Clone)]
pub struct RetryLink {
    pub session_id: String,
    pub idempotency_key: String,
    /// The `execution_id` of the run whose response was returned again.
    pub original_execution_id: Option<String>,
}

/// How a run ended.
pub struct FinishedRun {
    /// Whether the program reached the runner. `false` for a failure before it got there:
    /// a parse error, a package that would not resolve, a failure to prepare the session,
    /// or a caller that dropped the run before it was dispatched.
    /// A failure inside the runner (git resolution, compilation, setup of the store) is
    /// dispatched: the runner had the program and finished the run itself.
    pub dispatched: bool,
    /// `None` when the program completed.
    pub error: Option<ExecuteError>,
    /// `main`'s output, as the response carries it.
    pub result: Option<String>,
    /// Console output, in full (a success response omits it).
    pub console: String,
    pub usage: ExecutionUsage,
    /// Decisions and calls. Empty for a run that never dispatched.
    pub log: DecisionLogOutput,
    /// The `@mcp/<server>` packages the program was compiled against: only the declared
    /// servers it imports. A replay compiles against these instead of contacting the
    /// servers again; the catalog serializes for keeping with a recording. Usually shared
    /// with the server's discovery cache (a blueprint with harness secrets discovers per
    /// run), and not counted in the recorder's byte budget. `None` when the run never
    /// reached the runner or was dropped without reporting, and for an MCP file tool.
    pub mcp_catalog: Option<Arc<McpCatalog>>,
    /// From the run's start to its end.
    pub wall: Duration,
}

/// Records one run.
pub trait RunRecorder: Send + Sync {
    /// Caps for this run's decision and call records.
    fn log_config(&self) -> DecisionLogConfig {
        DecisionLogConfig::default()
    }

    /// The run ended. Called once, including when the client disconnected or the run
    /// timed out. Normally from the task that owns the run; a run that was abandoned or
    /// lost is finished by whichever task drops it last, possibly while unwinding.
    fn finish(&self, run: FinishedRun);

    /// The size of the response its caller received: its result, error, and console
    /// output, as JSON. Called once, after [`finish`](Self::finish), when the response is
    /// ready for its caller. A REST caller that disconnected meanwhile may still be
    /// counted: handlers run to completion under graceful shutdown, so the server cannot
    /// tell. Never called for an MCP file tool, nor for a run that never dispatched.
    fn returned(&self, _bytes: u64) {}
}

/// Decides, for each run, whether and how it is recorded.
pub trait RunRecorderFactory: Send + Sync {
    /// A run is starting. `None` leaves it unrecorded. Runs are announced only once they
    /// are underway: a request refused before that (an unknown blueprint, invalid
    /// variables) never reaches here, and events are produced only for runs whose recorder
    /// this returns.
    fn start(&self, run: RunStart) -> Option<Arc<dyn RunRecorder>>;

    /// An idempotent retry was answered without running.
    fn retried(&self, _retry: RetryLink) {}

    /// Whether to stream [`SessionEvent`]s to [`event`](Self::event), for the runs whose
    /// [`start`](Self::start) returned a recorder. Asked once, when the server starts.
    fn wants_events(&self) -> bool {
        false
    }

    /// One event, in sequence order, on a task of the server's that delivers nothing
    /// else meanwhile. See [`events`] for what is dropped under load.
    fn event(&self, _event: SessionEvent) {}
}

/// A run being recorded: its recorder and when it started. Clones share one run.
#[derive(Clone)]
pub(crate) struct Recording {
    shared: Arc<RunShared>,
}

impl std::ops::Deref for Recording {
    type Target = RunShared;

    fn deref(&self) -> &RunShared {
        &self.shared
    }
}

/// What the clones of a [`Recording`] share. Dropping the last clone of a run nobody
/// finished finishes it: an embedder's timeout that drops a [`run_program`] future before
/// the program is dispatched leaves no task to finish it, and a dispatched run whose
/// owner task and its watcher are both gone has no one left either.
pub(crate) struct RunShared {
    pub recorder: Arc<dyn RunRecorder>,
    pub started: std::time::Instant,
    events: Option<Arc<events::RunEvents>>,
    progress: Progress,
}

impl Drop for RunShared {
    fn drop(&mut self) {
        // The embedder's `finish` runs here, possibly while the thread is already
        // unwinding: a panic of its must not escape and abort the process.
        let finished = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let run = self.abandoned();
            finish_once(&self.recorder, self.events.as_deref(), &self.progress, run);
        }));
        if finished.is_err() {
            tracing::warn!("the run recorder panicked while finishing an abandoned run");
        }
    }
}

impl RunShared {
    fn abandoned(&self) -> FinishedRun {
        let dispatched = self.progress.dispatched.load(Ordering::Acquire);
        // A run that never ran was cancelled by its caller; one that ran and was lost
        // without a report is a server failure, as a panicked owner task is.
        let (kind, message) = if dispatched {
            (
                crate::error::ErrorKind::RuntimeError,
                "internal: the run ended without reporting how",
            )
        } else {
            (
                crate::error::ErrorKind::Cancelled,
                "the caller abandoned the run before it ran",
            )
        };
        FinishedRun {
            dispatched,
            error: Some(ExecuteError {
                kind,
                message: message.into(),
                diagnostics: Vec::new(),
                denial: None,
            }),
            result: None,
            console: String::new(),
            usage: ExecutionUsage::default(),
            log: DecisionLogOutput::default(),
            mcp_catalog: None,
            wall: self.started.elapsed(),
        }
    }
}

/// Reports the end of a run to its recorder and its events, the first time only.
fn finish_once(
    recorder: &Arc<dyn RunRecorder>,
    events: Option<&events::RunEvents>,
    progress: &Progress,
    run: FinishedRun,
) {
    if progress.finished.swap(true, Ordering::AcqRel) {
        return;
    }
    if let Some(events) = events {
        events.finished(
            run.dispatched,
            run.error.as_ref().map(|error| error.kind),
            run.wall,
            run.console.len() as u64,
        );
    }
    recorder.finish(run);
}

/// How far a recording has got, shared by its clones so each callback happens once.
#[derive(Default)]
struct Progress {
    dispatched: AtomicBool,
    finished: AtomicBool,
    returned: AtomicBool,
}

impl Recording {
    /// Starts recording a run, when the server has a factory and it takes the run. Builds
    /// the [`RunStart`] only then, so a server without one does none of its work.
    pub(crate) fn start(
        state: &crate::app::AppState,
        describe_run: impl FnOnce() -> RunStart,
    ) -> Option<Self> {
        Self::start_tapped(state, describe_run, None)
    }

    /// [`start`](Self::start), with a test run's `tap` told how the run ended.
    pub(crate) fn start_tapped(
        state: &crate::app::AppState,
        describe_run: impl FnOnce() -> RunStart,
        tap: Option<&Arc<test_run::CallTap>>,
    ) -> Option<Self> {
        let factory = state.run_recorder()?;
        let started = std::time::Instant::now();
        let run = describe_run();
        let mut recorder = factory.start(run.clone())?;
        if let Some(tap) = tap {
            recorder = Arc::new(test_run::TapRecorder::new(recorder, Arc::clone(tap)));
        }
        let events = state
            .event_hub()
            .map(|hub| events::RunEvents::start(hub, &run));
        Some(Self::new(recorder, started, events))
    }

    fn new(
        recorder: Arc<dyn RunRecorder>,
        started: std::time::Instant,
        events: Option<Arc<events::RunEvents>>,
    ) -> Self {
        Self {
            shared: Arc::new(RunShared {
                recorder,
                started,
                events,
                progress: Progress::default(),
            }),
        }
    }

    /// The program reached the runner, which spawns the task that owns the run. From here
    /// a run that ends unreported was lost, not abandoned before it ran.
    pub(crate) fn mark_dispatched(&self) {
        self.progress.dispatched.store(true, Ordering::Release);
    }

    /// What sees the run's records as they are made.
    pub(crate) fn observer(&self) -> Option<Arc<dyn RecordObserver>> {
        self.events
            .clone()
            .map(|events| events as Arc<dyn RecordObserver>)
    }

    /// The decision log for the run.
    pub(crate) fn log(&self) -> Arc<interpreter::runtime::DecisionLog> {
        interpreter::runtime::DecisionLog::new(self.recorder.log_config(), self.observer())
    }

    /// The run ended. Only the first call through any clone reports it.
    pub(crate) fn finish(&self, run: FinishedRun) {
        finish_once(&self.recorder, self.events.as_deref(), &self.progress, run);
    }

    /// What its caller received. Reported once, and only for a run that has finished.
    pub(crate) fn returned(&self, bytes: u64) {
        if !self.progress.finished.load(Ordering::Acquire)
            || self.progress.returned.swap(true, Ordering::AcqRel)
        {
            return;
        }
        if let Some(events) = &self.events {
            events.returned(bytes);
        }
        self.recorder.returned(bytes);
    }

    /// Finishes a run that never reached the runner.
    pub(crate) fn undispatched(&self, error: &ExecuteError) {
        self.finish(self.without_log(false, error));
    }

    /// Finishes a run whose owner task ended without finishing it (it panicked). Its
    /// records went with the task; the catalog it compiled against did not.
    pub(crate) fn lost(&self, error: &ExecuteError, mcp_catalog: Arc<McpCatalog>) {
        self.finish(FinishedRun {
            mcp_catalog: Some(mcp_catalog),
            ..self.without_log(true, error)
        });
    }

    fn without_log(&self, dispatched: bool, error: &ExecuteError) -> FinishedRun {
        FinishedRun {
            dispatched,
            error: Some(error.clone()),
            result: None,
            console: String::new(),
            usage: ExecutionUsage::default(),
            log: DecisionLogOutput::default(),
            mcp_catalog: None,
            wall: self.started.elapsed(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct Calls(Mutex<Vec<String>>);

    impl RunRecorder for Calls {
        fn finish(&self, run: FinishedRun) {
            self.0
                .lock()
                .unwrap()
                .push(format!("finish {}", run.dispatched));
        }
        fn returned(&self, bytes: u64) {
            self.0.lock().unwrap().push(format!("returned {bytes}"));
        }
    }

    fn recording(calls: &Arc<Calls>) -> Recording {
        Recording::new(calls.clone(), std::time::Instant::now(), None)
    }

    #[derive(Default)]
    struct Collect(Mutex<Vec<SessionEvent>>);

    impl RunRecorderFactory for Collect {
        fn start(&self, _run: RunStart) -> Option<Arc<dyn RunRecorder>> {
            None
        }
        fn wants_events(&self) -> bool {
            true
        }
        fn event(&self, event: SessionEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    fn run_start() -> RunStart {
        RunStart {
            execution_id: "run-1".into(),
            label: "test".into(),
            entry: RunEntry::Program,
            test_of: None,
            client: None,
            tool_call_id: None,
            session_id: None,
            idempotency_key: None,
            blueprint_name: "bp".into(),
            blueprint: Arc::new(Blueprint::default()),
            blueprint_hash: None,
            blueprint_version: None,
            variables: Arc::new(VarBindings::default()),
            harness_secrets: Arc::default(),
            code: None,
        }
    }

    fn error() -> ExecuteError {
        ExecuteError {
            kind: crate::error::ErrorKind::RuntimeError,
            message: "x".into(),
            diagnostics: Vec::new(),
            denial: None,
        }
    }

    #[test]
    fn a_run_is_finished_once_however_many_clones_try() {
        let calls = Arc::new(Calls::default());
        let owner = recording(&calls);
        let outside = owner.clone();
        owner.finish(owner.without_log(true, &error()));
        outside.lost(&error(), Arc::new(McpCatalog::empty()));
        outside.undispatched(&error());
        assert_eq!(*calls.0.lock().unwrap(), ["finish true"]);
    }

    #[test]
    fn dropping_every_clone_of_an_unfinished_run_finishes_it_undispatched() {
        let calls = Arc::new(Calls::default());
        let owner = recording(&calls);
        let outside = owner.clone();
        drop(owner);
        assert!(calls.0.lock().unwrap().is_empty(), "a clone is still alive");
        drop(outside);
        assert_eq!(*calls.0.lock().unwrap(), ["finish false"]);
    }

    #[test]
    fn an_abandoned_run_emits_its_end_and_leaves_no_drop_count_behind() {
        let collect = Arc::new(Collect::default());
        let hub = events::EventHub::new(collect.clone());
        let calls = Arc::new(Calls::default());
        let events = events::RunEvents::start(&hub, &run_start());
        assert_eq!(hub.tracked_runs(), 1);
        drop(Recording::new(
            calls.clone(),
            std::time::Instant::now(),
            Some(events),
        ));
        assert_eq!(*calls.0.lock().unwrap(), ["finish false"]);
        let kinds: Vec<_> = collect
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|event| match &event.kind {
                EventKind::RunFinished {
                    dispatched, error, ..
                } => format!("finished {dispatched} {error:?}"),
                _ => "other".to_owned(),
            })
            .collect();
        assert_eq!(kinds, ["other", "finished false Some(Cancelled)"]);
        assert_eq!(hub.tracked_runs(), 0);
    }

    #[test]
    fn a_dispatched_run_dropped_unreported_is_lost_not_abandoned() {
        let calls = Arc::new(Calls::default());
        let owner = recording(&calls);
        owner.mark_dispatched();
        drop(owner.clone());
        assert!(calls.0.lock().unwrap().is_empty());
        drop(owner);
        assert_eq!(*calls.0.lock().unwrap(), ["finish true"]);
    }

    #[test]
    fn a_recorder_that_panics_in_the_drop_path_does_not_propagate() {
        struct Panics;
        impl RunRecorder for Panics {
            fn finish(&self, _run: FinishedRun) {
                panic!("embedder refused");
            }
        }
        let recording = Recording::new(Arc::new(Panics), std::time::Instant::now(), None);
        drop(recording);
        // Also while the thread is already unwinding: a second panic would abort.
        let unwound = std::panic::catch_unwind(|| {
            let _recording = Recording::new(Arc::new(Panics), std::time::Instant::now(), None);
            std::panic::resume_unwind(Box::new("first panic"));
        });
        assert!(unwound.is_err());
    }

    #[test]
    fn dropping_a_finished_run_does_not_finish_it_again() {
        let calls = Arc::new(Calls::default());
        let owner = recording(&calls);
        owner.undispatched(&error());
        drop(owner);
        assert_eq!(*calls.0.lock().unwrap(), ["finish false"]);
    }

    #[test]
    fn returned_waits_for_the_finish_and_reports_once() {
        let calls = Arc::new(Calls::default());
        let recording = recording(&calls);
        recording.returned(1);
        recording.lost(&error(), Arc::new(McpCatalog::empty()));
        recording.returned(2);
        recording.returned(3);
        assert_eq!(*calls.0.lock().unwrap(), ["finish true", "returned 2"]);
    }
}
