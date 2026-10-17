use crate::adapters::sql::database;
use crate::application::error::StoreError;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};
use submilli_durable::{
    record_codec::{self, EffectColumns, ResourceColumns, WorkspaceColumns},
    sealing::Sealer,
    state::*,
};
fn invalid(error: impl std::fmt::Display) -> StoreError {
    StoreError::Io(error.to_string())
}
fn decode_reader(row: SqliteRow, sealer: &Sealer, workspace: &str) -> Result<Reader, StoreError> {
    record_codec::decode_reader(
        sealer,
        workspace,
        row.try_get("resource_id").map_err(database)?,
        ResourceColumns {
            request: row.try_get("request").map_err(database)?,
            result: row.try_get("position").map_err(database)?,
            error: row.try_get("error").map_err(database)?,
        },
    )
    .map_err(invalid)
}
pub(super) async fn get_reader(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    id: &str,
) -> Result<Option<Reader>, StoreError> {
    sqlx::query("SELECT resource_id,request,position,error FROM durable_filesystem_readers WHERE workspace_id=? AND resource_id=?")
        .bind(workspace).bind(id).fetch_optional(connection).await.map_err(database)?
        .map(|row| decode_reader(row, sealer, workspace)).transpose()
}
pub(super) async fn save_reader(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    value: Reader,
) -> Result<(), StoreError> {
    let prior = get_reader(connection, sealer, workspace, &value.id).await?;
    value.validate_successor(prior.as_ref()).map_err(invalid)?;
    let id = &value.id;
    let columns = record_codec::encode_reader(sealer, workspace, &value).map_err(invalid)?;
    sqlx::query("INSERT INTO durable_filesystem_readers(workspace_id,resource_id,request,position,error) VALUES(?,?,?,?,?) ON CONFLICT(workspace_id,resource_id) DO UPDATE SET request=excluded.request,position=excluded.position,error=excluded.error")
        .bind(workspace).bind(id).bind(columns.request).bind(columns.result).bind(columns.error).execute(connection).await.map_err(database)?;
    Ok(())
}
fn decode_listing(row: SqliteRow, sealer: &Sealer, workspace: &str) -> Result<Listing, StoreError> {
    record_codec::decode_listing(
        sealer,
        workspace,
        row.try_get("resource_id").map_err(database)?,
        ResourceColumns {
            request: row.try_get("request").map_err(database)?,
            result: row.try_get("content").map_err(database)?,
            error: row.try_get("error").map_err(database)?,
        },
    )
    .map_err(invalid)
}
pub(super) async fn get_listing(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    id: &str,
) -> Result<Option<Listing>, StoreError> {
    sqlx::query("SELECT resource_id,request,content,error FROM durable_filesystem_listings WHERE workspace_id=? AND resource_id=?")
        .bind(workspace).bind(id).fetch_optional(connection).await.map_err(database)?
        .map(|row| decode_listing(row, sealer, workspace)).transpose()
}
pub(super) async fn save_listing(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    value: Listing,
) -> Result<(), StoreError> {
    let prior = get_listing(connection, sealer, workspace, &value.id).await?;
    value.validate_successor(prior.as_ref()).map_err(invalid)?;
    let id = &value.id;
    let columns = record_codec::encode_listing(sealer, workspace, &value).map_err(invalid)?;
    sqlx::query("INSERT INTO durable_filesystem_listings(workspace_id,resource_id,request,content,error) VALUES(?,?,?,?,?) ON CONFLICT(workspace_id,resource_id) DO UPDATE SET request=excluded.request,content=excluded.content,error=excluded.error")
        .bind(workspace).bind(id).bind(columns.request).bind(columns.result).bind(columns.error).execute(connection).await.map_err(database)?;
    Ok(())
}
fn decode_writer(row: SqliteRow, sealer: &Sealer, workspace: &str) -> Result<Writer, StoreError> {
    let id: String = row.try_get("resource_id").map_err(database)?;
    let path = record_codec::decode_writer_path(
        sealer,
        workspace,
        &id,
        &row.try_get::<Vec<u8>, _>("request").map_err(database)?,
    )
    .map_err(invalid)?;
    let close_effect = row.try_get("close_effect").map_err(database)?;
    let state = row.try_get("state").map_err(database)?;
    Ok(Writer {
        id,
        path,
        close_effect,
        state,
    })
}
pub(super) async fn get_writer(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    id: &str,
) -> Result<Option<Writer>, StoreError> {
    sqlx::query("SELECT resource_id,request,close_effect,state FROM durable_filesystem_writers WHERE workspace_id=? AND resource_id=?")
        .bind(workspace).bind(id).fetch_optional(connection).await.map_err(database)?
        .map(|row| decode_writer(row, sealer, workspace)).transpose()
}
pub(super) async fn save_writer(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    value: Writer,
) -> Result<(), StoreError> {
    let prior = get_writer(connection, sealer, workspace, &value.id).await?;
    value.validate_successor(prior.as_ref()).map_err(invalid)?;
    let id = &value.id;
    sqlx::query("INSERT INTO durable_filesystem_writers(workspace_id,resource_id,request,close_effect,state) VALUES(?,?,?,?,?) ON CONFLICT(workspace_id,resource_id) DO UPDATE SET request=excluded.request,close_effect=excluded.close_effect,state=excluded.state")
        .bind(workspace).bind(id).bind(record_codec::encode_writer(sealer, workspace, &value).map_err(invalid)?).bind(&value.close_effect).bind(&value.state).execute(connection).await.map_err(database)?;
    Ok(())
}
fn decode_plan(row: SqliteRow, sealer: &Sealer, workspace: &str) -> Result<Plan, StoreError> {
    record_codec::decode_plan(
        sealer,
        workspace,
        row.try_get("plan_id").map_err(database)?,
        ResourceColumns {
            request: row.try_get("request").map_err(database)?,
            result: row.try_get("outcome").map_err(database)?,
            error: None,
        },
    )
    .map_err(invalid)
}
pub(super) async fn get_plan(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    id: &str,
) -> Result<Option<Plan>, StoreError> {
    sqlx::query("SELECT plan_id,request,outcome FROM durable_filesystem_plans WHERE workspace_id=? AND plan_id=?")
        .bind(workspace).bind(id).fetch_optional(connection).await.map_err(database)?
        .map(|row| decode_plan(row, sealer, workspace)).transpose()
}
pub(super) async fn save_plan(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    value: Plan,
) -> Result<(), StoreError> {
    let prior = get_plan(connection, sealer, workspace, &value.id).await?;
    value.validate_successor(prior.as_ref()).map_err(invalid)?;
    let id = &value.id;
    let columns = record_codec::encode_plan(sealer, workspace, &value).map_err(invalid)?;
    sqlx::query("INSERT INTO durable_filesystem_plans(workspace_id,plan_id,request,outcome) VALUES(?,?,?,?) ON CONFLICT(workspace_id,plan_id) DO UPDATE SET request=excluded.request,outcome=excluded.outcome")
        .bind(workspace).bind(id).bind(columns.request).bind(columns.result).execute(connection).await.map_err(database)?;
    Ok(())
}
pub(super) async fn plans_after(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    after: &str,
    limit: u32,
) -> Result<Vec<Plan>, StoreError> {
    sqlx::query("SELECT plan_id,request,outcome FROM durable_filesystem_plans WHERE workspace_id=? AND plan_id>? ORDER BY plan_id LIMIT ?")
        .bind(workspace).bind(after).bind(limit.min(10001)).fetch_all(connection).await.map_err(database)?
        .into_iter().map(|row| decode_plan(row, sealer, workspace)).collect()
}

