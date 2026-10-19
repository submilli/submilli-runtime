//! The most recent run of each session, as `GET /v1/sessions/{id}/last-run`
//! answers with it.

// Compatibility path for callers of the earlier session domain module.
pub use crate::domain::session as domain;

use serde::Serialize;

pub(crate) use crate::adapters::session::last_run::StoredLastRun;
use crate::error::ExecuteError;

/// A run's `{result, console, error}`, with the full console.
#[derive(Debug, Clone, Serialize)]
pub struct LastRun {
    pub result: Option<String>,
    pub console: Vec<String>,
    pub error: Option<ExecuteError>,
}

impl LastRun {
    /// The JSON a reader gets, byte for byte.
    pub(crate) fn encode(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }
}
