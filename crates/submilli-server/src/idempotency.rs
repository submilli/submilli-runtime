//! Reservation coordinator behind `Idempotency-Key` on the session execute
//! endpoint.
//!
//! [`Coordinator::reserve`] is the single decision point. It validates the key,
//! classifies it against both the in-process map of running executions and the
//! durable ledger, and returns one of: proceed under a fresh reservation, replay
//! a recorded outcome, or a refusal. A duplicate that arrives while the original
//! is still running waits for it rather than executing or refusing.
//!
//! Two things decide correctness here.
//!
//! **A waiter arms itself before it can miss a wake-up.** `notify_waiters`
//! stores no permit, so a broadcast a waiter is not yet listening for is gone.
//! [`tokio::sync::Notify::notified`] closes this by snapshotting the broadcast
//! count when the future is *constructed* and completing on first poll if it
//! has advanced — so the future must be constructed under the same lock the
//! completer takes to publish its resolution, even though it is awaited after
//! that lock is released.
//!
//! **A waiter carries its own bound.** The runtime's execution watchdog
//! (`RuntimeConfig::arm_timeout`) is optional and bounds the *executor*, not the
//! waiter, so it cannot substitute for this.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use axum::http::StatusCode;
use tokio::sync::Notify;

use crate::idempotency_store::{
    EntryState, IdempotencyStore, LedgerEntry, RecordedOutcome, code_fingerprint,
};

/// Longest key the ledger can store. Derived, not chosen: the on-disk layout
/// writes `<hex(key)>.json`, and hex doubles the byte length, so a longer key
/// would produce a file name past the 255-byte `NAME_MAX` on ext4 and APFS —
/// and the reservation would fail with `ENAMETOOLONG` at the exact moment the
/// design depends on it succeeding. A layout change must re-derive this bound
/// rather than inherit it.
pub const MAX_KEY_BYTES: usize = 120;

/// How long a duplicate waits for the execution it is shadowing before giving
/// up and reporting the outcome unknown.
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

/// How an in-flight reservation ended, as seen by a waiter.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Resolution {
    Completed(RecordedOutcome),
    /// The guard was dropped without an outcome.
    Indeterminate,
    /// The holder never got a durable reservation, or released one because
    /// nothing was dispatched. Nothing ran, so a waiter may classify again.
    Vacated,
}

/// The state of a claim as seen by a waiter whose bound has just expired.
#[derive(Debug, PartialEq, Eq)]
enum ExpiredWait {
    /// Still the live claim, still unresolved: the original execution really is
    /// in flight and will record an outcome.
    StillRunning,
    /// A resolution landed in the gap between the timeout and this read, or the
    /// slot changed hands. There is an answer — go and read it.
    Answered,
    /// No live claim and nothing published, so no one will ever answer for it.
    Abandoned,
}

/// One running execution, shared between its holder and any waiters.
struct InFlight {
    fingerprint: String,
    resolution: Mutex<Option<Resolution>>,
    notify: Notify,
}

impl InFlight {
    fn new(fingerprint: String) -> Self {
        Self {
            fingerprint,
            resolution: Mutex::new(None),
            notify: Notify::new(),
        }
    }

    /// Publish the outcome and wake every waiter, both under the lock a waiter
    /// holds while arming itself. First writer wins: an outcome already
    /// published is what waiters see, so a guard dropping after `complete` does
    /// not overwrite it with `Indeterminate`.
    fn resolve(&self, resolution: Resolution) {
        let mut slot = self.lock();
        if slot.is_none() {
            *slot = Some(resolution);
        }
        self.notify.notify_waiters();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Resolution>> {
        // A poisoned resolution may contain a partial update. AGENTS.md's
        // poisoned-lock exception accepts a panic instead of recovery, including in
        // guard cleanup, where a second panic during unwinding can abort the process.
        self.resolution
            .lock()
            .expect("idempotency resolution poisoned")
    }
}

/// Test-only injection point for the completion-races-a-waiter case.
#[cfg(test)]
type BeforeWaitHook = Arc<dyn Fn(&Arc<InFlight>) + Send + Sync>;

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct EntryKey {
    session_id: String,
    key: String,
}

pub struct Coordinator {
    store: Arc<dyn IdempotencyStore>,
    inflight: Mutex<HashMap<EntryKey, Arc<InFlight>>>,
    waiter_timeout: Duration,
    /// Fires after a waiter has armed itself and released the resolution lock,
    /// but before it awaits — the window a completion would race. Tests drive
    /// that race deterministically through it rather than gambling on timing.
    #[cfg(test)]
    before_wait: Mutex<Option<BeforeWaitHook>>,
}

impl Coordinator {
    pub fn new(store: Arc<dyn IdempotencyStore>) -> Self {
        Self {
            store,
            inflight: Mutex::new(HashMap::new()),
            waiter_timeout: DEFAULT_WAITER_TIMEOUT,
            #[cfg(test)]
            before_wait: Mutex::new(None),
        }
    }

    #[cfg(test)]
    fn with_waiter_timeout(mut self, timeout: Duration) -> Self {
        self.waiter_timeout = timeout;
        self
    }

