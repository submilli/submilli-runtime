//! SQLite mapping. All writes use the consumer's application unit of work.

use crate::adapters::sql::database;
use crate::application::error::StoreError;
use sqlx::{Row, SqliteConnection, sqlite::SqliteRow};
use submilli_durable::object::{
    CollectionState, DurableObject, ObjectId, ObjectStoreIdentity, PublicationState,
};
use submilli_durable::sealing::Sealer;

pub(crate) async fn store_identity(
    connection: &mut SqliteConnection,
) -> Result<Option<ObjectStoreIdentity>, StoreError> {
    let value: Option<String> =
        sqlx::query_scalar("SELECT identity FROM durable_object_store WHERE singleton=1")
            .fetch_optional(connection)
            .await
            .map_err(database)?;
    value
        .map(ObjectStoreIdentity::parse)
        .transpose()
        .map_err(invalid)
}

pub(crate) async fn identify_store(
    connection: &mut SqliteConnection,
    identity: ObjectStoreIdentity,
) -> Result<(), StoreError> {
    if let Some(existing) = store_identity(connection).await? {
        if existing != identity {
            return Err(StoreError::Conflict);
        }
        return Ok(());
    }
    sqlx::query("INSERT INTO durable_object_store(singleton, identity) VALUES(1,?)")
        .bind(identity.as_str())
        .execute(connection)
        .await
        .map_err(database)?;
    Ok(())
}

pub(crate) async fn get(
    connection: &mut SqliteConnection,
    id: &ObjectId,
    sealer: &Sealer,
) -> Result<Option<DurableObject>, StoreError> {
    sqlx::query("SELECT * FROM durable_objects WHERE object_id=?")
        .bind(id.as_str())
        .fetch_optional(connection)
        .await
        .map_err(database)?
        .map(|row| decode(row, sealer))
        .transpose()
}

pub(crate) async fn list(
    connection: &mut SqliteConnection,
    after: Option<ObjectId>,
    limit: u32,
    sealer: &Sealer,
) -> Result<Vec<DurableObject>, StoreError> {
    sqlx::query("SELECT * FROM durable_objects WHERE object_id>? AND collection!='gone' ORDER BY object_id LIMIT ?")
        .bind(after.as_ref().map_or("", ObjectId::as_str)).bind(i64::from(limit))
        .fetch_all(connection).await.map_err(database)?
        .into_iter().map(|row| decode(row, sealer)).collect()
}

pub(crate) async fn usage(
    connection: &mut SqliteConnection,
    owner: &str,
) -> Result<(u64, u64), StoreError> {
    let row = sqlx::query("SELECT COALESCE(SUM(reserved_bytes),0) AS total, COALESCE(SUM(CASE WHEN owner_id=? THEN reserved_bytes ELSE 0 END),0) AS owned FROM durable_objects WHERE collection!='gone'")
        .bind(owner).fetch_one(connection).await.map_err(database)?;
    Ok((
        unsigned(row.try_get("total").map_err(database)?)?,
        unsigned(row.try_get("owned").map_err(database)?)?,
    ))
}

pub(crate) async fn save(
    connection: &mut SqliteConnection,
    object: DurableObject,
    sealer: &Sealer,
) -> Result<(), StoreError> {
    let stored = get(connection, object.id(), sealer).await?;
    if stored.as_ref() == Some(&object) {
        return Ok(());
    }
    if let Some(stored) = &stored {
        stored.matches_request(&object).map_err(invalid)?;
        if stored.revision().checked_add(1) != Some(object.revision()) {
            return Err(StoreError::Conflict);
        }
    } else if object.revision() != 0 {
        return Err(StoreError::Conflict);
    }
    let (content, retention) =
        submilli_durable::object_codec::encode(&object, sealer).map_err(invalid)?;
    let publication = match object.publication() {
        PublicationState::Pending => "pending",
        PublicationState::Bound => "bound",
        PublicationState::Abandoned => "abandoned",
        PublicationState::Retired => "retired",
    };
    let collection = match object.collection() {
        CollectionState::Available => "available",
        CollectionState::Deleting => "deleting",
        CollectionState::Gone => "gone",
    };
    let result = sqlx::query("INSERT INTO durable_objects(object_id,owner_id,request_identity,maximum_bytes,reserved_bytes,publication,collection,content,retention,revision) VALUES (?,?,?,?,?,?,?,?,?,?) ON CONFLICT(object_id) DO UPDATE SET reserved_bytes=excluded.reserved_bytes,publication=excluded.publication,collection=excluded.collection,content=excluded.content,retention=excluded.retention,revision=excluded.revision WHERE durable_objects.revision=excluded.revision-1")
        .bind(object.id().as_str()).bind(object.owner()).bind(object.request().as_slice())
        .bind(signed(object.maximum_bytes())?).bind(signed(object.reserved_bytes())?)
        .bind(publication).bind(collection).bind(content).bind(retention).bind(object.revision())
        .execute(connection).await.map_err(database)?;
    if result.rows_affected() != 1 {
        return Err(StoreError::Conflict);
    }
    Ok(())
}

fn decode(row: SqliteRow, sealer: &Sealer) -> Result<DurableObject, StoreError> {
    let id = ObjectId::parse(row.try_get("object_id").map_err(database)?).map_err(invalid)?;
    let content: Option<Vec<u8>> = row.try_get("content").map_err(database)?;
    let retention: Vec<u8> = row.try_get("retention").map_err(database)?;
    let (content, retention) =
        submilli_durable::object_codec::decode(&id, content.as_deref(), &retention, sealer)
            .map_err(invalid)?;
    let request: Vec<u8> = row.try_get("request_identity").map_err(database)?;
    let publication = match row.try_get::<&str, _>("publication").map_err(database)? {
        "pending" => PublicationState::Pending,
        "bound" => PublicationState::Bound,
        "abandoned" => PublicationState::Abandoned,
        "retired" => PublicationState::Retired,
        _ => return Err(invalid("unknown publication state")),
    };
    let collection = match row.try_get::<&str, _>("collection").map_err(database)? {
        "available" => CollectionState::Available,
        "deleting" => CollectionState::Deleting,
        "gone" => CollectionState::Gone,
        _ => return Err(invalid("unknown collection state")),
    };
    DurableObject::restore(
        id,
        row.try_get("owner_id").map_err(database)?,
        request
            .try_into()
            .map_err(|_| invalid("invalid request identity"))?,
        unsigned(row.try_get("maximum_bytes").map_err(database)?)?,
        unsigned(row.try_get("reserved_bytes").map_err(database)?)?,
        content,
        publication,
        collection,
        retention,
        row.try_get("revision").map_err(database)?,
    )
    .map_err(invalid)
}

fn signed(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(invalid)
}
fn unsigned(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(invalid)
}
fn invalid(error: impl std::fmt::Display) -> StoreError {
    StoreError::Io(format!("invalid durable object: {error}"))
}
