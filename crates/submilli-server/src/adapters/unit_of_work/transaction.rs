use crate::application::error::StoreError;
use crate::database::{DatabaseError, ServerDatabase};
use futures::future::BoxFuture;
use sqlx::SqliteConnection;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

type Operation = Box<
    dyn for<'a> FnOnce(&'a mut SqliteConnection) -> BoxFuture<'a, Result<(), DatabaseError>> + Send,
>;
enum Command {
    Run(Operation),
    Commit,
}

pub(crate) struct SqliteTransaction {
    sender: mpsc::Sender<Command>,
    // Dropping this driver cancels and rolls back through the database owner.
    driver: Option<BoxFuture<'static, Result<(), DatabaseError>>>,
}

impl SqliteTransaction {
    pub(crate) async fn query<T, F>(&mut self, operation: F) -> Result<T, StoreError>
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

    /// [`Self::query`] for an operation that can also fail without SQL failing,
    /// such as a sealed value that does not open or a conditional write that
    /// lost: those come back to the caller and leave the transaction usable.
    /// A database failure still ends the transaction, since SQLite may already
    /// have rolled it back.
    pub(crate) async fn query_recoverable<T, F>(&mut self, operation: F) -> Result<T, StoreError>
    where
        T: Send + 'static,
        F: for<'a> FnOnce(&'a mut SqliteConnection) -> BoxFuture<'a, Result<T, StoreError>>
            + Send
            + 'static,
    {
        self.query(move |connection| {
            Box::pin(async move {
                match operation(connection).await {
                    Err(StoreError::Database(source)) => Err(match source.downcast() {
                        Ok(error) => *error,
                        Err(other) => DatabaseError::Import(other.to_string()),
                    }),
                    outcome => Ok(outcome),
                }
            })
        })
        .await?
    }

    async fn finish(&mut self) -> Result<(), StoreError> {
        let driver = self.driver.take().ok_or(DatabaseError::Closed)?;
        driver.await.map_err(Into::into)
    }
}

impl SqliteTransaction {
    pub(crate) async fn begin(database: Arc<ServerDatabase>) -> Result<Self, StoreError> {
        let (sender, receiver) = mpsc::channel(1);
        let driver = Box::pin(async move {
            database
                .transaction(move |connection| Box::pin(run(connection, receiver)))
                .await
        });
        let mut unit = Self {
            sender,
            driver: Some(driver),
        };
        unit.query(|_| Box::pin(async { Ok(()) })).await?;
        Ok(unit)
    }
    pub(crate) async fn commit(mut self) -> Result<(), StoreError> {
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