pub(super) async fn pending_plan(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
) -> Result<Option<Plan>, StoreError> {
    sqlx::query("SELECT * FROM durable_filesystem_plans WHERE workspace_id=? AND outcome IS NULL ORDER BY plan_id LIMIT 1")
        .bind(workspace).fetch_optional(connection).await.map_err(database)?.map(|row| decode_plan(row, sealer, workspace)).transpose()
}
pub(super) async fn closing_writer(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
) -> Result<Option<Writer>, StoreError> {
    sqlx::query("SELECT * FROM durable_filesystem_writers WHERE workspace_id=? AND state='closing' ORDER BY resource_id LIMIT 1")
        .bind(workspace).fetch_optional(connection).await.map_err(database)?.map(|row| decode_writer(row, sealer, workspace)).transpose()
}
pub(super) async fn writer_for_effect(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    effect: &str,
) -> Result<Option<Writer>, StoreError> {
    sqlx::query(
        "SELECT * FROM durable_filesystem_writers WHERE workspace_id=? AND close_effect=? LIMIT 1",
    )
    .bind(workspace)
    .bind(effect)
    .fetch_optional(connection)
    .await
    .map_err(database)?
    .map(|row| decode_writer(row, sealer, workspace))
    .transpose()
}
pub(super) async fn pending_resource(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
) -> Result<Option<PendingResource>, StoreError> {
    if let Some(row) = sqlx::query("SELECT * FROM durable_filesystem_readers WHERE workspace_id=? AND position IS NULL AND error IS NULL ORDER BY resource_id LIMIT 1").bind(workspace).fetch_optional(&mut *connection).await.map_err(database)? {
        let reader = decode_reader(row, sealer, workspace)?;
        return Ok(Some(PendingResource::Reader { id: reader.id, path: reader.path }));
    }
    let row = sqlx::query("SELECT * FROM durable_filesystem_listings WHERE workspace_id=? AND content IS NULL AND error IS NULL ORDER BY resource_id LIMIT 1").bind(workspace).fetch_optional(connection).await.map_err(database)?;
    row.map(|row| {
        let listing = decode_listing(row, sealer, workspace)?;
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
    workspace: &str,
) -> Result<ResourceCounts, StoreError> {
    let readers = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM durable_filesystem_readers WHERE workspace_id=?",
    )
    .bind(workspace)
    .fetch_one(&mut *connection)
    .await
    .map_err(database)?
    .try_into()
    .map_err(invalid)?;
    let listings = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM durable_filesystem_listings WHERE workspace_id=?",
    )
    .bind(workspace)
    .fetch_one(&mut *connection)
    .await
    .map_err(database)?
    .try_into()
    .map_err(invalid)?;
    let writers = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM durable_filesystem_writers WHERE workspace_id=?",
    )
    .bind(workspace)
    .fetch_one(&mut *connection)
    .await
    .map_err(database)?
    .try_into()
    .map_err(invalid)?;
    let plans = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM durable_filesystem_plans WHERE workspace_id=?",
    )
    .bind(workspace)
    .fetch_one(&mut *connection)
    .await
    .map_err(database)?
    .try_into()
    .map_err(invalid)?;
    let chunks = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM durable_filesystem_chunks WHERE workspace_id=?",
    )
    .bind(workspace)
    .fetch_one(&mut *connection)
    .await
    .map_err(database)?
    .try_into()
    .map_err(invalid)?;
    Ok(ResourceCounts {
        readers,
        listings,
        writers,
        plans,
        chunks,
    })
}

