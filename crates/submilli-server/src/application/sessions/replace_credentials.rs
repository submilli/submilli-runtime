use super::ports::AuditLog;
use crate::application::sessions::error::SessionError;
use crate::application::unit_of_work::UnitOfWorkFactory;
use std::sync::Arc;
use submilli_blueprint::HarnessSecretBindings;

pub(crate) struct ReplaceCredentials<'a> {
    unit_of_work: &'a dyn UnitOfWorkFactory,
    audit: &'a dyn AuditLog,
}
impl<'a> ReplaceCredentials<'a> {
    pub fn new(unit_of_work: &'a dyn UnitOfWorkFactory, audit: &'a dyn AuditLog) -> Self {
        Self {
            unit_of_work,
            audit,
        }
    }

    pub async fn execute(
        &self,
        id: &str,
        credentials: Arc<HarnessSecretBindings>,
    ) -> Result<(), SessionError> {
        let mut unit = self.unit_of_work.begin().await?;
        let mut session = unit
            .get_session(id)
            .await?
            .ok_or(SessionError::UnknownSession)?;
        let now = std::time::SystemTime::now();
        session.require_available(now)?;
        session.replace_credentials(credentials)?;
        unit.save_session(session).await?;
        unit.commit().await?;
        self.audit
            .record(id, super::ports::SessionEvent::CredentialsReplaced);
        Ok(())
    }
}
