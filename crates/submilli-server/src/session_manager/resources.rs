//! Process-local handles; no authoritative session lifecycle state.
use super::{HttpClientFactory, SessionKvSettings};
use interpreter::runtime::{HttpClient, SessionKvStore};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct SessionEntry {
    http_client: Option<Arc<dyn HttpClient>>,
    session_kv: Option<Arc<dyn SessionKvStore>>,
}

struct State {
    sessions: HashMap<String, SessionEntry>,
}

pub(super) struct SessionResources {
    inner: Mutex<State>,
    http_client_factory: HttpClientFactory,
    session_kv: SessionKvSettings,
}
impl SessionResources {
    pub fn new(http_client_factory: HttpClientFactory, session_kv: SessionKvSettings) -> Self {
        Self {
            inner: Mutex::new(State {
                sessions: HashMap::new(),
            }),
            http_client_factory,
            session_kv,
        }
    }
    pub fn retain_active(&self, active_ids: &[String]) {
        let active: std::collections::HashSet<_> = active_ids.iter().map(String::as_str).collect();
        self.lock()
            .sessions
            .retain(|id, _| active.contains(id.as_str()));
    }
    pub fn evict(&self, id: &str) {
        self.lock().sessions.remove(id);
    }
    pub fn http_client(&self, session_id: &str) -> Arc<dyn HttpClient> {
        let mut state = self.lock();
        // Running requests retain their Arc even if its cache slot is reused.
        if state
            .sessions
            .values()
            .filter(|entry| entry.http_client.is_some())
            .count()
            >= 256
            && state
                .sessions
                .get(session_id)
                .is_none_or(|entry| entry.http_client.is_none())
            && let Some((_, entry)) = state
                .sessions
                .iter_mut()
                .find(|(id, entry)| id.as_str() != session_id && entry.http_client.is_some())
        {
            entry.http_client = None;
        }
        state
            .sessions
            .retain(|_, entry| entry.http_client.is_some() || entry.session_kv.is_some());
        state
            .sessions
            .entry(session_id.to_owned())
            .or_default()
            .http_client
            .get_or_insert_with(|| (self.http_client_factory)())
            .clone()
    }

    pub fn session_kv_for_execute(&self, session_id: &str) -> Arc<dyn SessionKvStore> {
        self.lock()
            .sessions
            .entry(session_id.to_owned())
            .or_default()
            .session_kv
            .get_or_insert_with(|| self.session_kv.build())
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
        let resources = SessionResources::new(
            Arc::new(move || {
                count.fetch_add(1, Ordering::Relaxed);
                Arc::new(interpreter::runtime::ReqwestHttpClient::new(Arc::default()))
            }),
            SessionKvSettings::default(),
        );
        let kv = resources.session_kv_for_execute("one");
        assert_eq!(created.load(Ordering::Relaxed), 0);
        let first = resources.http_client("one");
        assert!(Arc::ptr_eq(&first, &resources.http_client("one")));
        assert!(!Arc::ptr_eq(&first, &resources.http_client("two")));
        assert_eq!(created.load(Ordering::Relaxed), 2);
        assert!(Arc::ptr_eq(&kv, &resources.session_kv_for_execute("one")));
        resources.evict("one");
        assert!(!Arc::ptr_eq(&first, &resources.http_client("one")));
        assert!(!Arc::ptr_eq(&kv, &resources.session_kv_for_execute("one")));
        assert_eq!(created.load(Ordering::Relaxed), 3);
    }
}
