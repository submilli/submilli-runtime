use super::ports::{AuditLog, SessionCleanupQueue, SessionEvent, SessionWorkspaces};
use crate::application::error::StoreError;
use crate::application::sessions::error::BootError;
use crate::application::sessions::ports::SessionCleanup;
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::domain::session::SessionStatus;
use std::collections::HashSet;

pub(crate) struct RecoverSessions<'a> {
    unit_of_work: &'a dyn UnitOfWorkFactory,
    audit: &'a dyn AuditLog,
    workspaces: &'a dyn SessionWorkspaces,
    cleanup_queue: &'a dyn SessionCleanupQueue,
}
impl<'a> RecoverSessions<'a> {
    pub fn new(
        unit_of_work: &'a dyn UnitOfWorkFactory,
        audit: &'a dyn AuditLog,
        workspaces: &'a dyn SessionWorkspaces,
        cleanup_queue: &'a dyn SessionCleanupQueue,
    ) -> Self {
        Self {
            unit_of_work,
            audit,
            workspaces,
            cleanup_queue,
        }
    }

    pub async fn execute(&self) -> Result<(), BootError> {
        let workspaces = self.workspaces.list().map_err(recovery_error)?;

        let mut unit = self
            .unit_of_work
            .begin()
            .await
            .map_err(BootError::Sessions)?;
        let sessions = unit.list_sessions().await.map_err(BootError::Sessions)?;
        let now = std::time::SystemTime::now();
        let mut events = Vec::new();
        let mut active = HashSet::new();
        for mut session in sessions {
            let id = session.id().as_str().to_owned();
            if session.status() == SessionStatus::Active {
                events.push((
                    id.clone(),
                    SessionEvent::Found {
                        blueprint: session.binding().blueprint().to_owned(),
                    },
                ));
            }
            if session.recover(now) {
                events.push((id.clone(), SessionEvent::Expired));
                unit.remove_session_requests(session.id().as_str())
                    .await
                    .map_err(BootError::Sessions)?;
                unit.save_session(session)
                    .await
                    .map_err(BootError::Sessions)?;
            } else if session.status() == SessionStatus::Active {
                active.insert(id);
            }
        }
        unit.commit().await.map_err(BootError::Sessions)?;
        for (id, event) in events {
            self.audit.record(&id, event);
        }
        for (id, path) in workspaces {
            if !active.contains(&id) {
                self.cleanup_queue
                    .enqueue(SessionCleanup {
                        session_id: id,
                        folder: Some(path),
                    })
                    .await
                    .map_err(BootError::Sessions)?;
            }
        }
        Ok(())
    }
}

fn recovery_error(error: crate::application::sessions::error::SessionError) -> BootError {
    BootError::Sessions(StoreError::Io(error.to_string()))
}
