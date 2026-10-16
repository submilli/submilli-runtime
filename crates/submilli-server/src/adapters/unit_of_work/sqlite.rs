use crate::adapters::session::repository::{persist_session, restore_domain};
use crate::domain::session::Session;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::adapters::sealing::Sealer;
use crate::adapters::session::credentials::CredentialCodec;
use crate::adapters::session::kv;
use crate::adapters::session::last_run;
use crate::application::sessions::ports::StagedLastRun;
use crate::application::unit_of_work::{SessionStateKeys, SessionStateSize, SessionStateUsage};
use crate::application::unit_of_work::{UnitOfWork, UnitOfWorkFactory};
use crate::blueprint::StoreError;
use crate::database::{DatabaseError, ServerDatabase};
use crate::domain::mcp_grant::{GrantOverview, GrantVersion, McpGrant};
use crate::domain::secret::{Secret, SecretName};

pub(crate) struct SqliteUnitOfWorkFactory {
    pub database: Arc<ServerDatabase>,
    pub session_root: PathBuf,
    pub cipher: Option<Arc<submilli_shared::secret_store::SecretCipher>>,
}

#[async_trait::async_trait]
impl UnitOfWorkFactory for SqliteUnitOfWorkFactory {
    async fn begin(&self) -> Result<Box<dyn UnitOfWork>, StoreError> {
        let unit = SqliteUnitOfWork {
            transaction: super::transaction::SqliteTransaction::begin(self.database.clone())
                .await?,
            session_root: self.session_root.clone(),
            codec: Arc::new(CredentialCodec::new(self.cipher.clone())),
            sealer: Sealer::new(self.cipher.clone()),
        };
        Ok(Box::new(unit))
    }
}

struct SqliteUnitOfWork {
    transaction: super::transaction::SqliteTransaction,
    session_root: PathBuf,
    codec: Arc<CredentialCodec>,
    sealer: Sealer,
}

