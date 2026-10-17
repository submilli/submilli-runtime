//! SQLite implementation of the durability-owned unit of work.
use super::transaction::SqliteTransaction;
use crate::application::error::StoreError;
use crate::database::ServerDatabase;
use std::sync::Arc;
use submilli_durable::sealing::Sealer;
use submilli_durable::state::*;
use submilli_durable::unit_of_work::{StoreError as DurableStoreError, UnitFactory, UnitOfWork};
use submilli_shared::secret_store::SecretCipher;

pub(crate) fn units(database: Arc<ServerDatabase>, cipher: Arc<SecretCipher>) -> UnitFactory {
    Arc::new(move || {
        let database = database.clone();
        let cipher = cipher.clone();
        Box::pin(async move {
            if database.is_ephemeral() {
                return Err(DurableStoreError::Io(
                    "durability requires a persistent database".into(),
                ));
            }
            let transaction = SqliteTransaction::begin(database)
                .await
                .map_err(durable_error)?;
            Ok(Box::new(SqliteDurableUnitOfWork {
                transaction,
                sealer: Sealer::new(Some(cipher)),
            }) as Box<dyn UnitOfWork>)
        })
    })
}

struct SqliteDurableUnitOfWork {
    transaction: SqliteTransaction,
    sealer: Sealer,
}

