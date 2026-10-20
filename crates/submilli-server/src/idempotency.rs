//! HTTP-facing coordination; the application and database decide ownership.
use crate::application::sessions::ports::{LastRunStaging, StagedLastRun};
use crate::application::sessions::runs::{
    PrepareRun, RecoverRuns, ResolveRun, RunDisposition, RunFailure, RunResolution,
};
use crate::application::unit_of_work::UnitOfWorkFactory;
pub use crate::domain::idempotent_request::RecordedOutcome;
use crate::domain::idempotent_request::{IdempotentRequest, RequestError};
use axum::http::StatusCode;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use submilli_core::RunId;
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
    /// The earlier request's run is still going, and may be waiting on the harness:
    /// answer with that run's current state. Nothing new runs.
    Attached(RunId),
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
    /// The earlier request completed, but its response outlived the ledger's
    /// retention. Terminal like [`Refusal::Indeterminate`], with more to say:
    /// the program did run to an outcome.
    Expired,
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
            Refusal::Expired => "idempotency_response_expired",
        }
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Refusal::InvalidKey(_) => StatusCode::BAD_REQUEST,
            Refusal::Conflict | Refusal::Indeterminate | Refusal::Expired => StatusCode::CONFLICT,
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
            Refusal::Expired => {
                "an earlier request with this idempotency key completed, but its response \
                 is no longer retained; it will not run again. Running the code again is a \
                 new, independent execution"
                    .to_string()
            }
        }
    }
}

/// Runs that may suspend, by the reservation they execute under. A suspended run
/// lives in this process's memory, so only this process can attach to it.
type AttachedRuns = Arc<Mutex<HashMap<RunId, RunId>>>;

pub(crate) struct Coordinator {
    units: Arc<dyn UnitOfWorkFactory>,
    generation: String,
    tasks: TaskTracker,
    waiter_timeout: Duration,
    attached: AttachedRuns,
    last_runs: Option<Arc<dyn LastRunStaging>>,
}

impl Coordinator {
    pub fn new(units: Arc<dyn UnitOfWorkFactory>, generation: String) -> Self {
        Self {
            units,
            generation,
            tasks: TaskTracker::new(),
            waiter_timeout: DEFAULT_WAITER_TIMEOUT,
            attached: AttachedRuns::default(),
            last_runs: None,
        }
    }

    /// Keep each session's last run with its run's end, staged through
    /// `last_runs`. Without it, a run's end forgets the session's last run.
    pub fn with_last_runs(mut self, last_runs: Arc<dyn LastRunStaging>) -> Self {
        self.last_runs = Some(last_runs);
        self
    }

    pub async fn recover(&self) -> Result<(), RunFailure> {
        RecoverRuns::new(self.units.as_ref())
            .execute(&self.generation)
            .await
    }

    pub async fn drain(&self) {
        self.tasks.close();
        self.tasks.wait().await;
    }

    #[cfg(test)]
    pub async fn reserve(self: &Arc<Self>, session: &str, key: &str, code: &str) -> Reservation {
        let mut identity = [0u8; 16];
        if let Err(error) = getrandom::getrandom(&mut identity) {
            return Reservation::Refused(Refusal::Unavailable(error.to_string()));
        }
        self.prepare(
            session,
            Some(key),
            code,
            &uuid::Uuid::from_bytes(identity).to_string().into(),
            None,
            "http_session",
        )
        .await
    }

