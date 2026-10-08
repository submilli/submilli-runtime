use super::error::SessionError;
use super::ports::SessionRepository;
use crate::domain::session::Session;
use std::time::SystemTime;

pub(crate) struct GetSession<'a> {
    repository: &'a dyn SessionRepository,
}
impl<'a> GetSession<'a> {
    pub fn new(repository: &'a dyn SessionRepository) -> Self {
        Self { repository }
    }

    pub async fn execute(&self, id: &str) -> Result<Session, SessionError> {
        let session = self
            .repository
            .get(id)
            .await?
            .ok_or(SessionError::UnknownSession)?;
        session.require_available(SystemTime::now())?;
        Ok(session)
    }
}
