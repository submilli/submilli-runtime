use crate::adapters::sql::database;
use crate::application::error::StoreError;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};
use submilli_durable::{
    record_codec::{self, OperationColumns, ResourceColumns},
    sealing::Sealer,
    state::*,
};
fn invalid(error: impl std::fmt::Display) -> StoreError {
    StoreError::Io(error.to_string())
}
fn decode_reader(row: SqliteRow, sealer: &Sealer, scope: &str) -> Result<Reader, StoreError> {
    record_codec::decode_reader(
        sealer,
        scope,
        row.try_get("resource_id").map_err(database)?,
        ResourceColumns {
            request: row.try_get("request").map_err(database)?,
            result: row.try_get("position").map_err(database)?,
            error: row.try_get("error").map_err(database)?,
            progress: row.try_get("progress").map_err(database)?,
        },
    )
    .map_err(invalid)
}
pub(super) async fn get_reader(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    scope: &str,
    id: &str,
) -> Result<Option<Reader>, StoreError> {
    sqlx::query("SELECT resource_id,request,position,error,progress FROM durable_filesystem_readers WHERE scope_id=? AND resource_id=?")
        .bind(scope).bind(id).fetch_optional(connection).await.map_err(database)?
        .map(|row| decode_reader(row, sealer, scope)).transpose()
}
pub(super) async fn save_reader(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    scope: &str,
    value: Reader,
) -> Result<(), StoreError> {
    let prior = get_reader(connection, sealer, scope, &value.id).await?;
    value.validate_successor(prior.as_ref()).map_err(invalid)?;
    let id = &value.id;
    let columns = record_codec::encode_reader(sealer, scope, &value).map_err(invalid)?;
    sqlx::query("INSERT INTO durable_filesystem_readers(scope_id,resource_id,request,position,error,progress) VALUES(?,?,?,?,?,?) ON CONFLICT(scope_id,resource_id) DO UPDATE SET request=excluded.request,position=excluded.position,error=excluded.error,progress=excluded.progress")
        .bind(scope).bind(id).bind(columns.request).bind(columns.result).bind(columns.error).bind(columns.progress).execute(connection).await.map_err(database)?;
    Ok(())
}
fn decode_listing(row: SqliteRow, sealer: &Sealer, scope: &str) -> Result<Listing, StoreError> {
    record_codec::decode_listing(
        sealer,
        scope,
        row.try_get("resource_id").map_err(database)?,
        ResourceColumns {
            request: row.try_get("request").map_err(database)?,
            result: row.try_get("content").map_err(database)?,
            error: row.try_get("error").map_err(database)?,
            progress: row.try_get("progress").map_err(database)?,
        },
    )
    .map_err(invalid)
}
pub(super) async fn get_listing(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    scope: &str,
    id: &str,
) -> Result<Option<Listing>, StoreError> {
    sqlx::query("SELECT resource_id,request,content,error,progress FROM durable_filesystem_listings WHERE scope_id=? AND resource_id=?")
        .bind(scope).bind(id).fetch_optional(connection).await.map_err(database)?
        .map(|row| decode_listing(row, sealer, scope)).transpose()
}
pub(super) async fn save_listing(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    scope: &str,
    value: Listing,
) -> Result<(), StoreError> {
    let prior = get_listing(connection, sealer, scope, &value.id).await?;
    value.validate_successor(prior.as_ref()).map_err(invalid)?;
    let id = &value.id;
    let columns = record_codec::encode_listing(sealer, scope, &value).map_err(invalid)?;
    sqlx::query("INSERT INTO durable_filesystem_listings(scope_id,resource_id,request,content,error,progress) VALUES(?,?,?,?,?,?) ON CONFLICT(scope_id,resource_id) DO UPDATE SET request=excluded.request,content=excluded.content,error=excluded.error,progress=excluded.progress")
        .bind(scope).bind(id).bind(columns.request).bind(columns.result).bind(columns.error).bind(columns.progress).execute(connection).await.map_err(database)?;
    Ok(())
}
fn decode_writer(row: SqliteRow, sealer: &Sealer, scope: &str) -> Result<Writer, StoreError> {
    let id: String = row.try_get("resource_id").map_err(database)?;
    let (binding, path) = record_codec::decode_writer_path(
        sealer,
        scope,
        &id,
        &row.try_get::<Vec<u8>, _>("request").map_err(database)?,
    )
    .map_err(invalid)?;
    let close_effect = row.try_get("close_effect").map_err(database)?;
    let state = row.try_get("state").map_err(database)?;
    Ok(Writer {
        binding,
        id,
        path,
        close_effect,
        state,
    })
}
pub(super) async fn get_writer(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    scope: &str,
    id: &str,
) -> Result<Option<Writer>, StoreError> {
    sqlx::query("SELECT resource_id,request,close_effect,state FROM durable_filesystem_writers WHERE scope_id=? AND resource_id=?")
        .bind(scope).bind(id).fetch_optional(connection).await.map_err(database)?
        .map(|row| decode_writer(row, sealer, scope)).transpose()
}
pub(super) async fn save_writer(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    scope: &str,
    value: Writer,
) -> Result<(), StoreError> {
    let prior = get_writer(connection, sealer, scope, &value.id).await?;
    value.validate_successor(prior.as_ref()).map_err(invalid)?;
    let id = &value.id;
    sqlx::query("INSERT INTO durable_filesystem_writers(scope_id,resource_id,request,close_effect,state) VALUES(?,?,?,?,?) ON CONFLICT(scope_id,resource_id) DO UPDATE SET request=excluded.request,close_effect=excluded.close_effect,state=excluded.state")
        .bind(scope).bind(id).bind(record_codec::encode_writer(sealer, scope, &value).map_err(invalid)?).bind(&value.close_effect).bind(&value.state).execute(connection).await.map_err(database)?;
    Ok(())
}

