//! Server-owned SQLite lifecycle. Entity tables arrive in later migrations.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};
use tokio::sync::{oneshot, watch};
use uuid::Uuid;

const MAX_WAITING: usize = 64;
const SCHEMA_VERSION: i64 = 1;

type Job = Box<dyn FnOnce(&mut Connection) + Send>;
type CloseResult = Option<Result<(), Arc<DatabaseError>>>;

#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("database path {0} has no file name or parent directory")]
    InvalidPath(PathBuf),
    #[error("database I/O at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("database {0} is already open by another server")]
    AlreadyOpen(PathBuf),
    #[error("database path {0} has multiple hard links")]
    MultipleLinks(PathBuf),
    #[error("database identity generation failed: {0}")]
    Entropy(String),
    #[error("SQLite operation failed: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("database schema version {found} is newer than supported version 1")]
    NewerSchema { found: i64 },
    #[error("database schema version {found} is invalid")]
    InvalidSchemaVersion { found: i64 },
    #[error("database work queue is full")]
    Busy,
    #[error("database WAL checkpoint could not complete because readers are active")]
    CheckpointBusy,
    #[error("database is shutting down")]
    Closed,
    #[error("database worker stopped before replying")]
    WorkerStopped,
    #[error("could not start database worker: {0}")]
    WorkerStart(#[source] std::io::Error),
    #[error("database submission lock is poisoned")]
    Poisoned,
    #[error("database transaction callback panicked")]
    CallbackPanicked,
    #[error("database cleanup failed: {0}")]
    Cleanup(#[source] Arc<DatabaseError>),
    #[error("database metadata is missing or malformed")]
    InvalidMetadata,
}

/// An async handle to one SQLite worker, with a bounded work queue. The worker
/// owns the native connection and process lock from startup through cleanup;
/// cancelling a caller or shutting down its executor cannot release them early.
pub struct ServerDatabase {
    path: PathBuf,
    sender: Mutex<Option<mpsc::SyncSender<Job>>>,
    completion: watch::Receiver<CloseResult>,
    store_id: Uuid,
    startup_generation: Uuid,
}

impl ServerDatabase {
    pub async fn open(path: &Path) -> Result<Self, DatabaseError> {
        let path = path.to_path_buf();
        let (sender, receiver) = mpsc::sync_channel(MAX_WAITING);
        let (ready, startup) = oneshot::channel();
        let (finished, completion) = watch::channel(None);
        std::thread::Builder::new()
            .name("submilli-sqlite".into())
            .spawn(move || database_worker(path, receiver, ready, finished))
            .map_err(DatabaseError::WorkerStart)?;
        let (path, store_id, startup_generation) =
            startup.await.map_err(|_| DatabaseError::WorkerStopped)??;
        Ok(Self {
            path,
            sender: Mutex::new(Some(sender)),
            completion,
            store_id,
            startup_generation,
        })
    }

    pub fn store_id(&self) -> Uuid {
        self.store_id
    }

    pub fn startup_generation(&self) -> Uuid {
        self.startup_generation
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Run a write transaction on the SQLite thread. The callback performs
    /// synchronous database work there; callers await only the reply. Success
    /// commits, errors roll back, and cancellation cannot abandon accepted work.
    /// Callbacks must finish without waiting for another call to this database.
    pub async fn transaction<T, F>(&self, operation: F) -> Result<T, DatabaseError>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> Result<T, DatabaseError> + Send + 'static,
    {
        let (reply, result) = oneshot::channel();
        let job: Job = Box::new(move |connection| {
            let outcome = run_transaction(connection, operation);
            let _ = reply.send(outcome);
        });
        {
            let sender = self.sender.lock().map_err(|_| DatabaseError::Poisoned)?;
            sender
                .as_ref()
                .ok_or(DatabaseError::Closed)?
                .try_send(job)
                .map_err(|error| match error {
                    mpsc::TrySendError::Full(_) => DatabaseError::Busy,
                    mpsc::TrySendError::Disconnected(_) => DatabaseError::Closed,
                })?;
        }
        result.await.map_err(|_| DatabaseError::WorkerStopped)?
    }

    /// Stop admission immediately; the owner drains accepted work, checkpoints,
    /// and closes SQLite before releasing its process lock. Dropping the final
    /// handle also disconnects the queue and starts the same cleanup.
    pub(crate) fn begin_close(&self) -> Result<(), DatabaseError> {
        self.sender
            .lock()
            .map_err(|_| DatabaseError::Poisoned)?
            .take();
        Ok(())
    }

    /// Await cleanup without tying its lifetime to the calling executor.
    pub async fn close(&self) -> Result<(), DatabaseError> {
        self.begin_close()?;
        let mut completion = self.completion.clone();
        let result = completion
            .wait_for(Option::is_some)
            .await
            .map_err(|_| DatabaseError::WorkerStopped)?;
        match result.as_ref() {
            Some(Ok(())) => Ok(()),
            Some(Err(error)) => Err(DatabaseError::Cleanup(Arc::clone(error))),
            None => Err(DatabaseError::WorkerStopped),
        }
    }
}

fn database_worker(
    path: PathBuf,
    receiver: mpsc::Receiver<Job>,
    ready: oneshot::Sender<Result<(PathBuf, Uuid, Uuid), DatabaseError>>,
    finished: watch::Sender<CloseResult>,
) {
    let mut database = match OwnedDatabase::open(&path) {
        Ok(database) => database,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let _ = ready.send(Ok((
        database.path.clone(),
        database.store_id,
        database.startup_generation,
    )));
    while let Ok(job) = receiver.recv() {
        // Cancelled callers leave returned values for the worker to drop. Their
        // destructors are caller code too and must not terminate the owner.
        if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            job(&mut database.connection);
        })) {
            dispose_panic_payload(payload);
            tracing::error!("database job cleanup panicked");
        }
    }
    let result = database.close().map_err(Arc::new);
    if let Err(error) = &result {
        tracing::error!(%error, "database cleanup failed");
    }
    finished.send_replace(Some(result));
}

