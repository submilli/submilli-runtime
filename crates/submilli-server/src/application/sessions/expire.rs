use super::ports::AuditLog;
use crate::application::sessions::error::SessionError;
use crate::application::unit_of_work::UnitOfWorkFactory;
use std::collections::HashSet;
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

    /// Expire the idle sessions, except those in `busy`, which have work in flight.
    pub async fn execute(
        &self,
        now: SystemTime,
        busy: &HashSet<String>,
    ) -> Result<usize, SessionError> {
        let mut unit = self.unit_of_work.begin().await?;
        let mut expired = Vec::new();
        let sessions = unit.sessions_due_for_expiry(now).await?;
        for mut session in sessions {
            if busy.contains(session.id().as_str()) {
                continue;
            }
            if session.expire(now) {
                expired.push(session.id().as_str().to_owned());
                unit.save_session(session).await?;
            }
        }
        unit.commit().await?;
        for id in &expired {
            self.audit.record(id, super::ports::SessionEvent::Expired);
        }
        Ok(expired.len())
    }
}
