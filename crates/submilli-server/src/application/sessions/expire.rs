use super::ports::AuditLog;
use crate::application::sessions::error::SessionError;
use crate::application::unit_of_work::UnitOfWorkFactory;
use std::time::SystemTime;

pub(crate) struct ExpireSessions<'a> {
    unit_of_work: &'a dyn UnitOfWorkFactory,
    audit: &'a dyn AuditLog,
}
impl<'a> ExpireSessions<'a> {
    pub fn new(unit_of_work: &'a dyn UnitOfWorkFactory, audit: &'a dyn AuditLog) -> Self {
        Self {
            unit_of_work,
            audit,
        }
    }

    pub async fn execute(&self, now: SystemTime) -> Result<usize, SessionError> {
        let mut unit = self.unit_of_work.begin().await?;
        let mut expired = Vec::new();
        let sessions = unit.list_sessions().await?;
        for mut session in sessions {
            if session.expire(now) {
                expired.push(session.id().as_str().to_owned());
                unit.save_session(session).await?;
            }
        }
        unit.commit().await?;
        for id in &expired {
            self.audit.record(id, super::ports::SessionEvent::Deleted);
        }
        Ok(expired.len())
    }
}
