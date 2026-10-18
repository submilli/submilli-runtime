//! SQLite implementation of the durability-owned unit of work.
use super::transaction::SqliteTransaction;
use crate::application::error::StoreError;
use crate::database::ServerDatabase;
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
        scope: &str,
        id: &str,
    ) -> Result<Option<Reader>, DurableStoreError> {
        let scope = scope.to_owned();
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::get_reader(connection, &sealer, &scope, &id).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn save_reader(&mut self, scope: &str, reader: Reader) -> Result<(), DurableStoreError> {
        let scope = scope.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::save_reader(connection, &sealer, &scope, reader).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn listing_bytes(&mut self, scope: &str, id: &str) -> Result<u64, DurableStoreError> {
        let (scope, id) = (scope.to_owned(), id.to_owned());
        self.transaction.query_recoverable(move |connection| Box::pin(async move {
            let bytes = sqlx::query_scalar::<_, i64>("SELECT length(content) FROM durable_filesystem_listings WHERE scope_id=? AND resource_id=? AND content IS NOT NULL")
                .bind(scope).bind(id).fetch_optional(connection).await.map_err(crate::adapters::sql::database)?.ok_or(StoreError::Conflict)?;
            u64::try_from(bytes).map_err(|_| StoreError::Conflict)
        })).await.map_err(durable_error)
    }
    async fn get_listing(
        &mut self,
        scope: &str,
        id: &str,
    ) -> Result<Option<Listing>, DurableStoreError> {
        let scope = scope.to_owned();
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::get_listing(connection, &sealer, &scope, &id).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn save_listing(
        &mut self,
        scope: &str,
        listing: Listing,
    ) -> Result<(), DurableStoreError> {
        let scope = scope.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::save_listing(connection, &sealer, &scope, listing).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn get_writer(
        &mut self,
        scope: &str,
        id: &str,
    ) -> Result<Option<Writer>, DurableStoreError> {
        let scope = scope.to_owned();
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::get_writer(connection, &sealer, &scope, &id).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn save_writer(&mut self, scope: &str, writer: Writer) -> Result<(), DurableStoreError> {
        let scope = scope.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::save_writer(connection, &sealer, &scope, writer).await
                })
            })
            .await
            .map_err(durable_error)
    }

    async fn closing_writer(&mut self, scope: &str) -> Result<Option<Writer>, DurableStoreError> {
        let scope = scope.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::closing_writer(connection, &sealer, &scope).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn writer_for_effect(
        &mut self,
        scope: &str,
        effect: &str,
    ) -> Result<Option<Writer>, DurableStoreError> {
        let scope = scope.to_owned();
        let effect = effect.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::writer_for_effect(connection, &sealer, &scope, &effect)
                        .await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn pending_resource(
        &mut self,
        scope: &str,
    ) -> Result<Option<PendingResource>, DurableStoreError> {
        let scope = scope.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::pending_resource(connection, &sealer, &scope).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn resource_counts(&mut self, scope: &str) -> Result<ResourceCounts, DurableStoreError> {
        let scope = scope.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::resource_counts(connection, &sealer, &scope).await
                })
            })
            .await
            .map_err(durable_error)
    }

    async fn get_operation(
        &mut self,
        scope: &str,
        id: &str,
    ) -> Result<Option<Operation>, DurableStoreError> {
        let scope = scope.to_owned();
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::get_operation(connection, &sealer, &scope, &id).await
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
        scope: &str,
    ) -> Result<Option<String>, DurableStoreError> {
        let scope = scope.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::pending_operation(connection, &sealer, &scope).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn operation_count(&mut self, scope: &str) -> Result<u64, DurableStoreError> {
        let scope = scope.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::operation_count(connection, &sealer, &scope).await
                })
            })
            .await
            .map_err(durable_error)
    }

    async fn get_chunk(
        &mut self,
        scope: &str,
        writer: &str,
        id: &str,
    ) -> Result<Option<Chunk>, DurableStoreError> {
        let scope = scope.to_owned();
        let writer = writer.to_owned();
        let id = id.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::get_chunk(connection, &sealer, &scope, &writer, &id).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn save_chunk(&mut self, scope: &str, chunk: Chunk) -> Result<(), DurableStoreError> {
        let scope = scope.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::save_chunk(connection, &sealer, &scope, chunk).await
                })
            })
            .await
            .map_err(durable_error)
    }
    async fn writer_chunks(
        &mut self,
        scope: &str,
        writer: &str,
        limit: u32,
    ) -> Result<Vec<Chunk>, DurableStoreError> {
        let scope = scope.to_owned();
        let writer = writer.to_owned();
        let sealer = self.sealer.clone();
        self.transaction
            .query_recoverable(move |connection| {
                Box::pin(async move {
                    super::durable_state::writer_chunks(connection, &sealer, &scope, &writer, limit)
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
