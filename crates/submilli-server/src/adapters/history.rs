//! SQLite history with private, immutable placement for large sealed entries.
//! Pending files are not outcomes: only a fenced history commit authorizes replay.
use crate::adapters::unit_of_work::{
    durable::durable_error, execution_history as entries, transaction::SqliteTransaction,
};
use crate::config::DurableObjectConfig;
use crate::database::ServerDatabase;
use interpreter::runtime::blocking::BlockingWork;
use interpreter::runtime::durable::RunId;
use interpreter::runtime::fuel;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use submilli_durable::codec::Sealer;
use submilli_durable::history::{
    ExecutionAttempt, HistoryEntry, HistoryProgress, HistoryStore, MAX_ENTRY_BYTES,
    MAX_STORED_ENTRY_BYTES,
};
use submilli_durable::unit_of_work::StoreError;
use submilli_shared::secret_store::{KEY_ID_LEN, SecretCipher};
use tokio::sync::{Mutex, RwLock};

mod files;
#[cfg(all(test, not(windows)))]
mod tests;

pub(crate) struct HistoryObjects {
    database: Arc<ServerDatabase>,
    files: Arc<files::ObjectFiles>,
    sealer: Sealer,
    config: DurableObjectConfig,
    // A collector must exclude complete append/read operations, not just file I/O.
    access: Arc<RwLock<()>>,
    maintenance: Mutex<BlockingWork>,
}
/// One persistence owner per attempt. Cancellation leaves file work registered here.
struct ObjectHistory {
    objects: Arc<HistoryObjects>,
    workers: Mutex<BlockingWork>,
    storage_fuel: Arc<AtomicU64>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ObjectReference {
    version: u8,
    id: String,
    plaintext_bytes: u64,
    sealed_bytes: u64,
    sha256: [u8; 32],
    key_id: [u8; KEY_ID_LEN],
}
impl ObjectReference {
    fn validate(&self) -> Result<(), StoreError> {
        validate_id(&self.id)?;
        if self.version != 1
            || self.plaintext_bytes > MAX_ENTRY_BYTES as u64
            || self.sealed_bytes > MAX_STORED_ENTRY_BYTES as u64
            || self.sealed_bytes < self.plaintext_bytes
        {
            return Err(invalid("invalid durable history object reference"));
        }
        Ok(())
    }
}

impl HistoryObjects {
    pub(crate) async fn has_retained_runs(database: &ServerDatabase) -> Result<bool, StoreError> {
        database
            .read(|connection| {
                Box::pin(async move {
                    Ok(
                        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM runs WHERE recovery_mode='recorded' AND resources_retired=0)")
                            .fetch_one(connection)
                            .await?,
                    )
                })
            })
            .await
            .map_err(|e| StoreError::Database(Box::new(e)))
    }

    pub(crate) async fn open(
        database: Arc<ServerDatabase>,
        directory: PathBuf,
        cipher: Arc<SecretCipher>,
        config: DurableObjectConfig,
    ) -> Result<Arc<Self>, StoreError> {
        require_supported_platform()?;
        config.validate().map_err(invalid)?;
        if database.is_ephemeral() {
            return Err(invalid("history objects require a persistent database"));
        }
        let mut workers = BlockingWork::default();
        let files = Arc::new(
            workers
                .spawn(move || files::ObjectFiles::open(&directory))
                .await
                .map_err(invalid)??,
        );
        Ok(Arc::new(Self {
            database,
            files,
            sealer: Sealer::new(Some(cipher)),
            config,
            access: Arc::new(RwLock::new(())),
            maintenance: Mutex::new(workers),
        }))
    }

    pub(crate) fn history(self: &Arc<Self>) -> Arc<dyn HistoryStore> {
        Arc::new(ObjectHistory {
            objects: self.clone(),
            workers: Mutex::new(BlockingWork::default()),
            storage_fuel: Arc::new(AtomicU64::new(0)),
        })
    }

    /// The terminal run remains eligible for retirement until deletion and its
    /// directory barriers succeed. Cancelled workers retain the exclusive guard.
    pub(crate) async fn remove_run(&self, session: &str, run: &RunId) -> Result<(), StoreError> {
        let directory = files::RunDirectory::new(session, run.as_str())?;
        let mut workers = self.maintenance.lock().await;
        let access = self.access.clone().write_owned().await;
        let files = self.files.clone();
        workers
            .spawn(move || {
                let _access = access;
                files.remove_run(&directory)
            })
            .await
            .map_err(invalid)?
    }

    async fn directory(&self, run: &RunId) -> Result<files::RunDirectory, StoreError> {
        let id = run.to_string();
        let session: String = self
            .database
            .read(move |connection| {
                Box::pin(async move {
                    sqlx::query_scalar("SELECT session_id FROM runs WHERE run_id=?")
                        .bind(id)
                        .fetch_one(connection)
                        .await
                        .map_err(Into::into)
                })
            })
            .await
            .map_err(|e| StoreError::Database(Box::new(e)))?;
        files::RunDirectory::new(&session, run.as_str())
    }
}

