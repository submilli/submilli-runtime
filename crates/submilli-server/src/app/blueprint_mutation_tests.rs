use super::*;
use crate::blueprint::{SqliteBlueprintStore, StoreError, StoredBlueprint};
use tower::ServiceExt;

struct GatedStore {
    inner: SqliteBlueprintStore,
    admitted: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

#[async_trait::async_trait]
impl BlueprintStore for GatedStore {
    async fn initialize(&self) -> Result<(), StoreError> {
        self.inner.initialize().await
    }
    async fn add_yaml(&self, value: StoredBlueprint) -> Result<(), StoreError> {
        self.admitted.notify_one();
        self.release.notified().await;
        self.inner.add_yaml(value).await
    }
    async fn upsert_yaml(&self, value: StoredBlueprint) -> Result<bool, StoreError> {
        self.admitted.notify_one();
        self.release.notified().await;
        self.inner.upsert_yaml(value).await
    }
    async fn remove(&self, name: &str) -> Result<bool, StoreError> {
        self.admitted.notify_one();
        self.release.notified().await;
        self.inner.remove(name).await
    }
    async fn get(&self, name: &str) -> Result<Option<Blueprint>, StoreError> {
        self.inner.get(name).await
    }
    async fn get_yaml(&self, name: &str) -> Result<Option<String>, StoreError> {
        self.inner.get_yaml(name).await
    }
    async fn list(&self) -> Result<Vec<String>, StoreError> {
        self.inner.list().await
    }
    async fn list_blueprints(&self) -> Result<Vec<Blueprint>, StoreError> {
        self.inner.list_blueprints().await
    }
}

#[tokio::test]
async fn cancelled_blueprint_mutations_complete_audit_and_postcommit_work() {
    for (creating, deleting) in [(true, false), (false, false), (false, true)] {
        let directory = tempfile::tempdir().unwrap();
        let database = Arc::new(
            crate::database::ServerDatabase::open(&directory.path().join("db"))
                .await
                .unwrap(),
        );
        let admitted = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let store = Arc::new(GatedStore {
            inner: SqliteBlueprintStore::new(database.clone(), None),
            admitted: admitted.clone(),
            release: release.clone(),
        });
        store.initialize().await.unwrap();
        if !creating {
            store
                .inner
                .add(submilli_blueprint::parse("name: demo").unwrap())
                .await
                .unwrap();
        }
        let audit_path = directory.path().join("audit.log");
        let token = "test_admin_token_012345678901234567890";
        let state = AppState::new(ServerConfig {
            auth: crate::auth::AuthConfig::Tokens(vec![
                crate::auth::ApiToken::new("operator", crate::auth::Role::Admin, token).unwrap(),
            ]),
            audit: crate::audit::AuditConfig {
                file: Some(audit_path.clone()),
                ..Default::default()
            },
            database: Some(database.clone()),
            blueprints: Some(store),
            ..Default::default()
        })
        .unwrap();
        state
            .session_manager()
            .bind(
                "session",
                &submilli_blueprint::parse("name: demo").unwrap(),
                Arc::new(Default::default()),
                Arc::new(Default::default()),
            )
            .await
            .unwrap();
        let generation = state.inner.mcp_catalog_generation.load(Ordering::Acquire);
        let router = app(state.clone());
        let request = tokio::spawn(async move {
            let body = if deleting {
                String::new()
            } else {
                serde_json::json!({"yaml": "# new\nname: demo"}).to_string()
            };
            router
                .oneshot(
                    axum::http::Request::builder()
                        .method(if creating {
                            "POST"
                        } else if deleting {
                            "DELETE"
                        } else {
                            "PUT"
                        })
                        .uri(if creating {
                            "/v1/blueprints"
                        } else {
                            "/v1/blueprints/demo"
                        })
                        .header("authorization", format!("Bearer {token}"))
                        .header("content-type", "application/json")
                        .body(axum::body::Body::from(body))
                        .unwrap(),
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(5), admitted.notified())
            .await
            .unwrap();
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        release.notify_one();
        let mutations = state.blueprint_mutations();
        mutations.close();
        tokio::time::timeout(Duration::from_secs(5), mutations.wait())
            .await
            .unwrap();
        if !creating {
            assert!(state.inner.mcp_catalog_generation.load(Ordering::Acquire) > generation);
        }
        let yaml = state.blueprints().get_yaml("demo").await.unwrap();
        if deleting {
            assert!(yaml.is_none());
        } else {
            assert_eq!(yaml.as_deref(), Some("# new\nname: demo"));
        }
        let audit = std::fs::read_to_string(&audit_path).unwrap();
        let mutation = audit
            .lines()
            .find(|line| line.contains("type=admin"))
            .unwrap();
        assert!(mutation.contains("principal=operator"), "{mutation}");
        assert!(mutation.contains("outcome=ok"), "{mutation}");
        if deleting {
            let eviction = audit
                .lines()
                .find(|line| line.contains("event=evicted"))
                .unwrap();
            assert!(eviction.contains("principal=operator"), "{eviction}");
            assert!(!state.session_manager().contains("session"));
        } else if creating {
            assert!(mutation.contains("event=blueprint_created"), "{mutation}");
        } else {
            assert!(mutation.contains("event=blueprint_replaced"), "{mutation}");
        }
        database.close().await.unwrap();
    }
}
