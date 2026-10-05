//! Run recording: an embedder's hook into every program the server runs.
//!
//! A [`RunRecorderFactory`] on [`ServerConfig`](crate::ServerConfig) is asked about each
//! run as it starts, whoever sent it: REST, a session, MCP, an MCP file tool, or the
//! [`run_program`](crate::record::run_program) entry point. The recorder it returns sees
//! the run's decisions and calls, through the interpreter's
//! [`DecisionLog`](interpreter::runtime::DecisionLog), and is finished once, from the
//! task that owns the run, so a run is recorded even when its client has gone.
//!
//! Without a factory nothing here runs, and responses and the server audit are exactly
//! what they are without this module.

use std::sync::Arc;
use std::time::Duration;

use interpreter::runtime::limits::ExecutionUsage;
use interpreter::runtime::{DecisionLogConfig, DecisionLogOutput, RecordObserver};
use submilli_blueprint::{Blueprint, VarBindings};

use crate::error::ExecuteError;

mod program;
pub use program::{ProgramRun, run_program};

/// How a run reached the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEntry {
    /// `POST /v1/execute`.
    Http,
    /// `POST /v1/sessions/{id}/execute`.
    Session,
    /// The MCP execute tool.
    Mcp,
    /// An MCP file tool (`read_file`, `list_files`); its run is the one decision.
    McpFileTool { tool: String },
    /// [`run_program`], with the label its caller gave.
    Program,
}

/// What a run is, captured as it starts.
#[derive(Clone)]
pub struct RunStart {
    /// The execution's audit id; a stored response carries it too.
    pub execution_id: String,
    /// Who started it: the API token's name, or the label given to [`run_program`].
    /// Never the token itself.
    pub label: String,
    pub entry: RunEntry,
    /// The MCP client's name from its `initialize`, such as `langchain-mcp-adapters`.
    pub client: Option<String>,
    pub session_id: Option<String>,
    pub idempotency_key: Option<String>,
    pub blueprint_name: String,
    /// The blueprint the run is decided under, read once as it starts.
    pub blueprint: Arc<Blueprint>,
    /// The audit's hash of that blueprint.
    pub blueprint_hash: Option<String>,
    pub variables: Arc<VarBindings>,
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
    /// Whether the program reached the runner. `false` for a failure before it ran: a
    /// parse error, a package that would not resolve, a setup failure.
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
    /// From the run's start to its end.
    pub wall: Duration,
}

/// Records one run.
pub trait RunRecorder: Send + Sync {
    /// Caps for this run's decision and call records.
    fn log_config(&self) -> DecisionLogConfig {
        DecisionLogConfig::default()
    }

    /// Sees records as the run makes them, for streaming a run while it executes.
    fn observer(&self) -> Option<Arc<dyn RecordObserver>> {
        None
    }

    /// The run ended. Called once, from the task that owns the run, including when the
    /// client disconnected or the run timed out.
    fn finish(&self, run: FinishedRun);

    /// The size of the response its caller received: its result, error, and console
    /// output, as JSON. Arrives after [`finish`](Self::finish), and never for a caller
    /// that had gone.
    fn returned(&self, _bytes: u64) {}
}

/// Decides, for each run, whether and how it is recorded.
pub trait RunRecorderFactory: Send + Sync {
    /// A run is starting. `None` leaves it unrecorded.
    fn start(&self, run: RunStart) -> Option<Arc<dyn RunRecorder>>;

    /// An idempotent retry was answered without running.
    fn retried(&self, _retry: RetryLink) {}
}

/// A run being recorded: its recorder and when it started.
#[derive(Clone)]
pub(crate) struct Recording {
    pub recorder: Arc<dyn RunRecorder>,
    pub started: std::time::Instant,
}

impl Recording {
    pub(crate) fn start(
        factory: Option<&Arc<dyn RunRecorderFactory>>,
        run: RunStart,
    ) -> Option<Self> {
        let recorder = factory?.start(run)?;
        Some(Self {
            recorder,
            started: std::time::Instant::now(),
        })
    }

    /// Finishes a run that never reached the runner.
    pub(crate) fn undispatched(&self, error: &ExecuteError) {
        self.recorder.finish(FinishedRun {
            dispatched: false,
            error: Some(error.clone()),
            result: None,
            console: String::new(),
            usage: ExecutionUsage::default(),
            log: DecisionLogOutput::default(),
            wall: self.started.elapsed(),
        });
    }
}
