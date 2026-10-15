//! Run preparation, admission, and terminal transitions. Execution waits outside transactions.
use crate::application::error::StoreError;
use crate::application::unit_of_work::{UnitOfWork, UnitOfWorkFactory};
use crate::domain::idempotent_request::{RecordedOutcome, RequestState, ResponseRetention};
use crate::domain::run::Run;
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Debug, thiserror::Error)]
pub(crate) enum RunFailure {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Rule(#[from] crate::domain::idempotent_request::RequestError),
    #[error("session is unavailable")]
    SessionUnavailable,
    #[error("the previous request outcome is indeterminate")]
    Indeterminate,
    #[error("the previous request completed, and its response is no longer kept")]
    Expired,
}

pub(crate) enum RunDisposition {
    Proceed,
    Wait(String),
    Completed(RecordedOutcome),
}

pub(crate) struct PrepareRun<'a> {
    units: &'a dyn UnitOfWorkFactory,
}
impl<'a> PrepareRun<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory) -> Self {
        Self { units }
    }
    pub async fn execute(&self, request: Run) -> Result<RunDisposition, RunFailure> {
        let mut unit = self.units.begin().await?;
        let session = unit
            .get_session(request.session_id())
            .await?
            .ok_or(RunFailure::SessionUnavailable)?;
        session
            .require_available(SystemTime::now())
            .map_err(|_| RunFailure::SessionUnavailable)?;
        let existing = match request.optional_key() {
            Some(key) => unit.run_for_key(request.session_id(), key).await?,
            None => unit.get_run(request.reservation_id()).await?,
        };
        if let Some(existing) = existing {
            existing.verify_fingerprint(request.fingerprint())?;
            return match existing.state() {
                RequestState::Reserved | RequestState::Running => {
                    Ok(RunDisposition::Wait(existing.reservation_id().to_owned()))
                }
                RequestState::Completed(outcome) => Ok(RunDisposition::Completed(outcome.clone())),
                RequestState::Indeterminate => Err(RunFailure::Indeterminate),
                RequestState::Expired | RequestState::Finished => Err(RunFailure::Expired),
                RequestState::Cancelled | RequestState::Blocked | RequestState::Aborted => {
                    Err(RunFailure::Indeterminate)
                }
            };
        }
        unit.save_run(request.clone()).await?;
        match unit.commit().await {
            Ok(()) => Ok(RunDisposition::Proceed),
            Err(error) => {
                // Only a commit attempted by this invocation can have lost its
                // acknowledgement. An earlier I/O failure grants no ownership.
                if self.owns_reservation(&request).await.unwrap_or(false) {
                    Ok(RunDisposition::Proceed)
                } else {
                    Err(error.into())
                }
            }
        }
    }

    async fn owns_reservation(&self, request: &Run) -> Result<bool, RunFailure> {
        let mut unit = self.units.begin().await?;
        let existing = unit.get_run(request.reservation_id()).await?;
        Ok(existing.is_some_and(|existing| {
            existing.reservation_id() == request.reservation_id()
                && existing.session_id() == request.session_id()
                && existing.optional_key() == request.optional_key()
                && existing.fingerprint() == request.fingerprint()
                && existing.owner_generation() == request.owner_generation()
                && existing.revision() == 0
                && existing.state() == &RequestState::Reserved
        }))
    }
}

pub(crate) enum RunResolution {
    Completed(RecordedOutcome),
    Finished,
    Undispatched,
    Indeterminate,
}

pub(crate) struct ResolveRun {
    units: Arc<dyn UnitOfWorkFactory>,
}
impl ResolveRun {
    pub fn new(units: Arc<dyn UnitOfWorkFactory>) -> Self {
        Self { units }
    }
    pub async fn execute(
        &self,
        reservation: &Run,
        resolution: &RunResolution,
    ) -> Result<(), RunFailure> {
        let mut unit = self.units.begin().await?;
        let Some(mut request) = unit.get_run(reservation.reservation_id()).await? else {
            return Ok(());
        };
        if request.reservation_id() != reservation.reservation_id() {
            return Ok(());
        }
        if !matches!(
            request.state(),
            RequestState::Reserved | RequestState::Running
        ) {
            return Ok(());
        }
        match resolution {
            RunResolution::Completed(outcome) => {
                let now = SystemTime::now();
                request.complete(reservation.reservation_id(), outcome.clone(), now)?;
                if let Some(mut session) = unit.get_session(reservation.session_id()).await?
                    && session.require_available(now).is_ok()
                {
                    session
                        .record_execution_completed(now)
                        .map_err(|_| RunFailure::SessionUnavailable)?;
                    unit.save_session(session).await?;
                }
                unit.save_run(request).await?;
            }
            RunResolution::Undispatched => {
                if request.state() == &RequestState::Reserved {
                    request.abort_preparation()?;
                } else {
                    request.mark_indeterminate(reservation.reservation_id())?;
                }
                unit.save_run(request).await?;
            }
            RunResolution::Finished => {
                request.finish_unkeyed(SystemTime::now())?;
                unit.save_run(request).await?;
            }
            RunResolution::Indeterminate => {
                request.mark_indeterminate(reservation.reservation_id())?;
                unit.save_run(request).await?;
            }
        }
        unit.commit().await?;
        Ok(())
    }
}

