use super::tests::{fixture, proceed, read_request};
use super::*;
use crate::application::error::StoreError;
use crate::application::unit_of_work::UnitOfWork;
use crate::domain::session::Session;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
struct CommitFault {
    lose_ack: AtomicBool,
    pause_after_commit: AtomicBool,
    committed: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
struct FaultFactory {
    inner: Arc<dyn UnitOfWorkFactory>,
    fault: Arc<CommitFault>,
}
struct FaultUnit {
    inner: Box<dyn UnitOfWork>,
    fault: Arc<CommitFault>,
}
#[async_trait::async_trait]
impl UnitOfWorkFactory for FaultFactory {
    async fn begin(&self) -> Result<Box<dyn UnitOfWork>, StoreError> {
        Ok(Box::new(FaultUnit {
            inner: self.inner.begin().await?,
            fault: self.fault.clone(),
        }))
    }
}
#[async_trait::async_trait]
impl UnitOfWork for FaultUnit {
    async fn get_session(&mut self, id: &str) -> Result<Option<Session>, StoreError> {
        self.inner.get_session(id).await
    }
    async fn sessions_due_for_expiry(
        &mut self,
        now: SystemTime,
    ) -> Result<Vec<Session>, StoreError> {
        self.inner.sessions_due_for_expiry(now).await
    }
    async fn list_sessions(&mut self) -> Result<Vec<Session>, StoreError> {
        self.inner.list_sessions().await
    }
    async fn sessions_for_blueprint(&mut self, name: &str) -> Result<Vec<Session>, StoreError> {
        self.inner.sessions_for_blueprint(name).await
    }
    async fn save_session(&mut self, session: Session) -> Result<(), StoreError> {
        self.inner.save_session(session).await
    }
    async fn blueprint_exists(&mut self, name: &str) -> Result<bool, StoreError> {
        self.inner.blueprint_exists(name).await
    }
    async fn remove_blueprint(&mut self, name: &str) -> Result<bool, StoreError> {
        self.inner.remove_blueprint(name).await
    }
    async fn get_request(
        &mut self,
        session_id: &str,
        key: &str,
    ) -> Result<Option<crate::domain::idempotent_request::IdempotentRequest>, StoreError> {
        self.inner.get_request(session_id, key).await
    }
    async fn save_request(
        &mut self,
        request: crate::domain::idempotent_request::IdempotentRequest,
    ) -> Result<(), StoreError> {
        self.inner.save_request(request).await
    }
    async fn remove_request(&mut self, session_id: &str, key: &str) -> Result<(), StoreError> {
        self.inner.remove_request(session_id, key).await
    }
    async fn remove_session_requests(&mut self, session_id: &str) -> Result<(), StoreError> {
        self.inner.remove_session_requests(session_id).await
    }
    async fn unfinished_requests(
        &mut self,
    ) -> Result<Vec<crate::domain::idempotent_request::IdempotentRequest>, StoreError> {
        self.inner.unfinished_requests().await
    }
    async fn commit(self: Box<Self>) -> Result<(), StoreError> {
        self.inner.commit().await?;
        if self.fault.pause_after_commit.swap(false, Ordering::SeqCst) {
            self.fault.committed.notify_one();
            self.fault.release.notified().await;
        }
        if self.fault.lose_ack.swap(false, Ordering::SeqCst) {
            return Err(StoreError::Io(
                "injected lost commit acknowledgement".into(),
            ));
        }
        Ok(())
    }
}

#[tokio::test]
async fn lost_acknowledgements_never_repeat_execution_or_lose_completed_bytes() {
    let (database, units, _) = fixture().await;
    let fault = Arc::new(CommitFault::default());
    let units: Arc<dyn UnitOfWorkFactory> = Arc::new(FaultFactory {
        inner: units,
        fault: fault.clone(),
    });
    let coordinator = Arc::new(Coordinator::new(units, database.generation().unwrap()));
    fault.lose_ack.store(true, Ordering::SeqCst);
    let guard = proceed(coordinator.reserve("session", "key", "code").await);
    fault.lose_ack.store(true, Ordering::SeqCst);
    guard.complete(200, "original bytes".into()).await;
    match coordinator.reserve("session", "key", "code").await {
        Reservation::Replay(outcome) => assert_eq!(outcome.body, "original bytes"),
        _ => panic!("committed outcome must replay"),
    }
    let guard = proceed(coordinator.reserve("session", "release", "code").await);
    fault.lose_ack.store(true, Ordering::SeqCst);
    guard.release().await;
    proceed(coordinator.reserve("session", "release", "code").await)
        .release()
        .await;
}

#[tokio::test]
async fn cancellation_after_reservation_commit_preserves_indeterminate_evidence() {
    let (database, units, _) = fixture().await;
    let fault = Arc::new(CommitFault::default());
    let units: Arc<dyn UnitOfWorkFactory> = Arc::new(FaultFactory {
        inner: units,
        fault: fault.clone(),
    });
    let coordinator = Arc::new(Coordinator::new(units, database.generation().unwrap()));
    fault.pause_after_commit.store(true, Ordering::SeqCst);
    let owner = coordinator.clone();
    let task = tokio::spawn(async move { owner.reserve("session", "key", "code").await });
    fault.committed.notified().await;
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    assert!(matches!(
        coordinator.reserve("session", "key", "code").await,
        Reservation::Refused(Refusal::Indeterminate)
    ));
}

#[tokio::test]
async fn cancelling_completion_waiter_does_not_cancel_owned_finalization() {
    let (database, units, _) = fixture().await;
    let fault = Arc::new(CommitFault::default());
    let units: Arc<dyn UnitOfWorkFactory> = Arc::new(FaultFactory {
        inner: units,
        fault: fault.clone(),
    });
    let coordinator = Arc::new(Coordinator::new(units, database.generation().unwrap()));
    let guard = proceed(coordinator.reserve("session", "key", "code").await);
    fault.pause_after_commit.store(true, Ordering::SeqCst);
    let task = tokio::spawn(guard.complete(200, "done".into()));
    fault.committed.notified().await;
    task.abort();
    assert!(matches!(task.await, Err(error) if error.is_cancelled()));
    assert!(matches!(
        coordinator.reserve("session", "key", "code").await,
        Reservation::Replay(_)
    ));
    fault.release.notify_one();
    coordinator.drain().await;
}

#[tokio::test]
async fn simultaneous_claims_dispatch_once_and_cancelled_waiters_do_not_release_the_owner() {
    let (database, units, first) = fixture().await;
    let second = Arc::new(Coordinator::new(units, database.generation().unwrap()));
    let left = first.clone();
    let mut a = tokio::spawn(async move { left.reserve("session", "key", "code").await });
    let mut b = tokio::spawn(async move { second.reserve("session", "key", "code").await });
    let (winner, loser) = tokio::select! {
        result = &mut a => (result.unwrap(), b),
        result = &mut b => (result.unwrap(), a),
    };
    let guard = proceed(winner);
    loser.abort();
    assert!(matches!(loser.await, Err(error) if error.is_cancelled()));
    guard.complete(200, "once".into()).await;
    assert!(matches!(
        first.reserve("session", "key", "code").await,
        Reservation::Replay(_)
    ));
}

#[tokio::test]
async fn waiter_deadline_does_not_authorize_another_execution() {
    let (_, units, first) = fixture().await;
    let second = Arc::new(Coordinator {
        units,
        generation: first.generation.clone(),
        tasks: TaskTracker::new(),
        waiter_timeout: Duration::from_millis(5),
    });
    let guard = proceed(first.reserve("session", "key", "code").await);
    assert!(matches!(
        second.reserve("session", "key", "code").await,
        Reservation::Refused(Refusal::InProgress)
    ));
    guard.complete(200, "eventual result".into()).await;
    assert!(matches!(
        second.reserve("session", "key", "code").await,
        Reservation::Replay(_)
    ));
}

#[test]
fn runtime_recreation_recovers_requests_even_when_database_worker_survives() {
    let database = Arc::new(
        futures::executor::block_on(crate::database::ServerDatabase::open_ephemeral()).unwrap(),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (guard, original_generation) = runtime.block_on(async {
        let (_, _, coordinator) = super::tests::fixture_with_database(database.clone()).await;
        (
            proceed(coordinator.reserve("session", "key", "code").await),
            coordinator.generation.clone(),
        )
    });
    drop(runtime);
    drop(guard);
    let replacement = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    replacement.block_on(async {
        let (_, _, coordinator) = super::tests::fixture_with_database(database.clone()).await;
        assert_ne!(coordinator.generation, original_generation);
        coordinator.recover().await.unwrap();
        assert!(matches!(
            coordinator.reserve("session", "key", "code").await,
            Reservation::Refused(Refusal::Indeterminate)
        ));
    });
}

#[tokio::test]
async fn disk_reopen_preserves_completed_bytes_and_recovers_unfinished_requests() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("server.db");
    let database = Arc::new(crate::database::ServerDatabase::open(&path).await.unwrap());
    let (_, units, coordinator) = super::tests::fixture_with_database(database.clone()).await;
    proceed(coordinator.reserve("session", "done", "code").await)
        .complete(201, "exact\nbytes".into())
        .await;
    let abandoned = IdempotentRequest::reserve(
        "session".into(),
        "unfinished".into(),
        format!("{:x}", Sha256::digest(b"code")),
        "old-claim".into(),
        coordinator.generation.clone(),
        SystemTime::now(),
    )
    .unwrap();
    let mut unit = units.begin().await.unwrap();
    unit.save_request(abandoned).await.unwrap();
    unit.commit().await.unwrap();
    coordinator.drain().await;
    database.close().await.unwrap();
    let reopened = Arc::new(crate::database::ServerDatabase::open(&path).await.unwrap());
    let (_, _, coordinator) = super::tests::fixture_with_database(reopened).await;
    coordinator.recover().await.unwrap();
    match coordinator.reserve("session", "done", "code").await {
        Reservation::Replay(outcome) => {
            assert_eq!(outcome.status, 201);
            assert_eq!(outcome.body, "exact\nbytes");
        }
        _ => panic!("stored bytes must survive reopen"),
    }
    assert!(matches!(
        coordinator.reserve("session", "unfinished", "code").await,
        Reservation::Refused(Refusal::Indeterminate)
    ));
}

#[tokio::test]
async fn failed_request_cleanup_rolls_back_expiry() {
    let (database, units, coordinator) = fixture().await;
    let guard = proceed(coordinator.reserve("session", "key", "code").await);
    database.transaction(|connection| Box::pin(async move {
        sqlx::query("CREATE TRIGGER reject_request_delete BEFORE DELETE ON idempotent_requests BEGIN SELECT RAISE(FAIL, 'injected'); END").execute(connection).await?;
        Ok(())
    })).await.unwrap();

    let expiry = crate::application::sessions::expire::ExpireSessions::new(units.as_ref(), &Audit);
    let later = SystemTime::now() + Duration::from_secs(7200);
    assert!(expiry.execute(later).await.is_err());
    let mut unit = units.begin().await.unwrap();
    assert_eq!(
        unit.get_session("session").await.unwrap().unwrap().status(),
        crate::domain::session::SessionStatus::Active
    );
    assert!(unit.get_request("session", "key").await.unwrap().is_some());
    drop(unit);
    database
        .transaction(|connection| {
            Box::pin(async move {
                sqlx::query("DROP TRIGGER reject_request_delete")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
        .unwrap();
    assert_eq!(expiry.execute(later).await.unwrap(), 1);
    guard.complete(200, "late".into()).await;
    assert!(
        read_request(units.as_ref(), "session", "key")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn failed_completion_activity_write_cannot_publish_a_partial_outcome() {
    let (database, units, coordinator) = fixture().await;
    let guard = proceed(coordinator.reserve("session", "key", "code").await);
    let mut unit = units.begin().await.unwrap();
    let before = unit
        .get_session("session")
        .await
        .unwrap()
        .unwrap()
        .lifetime()
        .last_activity();
    drop(unit);
    database.transaction(|connection| Box::pin(async move {
        sqlx::query("CREATE TRIGGER reject_activity BEFORE UPDATE ON sessions BEGIN SELECT RAISE(FAIL, 'injected'); END").execute(connection).await?; Ok(())
    })).await.unwrap();
    guard.complete(200, "original result".into()).await;
    assert!(matches!(
        coordinator.reserve("session", "key", "code").await,
        Reservation::Refused(Refusal::Indeterminate)
    ));
    let mut unit = units.begin().await.unwrap();
    assert_eq!(
        unit.get_session("session")
            .await
            .unwrap()
            .unwrap()
            .lifetime()
            .last_activity(),
        before
    );
}

#[tokio::test]
async fn database_contention_cannot_exceed_reservation_deadline() {
    let (_, units, first) = fixture().await;
    let guard = proceed(first.reserve("session", "key", "code").await);
    let second = Arc::new(Coordinator {
        units: units.clone(),
        generation: first.generation.clone(),
        tasks: TaskTracker::new(),
        waiter_timeout: Duration::from_millis(10),
    });
    let held = units.begin().await.unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        second.reserve("session", "key", "code"),
    )
    .await
    .unwrap();
    assert!(matches!(
        result,
        Reservation::Refused(Refusal::Unavailable(_))
    ));
    drop(held);
    guard.complete(200, "done".into()).await;
    second.drain().await;
}

struct Audit;
impl crate::application::sessions::ports::AuditLog for Audit {
    fn record(&self, _: &str, _: crate::application::sessions::ports::SessionEvent) {}
}
