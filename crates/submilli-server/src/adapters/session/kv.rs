//! Relational mapping for session key-value state. Transactions belong to the
//! caller.
//!
//! Keys and payloads are UTF-16 code units, stored big-endian: exact for lone
//! surrogates, and the byte order SQLite compares blobs in is then code-unit
//! order, which is the order `list` pages in. A payload is sealed before SQL
//! when the server has a key, bound to its session and key.
use sqlx::SqliteConnection;

use crate::adapters::sealing::{Sealer, hex};
use crate::adapters::sql::database;
use crate::application::error::StoreError;
use crate::application::unit_of_work::{SessionStateKeys, SessionStateSize, SessionStateUsage};

/// Whether the session is active. A closed or unknown session's state is gone,
/// and a write must not bring it back.
pub(crate) async fn is_active(
    connection: &mut SqliteConnection,
    session_id: &str,
) -> Result<bool, StoreError> {
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM sessions WHERE session_id=?")
            .bind(session_id)
            .fetch_optional(connection)
            .await
            .map_err(database)?;
    Ok(status.as_deref() == Some("active"))
}

pub(crate) async fn get(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    session_id: &str,
    key: &[u16],
) -> Result<Option<Vec<u16>>, StoreError> {
    let stored_key = units_to_bytes(key);
    let row: Option<(Vec<u8>, Option<Vec<u8>>)> =
        sqlx::query_as("SELECT payload, key_id FROM session_kv WHERE session_id=? AND key=?")
            .bind(session_id)
            .bind(&stored_key)
            .fetch_optional(connection)
            .await
            .map_err(database)?;
    let Some((payload, key_id)) = row else {
        return Ok(None);
    };
    let plain = match key_id {
        None => payload,
        Some(key_id) => {
            if Sealer::sealed_key_id(&payload) != Some(key_id.as_slice()) {
                return Err(StoreError::Credentials(format!(
                    "{} is corrupt: its key id does not match its value",
                    entry_label(key)
                )));
            }
            sealer.open(&owner(session_id, &stored_key), &payload, &entry_label(key))?
        }
    };
    bytes_to_units(&plain, &entry_label(key)).map(Some)
}

pub(crate) async fn has(
    connection: &mut SqliteConnection,
    session_id: &str,
    key: &[u16],
) -> Result<bool, StoreError> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM session_kv WHERE session_id=? AND key=?)")
        .bind(session_id)
        .bind(units_to_bytes(key))
        .fetch_one(connection)
        .await
        .map_err(database)
}

pub(crate) async fn usage(
    connection: &mut SqliteConnection,
    session_id: &str,
    key: &[u16],
) -> Result<SessionStateUsage, StoreError> {
    let (entries, bytes): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(retained_bytes), 0) FROM session_kv WHERE session_id=?",
    )
    .bind(session_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(database)?;
    let replaced: Option<i64> =
        sqlx::query_scalar("SELECT retained_bytes FROM session_kv WHERE session_id=? AND key=?")
            .bind(session_id)
            .bind(units_to_bytes(key))
            .fetch_optional(connection)
            .await
            .map_err(database)?;
    Ok(SessionStateUsage {
        entries: size_from_sql(entries)?,
        bytes: size_from_sql(bytes)?,
        replaced: replaced.map(size_from_sql).transpose()?,
    })
}

pub(crate) async fn put(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    session_id: &str,
    key: &[u16],
    payload: &[u16],
    size: SessionStateSize,
) -> Result<(), StoreError> {
    let stored_key = units_to_bytes(key);
    let plain = units_to_bytes(payload);
    let (payload_blob, key_id) = match sealer.key_id() {
        Some(key_id) => (
            sealer.seal(&owner(session_id, &stored_key), &plain)?,
            Some(key_id.to_vec()),
        ),
        None => (plain, None),
    };
    sqlx::query(
        "INSERT INTO session_kv(session_id,key,payload,key_id,value_bytes,retained_bytes) \
         VALUES (?,?,?,?,?,?) ON CONFLICT(session_id,key) DO UPDATE SET \
         payload=excluded.payload,key_id=excluded.key_id,value_bytes=excluded.value_bytes,\
         retained_bytes=excluded.retained_bytes",
    )
    .bind(session_id)
    .bind(stored_key)
    .bind(payload_blob)
    .bind(key_id)
    .bind(size_to_sql(size.value_bytes)?)
    .bind(size_to_sql(size.retained_bytes)?)
    .execute(connection)
    .await
    .map_err(database)?;
    Ok(())
}

pub(crate) async fn remove(
    connection: &mut SqliteConnection,
    session_id: &str,
    key: &[u16],
) -> Result<bool, StoreError> {
    let result = sqlx::query("DELETE FROM session_kv WHERE session_id=? AND key=?")
        .bind(session_id)
        .bind(units_to_bytes(key))
        .execute(connection)
        .await
        .map_err(database)?;
    Ok(result.rows_affected() > 0)
}

/// Up to `max_keys` keys after `after`, in code-unit order. Never opens a
/// value.
pub(crate) async fn keys(
    connection: &mut SqliteConnection,
    session_id: &str,
    after: Option<&[u16]>,
    max_keys: usize,
) -> Result<SessionStateKeys, StoreError> {
    // One row past the bound says whether keys remain after it.
    let limit = i64::try_from(max_keys.saturating_add(1))
        .map_err(|_| StoreError::Io("session scan bound overflow".into()))?;
    let rows: Vec<(Vec<u8>, i64)> = sqlx::query_as(
        "SELECT key, value_bytes FROM session_kv WHERE session_id=? AND key>? \
         ORDER BY key LIMIT ?",
    )
    .bind(session_id)
    // An empty blob sorts before every key, and the table never stores an empty one.
    .bind(after.map(units_to_bytes).unwrap_or_default())
    .bind(limit)
    .fetch_all(connection)
    .await
    .map_err(database)?;
    let more = rows.len() > max_keys;
    let keys = rows
        .into_iter()
        .take(max_keys)
        .map(|(key, size)| Ok((bytes_to_units(&key, "session key")?, size_from_sql(size)?)))
        .collect::<Result<_, StoreError>>()?;
    Ok(SessionStateKeys { keys, more })
}

/// Delete every entry of a session: its state ends with it.
pub(crate) async fn remove_all(
    connection: &mut SqliteConnection,
    session_id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM session_kv WHERE session_id=?")
        .bind(session_id)
        .execute(connection)
        .await?;
    Ok(())
}

fn owner(session_id: &str, key: &[u8]) -> String {
    format!("submilli:session-kv:v1:{session_id}:{}", hex(key))
}

fn entry_label(key: &[u16]) -> String {
    format!("session value '{}'", String::from_utf16_lossy(key))
}

fn units_to_bytes(units: &[u16]) -> Vec<u8> {
    units.iter().flat_map(|unit| unit.to_be_bytes()).collect()
}

fn bytes_to_units(bytes: &[u8], what: &str) -> Result<Vec<u16>, StoreError> {
    let (pairs, rest) = bytes.as_chunks::<2>();
    if !rest.is_empty() {
        return Err(StoreError::Io(format!(
            "{what} is not whole UTF-16 code units"
        )));
    }
    Ok(pairs.iter().map(|pair| u16::from_be_bytes(*pair)).collect())
}

fn size_to_sql(bytes: u64) -> Result<i64, StoreError> {
    i64::try_from(bytes).map_err(|_| StoreError::Io("session value size overflow".into()))
}

fn size_from_sql(bytes: i64) -> Result<u64, StoreError> {
    u64::try_from(bytes).map_err(|_| StoreError::Io("negative session value size".into()))
}
