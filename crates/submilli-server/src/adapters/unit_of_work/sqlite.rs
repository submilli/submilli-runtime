use crate::adapters::session::repository::{persist_session, restore_domain};
use crate::domain::session::Session;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use futures::future::BoxFuture;
use sqlx::SqliteConnection;
use tokio::sync::{mpsc, oneshot};

use crate::adapters::session::credentials::CredentialCodec;
use crate::application::unit_of_work::{UnitOfWork, UnitOfWorkFactory};
use crate::blueprint::StoreError;
use crate::database::{DatabaseError, ServerDatabase};

pub(crate) struct SqliteUnitOfWorkFactory {
    pub database: Arc<ServerDatabase>,
    pub session_root: PathBuf,
    pub cipher: Option<Arc<submilli_shared::secret_store::SecretCipher>>,
}

#[async_trait::async_trait]
impl UnitOfWorkFactory for SqliteUnitOfWorkFactory {
    async fn begin(&self) -> Result<Box<dyn UnitOfWork>, StoreError> {
        let (sender, receiver) = mpsc::channel(1);
        let database = self.database.clone();
        let driver = Box::pin(async move {
            database
                .transaction(move |connection| Box::pin(run(connection, receiver)))
                .await
        });
        let mut unit = SqliteUnitOfWork {
            sender,
            driver: Some(driver),
            session_root: self.session_root.clone(),
            codec: Arc::new(CredentialCodec::new(self.cipher.clone())),
        };
        unit.query(|_| Box::pin(async { Ok(()) })).await?;
        Ok(Box::new(unit))
    }
}

type Operation = Box<
    dyn for<'a> FnOnce(&'a mut SqliteConnection) -> BoxFuture<'a, Result<(), DatabaseError>> + Send,
>;
enum Command {
    Run(Operation),
    Commit,
}

struct SqliteUnitOfWork {
    sender: mpsc::Sender<Command>,
    // Polling submits one transaction to the database owner. Dropping this
    // future cancels that transaction through the owner's existing reply channel.
    driver: Option<BoxFuture<'static, Result<(), DatabaseError>>>,
    session_root: PathBuf,
    codec: Arc<CredentialCodec>,
}

impl SqliteUnitOfWork {
    async fn query<T, F>(&mut self, operation: F) -> Result<T, StoreError>
    where
        T: Send + 'static,
        F: for<'a> FnOnce(&'a mut SqliteConnection) -> BoxFuture<'a, Result<T, DatabaseError>>
            + Send
            + 'static,
    {
        let Some(driver) = self.driver.as_mut() else {
            return Err(DatabaseError::Closed.into());
        };
        let (reply, result) = oneshot::channel();
        let job = Command::Run(Box::new(move |connection| {
            Box::pin(async move {
                let value = operation(connection).await?;
                reply.send(value).map_err(|_| DatabaseError::Closed)
            })
        }));
        match self.sender.try_send(job) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => return Err(DatabaseError::Busy.into()),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return self.finish().await.and(Err(DatabaseError::Closed.into()));
            }
        }
        tokio::select! {
            biased;
            outcome = driver => {
                self.driver = None;
                outcome?;
                Err(DatabaseError::Closed.into())
            }
            value = result => match value {
                Ok(value) => Ok(value),
                Err(_) => self.finish().await.and(Err(DatabaseError::Closed.into())),
            }
        }
    }

    async fn finish(&mut self) -> Result<(), StoreError> {
        let driver = self.driver.take().ok_or(DatabaseError::Closed)?;
        driver.await.map_err(Into::into)
    }
}

#[async_trait::async_trait]
impl UnitOfWork for SqliteUnitOfWork {
    async fn blueprint_exists(&mut self, name: &str) -> Result<bool, StoreError> {
        let name = name.to_owned();
        self.query(move |connection| {
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
        self.query(move |connection| Box::pin(async move {
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
            .query(move |connection| {
                Box::pin(async move {
                    crate::adapters::session::sqlite::records::load(connection, &id).await
                })
            })
            .await?;
        let record = persist_session(session, existing, &self.codec, true)?;
        self.query(move |connection| {
            Box::pin(crate::adapters::session::sqlite::write_record(
                connection, record,
            ))
        })
        .await
    }

    async fn get_request(
        &mut self,
        session_id: &str,
        key: &str,
    ) -> Result<Option<crate::domain::idempotent_request::IdempotentRequest>, StoreError> {
        let session_id = session_id.to_owned();
        let key = key.to_owned();
        self.query(move |connection| {
            Box::pin(async move {
                crate::adapters::idempotency::get(connection, &session_id, &key).await
            })
        })
        .await
    }
    async fn save_request(
        &mut self,
        request: crate::domain::idempotent_request::IdempotentRequest,
    ) -> Result<(), StoreError> {
        self.query(move |connection| {
            Box::pin(crate::adapters::idempotency::save(connection, request))
        })
        .await
    }
    async fn remove_request(&mut self, session_id: &str, key: &str) -> Result<(), StoreError> {
        let session_id = session_id.to_owned();
        let key = key.to_owned();
        self.query(move |connection| {
            Box::pin(async move {
                sqlx::query("DELETE FROM idempotent_requests WHERE session_id=? AND request_key=?")
                    .bind(session_id)
                    .bind(key)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
    }
    async fn remove_session_requests(&mut self, session_id: &str) -> Result<(), StoreError> {
        let session_id = session_id.to_owned();
        self.query(move |connection| {
            Box::pin(async move {
                sqlx::query("DELETE FROM idempotent_requests WHERE session_id=?")
                    .bind(session_id)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .await
    }
    async fn unfinished_requests(
        &mut self,
    ) -> Result<Vec<crate::domain::idempotent_request::IdempotentRequest>, StoreError> {
        self.query(|connection| Box::pin(crate::adapters::idempotency::unfinished(connection)))
            .await
    }

    async fn commit(mut self: Box<Self>) -> Result<(), StoreError> {
        match self.sender.try_send(Command::Commit) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => return Err(DatabaseError::Busy.into()),
            Err(mpsc::error::TrySendError::Closed(_)) => {
                return self.finish().await.and(Err(DatabaseError::Closed.into()));
            }
        }
        self.finish().await
    }
}

async fn run(
    connection: &mut SqliteConnection,
    mut commands: mpsc::Receiver<Command>,
) -> Result<(), DatabaseError> {
    while let Some(command) = commands.recv().await {
        match command {
            Command::Run(operation) => operation(connection).await?,
            Command::Commit => return Ok(()),
        }
    }
    Err(DatabaseError::Closed)
}