#[async_trait::async_trait]
impl HistoryStore for ObjectHistory {
    fn take_storage_fuel(&self) -> u64 {
        self.storage_fuel.swap(0, Ordering::AcqRel)
    }

    async fn progress(&self, attempt: &ExecutionAttempt) -> Result<HistoryProgress, StoreError> {
        let mut unit = SqliteTransaction::begin(self.objects.database.clone())
            .await
            .map_err(durable_error)?;
        let attempt = attempt.clone();
        unit.query_recoverable(move |connection| {
            Box::pin(async move { entries::progress(connection, &attempt).await })
        })
        .await
        .map_err(durable_error)
    }

    async fn get(&self, run: &RunId, sequence: u64) -> Result<Option<HistoryEntry>, StoreError> {
        let access = Arc::new(self.objects.access.clone().read_owned().await);
        let record = {
            let mut unit = SqliteTransaction::begin(self.objects.database.clone())
                .await
                .map_err(durable_error)?;
            let run = run.clone();
            unit.query_recoverable(move |connection| {
                Box::pin(async move { entries::load(connection, &run, sequence).await })
            })
            .await
            .map_err(durable_error)?
        };
        let Some((kind, content)) = record else {
            return Ok(None);
        };
        let directory = if kind == 1 {
            Some(self.objects.directory(run).await?)
        } else {
            None
        };
        let files = self.objects.files.clone();
        let sealer = self.objects.sealer.clone();
        let run = run.clone();
        let mut workers = self.workers.lock().await;
        let storage_fuel = self.storage_fuel.clone();
        workers
            .spawn(move || {
                let _access = access;
                let content = match kind {
                    0 => content,
                    1 => {
                        if content.len() > 4096 {
                            return Err(invalid("oversized history object reference"));
                        }
                        let reference: ObjectReference = submilli_durable::codec::decode(
                            &sealer,
                            &reference_owner(&run, sequence),
                            &content,
                        )?;
                        reference.validate()?;
                        if sealer.key_id() != Some(reference.key_id) {
                            return Err(invalid("history object key mismatch"));
                        }
                        record_object_work(&storage_fuel, reference.sealed_bytes, 5);
                        let directory = directory
                            .as_ref()
                            .ok_or_else(|| invalid("history object directory unavailable"))?;
                        let content = files.read(directory, &reference)?;
                        if Sealer::sealed_key_id(&content) != Some(reference.key_id.as_slice()) {
                            return Err(invalid("history object envelope key mismatch"));
                        }
                        // The sealer authenticates both bytes and their run/sequence owner.
                        let entry = entries::decode(
                            &sealer,
                            &run,
                            sequence,
                            &content,
                            Some(reference.plaintext_bytes),
                        )
                        .map_err(durable_error)?;
                        return Ok(Some(entry));
                    }
                    _ => return Err(invalid("unknown history storage representation")),
                };
                entries::decode(&sealer, &run, sequence, &content, None)
                    .map(Some)
                    .map_err(durable_error)
            })
            .await
            .map_err(invalid)?
    }

    async fn append(
        &self,
        attempt: &ExecutionAttempt,
        entry: Arc<HistoryEntry>,
    ) -> Result<(), StoreError> {
        let access = Arc::new(self.objects.access.clone().read_owned().await);
        let mut workers = self.workers.lock().await;
        let sealer = self.objects.sealer.clone();
        let run = attempt.run_id.clone();
        let sequence = entry.sequence;
        let threshold = self.objects.config.inline_threshold;
        let storage_fuel = self.storage_fuel.clone();
        let prepared = workers
            .spawn(move || prepare_entry(&sealer, &run, &entry, threshold, &storage_fuel))
            .await
            .map_err(invalid)??;
        let stored = self
            .publish(attempt, sequence, prepared, &mut workers, access.clone())
            .await?;
        self.commit(attempt, sequence, stored).await
    }

    async fn drain(&self) -> Result<(), StoreError> {
        self.workers.lock().await.finish().await.map_err(invalid)
    }
}
impl ObjectHistory {
    async fn publish(
        &self,
        attempt: &ExecutionAttempt,
        sequence: u64,
        prepared: PreparedEntry,
        workers: &mut BlockingWork,
        access: Arc<tokio::sync::OwnedRwLockReadGuard<()>>,
    ) -> Result<StoredEntry, StoreError> {
        let Some(reference) = prepared.reference else {
            return Ok(StoredEntry::Inline(prepared.encoded.content));
        };
        // Fence before touching files. Retirement marks the run terminal before
        // taking exclusive access, so no late append can recreate a retired tree.
        let progress = self.progress(attempt).await?;
        if progress.position != sequence {
            return Err(StoreError::Conflict);
        }
        let directory = self.objects.directory(&attempt.run_id).await?;
        let sealed_reference = submilli_durable::codec::encode(
            &self.objects.sealer,
            &reference_owner(&attempt.run_id, sequence),
            &reference,
        )?;
        let bytes = reference.sealed_bytes;
        let files = self.objects.files.clone();
        workers
            .spawn(move || {
                let _access = access;
                files.publish(&directory, &reference, &prepared.encoded.content)
            })
            .await
            .map_err(invalid)??;
        Ok(StoredEntry::Object {
            sealed_reference,
            bytes,
        })
    }

