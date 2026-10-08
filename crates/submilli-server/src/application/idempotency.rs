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
    #[error("the previous request outcome is indeterminate")]
    Indeterminate,
}

pub(crate) enum RequestDisposition {
    Proceed,
    Wait(String),
    Completed(RecordedOutcome),
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
        match self.reserve(&request).await {
            Ok(disposition) => Ok(disposition),
            Err(error @ RequestFailure::Store(_)) => {
                // A lost acknowledgement can hide a successful claim. Only this
                // invocation's reservation identity permits it to proceed.
                if self.owns_reservation(&request).await.unwrap_or(false) {
                    Ok(RequestDisposition::Proceed)
                } else {
                    Err(error)
                }
            }
            Err(error) => Err(error),
        }
    }

    async fn reserve(
        &self,
        request: &IdempotentRequest,
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
            return match existing.state() {
                RequestState::Reserved => Ok(RequestDisposition::Wait(
                    existing.reservation_id().to_owned(),
                )),
                RequestState::Completed(outcome) => {
                    Ok(RequestDisposition::Completed(outcome.clone()))
                }
                RequestState::Indeterminate => Err(RequestFailure::Indeterminate),
            };
        }
        unit.save_request(request.clone()).await?;
        unit.commit().await?;
        Ok(RequestDisposition::Proceed)
    }

    async fn owns_reservation(&self, request: &IdempotentRequest) -> Result<bool, RequestFailure> {
        let mut unit = self.units.begin().await?;
        let existing = unit
            .get_request(request.session_id(), request.key())
            .await?;
        Ok(existing.is_some_and(|existing| {
            existing.reservation_id() == request.reservation_id()
                && existing.state() == &RequestState::Reserved
        }))
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