pub(super) async fn get_run(
    connection: &mut SqliteConnection,
    _sealer: &Sealer,
    run: &str,
) -> Result<Option<RunState>, StoreError> {
    sqlx::query("SELECT session_id,state FROM runs WHERE run_id=?")
        .bind(run)
        .fetch_optional(connection)
        .await
        .map_err(database)?
        .map(|row| {
            let state: String = row.try_get("state").map_err(database)?;
            Ok(RunState {
                session: row.try_get("session_id").map_err(database)?,
                live: matches!(state.as_str(), "reserved" | "running" | "blocked"),
                terminal: matches!(
                    state.as_str(),
                    "completed" | "finished" | "cancelled" | "aborted" | "expired"
                ),
            })
        })
        .transpose()
}
pub(super) async fn get_workspace(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    id: &str,
) -> Result<Option<WorkspaceState>, StoreError> {
    sqlx::query("SELECT * FROM durable_workspaces WHERE workspace_id=?")
        .bind(id)
        .fetch_optional(connection)
        .await
        .map_err(database)?
        .map(|row| {
            record_codec::decode_workspace(
                sealer,
                WorkspaceState {
                    id: id.into(),
                    run: row.try_get("run_id").map_err(database)?,
                    session: row.try_get("session_id").map_err(database)?,
                    owner: row.try_get("owner").map_err(database)?,
                    state: row.try_get("state").map_err(database)?,
                    identity: None,
                    layout: None,
                    reservation: None,
                },
                WorkspaceColumns {
                    identity: row.try_get("identity").map_err(database)?,
                    layout: row.try_get("layout").map_err(database)?,
                    reservation: row.try_get("reservation").map_err(database)?,
                },
            )
            .map_err(invalid)
        })
        .transpose()
}
pub(super) async fn save_workspace(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: WorkspaceState,
) -> Result<(), StoreError> {
    let prior = get_workspace(connection, sealer, &workspace.id).await?;
    workspace
        .validate_successor(prior.as_ref())
        .map_err(invalid)?;
    let id = &workspace.id;
    let columns = record_codec::encode_workspace(sealer, &workspace).map_err(invalid)?;
    let result = sqlx::query("INSERT INTO durable_workspaces(workspace_id,run_id,session_id,owner,identity,layout,reservation,state) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(workspace_id) DO UPDATE SET identity=excluded.identity,layout=excluded.layout,reservation=excluded.reservation,state=excluded.state WHERE run_id=excluded.run_id AND session_id=excluded.session_id AND owner=excluded.owner")
        .bind(id).bind(workspace.run).bind(workspace.session).bind(workspace.owner).bind(columns.identity).bind(columns.layout).bind(columns.reservation).bind(workspace.state).execute(connection).await.map_err(database)?;
    if result.rows_affected() != 1 {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
fn decode_effect(row: SqliteRow, sealer: &Sealer) -> Result<Effect, StoreError> {
    let effect = Effect {
        id: row.try_get("effect_id").map_err(database)?,
        run: row.try_get("run_id").map_err(database)?,
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
    record_codec::decode_effect(
        sealer,
        effect,
        EffectColumns {
            request: row.try_get("request").map_err(database)?,
            recovery: row.try_get("recovery_payload").map_err(database)?,
            outcome: row.try_get("outcome").map_err(database)?,
        },
    )
    .map_err(invalid)
}
pub(super) async fn get_effect(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    run: &str,
    id: &str,
) -> Result<Option<Effect>, StoreError> {
    sqlx::query("SELECT * FROM durable_effects WHERE run_id=? AND effect_id=?")
        .bind(run)
        .bind(id)
        .fetch_optional(connection)
        .await
        .map_err(database)?
        .map(|row| decode_effect(row, sealer))
        .transpose()
}
pub(super) async fn save_effect(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    effect: Effect,
) -> Result<(), StoreError> {
    let prior = get_effect(connection, sealer, &effect.run, &effect.id).await?;
    effect.validate_successor(prior.as_ref()).map_err(invalid)?;
    let columns = record_codec::encode_effect(sealer, &effect).map_err(invalid)?;
    let previous = prior.as_ref().map(Effect::state);
    let result = sqlx::query("INSERT INTO durable_effects(run_id,effect_id,operation,version,request,recovery_payload,outcome,state) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(run_id,effect_id) DO UPDATE SET recovery_payload=excluded.recovery_payload,outcome=excluded.outcome,state=excluded.state WHERE durable_effects.state=?")
        .bind(&effect.run).bind(&effect.id).bind(&effect.operation).bind(effect.version).bind(columns.request).bind(columns.recovery).bind(columns.outcome).bind(effect.state()).bind(previous).execute(connection).await.map_err(database)?;
    if result.rows_affected() != 1 {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
pub(super) async fn pending_effect(
    connection: &mut SqliteConnection,
    _sealer: &Sealer,
    run: &str,
) -> Result<Option<String>, StoreError> {
    sqlx::query_scalar(
        "SELECT effect_id FROM durable_effects WHERE run_id=? AND state IN ('preparing','pending')",
    )
    .bind(run)
    .fetch_optional(connection)
    .await
    .map_err(database)
}
pub(super) async fn effect_count(
    connection: &mut SqliteConnection,
    _sealer: &Sealer,
    run: &str,
) -> Result<u64, StoreError> {
    sqlx::query_scalar::<_, i64>("SELECT count(*) FROM durable_effects WHERE run_id=?")
        .bind(run)
        .fetch_one(connection)
        .await
        .map_err(database)?
        .try_into()
        .map_err(invalid)
}
pub(super) async fn effects_after(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    run: &str,
    operation: &str,
    after: &str,
    limit: u32,
) -> Result<Vec<Effect>, StoreError> {
    sqlx::query("SELECT * FROM durable_effects WHERE run_id=? AND operation=? AND effect_id>? ORDER BY effect_id LIMIT ?").bind(run).bind(operation).bind(after).bind(limit.min(10001)).fetch_all(connection).await.map_err(database)?.into_iter().map(|row| decode_effect(row, sealer)).collect()
}
fn decode_chunk(row: SqliteRow, sealer: &Sealer, workspace: &str) -> Result<Chunk, StoreError> {
    let writer: String = row.try_get("resource_id").map_err(database)?;
    let id: String = row.try_get("chunk_id").map_err(database)?;
    let content = record_codec::decode_chunk_content(
        sealer,
        workspace,
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
    workspace: &str,
    writer: &str,
    id: &str,
) -> Result<Option<Chunk>, StoreError> {
    sqlx::query("SELECT * FROM durable_filesystem_chunks WHERE workspace_id=? AND resource_id=? AND chunk_id=?").bind(workspace).bind(writer).bind(id).fetch_optional(connection).await.map_err(database)?.map(|row| decode_chunk(row, sealer, workspace)).transpose()
}
pub(super) async fn save_chunk(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    chunk: Chunk,
) -> Result<(), StoreError> {
    let prior = get_chunk(connection, sealer, workspace, &chunk.writer, &chunk.id).await?;
    chunk.validate_successor(prior.as_ref()).map_err(invalid)?;
    let content = record_codec::encode_chunk(sealer, workspace, &chunk).map_err(invalid)?;
    sqlx::query("INSERT INTO durable_filesystem_chunks(workspace_id,resource_id,chunk_id,sequence,content) VALUES(?,?,?,?,?)").bind(workspace).bind(chunk.writer).bind(chunk.id).bind(chunk.sequence).bind(content).execute(connection).await.map_err(database)?;
    Ok(())
}
pub(super) async fn writer_chunks(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    writer: &str,
    limit: u32,
) -> Result<Vec<Chunk>, StoreError> {
    sqlx::query("SELECT * FROM durable_filesystem_chunks WHERE workspace_id=? AND resource_id=? ORDER BY sequence LIMIT ?").bind(workspace).bind(writer).bind(limit.min(10001)).fetch_all(connection).await.map_err(database)?.into_iter().map(|row| decode_chunk(row, sealer, workspace)).collect()
}
pub(super) async fn chunks_after(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    after: (&str, &str),
    limit: u32,
) -> Result<Vec<Chunk>, StoreError> {
    sqlx::query("SELECT * FROM durable_filesystem_chunks WHERE workspace_id=? AND (resource_id,chunk_id)>(?,?) ORDER BY resource_id,chunk_id LIMIT ?").bind(workspace).bind(after.0).bind(after.1).bind(limit.min(10001)).fetch_all(connection).await.map_err(database)?.into_iter().map(|row| decode_chunk(row, sealer, workspace)).collect()
}
