//! Durable run lifecycle. Scheduling and execution happen outside transactions.
use crate::application::error::StoreError;
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::domain::run::Run;
use interpreter::runtime::durable::RunId;
use std::time::SystemTime;

pub(crate) use crate::domain::run_status::{RunStatus, StuckReason};

#[derive(Clone, Default)]
pub(crate) struct RunQuery {
    pub session: Option<String>,
    pub status: Option<RunStatusFilter>,
    /// Lexicographic run-ID cursor, independent of mutable lifecycle state.
    pub after: Option<RunId>,
    pub limit: usize,
    /// Recorded runs with retained resources, optionally restricted by work.
    pub recovery: Option<RecoveryFilter>,
}

#[derive(Clone, Copy)]
pub(crate) enum RecoveryFilter {
    Retained,
    Execution,
    Retirement,
}

/// Selects a lifecycle status regardless of its reason.
#[derive(Clone, Copy)]
pub(crate) enum RunStatusFilter {
    Queued,
    Running,
    Stuck,
    Cancelled,
    Aborted,
    Completed,
    Indeterminate,
    Expired,
}

/// Claim one eligible run, checking its predecessor in the same transaction.
/// An already claimed run in this startup is never another launch permit.
pub(crate) async fn claim(
    units: &dyn UnitOfWorkFactory,
    id: &RunId,
    startup: &str,
) -> Result<Option<Run>, StoreError> {
    let mut unit = units.begin().await?;
    let Some(mut run) = unit.get_run(id).await? else {
        return Ok(None);
    };
    if !run.claimable_by(startup) || unit.has_run_predecessor(id).await? {
        return Ok(None);
    }
    let Some(session) = unit.get_session(run.session_id()).await? else {
        return Err(StoreError::Io("run session is missing".into()));
    };
    session
        .require_active()
        .map_err(|error| StoreError::Io(error.to_string()))?;
    run.reopen(startup)
        .map_err(|error| StoreError::Io(error.to_string()))?;
    unit.save_run(run.clone()).await?;
    if let Err(error) = unit.commit().await {
        let mut check = units.begin().await?;
        let saved = check.get_run(id).await?;
        if !saved.is_some_and(|saved| {
            saved.status() == RunStatus::Running
                && saved.owner_generation() == startup
                && saved.attempt() == run.attempt()
                && saved.revision() == run.revision()
        }) {
            return Err(error);
        }
    }
    Ok(Some(run))
}

/// No automatic retry leaves `stuck`: only this explicit user request does.
pub(crate) async fn resume(
    units: &dyn UnitOfWorkFactory,
    id: &RunId,
    revision: i64,
) -> Result<(), StoreError> {
    let mut unit = units.begin().await?;
    let mut run = unit.get_run(id).await?.ok_or(StoreError::Conflict)?;
    if !run.is_durable()
        || !matches!(run.status(), RunStatus::Stuck(_))
        || run.revision() != revision
    {
        return Err(StoreError::Conflict);
    }
    let session = unit
        .get_session(run.session_id())
        .await?
        .ok_or(StoreError::Conflict)?;
    session.require_active().map_err(|_| StoreError::Conflict)?;
    run.resume(revision).map_err(|_| StoreError::Conflict)?;
    unit.save_run(run.clone()).await?;
    if let Err(error) = unit.commit().await {
        let mut check = units.begin().await?;
        let saved = check.get_run(id).await?;
        if !saved.is_some_and(|saved| {
            saved.status() == RunStatus::Queued
                && Some(saved.revision()) == revision.checked_add(1)
                && saved.attempt() == run.attempt()
        }) {
            return Err(error);
        }
    }
    Ok(())
}

pub(crate) async fn mark_stuck(
    units: &dyn UnitOfWorkFactory,
    id: &RunId,
    revision: i64,
    reason: StuckReason,
) -> Result<(), StoreError> {
    let mut unit = units.begin().await?;
    let mut run = unit.get_run(id).await?.ok_or(StoreError::Conflict)?;
    if run.revision() != revision {
        return Err(StoreError::Conflict);
    }
    run.mark_stuck(reason).map_err(|_| StoreError::Conflict)?;
    unit.save_run(run).await?;
    unit.commit().await
}

/// An idempotency retry attaches to the same run, including a stuck one.
pub(crate) async fn existing(
    units: &dyn UnitOfWorkFactory,
    request: &Run,
) -> Result<Option<RunId>, super::runs::RunFailure> {
    let Some(key) = request.optional_key() else {
        return Ok(None);
    };
    let mut unit = units.begin().await?;
    let Some(existing) = unit.run_for_key(request.session_id(), key).await? else {
        return Ok(None);
    };
    if existing.fingerprint() != request.fingerprint() {
        return Err(super::runs::RunFailure::Rule(
            crate::domain::run::RequestError::Conflict,
        ));
    }
    Ok(Some(existing.reservation_id().clone()))
}

pub(crate) async fn admit(
    units: &dyn UnitOfWorkFactory,
    request: Run,
    program: crate::domain::run_program::RunProgram,
) -> Result<RunId, super::runs::RunFailure> {
    let mut unit = units.begin().await?;
    let session = unit
        .get_session(request.session_id())
        .await?
        .ok_or(super::runs::RunFailure::SessionUnavailable)?;
    super::execution::require_available(unit.as_mut(), &session, SystemTime::now())
        .await
        .map_err(|_| super::runs::RunFailure::SessionUnavailable)?;
    if let Some(key) = request.optional_key()
        && let Some(existing) = unit.run_for_key(request.session_id(), key).await?
    {
        if existing.fingerprint() != request.fingerprint() {
            return Err(super::runs::RunFailure::Rule(
                crate::domain::run::RequestError::Conflict,
            ));
        }
        return Ok(existing.reservation_id().clone());
    }
    unit.save_run(request.clone()).await?;
    unit.save_run_program(request.reservation_id(), program)
        .await?;
    if let Err(error) = unit.commit().await {
        let mut check = units.begin().await?;
        let saved = check.get_run(request.reservation_id()).await?;
        if !saved
            .is_some_and(|saved| saved.is_durable() && saved.session_id() == request.session_id())
        {
            return Err(error.into());
        }
    }
    Ok(request.reservation_id().clone())
}

/// Native outcomes advance the revision. Fence by attempt before reading the
/// latest revision; cancellation or a newer attempt must win over this result.
pub(crate) async fn stop_attempt(
    units: &dyn UnitOfWorkFactory,
    run: &Run,
    reason: StuckReason,
) -> Result<(), StoreError> {
    let mut unit = units.begin().await?;
    let Some(mut saved) = unit.get_run(run.reservation_id()).await? else {
        return Ok(());
    };
    if saved.status() != RunStatus::Running
        || saved.attempt() != run.attempt()
        || saved.owner_generation() != run.owner_generation()
    {
        return Ok(());
    }
    saved.mark_stuck(reason).map_err(|_| StoreError::Conflict)?;
    unit.save_run(saved).await?;
    unit.commit().await
}
