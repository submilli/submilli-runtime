//! HTTP-facing coordination; the application and database decide ownership.
use crate::application::idempotency::{
    ReadRequest, RecoverRequests, RequestDisposition, RequestFailure, RequestResolution,
    ReserveRequest, ResolveRequest,
};
use crate::application::unit_of_work::UnitOfWorkFactory;
pub use crate::domain::idempotent_request::RecordedOutcome;
use crate::domain::idempotent_request::{IdempotentRequest, RequestError, RequestState};
use axum::http::StatusCode;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::oneshot;
use tokio_util::task::TaskTracker;
pub const MAX_KEY_BYTES: usize = 120;
const DEFAULT_WAITER_TIMEOUT: Duration = Duration::from_secs(300);
/// What a caller may do with a key.
pub enum Reservation {
    /// Run the program, then settle the guard with `complete` or `release`.
    Proceed(ReservationGuard),
    /// Return this recorded status and body verbatim. Nothing runs.
    Replay(RecordedOutcome),
    Refused(Refusal),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Empty, or past [`MAX_KEY_BYTES`].
    InvalidKey(&'static str),
    /// The key exists against different code.
    Conflict,
    /// A reservation exists that never reached a recorded outcome. Never
    /// re-executed automatically.
    Indeterminate,
    /// The original execution is provably still running — this waiter simply
    /// outlived its own bound. Distinct from [`Refusal::Indeterminate`] because
    /// it is *retryable*: the outcome is not unknown, it is not ready yet.
    /// Collapsing the two would let a slow-but-healthy execution be reported as
    /// permanently unknown, and then complete successfully afterwards.
    InProgress,
    /// The ledger could not be read or written. The ledger is authoritative, so
    /// a request it cannot record is refused rather than run unguarded.
    Unavailable(String),
}

impl Refusal {
    /// The stable `error` field clients discriminate on.
    pub fn code(&self) -> &'static str {
        match self {
            Refusal::InvalidKey(_) => "idempotency_key_invalid",
            Refusal::Conflict => "idempotency_conflict",
            Refusal::Indeterminate => "idempotency_incomplete",
            Refusal::InProgress => "idempotency_in_progress",
            Refusal::Unavailable(_) => "idempotency_unavailable",
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Refusal::InvalidKey(_) => StatusCode::BAD_REQUEST,
            Refusal::Conflict | Refusal::Indeterminate => StatusCode::CONFLICT,
            // Retryable, so it shares 503 with the ledger being down rather
            // than sitting with the terminal 409s.
            Refusal::InProgress | Refusal::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    pub fn detail(&self) -> String {
        match self {
            Refusal::InvalidKey(reason) => (*reason).to_string(),
            Refusal::Conflict => {
                "this idempotency key was already used with different code".to_string()
            }
            Refusal::Indeterminate => {
                "a previous request with this idempotency key never recorded an outcome; \
                 whether its program ran is unknown, so it will not be retried automatically. \
                 Running the code again is a new, independent execution — decide on whether \
                 it is safe to run twice, not on this key"
                    .to_string()
            }
            Refusal::InProgress => {
                "an earlier request with this idempotency key is still executing; \
                 it has not finished within the time this request was willing to wait. \
                 Nothing new was started. Retrying with this same key picks up its \
                 result; retrying under a new key starts a second, concurrent execution"
                    .to_string()
            }
            // The underlying error is logged server-side; a caller gets the
            // condition, not the host's filesystem paths or errno text.
            Refusal::Unavailable(_) => {
                "the server could not record this request's idempotency key, so it was not \
                 executed"
                    .to_string()
            }
        }
    }
}

pub(crate) struct Coordinator {
    units: Arc<dyn UnitOfWorkFactory>,
    generation: String,
    tasks: TaskTracker,
    waiter_timeout: Duration,
}

impl Coordinator {
    pub fn new(units: Arc<dyn UnitOfWorkFactory>, generation: String) -> Self {
        Self {
            units,
            generation,
            tasks: TaskTracker::new(),
            waiter_timeout: DEFAULT_WAITER_TIMEOUT,
        }
    }