    pub async fn prepare(
        self: &Arc<Self>,
        session: &str,
        key: Option<&str>,
        code: &str,
        run_id: &RunId,
        principal: Option<&str>,
        entry: &str,
    ) -> Reservation {
        let fingerprint = format!("{:x}", Sha256::digest(code.as_bytes()));
        let identity = crate::domain::run::RunIdentity {
            execution_id: Some(run_id.clone()),
            principal: principal.map(str::to_owned),
            entry: entry.to_owned(),
            ..Default::default()
        };
        let Ok(request) = IdempotentRequest::prepare(
            session.to_owned(),
            key.map(str::to_owned),
            fingerprint,
            run_id.clone(),
            self.generation.clone(),
            identity,
            SystemTime::now(),
        ) else {
            return Reservation::Refused(Refusal::InvalidKey("invalid idempotency key"));
        };
        // Registered before the first await: cancellation during commit is reconciled
        // against this identity, even when its acknowledgement never reaches us.
        let guard = self.supervise(request.clone());
        let deadline = tokio::time::Instant::now() + self.waiter_timeout;
        let mut waiting_for = None;
        loop {
            let reserve = async {
                PrepareRun::new(self.units.as_ref())
                    .execute(request.clone())
                    .await
            };
            let disposition = match tokio::time::timeout_at(deadline, reserve).await {
                Ok(disposition) => disposition,
                Err(_) if waiting_for.is_some() => {
                    return Reservation::Refused(Refusal::InProgress);
                }
                Err(_) => {
                    return Reservation::Refused(Refusal::Unavailable(
                        "reservation deadline exceeded".into(),
                    ));
                }
            };
            match disposition {
                Ok(RunDisposition::Proceed) => return Reservation::Proceed(guard),
                Ok(RunDisposition::Wait(id)) => {
                    // A run that may wait on the harness for minutes is answered
                    // with its current state rather than waited for.
                    if let Some(run_id) = self.attached_run(&id) {
                        return Reservation::Attached(run_id);
                    }
                    waiting_for = Some(id);
                }
                Ok(RunDisposition::Completed(outcome)) => return Reservation::Replay(outcome),
                Err(RunFailure::Rule(RequestError::Conflict)) => {
                    return Reservation::Refused(Refusal::Conflict);
                }
                Err(RunFailure::Indeterminate) => {
                    return Reservation::Refused(Refusal::Indeterminate);
                }
                Err(RunFailure::Expired) => {
                    return Reservation::Refused(Refusal::Expired);
                }
                Err(error) => {
                    return Reservation::Refused(Refusal::Unavailable(error.to_string()));
                }
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

    pub async fn claim(&self, run_id: &RunId) -> Result<(), RunFailure> {
        crate::application::sessions::runs::claim_run(self.units.as_ref(), run_id, &self.generation)
            .await
    }

    fn attached_run(&self, reservation_id: &str) -> Option<RunId> {
        lock_attached(&self.attached).get(reservation_id).cloned()
    }

    fn supervise(&self, request: IdempotentRequest) -> ReservationGuard {
        let (sender, receiver) = oneshot::channel();
        let (finished, completion) = oneshot::channel();
        let reservation_id = request.reservation_id().to_owned();
        let units = self.units.clone();
        let last_runs = self.last_runs.clone();
        self.tasks.spawn(async move {
            let (resolution, last_run) = receiver
                .await
                .unwrap_or((RunResolution::Indeterminate, None));
            let mut files = match last_runs.as_deref() {
                Some(last_runs) => {
                    Some(LastRunFiles::before(last_runs, request.session_id()).await)
                }
                None => None,
            };
            let staged = match (&mut files, last_run) {
                (Some(files), Some(json)) => files.stage(json).await,
                _ => None,
            };
            let resolver = ResolveRun::new(units.clone());
            let resolved = resolve(&resolver, &request, &resolution, staged.as_ref()).await;
            let _ = finished.send(());
            let replaced = match resolved {
                Ok(replaced) => replaced,
                Err(error) => {
                    tracing::warn!(?error, "idempotency finalization remains uncertain");
                    resolve_until_recorded(&resolver, &request, &resolution).await
                }
            };
            if let Some(files) = files {
                files.release(replaced).await;
            }
        });
        ReservationGuard {
            sender: Some(sender),
            completion: Some(completion),
            reservation_id,
            attached: self.attached.clone(),
            last_run: None,
        }
    }
}

/// Record the run's end, trying twice: a failed acknowledgement may hide a
/// successful commit, and reapplying this identity-checked transition is safe
/// and never dispatches work. Returns the spill file the end replaced or
/// forgot, to release once the end is settled.
async fn resolve(
    resolver: &ResolveRun,
    request: &IdempotentRequest,
    resolution: &RunResolution,
    staged: Option<&StagedLastRun>,
) -> Result<Option<String>, RunFailure> {
    match resolver.execute(request, resolution, staged).await {
        Ok(replaced) => Ok(replaced),
        Err(_) => resolver.execute(request, resolution, staged).await,
    }
}

/// Preserve evidence when storage recovers, without withholding the original
/// caller's result: record the run as never started, or else as ended unknown,
/// once storage takes it. Startup recovery covers process termination.
async fn resolve_until_recorded(
    resolver: &ResolveRun,
    request: &IdempotentRequest,
    resolution: &RunResolution,
) -> Option<String> {
    let fallback = match resolution {
        RunResolution::Undispatched => RunResolution::Undispatched,
        _ => RunResolution::Indeterminate,
    };
    loop {
        match resolver.execute(request, &fallback, None).await {
            Ok(replaced) => return replaced,
            Err(error) if database_has_closed(&error) => return None,
            Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

/// The spill files a run's end may leave no record naming: the one the
/// session's last run was in before the run ended, the one the end replaced
/// (another run's may have come between), and the one staged for this run.
struct LastRunFiles<'a> {
    last_runs: &'a dyn LastRunStaging,
    session_id: &'a str,
    before: Option<String>,
    staged: Option<String>,
}

impl<'a> LastRunFiles<'a> {
    async fn before(last_runs: &'a dyn LastRunStaging, session_id: &'a str) -> Self {
        // If this read fails, the only file lost track of is one replaced by
        // an end whose acknowledgement was lost; the startup sweep deletes it.
        let before = last_runs.spill_file(session_id).await.ok().flatten();
        Self {
            last_runs,
            session_id,
            before,
            staged: None,
        }
    }

    /// Seal the run's last-run JSON once, so a retry records the same spill
    /// file. One that cannot be staged leaves the session with no last run.
    async fn stage(&mut self, json: Vec<u8>) -> Option<StagedLastRun> {
        match self.last_runs.stage(self.session_id, json).await {
            Ok(staged) => {
                self.staged = staged.spill_file().map(str::to_owned);
                Some(staged)
            }
            Err(error) => {
                tracing::error!(session = self.session_id, %error, "last run not kept");
                None
            }
        }
    }

    /// Delete those no record names now that the run's end is settled, however
    /// it settled: replaced, never written, or written and then lost.
    async fn release(self, replaced: Option<String>) {
        let files = [self.before, replaced, self.staged]
            .into_iter()
            .flatten()
            .collect();
        self.last_runs.release(self.session_id, files).await;
    }
}

fn lock_attached(
    attached: &Mutex<HashMap<RunId, RunId>>,
) -> std::sync::MutexGuard<'_, HashMap<RunId, RunId>> {
    // A poisoned map may contain a partial update. AGENTS.md's poisoned-lock
    // exception accepts a panic instead of recovery.
    attached.lock().expect("attached runs poisoned")
}

fn database_has_closed(error: &RunFailure) -> bool {
    let RunFailure::Store(crate::application::error::StoreError::Database(source)) = error else {
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
    sender: Option<oneshot::Sender<(RunResolution, Option<Vec<u8>>)>>,
    completion: Option<oneshot::Receiver<()>>,
    reservation_id: RunId,
    attached: AttachedRuns,
    /// The run's last-run JSON, recorded with its end.
    last_run: Option<Vec<u8>>,
}
impl ReservationGuard {
    /// Name the run executing under this reservation, so a duplicate request is
    /// answered with that run's state instead of waiting for it to finish.
    pub fn attach_run(&self, run_id: &RunId) {
        lock_attached(&self.attached).insert(self.reservation_id.clone(), run_id.to_owned());
    }

    /// What the session's last run becomes when this run ends on its own. A run
    /// that ends without one leaves the session with none.
    pub fn set_last_run(&mut self, run: &crate::session::LastRun) {
        match run.encode() {
            Ok(json) => self.last_run = Some(json),
            Err(error) => tracing::error!(%error, "last run not kept"),
        }
    }

    pub async fn complete(self, status: u16, body: String) {
        self.resolve(RunResolution::Completed(RecordedOutcome { status, body }))
            .await;
    }
    pub async fn finish_unkeyed(self) {
        self.resolve(RunResolution::Finished).await;
    }
    pub async fn release(self) {
        self.resolve(RunResolution::Undispatched).await;
    }
    async fn resolve(mut self, resolution: RunResolution) {
        self.detach();
        if let Some(sender) = self.sender.take() {
            let _ = sender.send((resolution, self.last_run.take()));
        }
        if let Some(completion) = self.completion.take() {
            let _ = completion.await;
        }
    }

    /// A settled reservation has no run to attach to: a duplicate reads its
    /// recorded state instead.
    fn detach(&self) {
        lock_attached(&self.attached).remove(&self.reservation_id);
    }
}

impl Drop for ReservationGuard {
    fn drop(&mut self) {
        self.detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::unit_of_work::SqliteUnitOfWorkFactory;
    use crate::application::sessions::close::CloseSession;
    use crate::database::ServerDatabase;
    use crate::domain::idempotent_request::RequestState;
    use crate::domain::session::{RootVfs, Session, SessionBinding, SessionId, SessionLifetime};

    struct Audit;
    impl crate::application::sessions::ports::AuditLog for Audit {
        fn record(&self, _: &str, _: crate::application::sessions::ports::SessionEvent) {}
    }

    pub(super) async fn read_request(
        units: &dyn UnitOfWorkFactory,
        session: &str,
        key: &str,
    ) -> Result<Option<IdempotentRequest>, crate::application::error::StoreError> {
        units.begin().await?.run_for_key(session, key).await
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
    async fn responses_past_retention_become_tombstones_that_still_refuse_to_run() {
        use crate::application::sessions::runs::ExpireResponses;
        use crate::domain::idempotent_request::{RequestState, ResponseRetention};
        let (_, units, coordinator) = fixture().await;
        drop(proceed(coordinator.reserve("session", "k0", "code").await));
        for key in ["k1", "k2", "k3", "k4", "k5"] {
            proceed(coordinator.reserve("session", key, "code").await)
                .complete(201, format!("body of {key}"))
                .await;
        }
        let expire = ExpireResponses::new(units.as_ref(), ResponseRetention::default());
        let now = SystemTime::now();
        assert_eq!(
            expire.execute(now).await.unwrap(),
            0,
            "nothing is past the TTL yet"
        );
        let later = now + Duration::from_secs(25 * 60 * 60);
        assert_eq!(expire.execute(later).await.unwrap(), 2);
        assert_eq!(expire.execute(later).await.unwrap(), 0);

        for key in ["k3", "k4", "k5"] {
            match coordinator.reserve("session", key, "code").await {
                Reservation::Replay(outcome) => {
                    assert_eq!(outcome.body, format!("body of {key}"));
                }
                _ => panic!("{key} is among the newest and still replays"),
            }
        }
        for key in ["k1", "k2"] {
            assert!(matches!(
                coordinator.reserve("session", key, "code").await,
                Reservation::Refused(Refusal::Expired)
            ));
            assert_eq!(
                read_request(units.as_ref(), "session", key)
                    .await
                    .unwrap()
                    .unwrap()
                    .state(),
                &RequestState::Expired
            );
        }
        assert!(matches!(
            coordinator.reserve("session", "k1", "different").await,
            Reservation::Refused(Refusal::Conflict)
        ));
        assert_eq!(
            read_request(units.as_ref(), "session", "k0")
                .await
                .unwrap()
                .unwrap()
                .state(),
            &RequestState::Indeterminate,
            "only completed responses expire"
        );
        let mut unit = units.begin().await.unwrap();
        assert_eq!(
            unit.expire_responses("session", vec!["k0".into(), "k1".into()])
                .await
                .unwrap(),
            0,
            "a request that is no longer completed is left alone"
        );
        drop(unit);
        coordinator.drain().await;
    }

    #[tokio::test]
    async fn each_session_due_expires_in_its_own_unit_and_the_counts_add_up() {
        use crate::application::sessions::runs::ExpireResponses;
        use crate::domain::idempotent_request::{RequestState, ResponseRetention};
        let (_, units, coordinator) = fixture().await;
        let mut unit = units.begin().await.unwrap();
        unit.save_session(
            Session::create(
                SessionId::parse("other".into()).unwrap(),
                SessionBinding::new("test".into(), Default::default()).unwrap(),
                RootVfs::None,
                SessionLifetime::new(Duration::from_secs(3600), SystemTime::now()),
            )
            .unwrap(),
        )
        .await
        .unwrap();
        unit.commit().await.unwrap();
        for session in ["session", "other"] {
            for key in ["a", "b", "c"] {
                proceed(coordinator.reserve(session, key, "code").await)
                    .complete(200, "body".into())
                    .await;
            }
        }
        let retention = ResponseRetention {
            ttl: Duration::from_secs(60),
            keep_last: 1,
        };
        let later = SystemTime::now() + Duration::from_secs(120);
        assert_eq!(
            ExpireResponses::new(units.as_ref(), retention)
                .execute(later)
                .await
                .unwrap(),
            4
        );
        for session in ["session", "other"] {
            for (key, kept) in [("a", false), ("b", false), ("c", true)] {
                let state = read_request(units.as_ref(), session, key)
                    .await
                    .unwrap()
                    .unwrap()
                    .state()
                    .clone();
                assert_eq!(
                    matches!(state, RequestState::Completed(_)),
                    kept,
                    "{session}/{key}: {state:?}"
                );
            }
        }
        coordinator.drain().await;
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
    async fn a_duplicate_of_a_run_that_may_suspend_attaches_to_it_at_once() {
        let (_, _, coordinator) = fixture().await;
        let guard = proceed(coordinator.reserve("session", "key", "code").await);
        guard.attach_run(&"run-1".into());
        let attached = tokio::time::timeout(
            Duration::from_secs(1),
            coordinator.reserve("session", "key", "code"),
        )
        .await
        .expect("answered without waiting for the run");
        assert!(matches!(attached, Reservation::Attached(run) if run.as_str() == "run-1"));
        guard.complete(200, "{}".into()).await;
        match coordinator.reserve("session", "key", "code").await {
            Reservation::Replay(outcome) => assert_eq!(outcome.body, "{}"),
            _ => panic!("a settled run replays its outcome"),
        }
    }

    #[tokio::test]
    async fn recovery_keeps_current_owner_but_retires_previous_generation() {
        let (database, units, coordinator) = fixture().await;
        let guard = proceed(coordinator.reserve("session", "key", "code").await);
        coordinator.recover().await.unwrap();
        assert_eq!(
            read_request(units.as_ref(), "session", "key")
                .await
                .unwrap()
                .unwrap()
                .state(),
            &RequestState::Reserved
        );
        let next = Coordinator::new(units.clone(), "next-generation".into());
        next.recover().await.unwrap();
        assert_eq!(
            read_request(units.as_ref(), "session", "key")
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
    async fn closure_fences_runs_atomically_and_completion_cannot_resurrect_them() {
        let (_, units, coordinator) = fixture().await;
        let guard = proceed(coordinator.reserve("session", "key", "code").await);
        let mut unit = units.begin().await.unwrap();
        let mut session = unit.get_session("session").await.unwrap().unwrap();
        session.close(crate::domain::session::ClosedReason::Deleted);
        unit.save_session(session).await.unwrap();
        crate::application::sessions::runs::cancel_session_runs(unit.as_mut(), "session")
            .await
            .unwrap();
        drop(unit);
        assert!(
            read_request(units.as_ref(), "session", "key")
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
            read_request(units.as_ref(), "session", "key")
                .await
                .unwrap()
                .is_some_and(|run| run.state() == &RequestState::Cancelled)
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
            sqlx::query("CREATE TRIGGER reject_completion BEFORE UPDATE ON runs WHEN NEW.state='completed' BEGIN SELECT RAISE(FAIL, 'injected'); END").execute(connection).await?;
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

#[cfg(test)]
mod admission_tests;

#[cfg(test)]
mod last_run_tests;
