use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use submilli_server::session::{InMemorySessionStore, LastRun, LastRunStoreError, SessionStore};

/// Injects independent read/write failures without corrupting the backing store.
#[derive(Default)]
pub struct FaultStore {
    pub fail_reads: AtomicBool,
    pub fail_writes: AtomicBool,
    pub writes: AtomicUsize,
    inner: InMemorySessionStore,
}

#[async_trait::async_trait]
impl SessionStore for FaultStore {
    async fn record(&self, session_id: &str, run: LastRun) -> Result<(), LastRunStoreError> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        if self.fail_writes.load(Ordering::SeqCst) {
            return Err(backend_failure());
        }
        self.inner.record(session_id, run).await
    }

    async fn get(&self, session_id: &str) -> Result<Option<LastRun>, LastRunStoreError> {
        if self.fail_reads.load(Ordering::SeqCst) {
            return Err(backend_failure());
        }
        self.inner.get(session_id).await
    }
}

fn backend_failure() -> LastRunStoreError {
    LastRunStoreError::Backend(Box::new(std::io::Error::other("private backend detail")))
}
