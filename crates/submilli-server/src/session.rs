//! Per-session retention of the most recent `/v1/execute` run.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::ExecuteError;

#[derive(Debug, Clone)]
pub struct LastRun {
    pub result: Option<String>,
    pub console: Vec<String>,
    pub error: Option<ExecuteError>,
}

/// Async so a DB-backed implementation can be added without reshaping call sites.
#[async_trait::async_trait]
pub trait SessionStore: Send + Sync + 'static {
    async fn record(&self, session_id: &str, run: LastRun);
    async fn get(&self, session_id: &str) -> Option<LastRun>;
}

#[derive(Default)]
pub struct InMemorySessionStore {
    inner: Mutex<HashMap<String, LastRun>>,
}

#[async_trait::async_trait]
impl SessionStore for InMemorySessionStore {
    async fn record(&self, session_id: &str, run: LastRun) {
        self.inner
            .lock()
            .expect("session store mutex poisoned")
            .insert(session_id.to_string(), run);
    }

    async fn get(&self, session_id: &str) -> Option<LastRun> {
        self.inner
            .lock()
            .expect("session store mutex poisoned")
            .get(session_id)
            .cloned()
    }
}