    #[cfg(test)]
    fn set_before_wait(&self, hook: BeforeWaitHook) {
        *self.before_wait.lock().expect("hook poisoned") = Some(hook);
    }

    #[cfg(test)]
    fn run_before_wait_hook(&self, handle: &Arc<InFlight>) {
        let hook = self.before_wait.lock().expect("hook poisoned").clone();
        if let Some(hook) = hook {
            hook(handle);
        }
    }

    /// Classify `key` for this session and code, taking a durable reservation
    /// when the program should run.
    pub async fn reserve(self: &Arc<Self>, session_id: &str, key: &str, code: &str) -> Reservation {
        if let Err(reason) = validate_key(key) {
            return Reservation::Refused(Refusal::InvalidKey(reason));
        }
        let fingerprint = code_fingerprint(code);
        let entry_key = EntryKey {
            session_id: session_id.to_string(),
            key: key.to_string(),
        };

        loop {
            if let Some(handle) = self.live_handle(&entry_key) {
                if handle.fingerprint != fingerprint {
                    return Reservation::Refused(Refusal::Conflict);
                }
                match self.await_resolution(&handle).await {
                    Some(Resolution::Completed(outcome)) => {
                        return Reservation::Replay(outcome);
                    }
                    Some(Resolution::Indeterminate) => {
                        return Reservation::Refused(Refusal::Indeterminate);
                    }
                    // Our own bound expired. Whether that means "unknown" or
                    // "not yet" is decided by the holder, not the clock: a
                    // handle still live and unresolved is an execution still
                    // running, and reporting that as permanently unknown would
                    // be wrong — it may complete successfully a moment later.
                    //
                    // The holder can also finish in the gap between our timeout
                    // and this check. That is a real answer, so reclassify from
                    // scratch rather than refusing: telling a caller the
                    // outcome is unknown while it sits recorded on disk is the
                    // one mistake this branch must not make.
                    None => match self.classify_expired_wait(&entry_key, &handle) {
                        ExpiredWait::StillRunning => {
                            return Reservation::Refused(Refusal::InProgress);
                        }
                        ExpiredWait::Answered => continue,
                        ExpiredWait::Abandoned => {
                            return Reservation::Refused(Refusal::Indeterminate);
                        }
                    },
                    // Nothing ran under that handle — classify again from scratch.
                    Some(Resolution::Vacated) => continue,
                }
            }

            // Claim the slot *before* reading the ledger. Owning it is what
            // makes this key's on-disk state stable across the read below:
            // otherwise a duplicate could record its outcome between our read
            // and our write, and the fresh reservation would overwrite it.
            let handle = Arc::new(InFlight::new(fingerprint.clone()));
            {
                let mut map = self.lock();
                if map.contains_key(&entry_key) {
                    continue;
                }
                map.insert(entry_key.clone(), Arc::clone(&handle));
            }
            // From here the slot is owned by a guard, so every way out of this
            // block releases it — including the ways that are not returns. Both
            // awaits below are cancellation points, and a request dropped at
            // either one would otherwise strand the claim with no resolution
            // and no owner: every later request for that key would then wait
            // out its full bound and be refused, for the life of the process.
            let claim = ClaimGuard::new(Arc::clone(self), entry_key.clone(), Arc::clone(&handle));

            match self.store.load(session_id, key).await {
                Err(err) => {
                    tracing::warn!(?err, "idempotency ledger read failed; refusing the request");
                    return Reservation::Refused(Refusal::Unavailable(format!("{err:?}")));
                }
                Ok(Some(entry)) if entry.fingerprint != fingerprint => {
                    return Reservation::Refused(Refusal::Conflict);
                }
                Ok(Some(entry)) => {
                    return match entry.state {
                        EntryState::Completed(outcome) => Reservation::Replay(outcome),
                        // Reserved on disk with nothing live holding it: the
                        // writer never recorded an outcome. It crashed, or its
                        // guard dropped. Either way the program may have run.
                        EntryState::Reserved => Reservation::Refused(Refusal::Indeterminate),
                    };
                }
                Ok(None) => {}
            }

            // The reservation is fsynced before the program starts, on the
            // request's hot path, deliberately. The ledger only helps if it
            // survives a crash, so `reserved` must be physically on disk before
            // anything can execute. That rules out the debounce
            // `SessionManager::touch` uses for `last_activity`
            // (`PERSIST_INTERVAL`): a reservation the server has not yet
            // written is a reservation that does not exist. The cost is real —
            // this write plus the outcome write are roughly four fsyncs per
            // keyed execute — and is accepted knowingly.
            let reserved = LedgerEntry::reserved(session_id, key, fingerprint.clone());
            // Carried onto the guard so the outcome write preserves it. The
            // entry's age is measured from the reservation, not from whenever
            // the program happened to finish.
            let created_at = reserved.created_at;
            if let Err(err) = self.store.put(reserved).await {
                tracing::warn!(
                    ?err,
                    "idempotency reservation write failed; refusing the request"
                );
                return Reservation::Refused(Refusal::Unavailable(format!("{err:?}")));
            }

            // The reservation is durable, so ownership of the slot passes to
            // the caller: from here an abandoned request means indeterminate,
            // not vacated.
            return Reservation::Proceed(claim.into_reservation(fingerprint, created_at));
        }
    }

