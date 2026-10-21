use crate::application::sessions::error::SessionError;
use crate::application::unit_of_work::UnitOfWorkFactory;

pub(crate) struct StartExecution<'a> {
    unit_of_work: &'a dyn UnitOfWorkFactory,
}
impl<'a> StartExecution<'a> {
    pub fn new(unit_of_work: &'a dyn UnitOfWorkFactory) -> Self {
        Self { unit_of_work }
    }

    pub async fn execute(&self, id: &str) -> Result<(), SessionError> {
        let mut unit = self.unit_of_work.begin().await?;
        let mut session = unit
            .get_session(id)
            .await?
            .ok_or(SessionError::UnknownSession)?;
        let now = std::time::SystemTime::now();
        require_available(unit.as_mut(), &session, now).await?;
        session.record_activity(now)?;
        unit.save_session(session).await?;
        unit.commit().await?;
        Ok(())
    }
}

pub(crate) struct CompleteExecution<'a> {
    unit_of_work: &'a dyn UnitOfWorkFactory,
}
impl<'a> CompleteExecution<'a> {
    pub fn new(unit_of_work: &'a dyn UnitOfWorkFactory) -> Self {
        Self { unit_of_work }
    }

    pub async fn execute(&self, id: &str) -> Result<bool, SessionError> {
        let mut unit = self.unit_of_work.begin().await?;
        let Some(mut session) = unit.get_session(id).await? else {
            return Ok(false);
        };
        let now = std::time::SystemTime::now();
        match require_available(unit.as_mut(), &session, now).await {
            Ok(()) => {}
            Err(error @ SessionError::Storage(_)) => return Err(error),
            Err(_) => return Ok(false),
        }
        session.record_execution_completed(now)?;
        unit.save_session(session).await?;
        unit.commit().await?;
        Ok(true)
    }
}

/// Accepted work retains its session across idle deadlines. Closure still wins.
pub(crate) async fn require_available(
    unit: &mut dyn crate::application::unit_of_work::UnitOfWork,
    session: &crate::domain::session::Session,
    now: std::time::SystemTime,
) -> Result<(), SessionError> {
    if unit.has_retained_run(session.id().as_str()).await? {
        session.require_active()?;
    } else {
        session.require_available(now)?;
    }
    Ok(())
}
