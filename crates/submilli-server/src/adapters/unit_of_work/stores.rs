use crate::adapters::session::repository::{persist_session, restore_domain};
use crate::domain::session::Session;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use crate::adapters::session::credentials::CredentialCodec;
use crate::application::unit_of_work::{UnitOfWork, UnitOfWorkFactory};
use crate::blueprint::{BlueprintStore, StoreError};
use crate::database::DatabaseError;
use crate::domain::session::SessionStatus;
use crate::session_store::SessionStatus as RecordStatus;
use crate::session_store::{DurableSessionStore, SessionRecord};

pub(crate) struct StoreUnitOfWorkFactory {
    pub blueprints: Arc<dyn BlueprintStore>,
    pub sessions: Arc<dyn DurableSessionStore>,
    pub session_root: PathBuf,
    pub cipher: Option<Arc<submilli_shared::secret_store::SecretCipher>>,
}

#[async_trait::async_trait]
impl UnitOfWorkFactory for StoreUnitOfWorkFactory {
    async fn begin(&self) -> Result<Box<dyn UnitOfWork>, StoreError> {
        Ok(Box::new(StoreUnitOfWork {
            blueprints: self.blueprints.clone(),
            sessions: self.sessions.clone(),
            session_root: self.session_root.clone(),
            codec: CredentialCodec::new(self.cipher.clone()),
            original: BTreeMap::new(),
            saved_sessions: BTreeMap::new(),
            removed_blueprints: BTreeSet::new(),
        }))
    }
}

/// Compatibility stores overlay staged changes on reads. Commit is sequential
/// and can partially persist before failure; dropping before commit writes nothing.
struct StoreUnitOfWork {
    blueprints: Arc<dyn BlueprintStore>,
    sessions: Arc<dyn DurableSessionStore>,
    session_root: PathBuf,
    codec: CredentialCodec,
    original: BTreeMap<String, SessionRecord>,
    saved_sessions: BTreeMap<String, SessionRecord>,
    removed_blueprints: BTreeSet<String>,
}

#[async_trait::async_trait]
impl UnitOfWork for StoreUnitOfWork {
    async fn blueprint_exists(&mut self, name: &str) -> Result<bool, StoreError> {
        if self.removed_blueprints.contains(name) {
            return Ok(false);
        }
        Ok(self.blueprints.get_yaml(name).await?.is_some())
    }

    async fn remove_blueprint(&mut self, name: &str) -> Result<bool, StoreError> {
        if self
            .sessions_for_blueprint(name)
            .await?
            .iter()
            .any(|session| session.status() == SessionStatus::Active)
        {
            return Err(DatabaseError::SessionConflict.into());
        }
        if !self.blueprint_exists(name).await? {
            return Ok(false);
        }
        self.removed_blueprints.insert(name.to_owned());
        Ok(true)
    }

    async fn get_session(&mut self, id: &str) -> Result<Option<Session>, StoreError> {
        let record = match self.saved_sessions.get(id) {
            Some(record) => Some(record.clone()),
            None => self.sessions.load(id).await?,
        };
        if let Some(record) = &record {
            self.original
                .entry(id.to_owned())
                .or_insert_with(|| record.clone());
        }
        record
            .map(|record| restore_domain(&record, &self.session_root))
            .transpose()
    }

    async fn sessions_due_for_expiry(
        &mut self,
        now: SystemTime,
    ) -> Result<Vec<Session>, StoreError> {
        let mut records: BTreeMap<_, _> = self
            .sessions
            .expiry_candidates(now)
            .await?
            .into_iter()
            .map(|record| (record.session_id.clone(), record))
            .collect();
        records.extend(self.saved_sessions.clone());
        let mut sessions = Vec::new();
        for (id, record) in records {
            let session = restore_domain(&record, &self.session_root)?;
            if session.status() == SessionStatus::Active && session.lifetime().expired_at(now) {
                self.original.entry(id).or_insert(record);
                sessions.push(session);
            }
        }
        Ok(sessions)
    }

    async fn list_sessions(&mut self) -> Result<Vec<Session>, StoreError> {
        let mut records: BTreeMap<_, _> = self
            .sessions
            .load_all()
            .await?
            .into_iter()
            .map(|record| (record.session_id.clone(), record))
            .collect();
        records.extend(self.saved_sessions.clone());
        for (id, record) in &records {
            self.original
                .entry(id.clone())
                .or_insert_with(|| record.clone());
        }
        records
            .into_values()
            .map(|record| restore_domain(&record, &self.session_root))
            .collect()
    }

    async fn sessions_for_blueprint(&mut self, name: &str) -> Result<Vec<Session>, StoreError> {
        let mut records: BTreeMap<_, _> = self
            .sessions
            .for_blueprint(name)
            .await?
            .into_iter()
            .map(|record| (record.session_id.clone(), record))
            .collect();
        for (id, record) in &self.saved_sessions {
            if record.blueprint_name == name {
                records.insert(id.clone(), record.clone());
            } else {
                records.remove(id);
            }
        }
        for (id, record) in &records {
            self.original
                .entry(id.clone())
                .or_insert_with(|| record.clone());
        }
        records
            .into_values()
            .map(|record| restore_domain(&record, &self.session_root))
            .collect()
    }

    async fn save_session(&mut self, session: Session) -> Result<(), StoreError> {
        let id = session.id().as_str().to_owned();
        let previous = self
            .saved_sessions
            .get(&id)
            .or(self.original.get(&id))
            .cloned();
        let record = persist_session(
            session,
            previous.clone(),
            &self.codec,
            self.sessions.durable(),
        )?;
        crate::adapters::session::check_revision(previous.as_ref(), &record)?;
        self.saved_sessions.insert(id, record);
        Ok(())
    }

    async fn commit(self: Box<Self>) -> Result<(), StoreError> {
        for session in self.saved_sessions.into_values() {
            self.sessions.put(session).await?;
        }
        for name in self.removed_blueprints {
            if self
                .sessions
                .for_blueprint(&name)
                .await?
                .iter()
                .any(|session| session.status == RecordStatus::Active)
                || !self.blueprints.remove(&name).await?
            {
                return Err(DatabaseError::SessionConflict.into());
            }
        }
        Ok(())
    }
}
