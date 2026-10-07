use crate::domain::session::{ClosedReason, SessionStatus as DomainStatus};
use crate::session_store::SessionStatus;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::app::AppState;
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::blueprint::{BlueprintStore, SqliteBlueprintStore, StoredBlueprint};
use crate::config::ServerConfig;
use crate::database::ServerDatabase;
use crate::idempotency_store::{
    IdempotencyStore, InMemoryIdempotencyStore, LedgerEntry, code_fingerprint,
};
use crate::session_store::{DurableSessionStore, RootVfsType, SessionRecord, SqliteSessionStore};

struct Fixture {
    state: AppState,
    database: Arc<ServerDatabase>,
    sessions: Arc<SqliteSessionStore>,
    ledger: Arc<InMemoryIdempotencyStore>,
    root: tempfile::TempDir,
}

impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let database = Arc::new(
            ServerDatabase::open(&root.path().join("server.db"))
                .await
                .unwrap(),
        );
        let blueprints = Arc::new(SqliteBlueprintStore::new(database.clone(), None));
        let yaml = "name: test\ndefault: deny\nvfs: per_session\n";
        blueprints
            .add_yaml(StoredBlueprint::new(
                submilli_blueprint::parse(yaml).unwrap(),
                yaml.into(),
            ))
            .await
            .unwrap();
        let workspaces = root.path().join("workspaces");
        let sessions = Arc::new(SqliteSessionStore::new(
            database.clone(),
            None,
            workspaces.clone(),
        ));
        let ledger = Arc::new(InMemoryIdempotencyStore::default());
        let state = AppState::new(ServerConfig {
            database: Some(database.clone()),
            blueprints: Some(blueprints),
            session_store: Some(sessions.clone()),
            idempotency_store: Some(ledger.clone()),
            session_storage_root: Some(workspaces),
            ..Default::default()
        })
        .unwrap();
        Self {
            state,
            database,
            sessions,
            ledger,
            root,
        }
    }

    async fn session(&self, id: &str) {
        let path = self.root.path().join("workspaces").join(id);
        std::fs::create_dir_all(&path).unwrap();
        self.sessions
            .put(SessionRecord {
                session_id: id.into(),
                blueprint_name: "test".into(),
                root_vfs_type: RootVfsType::PerSession,
                root_vfs_path: Some(path),
                idle_timeout: Duration::from_secs(3600),
                last_activity: SystemTime::now(),
                ..Default::default()
            })
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn removal_closes_all_sessions_after_execution_starts() {
    let fixture = Fixture::new().await;
    fixture.session("one").await;
    fixture.session("two").await;
    fixture
        .state
        .session_manager()
        .start_execution("two")
        .await
        .unwrap();
    assert!(fixture.state.remove_blueprint("test").await.unwrap());
    assert!(
        fixture
            .sessions
            .load_all()
            .await
            .unwrap()
            .iter()
            .all(|r| r.status == SessionStatus::Closed
                && r.closed_reason == Some(ClosedReason::BlueprintRemoved))
    );
    assert!(fixture.root.path().join("workspaces/one").exists());
    assert!(fixture.root.path().join("workspaces/two").exists());
    assert_eq!(fixture.sessions.pending_cleanup().await.unwrap().len(), 2);
    fixture.state.session_manager().reap_now().await.unwrap();
    assert!(!fixture.root.path().join("workspaces/one").exists());
    assert!(!fixture.root.path().join("workspaces/two").exists());
    assert!(fixture.sessions.pending_cleanup().await.unwrap().is_empty());
    assert!(!fixture.state.remove_blueprint("test").await.unwrap());
    fixture.database.close().await.unwrap();
}

#[tokio::test]
async fn failed_session_write_rolls_back_blueprint_and_cleanup_changes() {
    let fixture = Fixture::new().await;
    fixture.session("one").await;
    fixture.session("two").await;
    fixture.database.transaction(|connection| Box::pin(async move {
        sqlx::query("CREATE TRIGGER fail_close BEFORE UPDATE ON sessions WHEN NEW.session_id='two' BEGIN SELECT RAISE(ABORT, 'injected failure'); END")
            .execute(connection).await?;
        Ok(())
    })).await.unwrap();
    assert!(fixture.state.remove_blueprint("test").await.is_err());
    assert!(
        fixture
            .state
            .blueprints()
            .get("test")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        fixture
            .sessions
            .load_all()
            .await
            .unwrap()
            .iter()
            .all(|r| r.status == SessionStatus::Active)
    );
    assert!(fixture.sessions.pending_cleanup().await.unwrap().is_empty());
    assert!(fixture.root.path().join("workspaces/one").exists());
    fixture
        .database
        .transaction(|connection| {
            Box::pin(async move {
                sqlx::query("DROP TRIGGER fail_close")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    assert!(fixture.state.remove_blueprint("test").await.unwrap());
    fixture.database.close().await.unwrap();
}

#[tokio::test]
async fn cleanup_failure_keeps_committed_closure_and_retry_obligation() {
    let fixture = Fixture::new().await;
    fixture.session("one").await;
    let path = fixture.root.path().join("workspaces/one");
    std::fs::remove_dir(&path).unwrap();
    std::fs::write(&path, "blocks folder cleanup").unwrap();
    assert!(fixture.state.remove_blueprint("test").await.unwrap());
    assert!(
        fixture
            .state
            .blueprints()
            .get("test")
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        fixture
            .sessions
            .load("one")
            .await
            .unwrap()
            .unwrap()
            .closed_reason,
        Some(ClosedReason::BlueprintRemoved)
    );
    assert!(fixture.state.session_manager().reap_now().await.is_err());
    assert_eq!(fixture.sessions.pending_cleanup().await.unwrap().len(), 1);
    std::fs::remove_file(&path).unwrap();
    fixture.state.session_manager().reap_now().await.unwrap();
    assert!(fixture.sessions.pending_cleanup().await.unwrap().is_empty());
    fixture.database.close().await.unwrap();
}

#[tokio::test]
async fn repository_removal_does_not_choose_session_transitions() {
    let fixture = Fixture::new().await;
    fixture.session("one").await;
    assert!(fixture.state.blueprints().remove("test").await.unwrap());
    assert_eq!(
        fixture.sessions.load("one").await.unwrap().unwrap().status,
        SessionStatus::Active
    );
    assert!(!fixture.state.remove_blueprint("test").await.unwrap());
    assert_eq!(
        fixture.sessions.load("one").await.unwrap().unwrap().status,
        SessionStatus::Active
    );
    fixture.database.close().await.unwrap();
}

#[tokio::test]
async fn omitted_session_prevents_removal_and_rolls_back_earlier_writes() {
    let fixture = Fixture::new().await;
    fixture.session("one").await;
    fixture.session("two").await;
    let factory = crate::adapters::unit_of_work::SqliteUnitOfWorkFactory {
        database: fixture.database.clone(),
        session_root: fixture.root.path().join("workspaces"),
        cipher: None,
    };
    let mut unit = factory.begin().await.unwrap();
    let mut sessions = unit.sessions_for_blueprint("test").await.unwrap();
    let mut session = sessions.pop().unwrap();
    session.close(ClosedReason::BlueprintRemoved);
    unit.save_session(session).await.unwrap();
    assert!(unit.remove_blueprint("test").await.is_err());
    assert!(unit.commit().await.is_err());
    assert!(
        fixture
            .state
            .blueprints()
            .get("test")
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        fixture
            .sessions
            .load_all()
            .await
            .unwrap()
            .iter()
            .all(|r| r.status == SessionStatus::Active)
    );
    assert!(fixture.sessions.pending_cleanup().await.unwrap().is_empty());
    fixture.database.close().await.unwrap();
}

#[tokio::test]
async fn removal_uses_configured_stores_even_when_a_database_is_present() {
    use crate::blueprint::InMemoryBlueprintStore;
    use crate::session_store::InMemoryDurableSessionStore;

    for (override_blueprints, override_sessions) in [(true, true), (false, true), (true, false)] {
        let fixture = Fixture::new().await;
        fixture.session("one").await;
        let blueprints: Arc<dyn BlueprintStore> = if override_blueprints {
            let store = Arc::new(InMemoryBlueprintStore::default());
            store
                .add(submilli_blueprint::parse("name: test\ndefault: deny\n").unwrap())
                .await
                .unwrap();
            store
        } else {
            Arc::new(SqliteBlueprintStore::new(fixture.database.clone(), None))
        };
        let sessions: Arc<dyn DurableSessionStore> = if override_sessions {
            let store = Arc::new(InMemoryDurableSessionStore::default());
            let mut record = fixture.sessions.load("one").await.unwrap().unwrap();
            record.revision = 0;
            store.put(record).await.unwrap();
            store
        } else {
            fixture.sessions.clone()
        };
        let state = AppState::new(ServerConfig {
            database: Some(fixture.database.clone()),
            blueprints: Some(blueprints.clone()),
            session_store: Some(sessions.clone()),
            session_storage_root: Some(fixture.root.path().join("workspaces")),
            ..Default::default()
        })
        .unwrap();
        assert!(state.remove_blueprint("test").await.unwrap());
        assert!(blueprints.get("test").await.unwrap().is_none());
        assert_eq!(
            sessions.load("one").await.unwrap().unwrap().status,
            SessionStatus::Closed
        );
        fixture.database.close().await.unwrap();
    }
}

#[tokio::test]
async fn unit_of_work_saves_sessions_without_removal() {
    use crate::domain::session::{RootVfs, Session, SessionBinding, SessionId, SessionLifetime};
    let fixture = Fixture::new().await;
    let yaml = "name: new\ndefault: deny\n";
    let session = Session::create(
        SessionId::parse("new-session".into()).unwrap(),
        SessionBinding::new("new".into(), Default::default()).unwrap(),
        RootVfs::None,
        SessionLifetime::new(Duration::from_secs(3600), SystemTime::now()),
    )
    .unwrap();
    let factory = crate::adapters::unit_of_work::SqliteUnitOfWorkFactory {
        database: fixture.database.clone(),
        session_root: fixture.root.path().join("workspaces"),
        cipher: None,
    };
    fixture
        .state
        .blueprints()
        .add(submilli_blueprint::parse(yaml).unwrap())
        .await
        .unwrap();
    let mut unit = factory.begin().await.unwrap();
    unit.save_session(session).await.unwrap();
    assert!(unit.blueprint_exists("new").await.unwrap());
    let found = unit.get_session("new-session").await.unwrap().unwrap();
    assert_eq!(found.binding().blueprint(), "new");
    let mut reread = unit
        .sessions_for_blueprint("new")
        .await
        .unwrap()
        .pop()
        .unwrap();
    reread.close(ClosedReason::Deleted);
    unit.save_session(reread).await.unwrap();
    assert!(matches!(
        unit.get_session("new-session")
            .await
            .unwrap()
            .unwrap()
            .require_available(SystemTime::now()),
        Err(crate::domain::session::SessionRuleError::Closed)
    ));
    assert_eq!(
        unit.sessions_for_blueprint("new").await.unwrap()[0].status(),
        DomainStatus::Closed(ClosedReason::Deleted)
    );
    unit.commit().await.unwrap();
    assert!(
        fixture
            .state
            .blueprints()
            .get("new")
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        fixture
            .sessions
            .load("new-session")
            .await
            .unwrap()
            .unwrap()
            .status,
        SessionStatus::Closed
    );
    assert!(
        fixture
            .state
            .blueprints()
            .get("test")
            .await
            .unwrap()
            .is_some()
    );
    fixture.database.close().await.unwrap();
}

#[tokio::test]
async fn dropping_a_unit_discards_writes_and_releases_the_database() {
    let fixture = Fixture::new().await;
    fixture.session("one").await;
    let factory = crate::adapters::unit_of_work::SqliteUnitOfWorkFactory {
        database: fixture.database.clone(),
        session_root: fixture.root.path().join("workspaces"),
        cipher: None,
    };
    let mut unit = factory.begin().await.unwrap();
    let mut session = unit
        .sessions_for_blueprint("test")
        .await
        .unwrap()
        .pop()
        .unwrap();
    session.close(ClosedReason::Deleted);
    unit.save_session(session).await.unwrap();
    assert!(unit.remove_blueprint("test").await.unwrap());
    assert!(!unit.blueprint_exists("test").await.unwrap());
    drop(unit);
    tokio::time::timeout(Duration::from_secs(5), async {
        assert!(
            fixture
                .state
                .blueprints()
                .get("test")
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(
            fixture.sessions.load("one").await.unwrap().unwrap().status,
            SessionStatus::Active
        );
        assert!(fixture.sessions.pending_cleanup().await.unwrap().is_empty());
        assert!(fixture.state.remove_blueprint("test").await.unwrap());
    })
    .await
    .unwrap();
    fixture.database.close().await.unwrap();
}

#[tokio::test]
async fn compatibility_reads_include_staged_changes_and_drop_discards_them() {
    use crate::blueprint::InMemoryBlueprintStore;
    use crate::session_store::InMemoryDurableSessionStore;
    let fixture = Fixture::new().await;
    fixture.session("one").await;
    let blueprints = Arc::new(InMemoryBlueprintStore::default());
    blueprints
        .add(submilli_blueprint::parse("name: test\ndefault: deny\n").unwrap())
        .await
        .unwrap();
    let sessions = Arc::new(InMemoryDurableSessionStore::default());
    let mut record = fixture.sessions.load("one").await.unwrap().unwrap();
    record.revision = 0;
    sessions.put(record).await.unwrap();
    let factory = crate::adapters::unit_of_work::StoreUnitOfWorkFactory {
        blueprints: blueprints.clone(),
        sessions: sessions.clone(),
        session_root: fixture.root.path().join("workspaces"),
        cipher: None,
    };
    let mut unit = factory.begin().await.unwrap();
    let mut session = unit
        .sessions_for_blueprint("test")
        .await
        .unwrap()
        .pop()
        .unwrap();
    session.close(ClosedReason::Deleted);
    unit.save_session(session).await.unwrap();
    assert_eq!(
        unit.sessions_for_blueprint("test").await.unwrap()[0].status(),
        DomainStatus::Closed(ClosedReason::Deleted)
    );
    assert!(matches!(
        unit.get_session("one")
            .await
            .unwrap()
            .unwrap()
            .require_available(SystemTime::now()),
        Err(crate::domain::session::SessionRuleError::Closed)
    ));
    assert!(unit.remove_blueprint("test").await.unwrap());
    assert!(!unit.blueprint_exists("test").await.unwrap());
    assert!(blueprints.get("test").await.unwrap().is_some());
    assert_eq!(
        sessions.load("one").await.unwrap().unwrap().status,
        SessionStatus::Active
    );
    drop(unit);
    let mut unit = factory.begin().await.unwrap();
    assert!(unit.blueprint_exists("test").await.unwrap());
    assert_eq!(
        unit.sessions_for_blueprint("test").await.unwrap()[0].status(),
        DomainStatus::Active
    );
    drop(unit);
    fixture.database.close().await.unwrap();
}

#[tokio::test]
async fn blueprint_removal_cleanup_purges_only_its_sessions_ledgers() {
    let fixture = Fixture::new().await;
    let yaml = "name: other\ndefault: deny\n";
    fixture
        .state
        .blueprints()
        .add_yaml(StoredBlueprint::new(
            submilli_blueprint::parse(yaml).unwrap(),
            yaml.into(),
        ))
        .await
        .unwrap();
    for id in ["one", "two", "other"] {
        fixture.session(id).await;
        fixture
            .ledger
            .put(LedgerEntry::reserved(id, "key", code_fingerprint("main")))
            .await
            .unwrap();
    }
    let mut other = fixture.sessions.load("other").await.unwrap().unwrap();
    other.blueprint_name = "other".into();
    fixture.sessions.put(other).await.unwrap();

    assert!(fixture.state.remove_blueprint("test").await.unwrap());
    for id in ["one", "two"] {
        assert!(fixture.ledger.load(id, "key").await.unwrap().is_some());
    }
    fixture.state.session_manager().reap_now().await.unwrap();
    for id in ["one", "two"] {
        assert!(fixture.ledger.load(id, "key").await.unwrap().is_none());
    }
    assert!(fixture.ledger.load("other", "key").await.unwrap().is_some());
    assert_eq!(
        fixture
            .sessions
            .load("other")
            .await
            .unwrap()
            .unwrap()
            .status,
        SessionStatus::Active
    );
    fixture.database.close().await.unwrap();
}

#[tokio::test]
async fn unknown_closure_survives_transactional_round_trip() {
    let fixture = Fixture::new().await;
    let factory = crate::adapters::unit_of_work::SqliteUnitOfWorkFactory {
        database: fixture.database.clone(),
        session_root: fixture.root.path().join("workspaces"),
        cipher: None,
    };
    for (id, reason) in [("legacy", None), ("unknown", Some(ClosedReason::Unknown))] {
        fixture.session(id).await;
        let mut record = fixture.sessions.load(id).await.unwrap().unwrap();
        record.status = SessionStatus::Closed;
        record.closed_reason = reason;
        fixture.sessions.put(record).await.unwrap();

        let mut unit = factory.begin().await.unwrap();
        let mut session = unit.get_session(id).await.unwrap().unwrap();
        assert_eq!(
            session.status(),
            DomainStatus::Closed(ClosedReason::Unknown)
        );
        assert_eq!(session.closed_reason(), Some(ClosedReason::Unknown));
        assert!(!session.close(ClosedReason::Deleted));
        unit.save_session(session).await.unwrap();
        unit.commit().await.unwrap();

        let record = fixture.sessions.load(id).await.unwrap().unwrap();
        assert_eq!(record.status, SessionStatus::Closed);
        assert_eq!(record.closed_reason, None);
        let mut unit = factory.begin().await.unwrap();
        assert_eq!(
            unit.get_session(id).await.unwrap().unwrap().status(),
            DomainStatus::Closed(ClosedReason::Unknown)
        );
    }
    fixture.database.close().await.unwrap();
}
