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
    fn database(&self) -> Option<Arc<crate::database::ServerDatabase>> {
        self.inner.database()
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

struct GatedUnits {
    inner: Arc<dyn crate::application::unit_of_work::UnitOfWorkFactory>,
    admitted: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}
#[async_trait::async_trait]
impl crate::application::unit_of_work::UnitOfWorkFactory for GatedUnits {
    async fn begin(
        &self,
    ) -> Result<Box<dyn crate::application::unit_of_work::UnitOfWork>, StoreError> {
        self.admitted.notify_one();
        self.release.notified().await;
        self.inner.begin().await
    }
}

#[tokio::test]
async fn blueprint_mutations_finish_after_request_cancellation() {
    for cancel in [false, true] {
        check_mutations(cancel).await;
    }
}

async fn check_mutations(cancel: bool) {
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
        store.inner.migrate().await.unwrap();
        if !creating {
            store
                .inner
                .add(submilli_blueprint::parse("name: demo").unwrap())
                .await
                .unwrap();
        }
        let audit_path = directory.path().join("audit.log");
        let token = "test_admin_token_012345678901234567890";
        let mut state = AppState::new(ServerConfig {
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
        .await
        .unwrap();
        if !creating {
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
        }
        if deleting {
            let inner = Arc::get_mut(&mut state.inner).unwrap();
            inner.unit_of_work = Arc::new(GatedUnits {
                inner: inner.unit_of_work.clone(),
                admitted: admitted.clone(),
                release: release.clone(),
            });
        }
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
        if cancel {
            request.abort();
            assert!(request.await.unwrap_err().is_cancelled());
            release.notify_one();
        } else {
            release.notify_one();
            let response = request.await.unwrap().unwrap();
            assert_eq!(response.status(), axum::http::StatusCode::OK);
        }
        let tasks = state.graceful_shutdown();
        tasks.close();
        tokio::time::timeout(Duration::from_secs(5), tasks.wait())
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
            assert!(!state.session_manager().contains("session").await.unwrap());
        } else if creating {
            assert!(mutation.contains("event=blueprint_created"), "{mutation}");
        } else {
            assert!(mutation.contains("event=blueprint_replaced"), "{mutation}");
        }
        database.close().await.unwrap();
    }
}

#[tokio::test]
async fn a_failed_http_write_keeps_the_version_tag() {
    let state = AppState::new(ServerConfig::default()).await.unwrap();
    state
        .apply_local_blueprint("name: demo\n", "v1")
        .await
        .unwrap();
    state.database().unwrap().transaction(|connection| Box::pin(async move {
        sqlx::query("CREATE TRIGGER reject_update BEFORE UPDATE ON blueprints BEGIN SELECT RAISE(FAIL, 'injected'); END").execute(&mut *connection).await?;
        sqlx::query("CREATE TRIGGER reject_delete BEFORE DELETE ON blueprints BEGIN SELECT RAISE(FAIL, 'injected'); END").execute(connection).await?;
        Ok(())
    })).await.unwrap();
    let router = app(state.clone());
    for (method, body) in [
        (
            "PUT",
            serde_json::json!({ "yaml": "name: demo\ndefault: allow\n" }).to_string(),
        ),
        ("DELETE", String::new()),
    ] {
        let response = router
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method(method)
                    .uri("/v1/blueprints/demo")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "{method}"
        );
        let found = state.blueprint_for_run("demo").await.unwrap().unwrap();
        assert_eq!(found.version_tag.as_deref(), Some("v1"), "{method}");
    }
}
