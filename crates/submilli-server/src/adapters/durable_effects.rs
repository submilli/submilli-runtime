//! Shared, encrypted effect records, independent of any operation adapter.
//!
//! Call mutations inside the caller's database transaction, together with its
//! admission checks and object retention. This bounded storage primitive does
//! not yet implement execution-history ordering, attempt fencing or accounting.
use super::sealing::Sealer;
use crate::database::DatabaseError;
use sqlx::{Row, SqliteConnection};

pub(crate) const MAX_EFFECTS: i64 = 10_000;
pub(crate) const MAX_RECORD: usize = 64 * 1024;

pub(crate) struct EffectKey<'a> {
    pub run: &'a str,
    pub effect: &'a str,
    pub operation: &'a str,
    pub version: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct EffectRecord {
    pub request: Vec<u8>,
    pub recovery_payload: Option<Vec<u8>>,
    pub outcome: Option<Vec<u8>>,
    pub rejected: bool,
}

pub(crate) async fn load(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    key: &EffectKey<'_>,
) -> Result<Option<EffectRecord>, DatabaseError> {
    let row = sqlx::query("SELECT operation,version,request,recovery_payload,outcome,state FROM durable_effects WHERE run_id=? AND effect_id=?")
        .bind(key.run).bind(key.effect).fetch_optional(connection).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    if row.try_get::<String, _>("operation")? != key.operation
        || row.try_get::<i64, _>("version")? != i64::from(key.version)
    {
        return Err(DatabaseError::SessionConflict);
    }
    let request = open(
        sealer,
        key,
        "request",
        &row.try_get::<Vec<u8>, _>("request")?,
    )?;
    let recovery_payload = row
        .try_get::<Option<Vec<u8>>, _>("recovery_payload")?
        .map(|bytes| open(sealer, key, "recovery", &bytes))
        .transpose()?;
    let outcome = row
        .try_get::<Option<Vec<u8>>, _>("outcome")?
        .map(|bytes| open(sealer, key, "outcome", &bytes))
        .transpose()?;
    Ok(Some(EffectRecord {
        request,
        recovery_payload,
        outcome,
        rejected: row.try_get::<&str, _>("state")? == "rejected",
    }))
}

pub(crate) async fn admit(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    key: &EffectKey<'_>,
    request: &[u8],
) -> Result<EffectRecord, DatabaseError> {
    if key.run.is_empty() || key.effect.is_empty() || key.operation.is_empty() || key.version == 0 {
        return Err(invalid("invalid effect identity"));
    }
    if let Some(record) = load(connection, sealer, key).await? {
        return if record.request == request {
            Ok(record)
        } else {
            Err(DatabaseError::SessionConflict)
        };
    }
    require_capacity(connection, key.run, 1).await?;
    if pending(connection, key.run).await?.is_some() {
        return Err(DatabaseError::SessionConflict);
    }
    let sealed = seal(sealer, key, "request", request)?;
    sqlx::query("INSERT INTO durable_effects(run_id,effect_id,operation,version,request,state) VALUES(?,?,?,?,?,'preparing')")
        .bind(key.run).bind(key.effect).bind(key.operation).bind(i64::from(key.version)).bind(sealed).execute(connection).await?;
    Ok(EffectRecord {
        request: request.to_vec(),
        recovery_payload: None,
        outcome: None,
        rejected: false,
    })
}

pub(crate) async fn prepare(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    key: &EffectKey<'_>,
    payload: &[u8],
) -> Result<(), DatabaseError> {
    let record = load(connection, sealer, key)
        .await?
        .ok_or(DatabaseError::SessionConflict)?;
    if record.outcome.is_some() {
        return Err(DatabaseError::SessionConflict);
    }
    if let Some(prior) = record.recovery_payload {
        return if prior == payload {
            Ok(())
        } else {
            Err(DatabaseError::SessionConflict)
        };
    }
    let sealed = seal(sealer, key, "recovery", payload)?;
    sqlx::query("UPDATE durable_effects SET recovery_payload=?,state='pending' WHERE run_id=? AND effect_id=? AND state='preparing'")
        .bind(sealed).bind(key.run).bind(key.effect).execute(connection).await?;
    Ok(())
}