pub(super) async fn closing_writer(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    scope: &str,
) -> Result<Option<Writer>, StoreError> {
    sqlx::query("SELECT * FROM durable_filesystem_writers WHERE scope_id=? AND state='closing' ORDER BY resource_id LIMIT 1")
        .bind(scope).fetch_optional(connection).await.map_err(database)?.map(|row| decode_writer(row, sealer, scope)).transpose()
}
pub(super) async fn writer_for_effect(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    scope: &str,
    effect: &str,
) -> Result<Option<Writer>, StoreError> {
    sqlx::query(
        "SELECT * FROM durable_filesystem_writers WHERE scope_id=? AND close_effect=? LIMIT 1",
    )
    .bind(scope)
    .bind(effect)
    .fetch_optional(connection)
    .await
    .map_err(database)?
    .map(|row| decode_writer(row, sealer, scope))
    .transpose()
}
pub(super) async fn pending_resource(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    scope: &str,
) -> Result<Option<PendingResource>, StoreError> {
    if let Some(row) = sqlx::query("SELECT * FROM durable_filesystem_readers WHERE scope_id=? AND position IS NULL AND error IS NULL ORDER BY resource_id LIMIT 1").bind(scope).fetch_optional(&mut *connection).await.map_err(database)? {
        let reader = decode_reader(row, sealer, scope)?;
        return Ok(Some(PendingResource::Reader { id: reader.id, path: reader.path }));
    }
    let row = sqlx::query("SELECT * FROM durable_filesystem_listings WHERE scope_id=? AND content IS NULL AND error IS NULL ORDER BY resource_id LIMIT 1").bind(scope).fetch_optional(connection).await.map_err(database)?;
    row.map(|row| {
        let listing = decode_listing(row, sealer, scope)?;
        Ok(PendingResource::Listing {
            id: listing.id,
            path: listing.path,
            recursive: listing.recursive,
        })
    })
    .transpose()
}
pub(super) async fn resource_counts(
    connection: &mut SqliteConnection,
    _sealer: &Sealer,
    scope: &str,
) -> Result<ResourceCounts, StoreError> {
    let readers = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM durable_filesystem_readers WHERE scope_id=?",
    )
    .bind(scope)
    .fetch_one(&mut *connection)
    .await
    .map_err(database)?
    .try_into()
    .map_err(invalid)?;
    let listings = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM durable_filesystem_listings WHERE scope_id=?",
    )
    .bind(scope)
    .fetch_one(&mut *connection)
    .await
    .map_err(database)?
    .try_into()
    .map_err(invalid)?;
    let writers = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM durable_filesystem_writers WHERE scope_id=?",
    )
    .bind(scope)
    .fetch_one(&mut *connection)
    .await
    .map_err(database)?
    .try_into()
    .map_err(invalid)?;
    let chunks = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM durable_filesystem_chunks WHERE scope_id=?",
    )
    .bind(scope)
    .fetch_one(&mut *connection)
    .await
    .map_err(database)?
    .try_into()
    .map_err(invalid)?;
    Ok(ResourceCounts {
        readers,
        listings,
        writers,
        chunks,
    })
}

