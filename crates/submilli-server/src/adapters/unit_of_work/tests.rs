use crate::domain::session::{ClosedReason, SessionStatus as DomainStatus};
use crate::session_store::SessionStatus;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::app::AppState;
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::blueprint::{BlueprintStore, SqliteBlueprintStore, StoredBlueprint};
use crate::config::ServerConfig;
use crate::database::ServerDatabase;
use crate::request_records::{Record as LedgerEntry, RequestRecords, code_fingerprint};
use crate::session_store::{DurableSessionStore, RootVfsType, SessionRecord, SqliteSessionStore};

struct Fixture {
    state: AppState,
    database: Arc<ServerDatabase>,
    sessions: Arc<SqliteSessionStore>,
    ledger: Arc<RequestRecords>,
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
        let ledger = Arc::new(RequestRecords::with_database(database.clone()));
        let state = AppState::new(ServerConfig {
            database: Some(database.clone()),
            blueprints: Some(blueprints),
            session_storage_root: Some(workspaces),
            ..Default::default()
        })
        .await
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
    for id in ["one", "two"] {
        fixture
            .ledger
            .put(LedgerEntry::indeterminate(
                id,
                "key",
                code_fingerprint("code"),
            ))
            .await
            .unwrap();
    }
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
    for id in ["one", "two"] {
        assert!(fixture.ledger.load(id, "key").await.unwrap().is_some());
    }
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
            .put(LedgerEntry::indeterminate(
                id,
                "key",
                code_fingerprint("main"),
            ))
            .await
            .unwrap();
    }
    let mut other = fixture.sessions.load("other").await.unwrap().unwrap();
    other.blueprint_name = "other".into();
    fixture.sessions.put(other).await.unwrap();

    assert!(fixture.state.remove_blueprint("test").await.unwrap());
    for id in ["one", "two"] {
        assert!(fixture.ledger.load(id, "key").await.unwrap().is_none());
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

#[tokio::test]
async fn reaping_ignores_closed_history_and_its_unreadable_protocol_state() {
    let fixture = Fixture::new().await;
    fixture.database.transaction(|connection| Box::pin(async move {
        sqlx::query("WITH RECURSIVE history(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM history WHERE n<2048) INSERT INTO sessions(session_id,record_version,status,blueprint_name,idle_timeout_ms,last_activity_unix_ms,root_vfs_type) SELECT 'old-'||n,1,'closed','test',0,0,'none' FROM history")
            .execute(&mut *connection).await?;
        // Inject protocol data that cannot be decoded. Expiry must never hydrate
        // historical protocol state, even though these are valid JSON/SQL rows.
        sqlx::query("INSERT INTO session_mcp(session_id,protocol_version,client_name,client_version,client_icons_present,capabilities_json) SELECT session_id,'2025-03-26','old','1',0,'false' FROM sessions")
            .execute(connection).await?;
        Ok(())
    })).await.unwrap();
    assert!(fixture.sessions.load("old-1").await.is_err());
    assert_eq!(fixture.state.session_manager().reap_now().await.unwrap(), 0);
    fixture.session("live").await;
    assert_eq!(fixture.state.session_manager().reap_now().await.unwrap(), 0);
    assert!(
        fixture
            .state
            .session_manager()
            .contains("live")
            .await
            .unwrap()
    );
    fixture.database.close().await.unwrap();
}

#[tokio::test]
async fn expiry_queries_observe_pending_activity_and_closure() {
    use crate::adapters::unit_of_work::SqliteUnitOfWorkFactory;
    use crate::domain::session::{RootVfs, Session, SessionBinding, SessionId, SessionLifetime};
    let fixture = Fixture::new().await;
    let factories: Vec<Box<dyn UnitOfWorkFactory>> = vec![Box::new(SqliteUnitOfWorkFactory {
        database: fixture.database.clone(),
        session_root: fixture.root.path().join("workspaces"),
        cipher: None,
    })];
    let now = std::time::UNIX_EPOCH
        + Duration::from_millis(
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
                .try_into()
                .unwrap(),
        );
    let timeout = Duration::from_secs(3600);
    for factory in factories {
        let mut unit = factory.begin().await.unwrap();
        let session = Session::create(
            SessionId::parse("candidate".into()).unwrap(),
            SessionBinding::new("test".into(), Default::default()).unwrap(),
            RootVfs::None,
            SessionLifetime::new(timeout, now),
        )
        .unwrap();
        unit.save_session(session).await.unwrap();
        assert!(unit.sessions_due_for_expiry(now).await.unwrap().is_empty());
        assert!(
            unit.sessions_due_for_expiry(now + timeout)
                .await
                .unwrap()
                .is_empty()
        );
        let due = now + timeout + Duration::from_millis(1);
        let mut session = unit
            .sessions_due_for_expiry(due)
            .await
            .unwrap()
            .pop()
            .unwrap();
        session
            .record_execution_completed(now + Duration::from_secs(1))
            .unwrap();
        unit.save_session(session).await.unwrap();
        assert!(unit.sessions_due_for_expiry(due).await.unwrap().is_empty());
        let mut session = unit.get_session("candidate").await.unwrap().unwrap();
        session.close(ClosedReason::Deleted);
        unit.save_session(session).await.unwrap();
        assert!(
            unit.sessions_due_for_expiry(due + timeout)
                .await
                .unwrap()
                .is_empty()
        );
        // Neither the newly saved row nor its closure escapes a dropped unit.
        drop(unit);
        let mut fresh = factory.begin().await.unwrap();
        assert!(fresh.get_session("candidate").await.unwrap().is_none());
    }
    fixture.database.close().await.unwrap();
}
