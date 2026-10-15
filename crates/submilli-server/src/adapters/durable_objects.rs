//! SQLite mapping. All writes use the consumer's application unit of work.
mod codec;
pub(crate) mod files;

use crate::adapters::{sealing::Sealer, sql::database};
use crate::application::error::StoreError;
use crate::domain::durable_object::{
    CollectionState, ContentIdentity, DurableObject, ObjectId, ObjectStoreIdentity,
    PublicationState, Retention,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqliteConnection, sqlite::SqliteRow};
use std::collections::BTreeSet;

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
    let owner = format!("durable-object:{}", object.id().as_str());
    let content = object
        .content()
        .map(|content| {
            let bytes = serde_json::to_vec(&StoredContent {
                bytes: content.bytes,
                sha256: content.sha256,
            })
            .map_err(invalid)?;
            sealer.seal(&format!("{owner}:content"), &bytes)
        })
        .transpose()?;
    let retention = serde_json::to_vec(
        &object
            .retention()
            .iter()
            .map(StoredRetention::from)
            .collect::<Vec<_>>(),
    )
    .map_err(invalid)?;
    let retention = sealer.seal(&format!("{owner}:retention"), &retention)?;
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
    let owner = format!("durable-object:{}", id.as_str());
    let content: Option<Vec<u8>> = row.try_get("content").map_err(database)?;
    let content = content
        .map(|blob| {
            let bytes =
                sealer.open(&format!("{owner}:content"), &blob, "durable object content")?;
            let value: StoredContent = serde_json::from_slice(&bytes).map_err(invalid)?;
            Ok::<_, StoreError>(ContentIdentity {
                bytes: value.bytes,
                sha256: value.sha256,
            })
        })
        .transpose()?;
    let bytes: Vec<u8> = row.try_get("retention").map_err(database)?;
    let bytes = sealer.open(
        &format!("{owner}:retention"),
        &bytes,
        "durable object retention",
    )?;
    let retention: Vec<StoredRetention> = serde_json::from_slice(&bytes).map_err(invalid)?;
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
        retention
            .into_iter()
            .map(Retention::from)
            .collect::<BTreeSet<_>>(),
        row.try_get("revision").map_err(database)?,
    )
    .map_err(invalid)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredContent {
    bytes: u64,
    sha256: [u8; 32],
}

#[derive(Serialize, Deserialize)]
enum StoredRetention {
    Run(String),
    Response(String),
    Workspace(String),
    Backup(String),
}

impl From<&Retention> for StoredRetention {
    fn from(value: &Retention) -> Self {
        match value {
            Retention::Run(id) => Self::Run(id.clone()),
            Retention::Response(id) => Self::Response(id.clone()),
            Retention::Workspace(id) => Self::Workspace(id.clone()),
            Retention::Backup(id) => Self::Backup(id.clone()),
        }
    }
}
impl From<StoredRetention> for Retention {
    fn from(value: StoredRetention) -> Self {
        match value {
            StoredRetention::Run(id) => Self::Run(id),
            StoredRetention::Response(id) => Self::Response(id),
            StoredRetention::Workspace(id) => Self::Workspace(id),
            StoredRetention::Backup(id) => Self::Backup(id),
        }
    }
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
