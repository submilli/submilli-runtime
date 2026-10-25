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

pub(crate) async fn progress(
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
    let Some((kind, content)) = load(connection, run_id, sequence).await? else {
        return Ok(None);
    };
    if kind != 0 {
        return Err(invalid(
            "external history requires the history payload store",
        ));
    }
    decode(sealer, run_id, sequence, &content, None).map(Some)
}

pub(crate) async fn load(
    connection: &mut SqliteConnection,
    run_id: &RunId,
    sequence: u64,
) -> Result<Option<(i64, Vec<u8>)>, StoreError> {
    let size: Option<i64> = sqlx::query_scalar(
        "SELECT length(content) FROM durable_outcomes WHERE run_id=? AND sequence=?",
    )
    .bind(run_id.as_str())
    .bind(signed(sequence)?)
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;
    let Some(size) = size else { return Ok(None) };
    if unsigned(size)? > MAX_STORED_ENTRY_BYTES as u64 {
        return Err(invalid("oversized outcome row"));
    }
    let row = sqlx::query(
        "SELECT storage_kind,content FROM durable_outcomes WHERE run_id=? AND sequence=?",
    )
    .bind(run_id.as_str())
    .bind(signed(sequence)?)
    .fetch_one(connection)
    .await
    .map_err(database)?;
    Ok(Some((
        row.try_get("storage_kind").map_err(database)?,
        row.try_get("content").map_err(database)?,
    )))
}

pub(crate) fn decode(
    sealer: &Sealer,
    run: &RunId,
    sequence: u64,
    content: &[u8],
    expected_plaintext_bytes: Option<u64>,
) -> Result<HistoryEntry, StoreError> {
    if content.len() > MAX_STORED_ENTRY_BYTES {
        return Err(invalid("oversized outcome"));
    }
    let plain = sealer
        .open(&owner(run, sequence), content, "native outcome")
        .map_err(invalid)?;
    if expected_plaintext_bytes.is_some_and(|length| length != plain.len() as u64) {
        return Err(invalid("history object plaintext length mismatch"));
    }
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
    Ok(entry)
}

pub(crate) struct EncodedEntry {
    pub content: Vec<u8>,
    pub plaintext_bytes: usize,
}

pub(crate) fn encode(
    sealer: &Sealer,
    run: &RunId,
    entry: &HistoryEntry,
) -> Result<EncodedEntry, StoreError> {
    if entry.version != CODEC_VERSION {
        return Err(StoreError::Conflict);
    }
    let mut count = ByteCount(0);
    serde::Serialize::serialize(
        entry,
        &mut rmp_serde::Serializer::new(&mut count).with_struct_map(),
    )
    .map_err(invalid)?;
    let mut plain = Vec::new();
    plain.try_reserve_exact(count.0).map_err(invalid)?;
    serde::Serialize::serialize(
        entry,
        &mut rmp_serde::Serializer::new(&mut plain).with_struct_map(),
    )
    .map_err(invalid)?;
    let content = sealer
        .seal(&owner(run, entry.sequence), &plain)
        .map_err(invalid)?;
    Ok(EncodedEntry {
        content,
        plaintext_bytes: plain.len(),
    })
}

pub(super) async fn append(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    attempt: &ExecutionAttempt,
    entry: &HistoryEntry,
) -> Result<(), StoreError> {
    let encoded = encode(sealer, &attempt.run_id, entry)?;
    append_stored(
        connection,
        attempt,
        entry.sequence,
        0,
        &encoded.content,
        encoded.content.len() as u64,
    )
    .await
}

pub(crate) async fn append_stored(
    connection: &mut SqliteConnection,
    attempt: &ExecutionAttempt,
    sequence: u64,
    kind: i64,
    content: &[u8],
    accounted_bytes: u64,
) -> Result<(), StoreError> {
    let prior = progress(connection, attempt).await?;
    if sequence != prior.position {
        return Err(StoreError::Conflict);
    }
    let bytes = prior
        .bytes
        .checked_add(accounted_bytes)
        .filter(|n| *n <= MAX_HISTORY_BYTES)
        .ok_or_else(|| invalid("run history exceeds storage limit"))?;
    let position = prior
        .position
        .checked_add(1)
        .ok_or_else(|| invalid("history sequence exhausted"))?;
    let updated = sqlx::query("UPDATE runs SET history_position=?,history_bytes=?,revision=revision+1 WHERE run_id=? AND owner_generation=? AND attempt=? AND state='running' AND recovery_mode='recorded' AND history_position=? AND revision<9223372036854775807")
        .bind(signed(position)?).bind(signed(bytes)?)
        .bind(attempt.run_id.as_str()).bind(&attempt.owner_generation).bind(attempt.attempt).bind(signed(sequence)?)
        .execute(&mut *connection).await.map_err(database)?;
    if updated.rows_affected() != 1 {
        return Err(StoreError::Conflict);
    }
    sqlx::query(
        "INSERT INTO durable_outcomes(run_id,sequence,storage_kind,content) VALUES(?,?,?,?)",
    )
    .bind(attempt.run_id.as_str())
    .bind(signed(sequence)?)
    .bind(kind)
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