    pub async fn recover(&self) -> Result<(), RequestFailure> {
        RecoverRequests::new(self.units.as_ref())
            .execute(&self.generation)
            .await
    }

    pub async fn drain(&self) {
        self.tasks.close();
        self.tasks.wait().await;
    }

    pub async fn reserve(self: &Arc<Self>, session: &str, key: &str, code: &str) -> Reservation {
        if IdempotentRequest::validate_key(key).is_err() {
            return Reservation::Refused(Refusal::InvalidKey(
                "Idempotency-Key must contain between 1 and 120 bytes",
            ));
        }
        let fingerprint = format!("{:x}", Sha256::digest(code.as_bytes()));
        let mut identity = [0u8; 16];
        if let Err(error) = getrandom::getrandom(&mut identity) {
            return Reservation::Refused(Refusal::Unavailable(error.to_string()));
        }
        let Ok(request) = IdempotentRequest::reserve(
            session.to_owned(),
            key.to_owned(),
            fingerprint,
            uuid::Uuid::from_bytes(identity).to_string(),
            self.generation.clone(),
            SystemTime::now(),
        ) else {
            return Reservation::Refused(Refusal::InvalidKey("invalid idempotency key"));
        };
        // Registered before the first await: cancellation during commit is reconciled
        // against this identity, even when its acknowledgement never reaches us.
        let guard = self.supervise(request.clone());
        let deadline = tokio::time::Instant::now() + self.waiter_timeout;
        let mut observed_owner = false;
        loop {
            let reserve = async {
                ReserveRequest::new(self.units.as_ref())
                    .execute(request.clone())
                    .await
            };
            let disposition = match tokio::time::timeout_at(deadline, reserve).await {
                Ok(disposition) => disposition,
                Err(_) if observed_owner => return Reservation::Refused(Refusal::InProgress),
                Err(_) => {
                    return Reservation::Refused(Refusal::Unavailable(
                        "reservation deadline exceeded".into(),
                    ));
                }
            };
            let existing = match disposition {
                Ok(RequestDisposition::Proceed) => return Reservation::Proceed(guard),
                Ok(RequestDisposition::Existing(existing)) => existing,
                Err(RequestFailure::Rule(RequestError::Conflict)) => {
                    return Reservation::Refused(Refusal::Conflict);
                }
                Err(error) => {
                    // The same invocation has not dispatched. Only its own durable
                    // reservation proves an uncertain claim commit succeeded.
                    match tokio::time::timeout_at(deadline, async {
                        ReadRequest::new(self.units.as_ref())
                            .execute(session, key)
                            .await
                    })
                    .await
                    {
                        Ok(Ok(Some(existing)))
                            if existing.reservation_id() == request.reservation_id()
                                && existing.state() == &RequestState::Reserved =>
                        {
                            return Reservation::Proceed(guard);
                        }
                        _ => return Reservation::Refused(Refusal::Unavailable(error.to_string())),
                    }
                }
            };
            match existing.state() {
                RequestState::Completed(outcome) => return Reservation::Replay(outcome.clone()),
                RequestState::Indeterminate => return Reservation::Refused(Refusal::Indeterminate),
                RequestState::Reserved => observed_owner = true,
            }
            if tokio::time::Instant::now() >= deadline {
                return Reservation::Refused(Refusal::InProgress);
            }
            tokio::time::sleep_until(std::cmp::min(
                deadline,
                tokio::time::Instant::now() + Duration::from_millis(100),
            ))
            .await;
        }
    }

