//! SQLite implementation of the durability-owned unit of work.
use super::transaction::SqliteTransaction;
use crate::application::error::StoreError;
use crate::database::ServerDatabase;
use interpreter::runtime::durable::{EffectId, RunId};
use std::sync::Arc;
use submilli_durable::codec::Sealer;
use submilli_durable::unit_of_work::*;
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
    async fn get_reader(
        &mut self,
        run_id: &RunId,
        id: &EffectId,
    ) -> Result<Option<Reader>, DurableStoreError> {
        let run_id = run_id.to_owned();
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::get_reader(connection, &sealer, &run_id, &id).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn save_reader(
        &mut self,
        run_id: &RunId,
        reader: Reader,
    ) -> Result<(), DurableStoreError> {
        let run_id = run_id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::save_reader(connection, &sealer, &run_id, reader).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn listing_bytes(
        &mut self,
        run_id: &RunId,
        id: &EffectId,
    ) -> Result<u64, DurableStoreError> {
        let (run_id, id) = (run_id.to_owned(), id.to_owned());
        self.transaction.query_recoverable(move |connection| Box::pin(async move {
            let bytes = sqlx::query_scalar::<_, i64>("SELECT length(content) FROM durable_filesystem_listings WHERE scope_id=? AND resource_id=? AND content IS NOT NULL")
                .bind(run_id.as_str()).bind(id.as_str()).fetch_optional(connection).await.map_err(crate::adapters::sql::database)?.ok_or(StoreError::Conflict)?;
            u64::try_from(bytes).map_err(|_| StoreError::Conflict)
        })).await.map_err(durable_error)
    }
    async fn get_listing(
        &mut self,
        run_id: &RunId,
        id: &EffectId,
    ) -> Result<Option<Listing>, DurableStoreError> {
        let run_id = run_id.to_owned();
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::get_listing(connection, &sealer, &run_id, &id).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn save_listing(
        &mut self,
        run_id: &RunId,
        listing: Listing,
    ) -> Result<(), DurableStoreError> {
        let run_id = run_id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::save_listing(connection, &sealer, &run_id, listing).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn get_writer(
        &mut self,
        run_id: &RunId,
        id: &EffectId,
    ) -> Result<Option<Writer>, DurableStoreError> {
        let run_id = run_id.to_owned();
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::get_writer(connection, &sealer, &run_id, &id).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn save_writer(
        &mut self,
        run_id: &RunId,
        writer: Writer,
    ) -> Result<(), DurableStoreError> {
        let run_id = run_id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::save_writer(connection, &sealer, &run_id, writer).await
                })
            })
            .await
            .map_err(durable_error)
    }

    async fn closing_writer(
        &mut self,
        run_id: &RunId,
    ) -> Result<Option<Writer>, DurableStoreError> {
        let run_id = run_id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::closing_writer(connection, &sealer, &run_id).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn writer_for_effect(
        &mut self,
        run_id: &RunId,
        effect: &EffectId,
    ) -> Result<Option<Writer>, DurableStoreError> {
        let run_id = run_id.to_owned();
        let effect = effect.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::writer_for_effect(connection, &sealer, &run_id, &effect)
                        .await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn pending_resource(
        &mut self,
        run_id: &RunId,
    ) -> Result<Option<PendingResource>, DurableStoreError> {
        let run_id = run_id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::pending_resource(connection, &sealer, &run_id).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn resource_counts(
        &mut self,
        run_id: &RunId,
    ) -> Result<ResourceCounts, DurableStoreError> {
        let run_id = run_id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::resource_counts(connection, &sealer, &run_id).await
                })
            })
            .await
            .map_err(durable_error)
    }

    async fn get_operation(
        &mut self,
        run_id: &RunId,
        id: &EffectId,
    ) -> Result<Option<Operation>, DurableStoreError> {
        let run_id = run_id.to_owned();
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::get_operation(connection, &sealer, &run_id, &id).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn save_operation(&mut self, effect: Operation) -> Result<(), DurableStoreError> {
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::save_operation(connection, &sealer, effect).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn pending_operation(
        &mut self,
        run_id: &RunId,
    ) -> Result<Option<EffectId>, DurableStoreError> {
        let run_id = run_id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::pending_operation(connection, &sealer, &run_id).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn operation_count(&mut self, run_id: &RunId) -> Result<u64, DurableStoreError> {
        let run_id = run_id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::operation_count(connection, &sealer, &run_id).await
                })
            })
            .await
            .map_err(durable_error)
    }

    async fn get_chunk(
        &mut self,
        run_id: &RunId,
        writer: &EffectId,
        id: &EffectId,
    ) -> Result<Option<Chunk>, DurableStoreError> {
        let run_id = run_id.to_owned();
        let writer = writer.to_owned();
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::get_chunk(connection, &sealer, &run_id, &writer, &id)
                        .await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn save_chunk(&mut self, run_id: &RunId, chunk: Chunk) -> Result<(), DurableStoreError> {
        let run_id = run_id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::save_chunk(connection, &sealer, &run_id, chunk).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn writer_chunks(
        &mut self,
        run_id: &RunId,
        writer: &EffectId,
        limit: u32,
    ) -> Result<Vec<Chunk>, DurableStoreError> {
        let run_id = run_id.to_owned();
        let writer = writer.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::writer_chunks(
                        connection, &sealer, &run_id, &writer, limit,
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
