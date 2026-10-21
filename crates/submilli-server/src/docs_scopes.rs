//! The documentation scopes discovery serves, built once per blueprint
//! revision and set of package artifacts, and shared by every request that
//! reads them.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use submilli_blueprint::Blueprint;
use submilli_build::Artifact;
use submilli_shared::docs_scope::DocsScope;
use tokio::sync::OnceCell;

/// Why a scope could not be read.
#[derive(Debug)]
pub(crate) enum DocsScopeError {
    /// The blueprint store failed.
    Store(crate::application::error::StoreError),
    /// Building the scope did not finish.
    Build(tokio::task::JoinError),
}

impl std::fmt::Display for DocsScopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DocsScopeError::Store(error) => write!(f, "{error}"),
            DocsScopeError::Build(error) => write!(f, "building the documentation failed: {error}"),
        }
    }
}

impl DocsScopeError {
    /// The response a REST route answers with.
    pub(crate) fn response(self) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
        match self {
            DocsScopeError::Store(error) => crate::blueprint::store_failure_response(error),
            DocsScopeError::Build(error) => {
                tracing::error!(error = ?error, "building documentation failed");
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(serde_json::json!({
                        "error": "internal_error",
                        "message": "building the documentation failed",
                    })),
                )
            }
        }
    }

    /// The error an MCP tool answers with.
    pub(crate) fn tool_error(self) -> rmcp::ErrorData {
        match self {
            DocsScopeError::Store(error) => rmcp::ErrorData::internal_error(
                crate::blueprint::store_failure_message(error),
                None,
            ),
            DocsScopeError::Build(error) => {
                tracing::error!(error = ?error, "building documentation failed");
                rmcp::ErrorData::internal_error("building the documentation failed", None)
            }
        }
    }
}

/// Scopes kept at once. Each holds a narrowed copy of the standard library
/// and its blueprint's packages, so the bound keeps memory in check when many
/// blueprints are read. A changed blueprint or package store evicts its
/// scopes; the key already keeps a stale one from being served.
const MAX_SCOPES: usize = 64;

/// What a scope is built from. A package's documentation depends on its own
/// declaration, capabilities and readme, which its fingerprint covers; its
/// dependencies' declarations never enter it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ScopeKey {
    blueprint: String,
    revision: u64,
    /// Name, version and fingerprint of each loaded declared package.
    packages: Vec<(String, String, [u8; 32])>,
}

struct Slot {
    cell: Arc<OnceCell<Arc<DocsScope>>>,
    /// When the slot was created, for evicting the oldest first.
    created: u64,
}

#[derive(Default)]
struct Slots {
    slots: BTreeMap<ScopeKey, Slot>,
    created: u64,
}

/// Concurrent first requests for one key build its scope once; a failed build
/// is not kept, so the next request tries again.
#[derive(Default)]
pub(crate) struct DocsScopeCache {
    // Poison means a panic interrupted an insert or eviction. AGENTS.md permits
    // panicking on poisoned access; the slot map holds no partial scope, since
    // building happens outside the lock.
    slots: Mutex<Slots>,
}

impl DocsScopeCache {
    /// The scope of `blueprint` at `revision` over `artifacts`, built on a
    /// blocking thread when no request has built it yet.
    pub(crate) async fn get_or_build(
        &self,
        blueprint: &Blueprint,
        revision: u64,
        artifacts: Vec<Artifact>,
    ) -> Result<Arc<DocsScope>, tokio::task::JoinError> {
        let key = ScopeKey {
            blueprint: blueprint.name.clone(),
            revision,
            packages: artifacts
                .iter()
                .map(|artifact| {
                    (
                        artifact.metadata.package_name.clone(),
                        artifact.metadata.package_version.clone(),
                        artifact.fingerprint,
                    )
                })
                .collect(),
        };
        let blueprint = blueprint.clone();
        self.get_or_build_with(key, move || {
            DocsScope::for_blueprint(&blueprint, &artifacts)
        })
        .await
    }

    async fn get_or_build_with(
        &self,
        key: ScopeKey,
        build: impl FnOnce() -> DocsScope + Send + 'static,
    ) -> Result<Arc<DocsScope>, tokio::task::JoinError> {
        let cell = self.cell(&key);
        let built = cell
            .get_or_try_init(|| async move {
                tokio::task::spawn_blocking(move || Arc::new(build())).await
            })
            .await
            .cloned();
        if built.is_err() {
            self.forget_failed(&key, &cell);
        }
        built
    }

