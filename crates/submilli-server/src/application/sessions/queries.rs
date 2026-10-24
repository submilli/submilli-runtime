use super::error::SessionError;
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::domain::session::Session;
use std::time::SystemTime;

pub(crate) struct GetSession<'a> {
    units: &'a dyn UnitOfWorkFactory,
}
impl<'a> GetSession<'a> {
    pub fn new(units: &'a dyn UnitOfWorkFactory) -> Self {
        Self { units }
    }

    pub async fn execute(&self, id: &str) -> Result<Session, SessionError> {
        let mut unit = self.units.begin().await?;
        let session = unit
            .get_session(id)
            .await?
            .ok_or(SessionError::UnknownSession)?;
        super::execution::require_available(unit.as_mut(), &session, SystemTime::now()).await?;
        Ok(session)
    }
}