    async fn commit(
        &self,
        attempt: &ExecutionAttempt,
        sequence: u64,
        stored: StoredEntry,
    ) -> Result<(), StoreError> {
        let mut unit = SqliteTransaction::begin(self.objects.database.clone())
            .await
            .map_err(durable_error)?;
        let attempt = attempt.clone();
        unit.query_recoverable(move |connection| {
            Box::pin(async move {
                match stored {
                    StoredEntry::Inline(content) => {
                        entries::append_stored(
                            connection,
                            &attempt,
                            sequence,
                            0,
                            &content,
                            content.len() as u64,
                        )
                        .await?;
                    }
                    StoredEntry::Object {
                        sealed_reference,
                        bytes,
                    } => {
                        entries::append_stored(
                            connection,
                            &attempt,
                            sequence,
                            1,
                            &sealed_reference,
                            bytes,
                        )
                        .await?;
                    }
                }
                Ok(())
            })
        })
        .await
        .map_err(durable_error)?;
        unit.commit().await.map_err(durable_error)?;
        checkpoint("committed");
        Ok(())
    }
}

struct PreparedEntry {
    encoded: entries::EncodedEntry,
    reference: Option<ObjectReference>,
}

enum StoredEntry {
    Inline(Vec<u8>),
    Object {
        sealed_reference: Vec<u8>,
        bytes: u64,
    },
}

fn prepare_entry(
    sealer: &Sealer,
    run: &RunId,
    entry: &HistoryEntry,
    threshold: usize,
    storage_fuel: &AtomicU64,
) -> Result<PreparedEntry, StoreError> {
    let encoded = entries::encode(sealer, run, entry).map_err(durable_error)?;
    let reference = if encoded.plaintext_bytes > threshold {
        record_object_work(storage_fuel, encoded.content.len() as u64, 14);
        Some(ObjectReference {
            version: 1,
            id: new_id()?,
            plaintext_bytes: encoded.plaintext_bytes as u64,
            sealed_bytes: encoded.content.len() as u64,
            sha256: Sha256::digest(&encoded.content).into(),
            key_id: sealer
                .key_id()
                .ok_or_else(|| invalid("history object key unavailable"))?,
        })
    } else {
        None
    };
    Ok(PreparedEntry { encoded, reference })
}

fn require_supported_platform() -> Result<(), StoreError> {
    if cfg!(windows) {
        return Err(invalid(
            "durable object storage requires Unix directory synchronization; native Windows is not supported",
        ));
    }
    Ok(())
}

fn reference_owner(run: &RunId, sequence: u64) -> String {
    format!("history-object-reference:v1:{}:{run}:{sequence}", run.len())
}
fn new_id() -> Result<String, StoreError> {
    let mut bytes = [0; 16];
    getrandom::getrandom(&mut bytes).map_err(invalid)?;
    Ok(uuid::Builder::from_random_bytes(bytes)
        .into_uuid()
        .to_string())
}
fn validate_id(id: &str) -> Result<(), StoreError> {
    let parsed = uuid::Uuid::parse_str(id).map_err(invalid)?;
    if parsed.to_string() != id {
        return Err(invalid("invalid history object identity"));
    }
    Ok(())
}
fn invalid(error: impl std::fmt::Display) -> StoreError {
    StoreError::Io(error.to_string())
}
fn checkpoint(_point: &str) {
    #[cfg(all(test, not(windows)))]
    tests::checkpoint(_point);
}

fn record_object_work(spent: &AtomicU64, bytes: u64, syscalls: u64) {
    // These bytes belong to an already observed outcome; charge before I/O,
    // then settle at the effect boundary, including failures and cancellation.
    let cost = fuel::IO
        .cost(bytes)
        .saturating_add(fuel::HASH.cost(bytes))
        .saturating_add(fuel::SYSCALL.cost(syscalls));
    let _ = spent.fetch_update(Ordering::AcqRel, Ordering::Acquire, |prior| {
        Some(prior.saturating_add(cost))
    });
}

#[cfg(all(test, windows))]
mod platform_tests {
    #[test]
    fn native_windows_reports_unsupported_durability() {
        assert!(
            super::require_supported_platform()
                .unwrap_err()
                .to_string()
                .contains("native Windows is not supported")
        );
    }
}
