use super::ports::AuditLog;
use crate::application::sessions::error::SessionError;
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::domain::session::ClosedReason;

pub(crate) struct CloseSession<'a> {
    unit_of_work: &'a dyn UnitOfWorkFactory,
    audit: &'a dyn AuditLog,
}
impl<'a> CloseSession<'a> {
    pub fn new(unit_of_work: &'a dyn UnitOfWorkFactory, audit: &'a dyn AuditLog) -> Self {
        Self {
            unit_of_work,
            audit,
        }
    }

    pub async fn execute(&self, id: &str, reason: ClosedReason) -> Result<bool, SessionError> {
        let mut unit = self.unit_of_work.begin().await?;
        let Some(mut session) = unit.get_session(id).await? else {
            return Ok(false);
        };
        if !session.close(reason) {
            return Ok(false);
        }
        unit.save_session(session).await?;
        unit.commit().await?;
        self.audit.record(id, super::ports::SessionEvent::Deleted);
        Ok(true)
    }
}