    fn live_handle(&self, entry_key: &EntryKey) -> Option<Arc<InFlight>> {
        self.lock().get(entry_key).map(Arc::clone)
    }

    /// Wait for `handle` to resolve, bounded by this coordinator's waiter
    /// timeout. `None` means the bound expired.
    async fn await_resolution(&self, handle: &Arc<InFlight>) -> Option<Resolution> {
        let notified = {
            let slot = handle.lock();
            if let Some(resolution) = slot.clone() {
                return Some(resolution);
            }
            // Construct the future while the resolution lock is still held.
            // `Notify::notified` snapshots the `notify_waiters` broadcast count
            // at construction and completes on first poll if it has advanced
            // since — so a future built here cannot miss a wake-up published
            // after we release the lock, even though it is not polled until
            // below. Build it after releasing the lock and that guarantee is
            // gone: a completion in the gap bumps the count first, the snapshot
            // is taken after it, and the waiter sleeps to its bound.
            handle.notify.notified()
        };
        #[cfg(test)]
        self.run_before_wait_hook(handle);
        match tokio::time::timeout(self.waiter_timeout, notified).await {
            Ok(()) => handle.lock().clone(),
            // Re-read rather than assuming: a resolution published as the timer
            // fired is a real answer and must not be thrown away for a refusal.
            Err(_) => handle.lock().clone(),
        }
    }

    /// What a waiter whose bound expired should do about `handle`.
    ///
    /// Read the claim and the resolution together, because the interesting case
    /// is the holder finishing in the gap between the two: every `unclaim` is
    /// preceded by a `resolve`, so a handle that is no longer the live claim has
    /// an answer waiting, and reporting *that* as unknown would deny a caller an
    /// outcome already recorded on disk.
    fn classify_expired_wait(&self, entry_key: &EntryKey, handle: &Arc<InFlight>) -> ExpiredWait {
        let live = self
            .lock()
            .get(entry_key)
            .is_some_and(|live| Arc::ptr_eq(live, handle));
        let resolved = handle.lock().is_some();
        match (live, resolved) {
            (true, false) => ExpiredWait::StillRunning,
            (_, true) => ExpiredWait::Answered,
            // Unclaimed with nothing published. `vacate`, `complete`, `release`
            // and `Drop` all resolve before unclaiming, so this is unreachable
            // today; treat it as unknown rather than as a reason to re-run.
            (false, false) => ExpiredWait::Abandoned,
        }
    }

    /// Release the slot with no outcome recorded and let waiters reclassify.
    fn vacate(&self, entry_key: &EntryKey, handle: &Arc<InFlight>) {
        handle.resolve(Resolution::Vacated);
        self.unclaim(entry_key, handle);
    }

    /// Drop our claim, but only if it is still ours — a later reservation for
    /// the same key must not be evicted by a straggler.
    fn unclaim(&self, entry_key: &EntryKey, handle: &Arc<InFlight>) {
        let mut map = self.lock();
        if map
            .get(entry_key)
            .is_some_and(|live| Arc::ptr_eq(live, handle))
        {
            map.remove(entry_key);
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<EntryKey, Arc<InFlight>>> {
        // Poison may leave the in-flight claims partly updated. AGENTS.md's
        // poisoned-lock exception accepts a panic instead of recovery, including in
        // guard cleanup, where a second panic during unwinding can abort the process.
        self.inflight
            .lock()
            .expect("idempotency coordinator poisoned")
    }
}

/// Owns the in-flight slot between claiming it and handing it to a
/// [`ReservationGuard`]. Nothing has executed yet while this is armed, so
/// dropping it releases the slot as vacated — waiters reclassify and a retry
/// runs, which is the honest answer when no reservation ever became durable.
struct ClaimGuard {
    coordinator: Arc<Coordinator>,
    entry_key: EntryKey,
    handle: Arc<InFlight>,
    armed: bool,
}

impl ClaimGuard {
    fn new(coordinator: Arc<Coordinator>, entry_key: EntryKey, handle: Arc<InFlight>) -> Self {
        Self {
            coordinator,
            entry_key,
            handle,
            armed: true,
        }
    }

    /// Pass the slot to the reservation the caller will execute under. The
    /// meaning of an abandoned request flips here: before this point nothing
    /// ran, after it the program may have.
    fn into_reservation(mut self, fingerprint: String, created_at: SystemTime) -> ReservationGuard {
        self.armed = false;
        ReservationGuard {
            coordinator: Arc::clone(&self.coordinator),
            entry_key: self.entry_key.clone(),
            fingerprint,
            created_at,
            handle: Arc::clone(&self.handle),
            armed: true,
        }
    }
}

impl Drop for ClaimGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.coordinator.vacate(&self.entry_key, &self.handle);
    }
}

/// Held for the duration of one execution. Settle it with [`Self::complete`] or
/// [`Self::release`]; dropping it armed reports the outcome unknown.
pub struct ReservationGuard {
    coordinator: Arc<Coordinator>,
    entry_key: EntryKey,
    fingerprint: String,
    /// The reservation's timestamp, so the outcome write preserves it instead
    /// of restamping the entry with the completion time.
    created_at: SystemTime,
    handle: Arc<InFlight>,
    armed: bool,
}

impl ReservationGuard {
    /// Record what the caller is about to receive, and hand the same bytes to
    /// any waiter.
    pub async fn complete(mut self, status: u16, body: String) {
        let outcome = RecordedOutcome { status, body };
        // Publish to waiters *before* the durable write. `resolve` is
        // synchronous and therefore uncancellable, whereas the write below is
        // an await point: a request dropped inside it would otherwise fall to
        // this guard's `Drop` and tell every waiter the outcome was unknown
        // when it is known — and may already be on disk. Ordering it this way
        // also matches the policy below, where a *failed* write still resolves
        // as completed.
        self.handle.resolve(Resolution::Completed(outcome.clone()));
        let entry = LedgerEntry {
            session_id: self.entry_key.session_id.clone(),
            key: self.entry_key.key.clone(),
            fingerprint: self.fingerprint.clone(),
            state: EntryState::Completed(outcome),
            created_at: self.created_at,
        };
        if let Err(err) = self.coordinator.store.put(entry).await {
            // The program ran and the caller is about to receive its response;
            // refusing now would throw that output away. The entry stays
            // `reserved` on disk, so a later retry is refused as indeterminate
            // — which is honest, because we cannot prove the outcome landed.
            tracing::warn!(?err, "failed to record idempotency outcome");
        }
        self.disarm();
    }

