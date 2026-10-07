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
        session.record_execution_started(now)?;
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
        if session.require_available(now).is_err() {
            return Ok(false);
        }
        session.record_execution_completed(now)?;
        unit.save_session(session).await?;
        unit.commit().await?;
        Ok(true)
    }
}
