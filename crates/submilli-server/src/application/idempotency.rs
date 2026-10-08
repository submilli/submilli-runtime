//! Transaction boundaries for keyed requests; execution and waiting occur outside them.
use super::error::StoreError;
use super::unit_of_work::UnitOfWorkFactory;
use crate::domain::idempotent_request::{IdempotentRequest, RecordedOutcome, RequestState};
use std::sync::Arc;
use std::time::SystemTime;

#[derive(Debug, thiserror::Error)]
pub(crate) enum RequestFailure {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Rule(#[from] crate::domain::idempotent_request::RequestError),
    #[error("session is unavailable")]
    SessionUnavailable,
}

pub(crate) enum RequestDisposition {
    Proceed,
    Existing(IdempotentRequest),
}

pub(crate) struct ReserveRequest<'a> {
    units: &'a dyn UnitOfWorkFactory,
}
impl<'a> ReserveRequest<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory) -> Self {
        Self { units }
    }
    pub async fn execute(
        &self,
        request: IdempotentRequest,
    ) -> Result<RequestDisposition, RequestFailure> {
        let mut unit = self.units.begin().await?;
        let session = unit
            .get_session(request.session_id())
            .await?
            .ok_or(RequestFailure::SessionUnavailable)?;
        session
            .require_available(SystemTime::now())
            .map_err(|_| RequestFailure::SessionUnavailable)?;
        if let Some(existing) = unit
            .get_request(request.session_id(), request.key())
            .await?
        {
            existing.verify_fingerprint(request.fingerprint())?;
            return Ok(RequestDisposition::Existing(existing));
        }
        unit.save_request(request).await?;
        unit.commit().await?;
        Ok(RequestDisposition::Proceed)
    }
}

pub(crate) struct ReadRequest<'a> {
    units: &'a dyn UnitOfWorkFactory,
}
impl<'a> ReadRequest<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory) -> Self {
        Self { units }
    }
    pub async fn execute(
        &self,
        session: &str,
        key: &str,
    ) -> Result<Option<IdempotentRequest>, RequestFailure> {
        let mut unit = self.units.begin().await?;
        Ok(unit.get_request(session, key).await?)
    }
}

pub(crate) enum RequestResolution {
    Completed(RecordedOutcome),
    Undispatched,
    Indeterminate,
}

pub(crate) struct ResolveRequest {
    units: Arc<dyn UnitOfWorkFactory>,
}
impl ResolveRequest {
    pub fn new(units: Arc<dyn UnitOfWorkFactory>) -> Self {
        Self { units }
    }
    pub async fn execute(
        &self,
        reservation: &IdempotentRequest,
        resolution: &RequestResolution,
    ) -> Result<(), RequestFailure> {
        let mut unit = self.units.begin().await?;
        let Some(mut request) = unit
            .get_request(reservation.session_id(), reservation.key())
            .await?
        else {
            return Ok(());
        };
        if request.reservation_id() != reservation.reservation_id() {
            return Ok(());
        }
        if request.state() != &RequestState::Reserved {
            return Ok(());
        }
        match resolution {
            RequestResolution::Completed(outcome) => {
                request.complete(reservation.reservation_id(), outcome.clone())?;
                if let Some(mut session) = unit.get_session(reservation.session_id()).await? {
                    let now = SystemTime::now();
                    if session.require_available(now).is_ok() {
                        session
                            .record_execution_completed(now)
                            .map_err(|_| RequestFailure::SessionUnavailable)?;
                        unit.save_session(session).await?;
                    }
                }
                unit.save_request(request).await?;
            }
            RequestResolution::Undispatched => {
                request.require_reservation(reservation.reservation_id())?;
                unit.remove_request(reservation.session_id(), reservation.key())
                    .await?;
            }
            RequestResolution::Indeterminate => {
                request.mark_indeterminate(reservation.reservation_id())?;
                unit.save_request(request).await?;
            }
        }
        unit.commit().await?;
        Ok(())
    }
}

pub(crate) struct RecoverRequests<'a> {
    units: &'a dyn UnitOfWorkFactory,
}
impl<'a> RecoverRequests<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory) -> Self {
        Self { units }
    }
    pub async fn execute(&self, generation: &str) -> Result<(), RequestFailure> {
        let mut unit = self.units.begin().await?;
        for mut request in unit.unfinished_requests().await? {
            if request.owner_generation() != generation {
                let id = request.reservation_id().to_owned();
                request.mark_indeterminate(&id)?;
                unit.save_request(request).await?;
            }
        }
        unit.commit().await?;
        Ok(())
    }
}