    /// Nothing was dispatched, so nothing can have happened. Drop the
    /// reservation entirely: a retry with this key must run.
    pub async fn release(mut self) {
        // Publish before the durable remove, for the same reason `complete`
        // does: `resolve` is synchronous and therefore uncancellable, while the
        // remove is a blocking-pool round trip a client disconnect can drop us
        // inside. Resolve second and that disconnect falls through to `Drop`,
        // which tells every waiter the outcome is unknown — about a program
        // that never reached the runner at all.
        self.handle.resolve(Resolution::Vacated);
        if let Err(err) = self
            .coordinator
            .store
            .remove(&self.entry_key.session_id, &self.entry_key.key)
            .await
        {
            tracing::warn!(?err, "failed to release idempotency reservation");
        }
        self.disarm();
    }

    fn disarm(&mut self) {
        self.armed = false;
        self.coordinator.unclaim(&self.entry_key, &self.handle);
    }
}

impl Drop for ReservationGuard {
    /// Whatever released this reservation — a client disconnect dropping the
    /// handler future, a panic, an early return — the program may already have
    /// written files or issued HTTP calls. "Cancelled" and "crashed" are the
    /// same epistemic state, so the key resolves to indeterminate rather than
    /// being deleted for a retry to re-run.
    ///
    /// Everything here is synchronous. `Drop` cannot await the store's `put`,
    /// and spawning a detached write would be worse than useless: the task is
    /// not guaranteed to run on shutdown or panic-abort, which is exactly when
    /// it would matter. It does not need to — the on-disk entry is still
    /// `reserved`, and a `reserved` entry with nothing live holding it *is*
    /// indeterminate to every later reader. Durability falls out of the write
    /// that already happened.
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.handle.resolve(Resolution::Indeterminate);
        self.coordinator.unclaim(&self.entry_key, &self.handle);
    }
}

