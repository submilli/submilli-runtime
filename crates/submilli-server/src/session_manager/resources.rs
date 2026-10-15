//! Process-local handles; no authoritative session lifecycle state.
use super::HttpClientFactory;
use interpreter::runtime::HttpClient;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

struct State {
    http_clients: HashMap<String, Arc<dyn HttpClient>>,
}

/// At most this many sessions keep an HTTP client cached.
const MAX_CACHED_HTTP_CLIENTS: usize = 256;

pub(super) struct SessionResources {
    inner: Mutex<State>,
    http_client_factory: HttpClientFactory,
}
impl SessionResources {
    pub fn new(http_client_factory: HttpClientFactory) -> Self {
        Self {
            inner: Mutex::new(State {
                http_clients: HashMap::new(),
            }),
            http_client_factory,
        }
    }
    pub fn retain_active(&self, active_ids: &[String]) {
        let active: std::collections::HashSet<_> = active_ids.iter().map(String::as_str).collect();
        self.lock()
            .http_clients
            .retain(|id, _| active.contains(id.as_str()));
    }
    pub fn evict(&self, id: &str) {
        self.lock().http_clients.remove(id);
    }
    pub fn http_client(&self, session_id: &str) -> Arc<dyn HttpClient> {
        let mut state = self.lock();
        // Running requests retain their Arc even if its cache slot is reused.
        if state.http_clients.len() >= MAX_CACHED_HTTP_CLIENTS
            && !state.http_clients.contains_key(session_id)
            && let Some(other) = state.http_clients.keys().next().cloned()
        {
            state.http_clients.remove(&other);
        }
        state
            .http_clients
            .entry(session_id.to_owned())
            .or_insert_with(|| (self.http_client_factory)())
            .clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        // Poison may leave session bindings and their resource ownership partly
        // updated. AGENTS.md permits poisoned-lock panics, including during cleanup,
        // instead of recovery; it does not permit the panic that caused poisoning.
        self.inner
            .lock()
            .expect("session manager state lock poisoned")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn http_clients_are_lazy_reused_and_evicted() {
        let created = Arc::new(AtomicUsize::new(0));
        let count = created.clone();
        let resources = SessionResources::new(Arc::new(move || {
            count.fetch_add(1, Ordering::Relaxed);
            Arc::new(interpreter::runtime::ReqwestHttpClient::new(Arc::default()))
        }));
        assert_eq!(created.load(Ordering::Relaxed), 0);
        let first = resources.http_client("one");
        assert!(Arc::ptr_eq(&first, &resources.http_client("one")));
        assert!(!Arc::ptr_eq(&first, &resources.http_client("two")));
        assert_eq!(created.load(Ordering::Relaxed), 2);
        resources.evict("one");
        assert!(!Arc::ptr_eq(&first, &resources.http_client("one")));
        assert_eq!(created.load(Ordering::Relaxed), 3);
    }
}
