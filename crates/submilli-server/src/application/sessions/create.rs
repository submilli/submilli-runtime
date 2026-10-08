use super::ports::{AuditLog, SessionCleanupQueue, SessionWorkspaces};
use crate::application::sessions::error::SessionError;
use crate::application::unit_of_work::UnitOfWorkFactory;
use crate::domain::session::{Session, SessionBinding, SessionId, SessionLifetime};
use std::sync::Arc;
use submilli_blueprint::{Blueprint, HarnessSecretBindings, VarBindings};

pub(crate) struct CreateSession<'a> {
    unit_of_work: &'a dyn UnitOfWorkFactory,
    workspaces: &'a dyn SessionWorkspaces,
    cleanup_queue: &'a dyn SessionCleanupQueue,
    audit: &'a dyn AuditLog,
}
impl<'a> CreateSession<'a> {
    pub fn new(
        unit_of_work: &'a dyn UnitOfWorkFactory,
        workspaces: &'a dyn SessionWorkspaces,
        cleanup_queue: &'a dyn SessionCleanupQueue,
        audit: &'a dyn AuditLog,
    ) -> Self {
        Self {
            unit_of_work,
            workspaces,
            cleanup_queue,
            audit,
        }
    }

    pub async fn execute(
        &self,
        id: &str,
        blueprint: &Blueprint,
        variables: Arc<VarBindings>,
        credentials: Arc<HarnessSecretBindings>,
    ) -> Result<(), SessionError> {
        let id_value = SessionId::parse(id.to_owned())?;
        let binding = SessionBinding::new(blueprint.name.clone(), (*variables).clone())?;
        let pending_cleanup = self.cleanup_queue.contains(id).await?;
        let root = self.workspaces.resolve_root(id, blueprint, &variables)?;
        let mut unit = self.unit_of_work.begin().await?;
        let existing = unit.get_session(id).await?;
        let now = std::time::SystemTime::now();
        let existed = existing.is_some();
        let mut session = if let Some(session) = existing {
            session
        } else {
            if pending_cleanup {
                return Err(SessionError::CleanupPending);
            }
            Session::create(
                id_value,
                binding.clone(),
                root.clone(),
                SessionLifetime::new(blueprint.idle_timeout, now),
            )?
        };
        session.rebind(binding, root, credentials, now)?;
        unit.save_session(session).await?;
        self.workspaces
            .provision(id, blueprint, &variables, existed)?;
        unit.commit().await?;
        let event = if existed {
            super::ports::SessionEvent::Rebound {
                blueprint: blueprint.name.clone(),
                variables: (*variables).clone(),
            }
        } else {
            super::ports::SessionEvent::Created {
                blueprint: blueprint.name.clone(),
                variables: (*variables).clone(),
            }
        };
        self.audit.record(id, event);
        Ok(())
    }
}

pub(crate) struct EnsureSession<'a> {
    unit_of_work: &'a dyn UnitOfWorkFactory,
    workspaces: &'a dyn SessionWorkspaces,
    cleanup_queue: &'a dyn SessionCleanupQueue,
    audit: &'a dyn AuditLog,
}
impl<'a> EnsureSession<'a> {
    pub fn new(
        unit_of_work: &'a dyn UnitOfWorkFactory,
        workspaces: &'a dyn SessionWorkspaces,
        cleanup_queue: &'a dyn SessionCleanupQueue,
        audit: &'a dyn AuditLog,
    ) -> Self {
        Self {
            unit_of_work,
            workspaces,
            cleanup_queue,
            audit,
        }
    }

    pub async fn execute(&self, id: &str, blueprint: &Blueprint) -> Result<(), SessionError> {
        let mut unit = self.unit_of_work.begin().await?;
        if let Some(session) = unit.get_session(id).await? {
            session.require_available(std::time::SystemTime::now())?;
            session
                .verify_blueprint(&blueprint.name)
                .map_err(|_| SessionError::UnknownSession)?;
            return Ok(());
        }
        drop(unit);
        CreateSession::new(
            self.unit_of_work,
            self.workspaces,
            self.cleanup_queue,
            self.audit,
        )
        .execute(id, blueprint, Arc::default(), Arc::default())
        .await
    }
}