fn decode_operation(row: SqliteRow, sealer: &Sealer) -> Result<Operation, StoreError> {
    let effect = Operation {
        id: row.try_get("operation_id").map_err(database)?,
        scope: row.try_get("scope_id").map_err(database)?,
        operation: row.try_get("operation").map_err(database)?,
        version: row
            .try_get::<i64, _>("version")
            .map_err(database)?
            .try_into()
            .map_err(invalid)?,
        request: vec![],
        recovery_payload: None,
        outcome: None,
        rejected: row.try_get::<&str, _>("state").map_err(database)? == "rejected",
    };
    record_codec::decode_operation(
        sealer,
        effect,
        OperationColumns {
            request: row.try_get("request").map_err(database)?,
            recovery: row.try_get("recovery_payload").map_err(database)?,
            outcome: row.try_get("outcome").map_err(database)?,
        },
    )
    .map_err(invalid)
}
pub(super) async fn get_operation(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    scope: &str,
    id: &str,
) -> Result<Option<Operation>, StoreError> {
    sqlx::query("SELECT * FROM durable_filesystem_operations WHERE scope_id=? AND operation_id=?")
        .bind(scope)
        .bind(id)
        .fetch_optional(connection)
        .await
        .map_err(database)?
        .map(|row| decode_operation(row, sealer))
        .transpose()
}
pub(super) async fn save_operation(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    effect: Operation,
) -> Result<(), StoreError> {
    let prior = get_operation(connection, sealer, &effect.scope, &effect.id).await?;
    effect.validate_successor(prior.as_ref()).map_err(invalid)?;
    let columns = record_codec::encode_operation(sealer, &effect).map_err(invalid)?;
    let previous = prior.as_ref().map(Operation::state);
    let result = sqlx::query("INSERT INTO durable_filesystem_operations(scope_id,operation_id,operation,version,request,recovery_payload,outcome,state) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(scope_id,operation_id) DO UPDATE SET recovery_payload=excluded.recovery_payload,outcome=excluded.outcome,state=excluded.state WHERE durable_filesystem_operations.state=?")
        .bind(&effect.scope).bind(&effect.id).bind(&effect.operation).bind(effect.version).bind(columns.request).bind(columns.recovery).bind(columns.outcome).bind(effect.state()).bind(previous).execute(connection).await.map_err(database)?;
    if result.rows_affected() != 1 {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
pub(super) async fn pending_operation(
    connection: &mut SqliteConnection,
    _sealer: &Sealer,
    scope: &str,
) -> Result<Option<String>, StoreError> {
    sqlx::query_scalar(
        "SELECT operation_id FROM durable_filesystem_operations WHERE scope_id=? AND state IN ('preparing','pending')",
    )
    .bind(scope)
    .fetch_optional(connection)
    .await
    .map_err(database)
}
pub(super) async fn operation_count(
    connection: &mut SqliteConnection,
    _sealer: &Sealer,
    scope: &str,
) -> Result<u64, StoreError> {
    sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM durable_filesystem_operations WHERE scope_id=?",
    )
    .bind(scope)
    .fetch_one(connection)
    .await
    .map_err(database)?
    .try_into()
    .map_err(invalid)
}

fn decode_chunk(row: SqliteRow, sealer: &Sealer, scope: &str) -> Result<Chunk, StoreError> {
    let writer: String = row.try_get("resource_id").map_err(database)?;
    let id: String = row.try_get("chunk_id").map_err(database)?;
    let content = record_codec::decode_chunk_content(
        sealer,
        scope,
        &writer,
        &id,
        &row.try_get::<Vec<u8>, _>("content").map_err(database)?,
    )
    .map_err(invalid)?;
    Ok(Chunk {
        writer,
        id,
        content,
        sequence: row
            .try_get::<i64, _>("sequence")
            .map_err(database)?
            .try_into()
            .map_err(invalid)?,
    })
}
pub(super) async fn get_chunk(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    scope: &str,
    writer: &str,
    id: &str,
) -> Result<Option<Chunk>, StoreError> {
    sqlx::query(
        "SELECT * FROM durable_filesystem_chunks WHERE scope_id=? AND resource_id=? AND chunk_id=?",
    )
    .bind(scope)
    .bind(writer)
    .bind(id)
    .fetch_optional(connection)
    .await
    .map_err(database)?
    .map(|row| decode_chunk(row, sealer, scope))
    .transpose()
}
pub(super) async fn save_chunk(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    scope: &str,
    chunk: Chunk,
) -> Result<(), StoreError> {
    let prior = get_chunk(connection, sealer, scope, &chunk.writer, &chunk.id).await?;
    chunk.validate_successor(prior.as_ref()).map_err(invalid)?;
    let content = record_codec::encode_chunk(sealer, scope, &chunk).map_err(invalid)?;
    sqlx::query("INSERT INTO durable_filesystem_chunks(scope_id,resource_id,chunk_id,sequence,content) VALUES(?,?,?,?,?)").bind(scope).bind(chunk.writer).bind(chunk.id).bind(chunk.sequence).bind(content).execute(connection).await.map_err(database)?;
    Ok(())
}
pub(super) async fn writer_chunks(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    scope: &str,
    writer: &str,
    limit: u32,
) -> Result<Vec<Chunk>, StoreError> {
    sqlx::query("SELECT * FROM durable_filesystem_chunks WHERE scope_id=? AND resource_id=? ORDER BY sequence LIMIT ?").bind(scope).bind(writer).bind(limit.min(10001)).fetch_all(connection).await.map_err(database)?.into_iter().map(|row| decode_chunk(row, sealer, scope)).collect()
}