pub(crate) async fn complete(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    key: &EffectKey<'_>,
    outcome: &[u8],
    rejected: bool,
) -> Result<(), DatabaseError> {
    let record = load(connection, sealer, key)
        .await?
        .ok_or(DatabaseError::SessionConflict)?;
    if let Some(prior) = record.outcome {
        return if prior == outcome && record.rejected == rejected {
            Ok(())
        } else {
            Err(DatabaseError::SessionConflict)
        };
    }
    // A dispatched operation cannot turn an uncertain effect into a definitive
    // rejection. Refusals are only recorded before preparation/public mutation.
    if rejected == record.recovery_payload.is_some() {
        return Err(DatabaseError::SessionConflict);
    }
    let (state, previous) = if rejected {
        ("rejected", "preparing")
    } else {
        ("done", "pending")
    };
    let sealed = seal(sealer, key, "outcome", outcome)?;
    sqlx::query(
        "UPDATE durable_effects SET outcome=?,state=? WHERE run_id=? AND effect_id=? AND state=?",
    )
    .bind(sealed)
    .bind(state)
    .bind(key.run)
    .bind(key.effect)
    .bind(previous)
    .execute(connection)
    .await?;
    Ok(())
}

pub(crate) async fn pending(
    connection: &mut SqliteConnection,
    run: &str,
) -> Result<Option<String>, DatabaseError> {
    Ok(sqlx::query_scalar(
        "SELECT effect_id FROM durable_effects WHERE run_id=? AND state IN ('preparing','pending')",
    )
    .bind(run)
    .fetch_optional(connection)
    .await?)
}

pub(crate) async fn require_capacity(
    connection: &mut SqliteConnection,
    run: &str,
    additional: usize,
) -> Result<(), DatabaseError> {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM durable_effects WHERE run_id=?")
        .bind(run)
        .fetch_one(connection)
        .await?;
    let additional = i64::try_from(additional).map_err(invalid)?;
    if additional > MAX_EFFECTS.saturating_sub(count) {
        return Err(DatabaseError::SessionConflict);
    }
    Ok(())
}

pub(crate) async fn exists(
    connection: &mut SqliteConnection,
    run: &str,
    effect: &str,
) -> Result<bool, DatabaseError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM durable_effects WHERE run_id=? AND effect_id=?)",
    )
    .bind(run)
    .bind(effect)
    .fetch_one(connection)
    .await?)
}

fn owner(key: &EffectKey<'_>, field: &str) -> Result<String, DatabaseError> {
    // A structured identity prevents collisions when IDs contain separators.
    serde_json::to_string(&(
        "durable-effect",
        key.run,
        key.effect,
        key.operation,
        key.version,
        field,
    ))
    .map_err(invalid)
}

fn seal(
    sealer: &Sealer,
    key: &EffectKey<'_>,
    field: &str,
    bytes: &[u8],
) -> Result<Vec<u8>, DatabaseError> {
    if bytes.len() > MAX_RECORD {
        return Err(invalid("effect record exceeds limit"));
    }
    sealer.seal(&owner(key, field)?, bytes).map_err(invalid)
}

fn open(
    sealer: &Sealer,
    key: &EffectKey<'_>,
    field: &str,
    bytes: &[u8],
) -> Result<Vec<u8>, DatabaseError> {
    if bytes.len() > MAX_RECORD + 128 {
        return Err(invalid("effect record exceeds limit"));
    }
    let plain = sealer
        .open(&owner(key, field)?, bytes, "durable effect record")
        .map_err(invalid)?;
    if plain.len() > MAX_RECORD {
        return Err(invalid("effect record exceeds limit"));
    }
    Ok(plain)
}

fn invalid(error: impl std::fmt::Display) -> DatabaseError {
    DatabaseError::Import(error.to_string())
}

#[cfg(test)]
mod tests;