    fn supervise(&self, request: IdempotentRequest) -> ReservationGuard {
        let (sender, receiver) = oneshot::channel();
        let (finished, completion) = oneshot::channel();
        let units = self.units.clone();
        self.tasks.spawn(async move {
            let resolution = receiver.await.unwrap_or(RequestResolution::Indeterminate);
            let resolver = ResolveRequest::new(units.clone());
            let first = resolver.execute(&request, &resolution).await;
            if first.is_ok() {
                let _ = finished.send(());
                return;
            }
            // A failed acknowledgement may hide a successful commit. Reapplying
            // this identity-checked transition is safe and never dispatches work.
            let second = resolver.execute(&request, &resolution).await;
            let _ = finished.send(());
            if second.is_ok() {
                return;
            }
            tracing::warn!(error = ?second.err(), "idempotency finalization remains uncertain");
            // Preserve evidence when storage recovers, without withholding the
            // original caller's result. Startup recovery covers process termination.
            let fallback = match resolution {
                RequestResolution::Undispatched => RequestResolution::Undispatched,
                _ => RequestResolution::Indeterminate,
            };
            loop {
                match resolver.execute(&request, &fallback).await {
                    Ok(()) => break,
                    Err(error) if database_has_closed(&error) => break,
                    Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
                }
            }
        });
        ReservationGuard {
            sender: Some(sender),
            completion,
        }
    }
}

fn database_has_closed(error: &RequestFailure) -> bool {
    let RequestFailure::Store(crate::application::error::StoreError::Database(source)) = error
    else {
        return false;
    };
    matches!(
        source.downcast_ref::<crate::database::DatabaseError>(),
        Some(
            crate::database::DatabaseError::Closed | crate::database::DatabaseError::WorkerStopped
        )
    )
}

pub struct ReservationGuard {
    sender: Option<oneshot::Sender<RequestResolution>>,
    completion: oneshot::Receiver<()>,
}
impl ReservationGuard {
    pub async fn complete(self, status: u16, body: String) {
        self.resolve(RequestResolution::Completed(RecordedOutcome {
            status,
            body,
        }))
        .await;
    }
    pub async fn release(self) {
        self.resolve(RequestResolution::Undispatched).await;
    }
    async fn resolve(mut self, resolution: RequestResolution) {
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(resolution);
        }
        let _ = self.completion.await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::unit_of_work::SqliteUnitOfWorkFactory;
    use crate::application::sessions::close::CloseSession;
    use crate::database::ServerDatabase;
    use crate::domain::session::{RootVfs, Session, SessionBinding, SessionId, SessionLifetime};

    struct Audit;
    impl crate::application::sessions::ports::AuditLog for Audit {
        fn record(&self, _: &str, _: crate::application::sessions::ports::SessionEvent) {}
    }

    pub(super) async fn fixture() -> (
        Arc<ServerDatabase>,
        Arc<dyn UnitOfWorkFactory>,
        Arc<Coordinator>,
    ) {
        fixture_with_database(Arc::new(ServerDatabase::open_ephemeral().await.unwrap())).await
    }

    pub(super) async fn fixture_with_database(
        database: Arc<ServerDatabase>,
    ) -> (
        Arc<ServerDatabase>,
        Arc<dyn UnitOfWorkFactory>,
        Arc<Coordinator>,
    ) {
        use crate::blueprint::BlueprintStore;
        crate::blueprint::SqliteBlueprintStore::new(database.clone(), None)
            .upsert(submilli_blueprint::parse("name: test\ndefault: deny\n").unwrap())
            .await
            .unwrap();
        let units: Arc<dyn UnitOfWorkFactory> = Arc::new(SqliteUnitOfWorkFactory {
            database: database.clone(),
            session_root: Default::default(),
            cipher: None,
        });
        let mut unit = units.begin().await.unwrap();
        unit.save_session(
            Session::create(
                SessionId::parse("session".into()).unwrap(),
                SessionBinding::new("test".into(), Default::default()).unwrap(),
                RootVfs::None,
                SessionLifetime::new(Duration::from_secs(3600), SystemTime::now()),
            )
            .unwrap(),
        )
        .await
        .unwrap();
        unit.commit().await.unwrap();
        let coordinator = Arc::new(Coordinator::new(
            units.clone(),
            database.generation().unwrap(),
        ));
        (database, units, coordinator)
    }

    pub(super) fn proceed(reservation: Reservation) -> ReservationGuard {
        match reservation {
            Reservation::Proceed(guard) => guard,
            _ => panic!("expected a fresh reservation"),
        }
    }

    #[tokio::test]
    async fn independent_coordinators_wait_for_one_committed_result() {
        let (database, units, first) = fixture().await;
        let second = Arc::new(Coordinator::new(units, database.generation().unwrap()));
        let guard = proceed(first.reserve("session", "key", "code").await);
        let duplicate = tokio::spawn(async move { second.reserve("session", "key", "code").await });
        tokio::task::yield_now().await;
        assert!(!duplicate.is_finished());
        guard.complete(200, "exact bytes\n".into()).await;
        match duplicate.await.unwrap() {
            Reservation::Replay(outcome) => assert_eq!(outcome.body, "exact bytes\n"),
            _ => panic!("duplicate must replay"),
        }
        first.drain().await;
    }

