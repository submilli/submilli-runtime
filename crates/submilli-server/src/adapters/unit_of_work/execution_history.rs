//! Outcome append and run progress share the caller's SQLite transaction.
use interpreter::runtime::durable::RunId;
use interpreter::runtime::effects::CODEC_VERSION;
use sqlx::{Row, SqliteConnection};
use submilli_durable::codec::Sealer;
use submilli_durable::history::{
    ExecutionAttempt, HistoryEntry, HistoryProgress, MAX_ENTRY_BYTES, MAX_HISTORY_BYTES,
    MAX_STORED_ENTRY_BYTES,
};

use crate::adapters::sql::database;
use crate::application::error::StoreError;

pub(super) async fn progress(
    connection: &mut SqliteConnection,
    attempt: &ExecutionAttempt,
) -> Result<HistoryProgress, StoreError> {
    let row = sqlx::query("SELECT history_position,history_bytes FROM runs WHERE run_id=? AND owner_generation=? AND attempt=? AND state='running' AND recovery_mode='recorded'")
        .bind(attempt.run_id.as_str()).bind(&attempt.owner_generation).bind(attempt.attempt)
        .fetch_optional(connection).await.map_err(database)?.ok_or(StoreError::Conflict)?;
    Ok(HistoryProgress {
        position: unsigned(row.try_get("history_position").map_err(database)?)?,
        bytes: unsigned(row.try_get("history_bytes").map_err(database)?)?,
    })
}

pub(super) async fn get(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    run_id: &RunId,
    sequence: u64,
) -> Result<Option<HistoryEntry>, StoreError> {
    // Bound the SQL value before loading or decrypting it.
    let size: Option<i64> = sqlx::query_scalar(
        "SELECT length(content) FROM durable_outcomes WHERE run_id=? AND sequence=?",
    )
    .bind(run_id.as_str())
    .bind(signed(sequence)?)
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;
    let Some(size) = size else {
        return Ok(None);
    };
    if unsigned(size)? > MAX_STORED_ENTRY_BYTES as u64 {
        return Err(invalid("oversized outcome row"));
    }
    let content: Vec<u8> =
        sqlx::query_scalar("SELECT content FROM durable_outcomes WHERE run_id=? AND sequence=?")
            .bind(run_id.as_str())
            .bind(signed(sequence)?)
            .fetch_one(connection)
            .await
            .map_err(database)?;
    let plain = sealer
        .open(&owner(run_id, sequence), &content, "native outcome")
        .map_err(invalid)?;
    if plain.len() > MAX_ENTRY_BYTES {
        return Err(invalid("oversized native outcome"));
    }
    interpreter::runtime::effects::stored_heap_bound(&plain, 1, false).map_err(invalid)?;
    let mut decoder = rmp_serde::Deserializer::new(std::io::Cursor::new(&plain));
    decoder.set_max_depth(16);
    let entry = <HistoryEntry as serde::Deserialize>::deserialize(&mut decoder).map_err(invalid)?;
    if decoder.position() != plain.len() as u64 {
        return Err(invalid("trailing history bytes"));
    }
    if entry.sequence != sequence || entry.version != CODEC_VERSION {
        return Err(invalid("native outcome identity mismatch"));
    }
    Ok(Some(entry))
}

pub(super) async fn append(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    attempt: &ExecutionAttempt,
    entry: &HistoryEntry,
) -> Result<(), StoreError> {
    let prior = progress(connection, attempt).await?;
    if entry.sequence != prior.position || entry.version != CODEC_VERSION {
        return Err(StoreError::Conflict);
    }
    let mut count = ByteCount(0);
    serde::Serialize::serialize(
        entry,
        &mut rmp_serde::Serializer::new(&mut count).with_struct_map(),
    )
    .map_err(invalid)?;
    if count.0 > MAX_ENTRY_BYTES {
        return Err(invalid("native outcome exceeds storage limit"));
    }
    let plain = rmp_serde::to_vec_named(entry).map_err(invalid)?;
    if plain.len() > MAX_ENTRY_BYTES {
        return Err(invalid("native outcome exceeds storage limit"));
    }
    let content = sealer
        .seal(&owner(&attempt.run_id, entry.sequence), &plain)
        .map_err(invalid)?;
    let bytes = prior
        .bytes
        .checked_add(content.len() as u64)
        .filter(|n| *n <= MAX_HISTORY_BYTES)
        .ok_or_else(|| invalid("run history exceeds storage limit"))?;
    let position = prior
        .position
        .checked_add(1)
        .ok_or_else(|| invalid("history sequence exhausted"))?;
    let updated = sqlx::query("UPDATE runs SET history_position=?,history_bytes=?,revision=revision+1 WHERE run_id=? AND owner_generation=? AND attempt=? AND state='running' AND recovery_mode='recorded' AND history_position=? AND revision<9223372036854775807")
        .bind(signed(position)?).bind(signed(bytes)?)
        .bind(attempt.run_id.as_str()).bind(&attempt.owner_generation).bind(attempt.attempt).bind(signed(entry.sequence)?)
        .execute(&mut *connection).await.map_err(database)?;
    if updated.rows_affected() != 1 {
        return Err(StoreError::Conflict);
    }
    sqlx::query("INSERT INTO durable_outcomes(run_id,sequence,content) VALUES(?,?,?)")
        .bind(attempt.run_id.as_str())
        .bind(signed(entry.sequence)?)
        .bind(content)
        .execute(connection)
        .await
        .map_err(database)?;
    Ok(())
}
fn owner(run_id: &RunId, sequence: u64) -> String {
    format!("durable-outcome:v1:{}:{run_id}:{sequence}", run_id.len())
}
fn signed(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(invalid)
}
fn unsigned(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(invalid)
}
fn invalid(error: impl std::fmt::Display) -> StoreError {
    StoreError::Io(error.to_string())
}

struct ByteCount(usize);
impl std::io::Write for ByteCount {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|length| *length <= MAX_ENTRY_BYTES)
            .ok_or_else(|| std::io::Error::other("history entry too large"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