// Field order ensures native SQLite closes before the lock on every error and
// unwind path, including a callback that unwinds during worker execution.
struct OwnedDatabase {
    connection: Connection,
    _lock: File,
    path: PathBuf,
    store_id: Uuid,
    startup_generation: Uuid,
}

impl OwnedDatabase {
    fn open(path: &Path) -> Result<Self, DatabaseError> {
        let (lock, path) = lock_database(path)?;
        let mut connection = Connection::open(&path)?;
        reject_multiple_links(&path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        let mode: String = connection.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(DatabaseError::InvalidMetadata);
        }
        connection.execute_batch(
            "PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA wal_autocheckpoint=1000;",
        )?;
        let (store_id, startup_generation) = migrate(&mut connection)?;
        Ok(Self {
            connection,
            _lock: lock,
            path,
            store_id,
            startup_generation,
        })
    }

    fn close(self) -> Result<(), DatabaseError> {
        let checkpoint = self
            .connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                row.get::<_, i64>(0)
            });
        self.connection.close().map_err(|(connection, error)| {
            drop(connection);
            DatabaseError::Sql(error)
        })?;
        if checkpoint? != 0 {
            return Err(DatabaseError::CheckpointBusy);
        }
        Ok(())
    }
}

fn run_transaction<T, F>(connection: &mut Connection, operation: F) -> Result<T, DatabaseError>
where
    F: FnOnce(&Transaction<'_>) -> Result<T, DatabaseError>,
{
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    // Contain panics from caller-supplied code while retaining the transaction
    // for rollback. Database setup and cleanup use ordinary typed errors.
    let outcome =
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(&transaction))) {
            Ok(outcome) => outcome,
            Err(payload) => {
                dispose_panic_payload(payload);
                Err(DatabaseError::CallbackPanicked)
            }
        };
    match outcome {
        Ok(value) => {
            transaction.commit()?;
            Ok(value)
        }
        Err(error) => {
            transaction.rollback()?;
            Err(error)
        }
    }
}

fn dispose_panic_payload(mut payload: Box<dyn std::any::Any + Send>) {
    // A caller's payload destructor may panic too. Dispose replacement payloads
    // outside an active unwind so cleanup cannot abort or recurse.
    loop {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload))) {
            Ok(()) => return,
            Err(replacement) => payload = replacement,
        }
    }
}