    /// Drop every scope of the blueprint `name`, after it changes or goes.
    pub(crate) fn evict_blueprint(&self, name: &str) {
        self.lock().slots.retain(|key, _| key.blueprint != name);
    }

    /// Drop every scope, after installed packages change.
    pub(crate) fn evict_all(&self) {
        self.lock().slots.clear();
    }

    fn cell(&self, key: &ScopeKey) -> Arc<OnceCell<Arc<DocsScope>>> {
        let mut slots = self.lock();
        if let Some(slot) = slots.slots.get(key) {
            return Arc::clone(&slot.cell);
        }
        while slots.slots.len() >= MAX_SCOPES {
            let Some(oldest) = slots
                .slots
                .iter()
                .min_by_key(|(_, slot)| slot.created)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            slots.slots.remove(&oldest);
        }
        slots.created = slots.created.wrapping_add(1);
        let cell = Arc::new(OnceCell::new());
        let created = slots.created;
        slots.slots.insert(
            key.clone(),
            Slot {
                cell: Arc::clone(&cell),
                created,
            },
        );
        cell
    }

    fn forget_failed(&self, key: &ScopeKey, cell: &Arc<OnceCell<Arc<DocsScope>>>) {
        let mut slots = self.lock();
        if slots
            .slots
            .get(key)
            .is_some_and(|slot| Arc::ptr_eq(&slot.cell, cell) && !slot.cell.initialized())
        {
            slots.slots.remove(key);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Slots> {
        self.slots.lock().expect("docs scope cache lock poisoned")
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().slots.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blueprint(name: &str) -> Blueprint {
        submilli_blueprint::parse(&format!(
            "name: {name}\ndefault: deny\npermissions:\n  main:\n    - capability: fs.read\n      action: allow\n"
        ))
        .expect("blueprint parses")
    }

    #[tokio::test]
    async fn concurrent_requests_share_one_scope() {
        let cache = DocsScopeCache::default();
        let bp = blueprint("a");
        let (first, second) = tokio::join!(
            cache.get_or_build(&bp, 1, Vec::new()),
            cache.get_or_build(&bp, 1, Vec::new())
        );
        assert!(Arc::ptr_eq(&first.unwrap(), &second.unwrap()));
        assert_eq!(cache.len(), 1);
    }

    #[tokio::test]
    async fn blueprints_and_revisions_are_kept_apart() {
        let cache = DocsScopeCache::default();
        let a = cache
            .get_or_build(&blueprint("a"), 1, Vec::new())
            .await
            .unwrap();
        let b = cache
            .get_or_build(&blueprint("b"), 1, Vec::new())
            .await
            .unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
        let a2 = cache
            .get_or_build(&blueprint("a"), 2, Vec::new())
            .await
            .unwrap();
        assert!(!Arc::ptr_eq(&a, &a2));
        assert_eq!(cache.len(), 3);
        cache.evict_blueprint("a");
        assert_eq!(cache.len(), 1);
        cache.evict_all();
        assert_eq!(cache.len(), 0);
    }

    fn key(packages: Vec<(String, String, [u8; 32])>) -> ScopeKey {
        ScopeKey {
            blueprint: "a".into(),
            revision: 1,
            packages,
        }
    }

    #[tokio::test]
    async fn a_failed_build_is_retried() {
        let cache = DocsScopeCache::default();
        let failed = cache
            .get_or_build_with(key(Vec::new()), || panic!("build failed"))
            .await;
        assert!(failed.is_err());
        assert_eq!(cache.len(), 0);
        let built = cache
            .get_or_build_with(key(Vec::new()), DocsScope::unscoped)
            .await;
        assert!(built.is_ok());
    }

    #[tokio::test]
    async fn a_rebuilt_artifact_under_the_same_version_is_another_scope() {
        let cache = DocsScopeCache::default();
        let package = |fingerprint| vec![("@a/p".to_string(), "1.0.0".to_string(), fingerprint)];
        let first = cache
            .get_or_build_with(key(package([1; 32])), DocsScope::unscoped)
            .await
            .unwrap();
        let rebuilt = cache
            .get_or_build_with(key(package([2; 32])), DocsScope::unscoped)
            .await
            .unwrap();
        assert!(!Arc::ptr_eq(&first, &rebuilt));
    }

    #[tokio::test]
    async fn the_cache_is_bounded() {
        let cache = DocsScopeCache::default();
        for index in 0..MAX_SCOPES + 3 {
            cache
                .get_or_build(&blueprint(&format!("b{index}")), 1, Vec::new())
                .await
                .unwrap();
        }
        assert_eq!(cache.len(), MAX_SCOPES);
    }
}