    #[tokio::test]
    async fn dropped_reservation_is_indeterminate_and_conflicts_still_win() {
        let (_, _, coordinator) = fixture().await;
        drop(proceed(coordinator.reserve("session", "key", "code").await));
        assert!(matches!(
            coordinator.reserve("session", "key", "code").await,
            Reservation::Refused(Refusal::Indeterminate)
        ));
        assert!(matches!(
            coordinator.reserve("session", "key", "different").await,
            Reservation::Refused(Refusal::Conflict)
        ));
    }

    #[tokio::test]
    async fn undispatched_release_allows_a_new_claim() {
        let (_, _, coordinator) = fixture().await;
        proceed(coordinator.reserve("session", "key", "code").await)
            .release()
            .await;
        proceed(coordinator.reserve("session", "key", "code").await)
            .complete(200, "done".into())
            .await;
    }

    #[tokio::test]
    async fn recovery_keeps_current_owner_but_retires_previous_generation() {
        let (database, units, coordinator) = fixture().await;
        let guard = proceed(coordinator.reserve("session", "key", "code").await);
        coordinator.recover().await.unwrap();
        assert_eq!(
            ReadRequest::new(units.as_ref())
                .execute("session", "key")
                .await
                .unwrap()
                .unwrap()
                .state(),
            &RequestState::Reserved
        );
        let next = Coordinator::new(units.clone(), "next-generation".into());
        next.recover().await.unwrap();
        assert_eq!(
            ReadRequest::new(units.as_ref())
                .execute("session", "key")
                .await
                .unwrap()
                .unwrap()
                .state(),
            &RequestState::Indeterminate
        );
        guard.complete(200, "late".into()).await;
        assert!(matches!(
            coordinator.reserve("session", "key", "code").await,
            Reservation::Refused(Refusal::Indeterminate)
        ));
        coordinator.drain().await;
        database.close().await.unwrap();
    }

    #[tokio::test]
    async fn closure_removes_requests_atomically_and_completion_cannot_resurrect_them() {
        let (_, units, coordinator) = fixture().await;
        let guard = proceed(coordinator.reserve("session", "key", "code").await);
        let mut unit = units.begin().await.unwrap();
        let mut session = unit.get_session("session").await.unwrap().unwrap();
        session.close(crate::domain::session::ClosedReason::Deleted);
        unit.save_session(session).await.unwrap();
        unit.remove_session_requests("session").await.unwrap();
        drop(unit);
        assert!(
            ReadRequest::new(units.as_ref())
                .execute("session", "key")
                .await
                .unwrap()
                .is_some()
        );

        CloseSession::new(units.as_ref(), &Audit)
            .execute("session", crate::domain::session::ClosedReason::Deleted)
            .await
            .unwrap();
        guard.complete(200, "late".into()).await;
        assert!(
            ReadRequest::new(units.as_ref())
                .execute("session", "key")
                .await
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            coordinator.reserve("session", "key", "code").await,
            Reservation::Refused(_)
        ));
    }

    #[tokio::test]
    async fn completion_failure_does_not_publish_an_uncommitted_response() {
        let (database, _, coordinator) = fixture().await;
        let guard = proceed(coordinator.reserve("session", "key", "code").await);
        database.transaction(|connection| Box::pin(async move {
            sqlx::query("CREATE TRIGGER reject_completion BEFORE UPDATE ON idempotent_requests WHEN NEW.state='completed' BEGIN SELECT RAISE(FAIL, 'injected'); END").execute(connection).await?;
            Ok(())
        })).await.unwrap();
        guard
            .complete(200, "caller still receives this".into())
            .await;
        assert!(matches!(
            coordinator.reserve("session", "key", "code").await,
            Reservation::Refused(Refusal::Indeterminate)
        ));
    }
}

#[cfg(test)]
mod fault_tests;
