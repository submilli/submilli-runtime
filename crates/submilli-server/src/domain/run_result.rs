//! A completed run's output, independently retained from its lifecycle.
use super::execution_error::ExecuteError;
use interpreter::runtime::durable::RunId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunOutput {
    pub result: Option<String>,
    pub console: Vec<String>,
    pub error: Option<ExecuteError>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub discovery_warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunResult {
    run_id: RunId,
    output: RunOutput,
}

impl RunResult {
    pub(crate) fn new(run_id: RunId, output: RunOutput) -> Self {
        Self { run_id, output }
    }

    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    pub fn output(&self) -> &RunOutput {
        &self.output
    }
}