#[async_trait::async_trait]
impl UnitOfWork for SqliteUnitOfWork {
    async fn session_has_retained_workspace(&mut self, session: &str) -> Result<bool, StoreError> {
        let session = session.to_owned();
        self.transaction.query(move |connection| Box::pin(async move {
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM durable_workspaces WHERE session_id=? AND state != 'gone')")
                .bind(session).fetch_one(connection).await.map_err(DatabaseError::Sql)
        })).await
    }
    async fn blueprint_exists(&mut self, name: &str) -> Result<bool, StoreError> {
        let name = name.to_owned();
        self.transaction
            .query(move |connection| {
                Box::pin(async move {
                    Ok(
                        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM blueprints WHERE name=?)")
                            .bind(name)
                            .fetch_one(connection)
                            .await?,
                    )
                })
            })
            .await
    }

    async fn remove_blueprint(&mut self, name: &str) -> Result<bool, StoreError> {
        let name = name.to_owned();
        self.transaction.query(move |connection| Box::pin(async move {
            let remaining: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions WHERE blueprint_name=? AND status='active')")
                .bind(&name).fetch_one(&mut *connection).await?;
            if remaining {
                return Err(DatabaseError::SessionConflict);
            }
            crate::adapters::blueprint::sqlite::remove(connection, &name).await
        })).await
    }

    async fn get_session(&mut self, id: &str) -> Result<Option<Session>, StoreError> {
        let id = id.to_owned();
        let record = self
            .transaction
            .query(move |connection| {
                Box::pin(async move {
                    crate::adapters::session::sqlite::records::load(connection, &id).await
                })
            })
            .await?;
        record
            .map(|record| restore_domain(&record, &self.session_root))
            .transpose()
    }

    async fn sessions_due_for_expiry(
        &mut self,
        now: SystemTime,
    ) -> Result<Vec<Session>, StoreError> {
        let now = i64::try_from(
            now.duration_since(UNIX_EPOCH)
                .map_err(|_| StoreError::Io("invalid expiry clock".into()))?
                .as_millis(),
        )
        .map_err(|_| StoreError::Io("expiry clock overflow".into()))?;
        let records = self
            .transaction
            .query(move |connection| {
                Box::pin(
                    crate::adapters::session::sqlite::records::expiry_candidates(connection, now),
                )
            })
            .await?;
        records
            .into_iter()
            .map(|record| restore_domain(&record, &self.session_root))
            .collect()
    }

    async fn list_sessions(&mut self) -> Result<Vec<Session>, StoreError> {
        let records = self
            .transaction
            .query(|connection| {
                Box::pin(crate::adapters::session::sqlite::records::load_all(
                    connection,
                ))
            })
            .await?;
        records
            .into_iter()
            .map(|record| restore_domain(&record, &self.session_root))
            .collect()
    }

    async fn sessions_for_blueprint(&mut self, name: &str) -> Result<Vec<Session>, StoreError> {
        let name = name.to_owned();
        let records = self
            .transaction
            .query(move |connection| {
                Box::pin(async move {
                    crate::adapters::session::sqlite::records::for_blueprint(connection, &name)
                        .await
                })
            })
            .await?;
        records
            .into_iter()
            .map(|record| restore_domain(&record, &self.session_root))
            .collect()
    }

    async fn save_session(&mut self, session: Session) -> Result<(), StoreError> {
        let id = session.id().as_str().to_owned();
        let existing = self
            .transaction
            .query(move |connection| {
                Box::pin(async move {
                    crate::adapters::session::sqlite::records::load(connection, &id).await
                })
            })
            .await?;
        let record = persist_session(session, existing, &self.codec, true)?;
        self.transaction
            .query(move |connection| {
                Box::pin(crate::adapters::session::sqlite::write_record(
                    connection, record,
                ))
            })
            .await
    }

    async fn get_run(&mut self, id: &str) -> Result<Option<crate::domain::run::Run>, StoreError> {
        let id = id.to_owned();
        self.transaction
            .query(move |connection| {
                Box::pin(async move { crate::adapters::run::get_run(connection, &id).await })
            })
            .await
    }
    async fn run_for_key(
        &mut self,
        session_id: &str,
        key: &str,
    ) -> Result<Option<crate::domain::run::Run>, StoreError> {
        let session_id = session_id.to_owned();
        let key = key.to_owned();
        self.transaction
            .query(move |connection| {
                Box::pin(
                    async move { crate::adapters::run::get(connection, &session_id, &key).await },
                )
            })
            .await
    }
    async fn save_run(&mut self, request: crate::domain::run::Run) -> Result<(), StoreError> {
        self.transaction
            .query(move |connection| Box::pin(crate::adapters::run::save(connection, request)))
            .await
    }
    async fn active_session_runs(
        &mut self,
        session_id: &str,
    ) -> Result<Vec<crate::domain::run::Run>, StoreError> {
        let id = session_id.to_owned();
        self.transaction
            .query(move |connection| {
                Box::pin(
                    async move { crate::adapters::run::active_for_session(connection, &id).await },
                )
            })
            .await
    }

    async fn unfinished_runs(&mut self) -> Result<Vec<crate::domain::run::Run>, StoreError> {
        self.transaction
            .query(|connection| Box::pin(crate::adapters::run::unfinished(connection)))
            .await
    }
    async fn sessions_with_responses_to_expire(
        &mut self,
        cutoff: SystemTime,
        kept: usize,
    ) -> Result<Vec<String>, StoreError> {
        self.transaction
            .query(move |connection| {
                Box::pin(crate::adapters::run::sessions_with_responses_to_expire(
                    connection, cutoff, kept,
                ))
            })
            .await
    }
    async fn completed_responses(
        &mut self,
        session_id: &str,
    ) -> Result<Vec<crate::domain::idempotent_request::CompletedResponse>, StoreError> {
        let session_id = session_id.to_owned();
        self.transaction
            .query(move |connection| {
                Box::pin(async move {
                    crate::adapters::run::completed_responses(connection, &session_id).await
                })
            })
            .await
    }
    async fn expire_responses(
        &mut self,
        session_id: &str,
        keys: Vec<String>,
    ) -> Result<u64, StoreError> {
        let session_id = session_id.to_owned();
        self.transaction
            .query(move |connection| {
                Box::pin(async move {
                    crate::adapters::run::expire_responses(connection, &session_id, &keys).await
                })
            })
            .await
    }

    async fn get_secret(&mut self, name: &SecretName) -> Result<Option<Secret>, StoreError> {
        let (name, sealer) = (name.clone(), self.sealer.clone());
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(
                    async move { crate::adapters::secret::get(connection, &sealer, &name).await },
                )
            })
            .await
    }

    async fn save_secret(&mut self, secret: Secret, now: SystemTime) -> Result<(), StoreError> {
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    crate::adapters::secret::save(connection, &sealer, &secret, now).await
                })
            })
            .await
    }

    async fn remove_secret(&mut self, name: &SecretName) -> Result<bool, StoreError> {
        let name = name.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move { crate::adapters::secret::remove(connection, &name).await })
            })
            .await
    }

    async fn secret_names(&mut self, prefix: Option<&str>) -> Result<Vec<String>, StoreError> {
        let prefix = prefix.map(str::to_owned);
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    crate::adapters::secret::names(connection, prefix.as_deref()).await
                })
            })
            .await
    }

    async fn get_grant(
        &mut self,
        blueprint: &str,
        server: &str,
    ) -> Result<Option<McpGrant>, StoreError> {
        let (blueprint, server, sealer) =
            (blueprint.to_owned(), server.to_owned(), self.sealer.clone());
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    crate::adapters::mcp_grant::get(connection, &sealer, &blueprint, &server).await
                })
            })
            .await
    }

    async fn save_grant(&mut self, grant: McpGrant) -> Result<(), StoreError> {
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    crate::adapters::mcp_grant::save(connection, &sealer, &grant).await
                })
            })
            .await
    }

    async fn remove_grant(&mut self, blueprint: &str, server: &str) -> Result<bool, StoreError> {
        let (blueprint, server) = (blueprint.to_owned(), server.to_owned());
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    crate::adapters::mcp_grant::remove(connection, &blueprint, &server).await
                })
            })
            .await
    }

    async fn grant_version(
        &mut self,
        blueprint: &str,
        server: &str,
    ) -> Result<Option<GrantVersion>, StoreError> {
        let (blueprint, server) = (blueprint.to_owned(), server.to_owned());
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    crate::adapters::mcp_grant::version(connection, &blueprint, &server).await
                })
            })
            .await
    }

    async fn list_grants(
        &mut self,
        blueprint: Option<&str>,
    ) -> Result<Vec<GrantOverview>, StoreError> {
        let blueprint = blueprint.map(str::to_owned);
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    crate::adapters::mcp_grant::overviews(connection, blueprint.as_deref()).await
                })
            })
            .await
    }

    async fn remove_blueprint_grants(&mut self, blueprint: &str) -> Result<(), StoreError> {
        let blueprint = blueprint.to_owned();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    crate::adapters::mcp_grant::remove_for_blueprint(connection, &blueprint).await
                })
            })
            .await
    }

    async fn session_is_active(&mut self, id: &str) -> Result<bool, StoreError> {
        let id = id.to_owned();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move { kv::is_active(connection, &id).await })
            })
            .await
    }

    async fn session_state_get(
        &mut self,
        session_id: &str,
        key: &[u16],
    ) -> Result<Option<Vec<u16>>, StoreError> {
        let (session_id, key, sealer) = (session_id.to_owned(), key.to_vec(), self.sealer.clone());
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move { kv::get(connection, &sealer, &session_id, &key).await })
            })
            .await
    }

    async fn session_state_has(
        &mut self,
        session_id: &str,
        key: &[u16],
    ) -> Result<bool, StoreError> {
        let (session_id, key) = (session_id.to_owned(), key.to_vec());
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move { kv::has(connection, &session_id, &key).await })
            })
            .await
    }

    async fn session_state_usage(
        &mut self,
        session_id: &str,
        key: &[u16],
    ) -> Result<SessionStateUsage, StoreError> {
        let (session_id, key) = (session_id.to_owned(), key.to_vec());
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move { kv::usage(connection, &session_id, &key).await })
            })
            .await
    }

    async fn session_state_put(
        &mut self,
        session_id: &str,
        key: &[u16],
        payload: &[u16],
        size: SessionStateSize,
    ) -> Result<(), StoreError> {
        let (session_id, key, payload, sealer) = (
            session_id.to_owned(),
            key.to_vec(),
            payload.to_vec(),
            self.sealer.clone(),
        );
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    kv::put(connection, &sealer, &session_id, &key, &payload, size).await
                })
            })
            .await
    }

    async fn session_state_remove(
        &mut self,
        session_id: &str,
        key: &[u16],
    ) -> Result<bool, StoreError> {
        let (session_id, key) = (session_id.to_owned(), key.to_vec());
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move { kv::remove(connection, &session_id, &key).await })
            })
            .await
    }

    async fn session_state_keys(
        &mut self,
        session_id: &str,
        after: Option<&[u16]>,
        max_keys: usize,
    ) -> Result<SessionStateKeys, StoreError> {
        let (session_id, after) = (session_id.to_owned(), after.map(<[u16]>::to_vec));
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    kv::keys(connection, &session_id, after.as_deref(), max_keys).await
                })
            })
            .await
    }

    async fn put_last_run(
        &mut self,
        session_id: &str,
        run_id: &str,
        completed_at: SystemTime,
        staged: &StagedLastRun,
    ) -> Result<Option<String>, StoreError> {
        let (session_id, run_id, staged) =
            (session_id.to_owned(), run_id.to_owned(), staged.clone());
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    last_run::put(connection, &session_id, &run_id, completed_at, &staged).await
                })
            })
            .await
    }

    async fn remove_last_run(&mut self, session_id: &str) -> Result<Option<String>, StoreError> {
        let session_id = session_id.to_owned();
        self.transaction
            .query(move |connection| {
                Box::pin(async move { Ok(last_run::remove(connection, &session_id).await?) })
            })
            .await
    }

    async fn commit(self: Box<Self>) -> Result<(), StoreError> {
        self.transaction.commit().await
    }
}