fn validate_key(key: &str) -> Result<(), &'static str> {
    if key.is_empty() {
        return Err("Idempotency-Key must not be empty");
    }
    if key.len() > MAX_KEY_BYTES {
        return Err("Idempotency-Key must be at most 120 bytes");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blueprint::StoreError;
    use crate::idempotency_store::InMemoryIdempotencyStore;

    const FAST: Duration = Duration::from_millis(200);

    fn coordinator() -> Arc<Coordinator> {
        Arc::new(
            Coordinator::new(Arc::new(InMemoryIdempotencyStore::default()))
                .with_waiter_timeout(FAST),
        )
    }

    fn with_store(store: Arc<dyn IdempotencyStore>) -> Arc<Coordinator> {
        Arc::new(Coordinator::new(store).with_waiter_timeout(FAST))
    }

    /// A ledger whose writes always fail. Reads still work, so a test can drive
    /// the reservation path specifically.
    struct FailingWrites;

    #[async_trait::async_trait]
    impl IdempotencyStore for FailingWrites {
        async fn put(&self, _entry: LedgerEntry) -> Result<(), StoreError> {
            Err(StoreError::Io("disk on fire".into()))
        }
        async fn load(&self, _: &str, _: &str) -> Result<Option<LedgerEntry>, StoreError> {
            Ok(None)
        }
        async fn remove(&self, _: &str, _: &str) -> Result<(), StoreError> {
            Ok(())
        }
        async fn purge_session(&self, _: &str) -> Result<(), StoreError> {
            Ok(())
        }
        async fn session_ids(&self) -> Result<Vec<String>, StoreError> {
            Ok(Vec::new())
        }
    }

    fn expect_guard(reservation: Reservation) -> ReservationGuard {
        match reservation {
            Reservation::Proceed(guard) => guard,
            Reservation::Replay(_) => panic!("expected a fresh reservation, got a replay"),
            Reservation::Refused(refusal) => {
                panic!("expected a fresh reservation, got {refusal:?}")
            }
        }
    }

    fn expect_refusal(reservation: Reservation) -> Refusal {
        match reservation {
            Reservation::Refused(refusal) => refusal,
            Reservation::Proceed(_) => panic!("expected a refusal, got a reservation"),
            Reservation::Replay(_) => panic!("expected a refusal, got a replay"),
        }
    }

    fn expect_replay(reservation: Reservation) -> RecordedOutcome {
        match reservation {
            Reservation::Replay(outcome) => outcome,
            Reservation::Proceed(_) => panic!("expected a replay, got a reservation"),
            Reservation::Refused(refusal) => panic!("expected a replay, got {refusal:?}"),
        }
    }

    // --- the guard -------------------------------------------------------

    /// Every assertion here is about the *guard*, so each must fail if the
    /// guard stops working. Without a `Drop` impl the claim would simply be
    /// abandoned in the map and the waiter bound would eventually produce the
    /// same `Indeterminate` — a pass for the wrong reason. The timing and
    /// emptiness assertions are what separate the two.
    #[tokio::test]
    async fn an_abandoned_reservation_is_indeterminate() {
        let coordinator = coordinator();
        // Reserve, then drop the guard without settling it — the shape of a
        // client disconnect, a panic, or an early return.
        drop(expect_guard(coordinator.reserve("sid", "k", "main").await));
        assert!(
            coordinator.lock().is_empty(),
            "a dropped guard must give up its claim"
        );

        let started = std::time::Instant::now();
        assert_eq!(
            expect_refusal(coordinator.reserve("sid", "k", "main").await),
            Refusal::Indeterminate,
        );
        assert!(
            started.elapsed() < FAST,
            "the refusal must come from the ledger, not from waiting out the bound"
        );
    }

    #[tokio::test]
    async fn a_waiter_on_an_abandoned_reservation_is_released() {
        let coordinator = coordinator();
        let guard = expect_guard(coordinator.reserve("sid", "k", "main").await);

        let waiter = {
            let coordinator = Arc::clone(&coordinator);
            tokio::spawn(async move { coordinator.reserve("sid", "k", "main").await })
        };
        tokio::task::yield_now().await;
        let started = std::time::Instant::now();
        drop(guard);

        assert_eq!(
            expect_refusal(waiter.await.unwrap()),
            Refusal::Indeterminate,
        );
        assert!(
            started.elapsed() < FAST,
            "the waiter must be woken by the guard, not left to time out"
        );
    }

    /// An entry's age is measured from when the key was reserved, not from
    /// whenever the program happened to finish. Restamping on completion would
    /// make the slowest executions look like the freshest entries, so any
    /// policy evicting by age would keep exactly the wrong ones.
    #[tokio::test]
    async fn completing_a_reservation_keeps_its_original_timestamp() {
        let store = Arc::new(InMemoryIdempotencyStore::default());
        let coordinator = with_store(Arc::clone(&store) as Arc<dyn IdempotencyStore>);

        let guard = expect_guard(coordinator.reserve("sid", "k", "main").await);
        let reserved_at = store.load("sid", "k").await.unwrap().unwrap().created_at;

        // Long enough to move the stored millisecond, so a restamp is visible.
        tokio::time::sleep(Duration::from_millis(5)).await;
        guard.complete(200, "done".to_string()).await;

        let entry = store.load("sid", "k").await.unwrap().unwrap();
        assert!(matches!(entry.state, EntryState::Completed(_)));
        assert_eq!(
            entry.created_at, reserved_at,
            "completion must carry the reservation's timestamp, not a fresh one"
        );
    }

    // --- replay and wait -------------------------------------------------

    #[tokio::test]
    async fn a_sequential_duplicate_replays_the_recorded_outcome() {
        let coordinator = coordinator();
        expect_guard(coordinator.reserve("sid", "k", "main").await)
            .complete(200, "{\"result\":\"7\"}".into())
            .await;

        let replayed = expect_replay(coordinator.reserve("sid", "k", "main").await);
        assert_eq!(replayed.status, 200);
        assert_eq!(replayed.body, "{\"result\":\"7\"}");
    }

    #[tokio::test]
    async fn a_concurrent_duplicate_waits_and_gets_the_same_outcome() {
        let coordinator = coordinator();
        // The hold point: the guard stays alive until the test releases it, so
        // the duplicate provably arrives mid-execution and takes the wait
        // branch rather than the replay branch.
        let guard = expect_guard(coordinator.reserve("sid", "k", "main").await);

        let waiter = {
            let coordinator = Arc::clone(&coordinator);
            tokio::spawn(async move { coordinator.reserve("sid", "k", "main").await })
        };
        tokio::task::yield_now().await;
        guard.complete(200, "once".into()).await;

        let replayed = expect_replay(waiter.await.unwrap());
        assert_eq!(replayed.body, "once");
    }

    #[tokio::test]
    async fn two_simultaneous_waiters_are_both_released() {
        let coordinator = coordinator();
        let guard = expect_guard(coordinator.reserve("sid", "k", "main").await);

        let waiters: Vec<_> = (0..2)
            .map(|_| {
                let coordinator = Arc::clone(&coordinator);
                tokio::spawn(async move { coordinator.reserve("sid", "k", "main").await })
            })
            .collect();
        tokio::task::yield_now().await;
        guard.complete(200, "once".into()).await;

        for waiter in waiters {
            assert_eq!(expect_replay(waiter.await.unwrap()).body, "once");
        }
    }

    /// A completion landing between a waiter's classification and its await
    /// must still release it. The hook fires the completion in exactly that
    /// window, so this is deterministic rather than a timing gamble: with
    /// registration inside the resolution lock the waiter is already listening
    /// and wakes at once; move it out and `notify_waiters` — which stores no
    /// permit — has nobody to wake, and the waiter hangs to its bound.
    #[tokio::test]
    async fn a_completion_racing_registration_still_releases_the_waiter() {
        let coordinator = coordinator();
        let _guard = expect_guard(coordinator.reserve("sid", "k", "main").await);
        coordinator.set_before_wait(Arc::new(|handle: &Arc<InFlight>| {
            handle.resolve(Resolution::Completed(RecordedOutcome {
                status: 200,
                body: "once".into(),
            }));
        }));

        let started = std::time::Instant::now();
        let replayed = expect_replay(coordinator.reserve("sid", "k", "main").await);

        assert_eq!(replayed.body, "once");
        assert!(
            started.elapsed() < FAST,
            "the waiter must wake on the notification, not wait out its bound"
        );
    }

    /// A holder that outlives the waiter's bound is still *running*, not
    /// abandoned. Reporting it as terminal `Indeterminate` would be a lie the
    /// holder may contradict a moment later by completing successfully.
    #[tokio::test]
    async fn a_waiter_that_outlives_its_bound_reports_the_original_still_running() {
        let coordinator = coordinator();
        // Leaked on purpose: the guard stays live and unresolved, so only the
        // waiter's own bound can end the wait.
        std::mem::forget(expect_guard(coordinator.reserve("sid", "k", "main").await));

        let started = std::time::Instant::now();
        assert_eq!(
            expect_refusal(coordinator.reserve("sid", "k", "main").await),
            Refusal::InProgress,
        );
        assert!(
            started.elapsed() >= FAST,
            "the waiter must honour its bound"
        );
    }

    /// The other half of that distinction, exercised directly.
    ///
    /// Driving it through `reserve` cannot reach the interesting states: the
    /// holder finishing between the waiter's timeout and its re-read is a race
    /// nanoseconds wide, and a test that merely drops a guard never enters this
    /// function at all — it finds no live handle and classifies from the ledger,
    /// which is what `an_abandoned_reservation_is_indeterminate` already covers.
    /// So build the four states by hand and assert each one.
    #[tokio::test]
    async fn an_expired_wait_reads_the_claim_and_the_resolution_together() {
        let coordinator = coordinator();
        let entry_key = EntryKey {
            session_id: "sid".to_string(),
            key: "k".to_string(),
        };
        let handle = Arc::new(InFlight::new(code_fingerprint("main")));
        coordinator
            .lock()
            .insert(entry_key.clone(), Arc::clone(&handle));

        assert_eq!(
            coordinator.classify_expired_wait(&entry_key, &handle),
            ExpiredWait::StillRunning,
            "a live, unresolved claim is an execution still in flight"
        );

        // Mid-`complete`: resolved, not yet unclaimed.
        handle.resolve(Resolution::Completed(RecordedOutcome {
            status: 200,
            body: "{}".to_string(),
        }));
        assert_eq!(
            coordinator.classify_expired_wait(&entry_key, &handle),
            ExpiredWait::Answered,
            "an outcome published in the race window must not be reported unknown"
        );

        // The ordinary finish: resolved and unclaimed.
        coordinator.lock().remove(&entry_key);
        assert_eq!(
            coordinator.classify_expired_wait(&entry_key, &handle),
            ExpiredWait::Answered,
            "an unclaimed handle still carries the answer it published"
        );

        let never_resolved = Arc::new(InFlight::new(code_fingerprint("main")));
        assert_eq!(
            coordinator.classify_expired_wait(&entry_key, &never_resolved),
            ExpiredWait::Abandoned,
        );
    }

    // --- conflict and indeterminate --------------------------------------

    #[tokio::test]
    async fn a_key_reused_with_different_code_is_a_conflict() {
        let coordinator = coordinator();
        expect_guard(coordinator.reserve("sid", "k", "main").await)
            .complete(200, "original".into())
            .await;

        assert_eq!(
            expect_refusal(coordinator.reserve("sid", "k", "other").await),
            Refusal::Conflict,
        );
        // The original entry is untouched.
        assert_eq!(
            expect_replay(coordinator.reserve("sid", "k", "main").await).body,
            "original",
        );
    }

    #[tokio::test]
    async fn a_conflict_against_an_in_flight_reservation_does_not_wait() {
        let coordinator = coordinator();
        let _guard = expect_guard(coordinator.reserve("sid", "k", "main").await);

        assert_eq!(
            expect_refusal(coordinator.reserve("sid", "k", "other").await),
            Refusal::Conflict,
        );
    }

    #[tokio::test]
    async fn a_fingerprint_mismatch_against_an_indeterminate_entry_is_a_conflict() {
        let coordinator = coordinator();
        drop(expect_guard(coordinator.reserve("sid", "k", "main").await));

        assert_eq!(
            expect_refusal(coordinator.reserve("sid", "k", "other").await),
            Refusal::Conflict,
            "a mismatch is a conflict even when the entry is indeterminate",
        );
    }

    #[tokio::test]
    async fn an_entry_rehydrated_as_reserved_is_indeterminate() {
        let store: Arc<dyn IdempotencyStore> = Arc::new(InMemoryIdempotencyStore::default());
        // A reservation written by a process that is now gone: on disk it is
        // still `reserved`, and no live handle owns it.
        store
            .put(LedgerEntry::reserved("sid", "k", code_fingerprint("main")))
            .await
            .unwrap();

        assert_eq!(
            expect_refusal(with_store(store).reserve("sid", "k", "main").await),
            Refusal::Indeterminate,
        );
    }

    #[tokio::test]
    async fn an_indeterminate_entry_is_refused_every_time_and_never_runs() {
        let coordinator = coordinator();
        drop(expect_guard(coordinator.reserve("sid", "k", "main").await));

        for _ in 0..3 {
            assert_eq!(
                expect_refusal(coordinator.reserve("sid", "k", "main").await),
                Refusal::Indeterminate,
            );
        }
    }

    // --- scoping ---------------------------------------------------------

    #[tokio::test]
    async fn the_same_key_in_two_sessions_reserves_independently() {
        let coordinator = coordinator();
        expect_guard(coordinator.reserve("a", "k", "main").await)
            .complete(200, "from-a".into())
            .await;

        // The second session sees a fresh key, not a's outcome.
        expect_guard(coordinator.reserve("b", "k", "main").await)
            .complete(200, "from-b".into())
            .await;

        assert_eq!(
            expect_replay(coordinator.reserve("a", "k", "main").await).body,
            "from-a",
        );
        assert_eq!(
            expect_replay(coordinator.reserve("b", "k", "main").await).body,
            "from-b",
        );
    }

    // --- release ---------------------------------------------------------

    #[tokio::test]
    async fn a_released_reservation_leaves_no_entry_and_a_retry_runs() {
        let coordinator = coordinator();
        expect_guard(coordinator.reserve("sid", "k", "main").await)
            .release()
            .await;

        // Nothing was dispatched, so the retry must get a fresh reservation.
        expect_guard(coordinator.reserve("sid", "k", "main").await)
            .complete(200, "ran".into())
            .await;
        assert_eq!(
            expect_replay(coordinator.reserve("sid", "k", "main").await).body,
            "ran",
        );
    }

    #[tokio::test]
    async fn a_waiter_on_a_released_reservation_reclassifies_and_runs() {
        let coordinator = coordinator();
        let guard = expect_guard(coordinator.reserve("sid", "k", "main").await);

        let waiter = {
            let coordinator = Arc::clone(&coordinator);
            tokio::spawn(async move { coordinator.reserve("sid", "k", "main").await })
        };
        tokio::task::yield_now().await;
        guard.release().await;

        // Nothing ran, so the waiter becomes the fresh reserver rather than
        // inheriting an outcome that never existed.
        expect_guard(waiter.await.unwrap());
    }

    // --- cancellation ----------------------------------------------------

    /// A store whose `load` parks forever, so a test can drop `reserve`
    /// precisely inside the claim -> durable-write window.
    struct ParkedLoad;

    #[async_trait::async_trait]
    impl IdempotencyStore for ParkedLoad {
        async fn put(&self, _entry: LedgerEntry) -> Result<(), StoreError> {
            Ok(())
        }
        async fn load(&self, _: &str, _: &str) -> Result<Option<LedgerEntry>, StoreError> {
            std::future::pending::<()>().await;
            unreachable!()
        }
        async fn remove(&self, _: &str, _: &str) -> Result<(), StoreError> {
            Ok(())
        }
        async fn purge_session(&self, _: &str) -> Result<(), StoreError> {
            Ok(())
        }
        async fn session_ids(&self) -> Result<Vec<String>, StoreError> {
            Ok(Vec::new())
        }
    }

    /// The window between claiming the slot and owning a reservation contains
    /// two awaits. A request dropped there must not leave the claim behind: a
    /// stranded claim has no owner to resolve it, so every later request for
    /// that key would wait out its bound and be refused for the life of the
    /// process — even though nothing ever executed.
    #[tokio::test]
    async fn cancelling_reserve_before_the_reservation_lands_releases_the_claim() {
        let coordinator = with_store(Arc::new(ParkedLoad));
        {
            let mut reserving = Box::pin(coordinator.reserve("sid", "k", "main"));
            // Poll once to park inside `store.load`, then drop it.
            assert!(futures::poll!(reserving.as_mut()).is_pending());
        }

        assert!(
            coordinator.lock().is_empty(),
            "a cancelled reserve must release its claim"
        );
    }

    /// The claim a cancelled request leaves behind must be *resolved*, not just
    /// removed: a waiter already parked on that handle is woken by the
    /// resolution, and `Vacated` is what tells it to reclassify rather than
    /// inherit an outcome that never existed.
    #[tokio::test]
    async fn a_cancelled_claim_resolves_as_vacated_for_anyone_waiting_on_it() {
        let coordinator = with_store(Arc::new(ParkedLoad));
        let handle = {
            let mut reserving = Box::pin(coordinator.reserve("sid", "k", "main"));
            assert!(futures::poll!(reserving.as_mut()).is_pending());
            let handle = coordinator
                .live_handle(&EntryKey {
                    session_id: "sid".into(),
                    key: "k".into(),
                })
                .expect("the claim is live while reserve is parked");
            drop(reserving);
            handle
        };

        assert_eq!(
            *handle.lock(),
            Some(Resolution::Vacated),
            "nothing ran, so a waiter must be told to reclassify"
        );
    }

    /// `complete` publishes to waiters before its durable write, so a request
    /// cancelled inside that write cannot downgrade a known outcome to
    /// "unknown" for everyone waiting on it.
    #[tokio::test]
    async fn a_waiter_sees_the_outcome_even_if_completion_is_cancelled_mid_write() {
        struct ParkedPut;

        #[async_trait::async_trait]
        impl IdempotencyStore for ParkedPut {
            async fn put(&self, entry: LedgerEntry) -> Result<(), StoreError> {
                // Only the outcome write parks; the reservation write returns.
                if entry.outcome().is_some() {
                    std::future::pending::<()>().await;
                }
                Ok(())
            }
            async fn load(&self, _: &str, _: &str) -> Result<Option<LedgerEntry>, StoreError> {
                Ok(None)
            }
            async fn remove(&self, _: &str, _: &str) -> Result<(), StoreError> {
                Ok(())
            }
            async fn purge_session(&self, _: &str) -> Result<(), StoreError> {
                Ok(())
            }
            async fn session_ids(&self) -> Result<Vec<String>, StoreError> {
                Ok(Vec::new())
            }
        }

        let coordinator = with_store(Arc::new(ParkedPut));
        let guard = expect_guard(coordinator.reserve("sid", "k", "main").await);

        let waiter = {
            let coordinator = Arc::clone(&coordinator);
            tokio::spawn(async move { coordinator.reserve("sid", "k", "main").await })
        };
        tokio::task::yield_now().await;

        {
            let mut completing = Box::pin(guard.complete(200, "recorded".into()));
            assert!(futures::poll!(completing.as_mut()).is_pending());
        }

        assert_eq!(
            expect_replay(waiter.await.unwrap()).body,
            "recorded",
            "a cancelled write must not downgrade a known outcome to indeterminate"
        );
    }

    // --- store failure ----------------------------------------------------

    #[tokio::test]
    async fn a_failed_reservation_write_refuses_the_request() {
        let coordinator = with_store(Arc::new(FailingWrites));

        let refusal = expect_refusal(coordinator.reserve("sid", "k", "main").await);
        assert!(
            matches!(refusal, Refusal::Unavailable(_)),
            "got {refusal:?}"
        );
        assert_eq!(refusal.status(), StatusCode::SERVICE_UNAVAILABLE);
        // The failed claim is not left behind to block the next attempt.
        assert!(coordinator.lock().is_empty());
    }

    // --- key validation ---------------------------------------------------

    #[tokio::test]
    async fn an_empty_key_is_refused_before_any_entry_exists() {
        let store: Arc<dyn IdempotencyStore> = Arc::new(InMemoryIdempotencyStore::default());
        let coordinator = with_store(Arc::clone(&store));

        let refusal = expect_refusal(coordinator.reserve("sid", "", "main").await);
        assert!(matches!(refusal, Refusal::InvalidKey(_)), "got {refusal:?}");
        assert_eq!(refusal.status(), StatusCode::BAD_REQUEST);
        assert!(
            store
                .session_ids()
                .await
                .expect("list ledger sessions")
                .is_empty()
        );
    }

    #[tokio::test]
    async fn the_key_length_bound_is_inclusive() {
        let coordinator = coordinator();
        let at_bound = "k".repeat(MAX_KEY_BYTES);
        let over_bound = "k".repeat(MAX_KEY_BYTES + 1);

        expect_guard(coordinator.reserve("sid", &at_bound, "main").await);
        let refusal = expect_refusal(coordinator.reserve("sid", &over_bound, "main").await);
        assert!(matches!(refusal, Refusal::InvalidKey(_)), "got {refusal:?}");
    }

    #[test]
    fn refusal_codes_are_distinct() {
        let codes = [
            Refusal::InvalidKey("x").code(),
            Refusal::Conflict.code(),
            Refusal::Indeterminate.code(),
            Refusal::InProgress.code(),
            Refusal::Unavailable("x".into()).code(),
        ];
        let unique: std::collections::BTreeSet<_> = codes.iter().collect();
        assert_eq!(unique.len(), codes.len(), "clients discriminate on these");
    }
}