#[async_trait::async_trait]
impl UnitOfWork for SqliteDurableUnitOfWork {
    async fn filesystem_store_identity(&mut self) -> Result<Option<String>, DurableStoreError> {
        self.transaction
            .query_recoverable(|connection| {
                Box::pin(async move {
                    sqlx::query_scalar(
                        "SELECT identity FROM durable_filesystem_store WHERE singleton=1",
                    )
                    .fetch_optional(connection)
                    .await
                    .map_err(|error| StoreError::Database(Box::new(error)))
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn identify_filesystem_store(
        &mut self,
        identity: String,
    ) -> Result<(), DurableStoreError> {
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    sqlx::query(
                        "INSERT INTO durable_filesystem_store(singleton,identity) VALUES(1,?)",
                    )
                    .bind(identity)
                    .execute(connection)
                    .await
                    .map_err(|error| StoreError::Database(Box::new(error)))?;
                    Ok(())
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn get_reader(
        &mut self,
        workspace: &str,
        id: &str,
    ) -> Result<Option<Reader>, DurableStoreError> {
        let workspace = workspace.to_owned();
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::get_reader(connection, &sealer, &workspace, &id).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn save_reader(
        &mut self,
        workspace: &str,
        reader: Reader,
    ) -> Result<(), DurableStoreError> {
        let workspace = workspace.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::save_reader(connection, &sealer, &workspace, reader).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn get_listing(
        &mut self,
        workspace: &str,
        id: &str,
    ) -> Result<Option<Listing>, DurableStoreError> {
        let workspace = workspace.to_owned();
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::get_listing(connection, &sealer, &workspace, &id).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn save_listing(
        &mut self,
        workspace: &str,
        listing: Listing,
    ) -> Result<(), DurableStoreError> {
        let workspace = workspace.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::save_listing(connection, &sealer, &workspace, listing)
                        .await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn get_writer(
        &mut self,
        workspace: &str,
        id: &str,
    ) -> Result<Option<Writer>, DurableStoreError> {
        let workspace = workspace.to_owned();
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::get_writer(connection, &sealer, &workspace, &id).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn save_writer(
        &mut self,
        workspace: &str,
        writer: Writer,
    ) -> Result<(), DurableStoreError> {
        let workspace = workspace.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::save_writer(connection, &sealer, &workspace, writer).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn get_plan(
        &mut self,
        workspace: &str,
        id: &str,
    ) -> Result<Option<Plan>, DurableStoreError> {
        let workspace = workspace.to_owned();
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::get_plan(connection, &sealer, &workspace, &id).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn save_plan(&mut self, workspace: &str, plan: Plan) -> Result<(), DurableStoreError> {
        let workspace = workspace.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::save_plan(connection, &sealer, &workspace, plan).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn plans_after(
        &mut self,
        workspace: &str,
        after: &str,
        limit: u32,
    ) -> Result<Vec<Plan>, DurableStoreError> {
        let workspace = workspace.to_owned();
        let after = after.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::plans_after(
                        connection, &sealer, &workspace, &after, limit,
                    )
                    .await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn pending_plan(&mut self, workspace: &str) -> Result<Option<Plan>, DurableStoreError> {
        let workspace = workspace.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::pending_plan(connection, &sealer, &workspace).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn closing_writer(
        &mut self,
        workspace: &str,
    ) -> Result<Option<Writer>, DurableStoreError> {
        let workspace = workspace.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::closing_writer(connection, &sealer, &workspace).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn writer_for_effect(
        &mut self,
        workspace: &str,
        effect: &str,
    ) -> Result<Option<Writer>, DurableStoreError> {
        let workspace = workspace.to_owned();
        let effect = effect.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::writer_for_effect(
                        connection, &sealer, &workspace, &effect,
                    )
                    .await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn pending_resource(
        &mut self,
        workspace: &str,
    ) -> Result<Option<PendingResource>, DurableStoreError> {
        let workspace = workspace.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::pending_resource(connection, &sealer, &workspace).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn resource_counts(
        &mut self,
        workspace: &str,
    ) -> Result<ResourceCounts, DurableStoreError> {
        let workspace = workspace.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::resource_counts(connection, &sealer, &workspace).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn get_run(&mut self, run: &str) -> Result<Option<RunState>, DurableStoreError> {
        let run = run.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(
                    async move { super::durable_state::get_run(connection, &sealer, &run).await },
                )
            })
            .await
            .map_err(durable_error)
    }
    async fn get_workspace(
        &mut self,
        id: &str,
    ) -> Result<Option<WorkspaceState>, DurableStoreError> {
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::get_workspace(connection, &sealer, &id).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn save_workspace(&mut self, workspace: WorkspaceState) -> Result<(), DurableStoreError> {
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::save_workspace(connection, &sealer, workspace).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn get_effect(
        &mut self,
        run: &str,
        id: &str,
    ) -> Result<Option<Effect>, DurableStoreError> {
        let run = run.to_owned();
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::get_effect(connection, &sealer, &run, &id).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn save_effect(&mut self, effect: Effect) -> Result<(), DurableStoreError> {
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::save_effect(connection, &sealer, effect).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn pending_effect(&mut self, run: &str) -> Result<Option<String>, DurableStoreError> {
        let run = run.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::pending_effect(connection, &sealer, &run).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn effect_count(&mut self, run: &str) -> Result<u64, DurableStoreError> {
        let run = run.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::effect_count(connection, &sealer, &run).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn effects_after(
        &mut self,
        run: &str,
        operation: &str,
        after: &str,
        limit: u32,
    ) -> Result<Vec<Effect>, DurableStoreError> {
        let run = run.to_owned();
        let operation = operation.to_owned();
        let after = after.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::effects_after(
                        connection, &sealer, &run, &operation, &after, limit,
                    )
                    .await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn get_chunk(
        &mut self,
        workspace: &str,
        writer: &str,
        id: &str,
    ) -> Result<Option<Chunk>, DurableStoreError> {
        let workspace = workspace.to_owned();
        let writer = writer.to_owned();
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::get_chunk(connection, &sealer, &workspace, &writer, &id)
                        .await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn save_chunk(&mut self, workspace: &str, chunk: Chunk) -> Result<(), DurableStoreError> {
        let workspace = workspace.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::save_chunk(connection, &sealer, &workspace, chunk).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn writer_chunks(
        &mut self,
        workspace: &str,
        writer: &str,
        limit: u32,
    ) -> Result<Vec<Chunk>, DurableStoreError> {
        let workspace = workspace.to_owned();
        let writer = writer.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::writer_chunks(
                        connection, &sealer, &workspace, &writer, limit,
                    )
                    .await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn chunks_after(
        &mut self,
        workspace: &str,
        after: (&str, &str),
        limit: u32,
    ) -> Result<Vec<Chunk>, DurableStoreError> {
        let workspace = workspace.to_owned();
        let after = (after.0.to_owned(), after.1.to_owned());
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::chunks_after(
                        connection,
                        &sealer,
                        &workspace,
                        (&after.0, &after.1),
                        limit,
                    )
                    .await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn commit(self: Box<Self>) -> Result<(), DurableStoreError> {
        self.transaction.commit().await.map_err(durable_error)
    }
}

pub(crate) fn durable_error(error: StoreError) -> DurableStoreError {
    match error {
        StoreError::Conflict => DurableStoreError::Conflict,
        StoreError::Busy => DurableStoreError::Busy,
        StoreError::Credentials(message) => DurableStoreError::Credentials(message),
        StoreError::Io(message) => DurableStoreError::Io(message),
        other => DurableStoreError::Database(Box::new(other)),
    }
}
