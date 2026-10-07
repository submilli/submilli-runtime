//! What the store keeps of one run, and the summary line that lists it.

use std::collections::BTreeMap;

use interpreter::runtime::limits::ExecutionUsage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use submilli_server::error::ErrorKind;
use submilli_server::record::{FinishedRun, RecordedRun, RunEntry, RunStart};

use super::FORMAT;

/// One run as the store keeps it: who ran it and how, how it ended, and its recording,
/// which loads back as the [`RecordedRun`] a test run takes.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct StoredRun {
    pub(crate) format: u32,
    /// The playground's run id: sequential from 1, never reused.
    pub(crate) id: u64,
    /// The API token's name (`app`, `stand-in`, ...) or the playground's own label
    /// (see [`labels`](crate::commands::playground::labels)).
    pub(crate) label: String,
    /// How the run reached the server: `http`, `session`, `mcp`, `mcp:<tool>`,
    /// `program`, or `test`.
    pub(crate) entry: String,
    /// The MCP client's name, such as `langchain-mcp-adapters`.
    pub(crate) client: Option<String>,
    pub(crate) tool_call_id: Option<String>,
    pub(crate) idempotency_key: Option<String>,
    /// For a test run, the run it tested.
    pub(crate) test_of: Option<RunLink>,
    /// Microseconds since the Unix epoch when the run started.
    pub(crate) started_at_micros: u64,
    pub(crate) wall_ms: u64,
    /// Whether the program reached the runner.
    pub(crate) dispatched: bool,
    /// `None` when the program completed.
    pub(crate) error: Option<StoredError>,
    pub(crate) result: Option<String>,
    pub(crate) console: String,
    pub(crate) usage: ExecutionUsage,
    /// Decisions the recorder did not keep at all.
    pub(crate) decisions_dropped: u64,
    /// Calls the recorder did not keep at all.
    pub(crate) calls_dropped: u64,
    /// The session, variables, code, decisions, and calls: what a re-check and a test
    /// run read.
    pub(crate) recording: RecordedRun,
}

/// A link from one run to another: the store's id when that run is stored, and the
/// server's execution id in any case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RunLink {
    pub(crate) run: Option<u64>,
    pub(crate) execution_id: String,
}

/// How a run failed, as its response said. The shape of the server's `ExecuteError`,
/// whose denial fields sit beside `kind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct StoredError {
    pub(crate) kind: ErrorKind,
    pub(crate) message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) diagnostics: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) caller: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) capability: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<String>,
}

impl StoredRun {
    /// The run `start` describes, as it ended in `finished`. Not yet redacted.
    pub(crate) fn from_parts(
        id: u64,
        start: &RunStart,
        finished: &FinishedRun,
        test_of: Option<RunLink>,
        started_at_micros: u64,
    ) -> Self {
        Self {
            format: FORMAT,
            id,
            label: start.label.clone(),
            entry: entry_name(&start.entry),
            client: start.client.clone(),
            tool_call_id: start.tool_call_id.clone(),
            idempotency_key: start.idempotency_key.clone(),
            test_of,
            started_at_micros,
            wall_ms: u64::try_from(finished.wall.as_millis()).unwrap_or(u64::MAX),
            dispatched: finished.dispatched,
            error: finished.error.as_ref().map(|error| {
                serde_json::to_value(error)
                    .ok()
                    .and_then(|value| serde_json::from_value(value).ok())
                    .unwrap_or_else(|| StoredError {
                        kind: error.kind,
                        message: error.message.clone(),
                        diagnostics: Vec::new(),
                        caller: None,
                        capability: None,
                        source: None,
                    })
            }),
            result: finished.result.clone(),
            console: finished.console.clone(),
            usage: finished.usage,
            decisions_dropped: finished.log.dropped,
            calls_dropped: finished.log.calls_dropped,
            recording: RecordedRun::from_parts(start, finished),
        }
    }

    /// The `n`th decision, counting from 1, as `<run>.<n>` names it.
    pub(crate) fn decision(&self, n: usize) -> Option<&interpreter::runtime::DecisionRecord> {
        self.recording.decisions.get(n.checked_sub(1)?)
    }
}

/// How a run's entry reads in the store and its events.
pub(crate) fn entry_name(entry: &RunEntry) -> String {
    match entry {
        RunEntry::Http => "http".into(),
        RunEntry::Session => "session".into(),
        RunEntry::Mcp => "mcp".into(),
        RunEntry::McpFileTool { tool } => format!("mcp:{tool}"),
        RunEntry::Program => "program".into(),
        RunEntry::Test => "test".into(),
    }
}

/// One line of the index: what listing runs and sessions needs, without the recording.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct RunSummary {
    pub(crate) format: u32,
    pub(crate) id: u64,
    pub(crate) execution_id: String,
    pub(crate) label: String,
    pub(crate) entry: String,
    pub(crate) session_id: Option<String>,
    pub(crate) variables: BTreeMap<String, String>,
    pub(crate) blueprint: String,
    /// The blueprint version the run was decided under: the playground's version
    /// number, or the blueprint's hash for a run recorded before versions were kept.
    #[serde(default)]
    pub(crate) blueprint_version: Option<String>,
    pub(crate) started_at_micros: u64,
    pub(crate) wall_ms: u64,
    pub(crate) dispatched: bool,
    pub(crate) error: Option<ErrorKind>,
    pub(crate) decisions: usize,
    pub(crate) denied: usize,
    pub(crate) test_of: Option<RunLink>,
}

impl RunSummary {
    pub(crate) fn of(run: &StoredRun) -> Self {
        Self {
            format: FORMAT,
            id: run.id,
            execution_id: run.recording.execution_id.clone(),
            label: run.label.clone(),
            entry: run.entry.clone(),
            session_id: run.recording.session_id.clone(),
            variables: run.recording.variables.clone(),
            blueprint: run.recording.blueprint_name.clone(),
            blueprint_version: run.recording.blueprint_version.clone(),
            started_at_micros: run.started_at_micros,
            wall_ms: run.wall_ms,
            dispatched: run.dispatched,
            error: run.error.as_ref().map(|error| error.kind),
            decisions: run.recording.decisions.len(),
            denied: run
                .recording
                .decisions
                .iter()
                .filter(|decision| !decision.allowed && !decision.filtered)
                .count(),
            test_of: run.test_of.clone(),
        }
    }
}

/// An idempotent retry answered from the ledger, linked to the run it repeated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RetryRecord {
    pub(crate) format: u32,
    pub(crate) at_micros: u64,
    pub(crate) session_id: String,
    pub(crate) idempotency_key: String,
    pub(crate) original: Option<RunLink>,
}

/// A decision named as `<run>.<n>`: the run's id and the decision's position, from 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct DecisionRef {
    pub(crate) run: u64,
    pub(crate) n: usize,
}

impl std::fmt::Display for DecisionRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.run, self.n)
    }
}

impl std::str::FromStr for DecisionRef {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid =
            || format!("`{text}` is not a decision; decisions read `<run>.<n>`, like `12.3`");
        let (run, n) = text.split_once('.').ok_or_else(invalid)?;
        let run = run.parse::<u64>().map_err(|_| invalid())?;
        let n = n.parse::<usize>().map_err(|_| invalid())?;
        if run == 0 || n == 0 {
            return Err(invalid());
        }
        Ok(Self { run, n })
    }
}