pub(crate) struct RecoverRuns<'a> {
    units: &'a dyn UnitOfWorkFactory,
}
impl<'a> RecoverRuns<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory) -> Self {
        Self { units }
    }
    pub async fn execute(&self, generation: &str) -> Result<(), RunFailure> {
        let mut unit = self.units.begin().await?;
        for mut request in unit.unfinished_runs().await? {
            if request.owner_generation() != generation {
                let id = request.reservation_id().to_owned();
                request.mark_indeterminate(&id)?;
                unit.save_run(request).await?;
            }
        }
        unit.commit().await?;
        Ok(())
    }
}

/// Drop the response bodies [`ResponseRetention`] no longer keeps, leaving
/// tombstones, so a session's ledger stops growing with its age.
pub(crate) struct ExpireResponses<'a> {
    units: &'a dyn UnitOfWorkFactory,
    retention: ResponseRetention,
}
impl<'a> ExpireResponses<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory, retention: ResponseRetention) -> Self {
        Self { units, retention }
    }

    /// Returns how many responses expired.
    pub async fn execute(&self, now: SystemTime) -> Result<u64, StoreError> {
        let Some(cutoff) = self.retention.cutoff(now) else {
            return Ok(0);
        };
        let mut unit = self.units.begin().await?;
        let sessions = unit
            .sessions_with_responses_to_expire(cutoff, self.retention.keep_last)
            .await?;
        unit.commit().await?;
        let mut expired = 0;
        // One unit per session, so a large backlog (the first sweep after an
        // upgrade) never holds the writer for the whole ledger at once.
        for session in sessions {
            let mut unit = self.units.begin().await?;
            let completed = unit.completed_responses(&session).await?;
            let keys: Vec<String> = self
                .retention
                .expired(&completed, now)
                .into_iter()
                .map(str::to_owned)
                .collect();
            if keys.is_empty() {
                // The session changed since the listing; nothing was written,
                // so dropping the unit rolls back a read.
                continue;
            }
            expired += unit.expire_responses(&session, keys).await?;
            unit.commit().await?;
        }
        Ok(expired)
    }
}

/// Admit prepared compatibility work immediately before entering the runner.
/// There is no recovery claim without a manifest/history in this increment.
pub(crate) async fn claim_run(
    units: &dyn UnitOfWorkFactory,
    id: &str,
    startup: &str,
) -> Result<(), RunFailure> {
    let mut unit = units.begin().await?;
    let mut run = unit.get_run(id).await?.ok_or(RunFailure::Indeterminate)?;
    let session = unit
        .get_session(run.session_id())
        .await?
        .ok_or(RunFailure::SessionUnavailable)?;
    session
        .require_available(SystemTime::now())
        .map_err(|_| RunFailure::SessionUnavailable)?;
    run.claim(startup)?;
    let expected_revision = run.revision();
    let expected_attempt = run.attempt();
    unit.save_run(run).await?;
    let Err(error) = unit.commit().await else {
        return Ok(());
    };
    // Reconcile only our staged transition. A read/begin/save failure above
    // cannot turn an already-running row into a second launch permit.
    let mut unit = units.begin().await?;
    if let Some(run) = unit.get_run(id).await?
        && run.owner_generation() == startup
        && run.state() == &RequestState::Running
        && run.revision() == expected_revision
        && run.attempt() == expected_attempt
    {
        return Ok(());
    }
    Err(error.into())
}

/// Fence unfinished runs in the same transaction that closes their session.
pub(crate) async fn cancel_session_runs(
    unit: &mut dyn UnitOfWork,
    session_id: &str,
) -> Result<(), StoreError> {
    for mut run in unit.active_session_runs(session_id).await? {
        run.cancel()
            .map_err(|error| StoreError::Io(error.to_string()))?;
        unit.save_run(run).await?;
    }
    Ok(())
}
