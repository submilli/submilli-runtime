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
    /// Insert or replace the record. Failure must not be reported as successful storage.
    async fn record(&self, session_id: &str, run: LastRun) -> Result<(), LastRunStoreError>;
    /// `Ok(None)` means no record; an unavailable store returns `Err`.
    async fn get(&self, session_id: &str) -> Result<Option<LastRun>, LastRunStoreError>;
}

/// Failure to retain or retrieve an execution's last-run record.
#[derive(Debug)]
pub enum LastRunStoreError {
    Poisoned,
    Backend(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for LastRunStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Poisoned => f.write_str("last-run storage mutex poisoned"),
            Self::Backend(error) => write!(f, "last-run storage backend failed: {error}"),
        }
    }
}

impl std::error::Error for LastRunStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Poisoned => None,
            Self::Backend(error) => Some(error.as_ref()),
        }
    }
}

#[derive(Default)]
pub struct InMemorySessionStore {
    inner: Mutex<HashMap<String, LastRun>>,
}

#[async_trait::async_trait]
impl SessionStore for InMemorySessionStore {
    async fn record(&self, session_id: &str, run: LastRun) -> Result<(), LastRunStoreError> {
        self.inner
            .lock()
            .map_err(|_| LastRunStoreError::Poisoned)?
            .insert(session_id.to_string(), run);
        Ok(())
    }

    async fn get(&self, session_id: &str) -> Result<Option<LastRun>, LastRunStoreError> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| LastRunStoreError::Poisoned)?
            .get(session_id)
            .cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn run(value: &str) -> LastRun {
        LastRun {
            result: Some(value.into()),
            console: vec!["captured output".into()],
            error: Some(ExecuteError {
                kind: crate::error::ErrorKind::RuntimeError,
                message: "original failure".into(),
                diagnostics: Vec::new(),
            }),
        }
    }

    #[tokio::test]
    async fn records_replace_and_preserve_all_fields() {
        let store = InMemorySessionStore::default();
        assert!(store.get("missing").await.unwrap().is_none());
        store.record("session", run("first")).await.unwrap();
        store.record("session", run("second")).await.unwrap();
        let stored = store.get("session").await.unwrap().unwrap();
        assert_eq!(stored.result.as_deref(), Some("second"));
        assert_eq!(stored.console, ["captured output"]);
        assert_eq!(stored.error.unwrap().message, "original failure");
    }

    #[tokio::test]
    async fn poisoned_storage_rejects_reads_and_writes_repeatedly() {
        let store = Arc::new(InMemorySessionStore::default());
        store.record("session", run("original")).await.unwrap();
        let poisoned = Arc::clone(&store);
        assert!(
            std::thread::spawn(move || {
                let _guard = poisoned.inner.lock().unwrap();
                panic!("injected last-run poison");
            })
            .join()
            .is_err()
        );
        for _ in 0..2 {
            assert!(matches!(
                store.get("session").await,
                Err(LastRunStoreError::Poisoned)
            ));
            assert!(matches!(
                store.get("missing").await,
                Err(LastRunStoreError::Poisoned)
            ));
            assert!(matches!(
                store.record("session", run("replacement")).await,
                Err(LastRunStoreError::Poisoned)
            ));
        }
        let guard = store.inner.lock().unwrap_err().into_inner();
        assert_eq!(
            guard.get("session").unwrap().result.as_deref(),
            Some("original")
        );
    }

    #[test]
    fn backend_error_retains_its_source() {
        use std::error::Error;
        let error = LastRunStoreError::Backend(Box::new(std::io::Error::other("backend detail")));
        assert_eq!(error.source().unwrap().to_string(), "backend detail");
    }
}