fn lock_database(path: &Path) -> Result<(File, PathBuf), DatabaseError> {
    let parent = path
        .parent()
        .ok_or_else(|| DatabaseError::InvalidPath(path.to_path_buf()))?;
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    std::fs::create_dir_all(parent).map_err(|source| DatabaseError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let canonical_parent = std::fs::canonicalize(parent).map_err(|source| DatabaseError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let name = path
        .file_name()
        .ok_or_else(|| DatabaseError::InvalidPath(path.to_path_buf()))?;
    let canonical_path = match std::fs::symlink_metadata(path) {
        Ok(_) => std::fs::canonicalize(path).map_err(|source| DatabaseError::Io {
            path: path.to_path_buf(),
            source,
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => canonical_parent.join(name),
        Err(source) => {
            return Err(DatabaseError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    reject_multiple_links(&canonical_path)?;
    let mut lock_name = canonical_path.as_os_str().to_os_string();
    lock_name.push(".lock");
    let lock_path = PathBuf::from(lock_name);
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|source| DatabaseError::Io {
            path: lock_path.clone(),
            source,
        })?;
    file.try_lock().map_err(|error| match error {
        std::fs::TryLockError::WouldBlock => DatabaseError::AlreadyOpen(path.to_path_buf()),
        std::fs::TryLockError::Error(source) => DatabaseError::Io {
            path: lock_path,
            source,
        },
    })?;
    Ok((file, canonical_path))
}

fn reject_multiple_links(path: &Path) -> Result<(), DatabaseError> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(DatabaseError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    #[cfg(not(unix))]
    let _metadata = metadata;
    #[cfg(unix)]
    let links = {
        use std::os::unix::fs::MetadataExt;
        metadata.nlink()
    };
    #[cfg(windows)]
    let links = windows_link_count(path).map_err(|source| DatabaseError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    #[cfg(not(any(unix, windows)))]
    let links = 2;
    if links > 1 {
        return Err(DatabaseError::MultipleLinks(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(windows)]
fn windows_link_count(path: &Path) -> std::io::Result<u64> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let file = File::open(path)?;
    let mut information = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // The file owns a valid handle and the output pointer has the required size.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    // A successful call initializes every field of BY_HANDLE_FILE_INFORMATION.
    Ok(u64::from(
        unsafe { information.assume_init() }.nNumberOfLinks,
    ))
}

fn random_uuid() -> Result<Uuid, DatabaseError> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|error| DatabaseError::Entropy(error.to_string()))?;
    Ok(uuid::Builder::from_random_bytes(bytes).into_uuid())
}

fn migrate(connection: &mut Connection) -> Result<(Uuid, Uuid), DatabaseError> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version: i64 = transaction.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(DatabaseError::NewerSchema { found: version });
    }
    if version < 0 {
        return Err(DatabaseError::InvalidSchemaVersion { found: version });
    }
    if version == 0 {
        transaction.execute_batch(
            "CREATE TABLE server_metadata (singleton INTEGER PRIMARY KEY CHECK (singleton = 1), store_id TEXT NOT NULL, startup_generation TEXT NOT NULL);
             CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL);
             INSERT INTO schema_migrations (version, name) VALUES (1, 'server_metadata');
             PRAGMA user_version=1;",
        )?;
        transaction.execute(
            "INSERT INTO server_metadata (singleton, store_id, startup_generation) VALUES (1, ?1, ?2)",
            (random_uuid()?.to_string(), Uuid::nil().to_string()),
        )?;
    }
    let migration_name: Option<String> = transaction
        .query_row(
            "SELECT name FROM schema_migrations WHERE version = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if migration_name.as_deref() != Some("server_metadata") {
        return Err(DatabaseError::InvalidMetadata);
    }
    let unknown_migrations: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM schema_migrations WHERE version != 1",
        [],
        |row| row.get(0),
    )?;
    if unknown_migrations != 0 {
        return Err(DatabaseError::InvalidMetadata);
    }
    let store_id: String = transaction
        .query_row(
            "SELECT store_id FROM server_metadata WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()?
        .ok_or(DatabaseError::InvalidMetadata)?;
    let store_id = Uuid::parse_str(&store_id).map_err(|_| DatabaseError::InvalidMetadata)?;
    let generation = random_uuid()?;
    transaction.execute(
        "UPDATE server_metadata SET startup_generation = ?1 WHERE singleton = 1",
        (generation.to_string(),),
    )?;
    transaction.commit()?;
    Ok((store_id, generation))
}

#[cfg(test)]
mod tests;
