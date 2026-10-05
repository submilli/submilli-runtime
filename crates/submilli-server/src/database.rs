//! Server-owned SQLx connection, migrations, and native connection cleanup.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use futures::{FutureExt, future::BoxFuture};
use sqlx::{
    Connection, SqliteConnection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
};
use tokio::sync::{oneshot, watch};

const MAX_WAITING: usize = 64;
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

type Job = Box<dyn for<'a> FnOnce(&'a mut SqliteConnection) -> BoxFuture<'a, ()> + Send>;
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
    #[error("SQLite operation failed: {0}")]
    Sql(#[from] sqlx::Error),
    #[error("database migration failed: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("blueprint import failed: {0}")]
    Import(String),
    #[error("blueprint already exists")]
    AlreadyExists,
    #[error("blueprint '{name}' revision counter exhausted")]
    RevisionExhausted { name: String },
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
}

/// An async handle to one SQLite worker, with a bounded work queue. The worker
/// owns the native connection and process lock from startup through cleanup;
/// cancelling a caller or shutting down its executor cannot release them early.
pub struct ServerDatabase {
    path: PathBuf,
    sender: Mutex<Option<mpsc::SyncSender<Job>>>,
    completion: watch::Receiver<CloseResult>,
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
        let path = startup.await.map_err(|_| DatabaseError::WorkerStopped)??;
        Ok(Self {
            path,
            sender: Mutex::new(Some(sender)),
            completion,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Run a transaction while its caller is waiting. Cancellation drops the
    /// transaction, allowing SQLx to roll it back unless commit has already begun.
    /// The callback must not submit another operation to this database.
    pub async fn transaction<T, F>(&self, operation: F) -> Result<T, DatabaseError>
    where
        T: Send + 'static,
        F: for<'a> FnOnce(&'a mut SqliteConnection) -> BoxFuture<'a, Result<T, DatabaseError>>
            + Send
            + 'static,
    {
        self.submit(move |connection| Box::pin(run_transaction(connection, operation)))
            .await
    }

    /// Execute a read without acquiring a SQLite write transaction.
    pub(crate) async fn read<T, F>(&self, operation: F) -> Result<T, DatabaseError>
    where
        T: Send + 'static,
        F: for<'a> FnOnce(&'a mut SqliteConnection) -> BoxFuture<'a, Result<T, DatabaseError>>
            + Send
            + 'static,
    {
        self.submit(operation).await
    }

    async fn submit<T, F>(&self, operation: F) -> Result<T, DatabaseError>
    where
        T: Send + 'static,
        F: for<'a> FnOnce(&'a mut SqliteConnection) -> BoxFuture<'a, Result<T, DatabaseError>>
            + Send
            + 'static,
    {
        let (mut reply, result) = oneshot::channel();
        let job: Job = Box::new(move |connection| {
            Box::pin(async move {
                let outcome = tokio::select! {
                    biased;
                    () = reply.closed() => return,
                    outcome = contain_panic(async move { operation(connection).await }) => outcome,
                };
                let _ = reply.send(outcome);
            })
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

    /// Stop admission immediately; the owner finishes work whose callers still
    /// wait, skips cancelled work, checkpoints,
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
    ready: oneshot::Sender<Result<PathBuf, DatabaseError>>,
    finished: watch::Sender<CloseResult>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = ready.send(Err(DatabaseError::WorkerStart(error)));
            return;
        }
    };
    let mut database = match runtime.block_on(OwnedDatabase::open(&path)) {
        Ok(database) => database,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let _ = ready.send(Ok(database.path.clone()));
    while let Ok(job) = receiver.recv() {
        if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runtime.block_on(job(&mut database.connection));
        })) {
            dispose_panic_payload(payload);
            tracing::error!("database job cleanup panicked");
        }
    }
    let result = runtime.block_on(database.close()).map_err(Arc::new);
    if let Err(error) = &result {
        tracing::error!(%error, "database cleanup failed");
    }
    finished.send_replace(Some(result));
}

/// An uncertain native close must never allow another server to acquire the lock.
struct DatabaseLock(Option<File>);

impl DatabaseLock {
    fn release(&mut self, path: &Path) -> Result<(), DatabaseError> {
        if let Some(lock) = &self.0 {
            // A child may inherit this descriptor before exec closes it.
            // Explicit unlock releases ownership even while that copy exists.
            lock.unlock().map_err(|source| DatabaseError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        }
        drop(self.0.take());
        Ok(())
    }
}

impl Drop for DatabaseLock {
    fn drop(&mut self) {
        if let Some(lock) = self.0.take() {
            std::mem::forget(lock);
            tracing::error!(
                "retaining database lock until process exit: native close was not confirmed"
            );
        }
    }
}

struct OwnedDatabase {
    connection: SqliteConnection,
    lock: DatabaseLock,
    path: PathBuf,
}

impl OwnedDatabase {
    async fn open(path: &Path) -> Result<Self, DatabaseError> {
        let (lock, path) = lock_database(path)?;
        let mut lock = DatabaseLock(Some(lock));
        let options = SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5))
            .pragma("wal_autocheckpoint", "1000")
            .optimize_on_close(false, None);
        let mut connection = SqliteConnection::connect_with(&options).await?;
        let initialized = async {
            reject_multiple_links(&path)?;
            MIGRATOR.run(&mut connection).await?;
            Ok::<(), DatabaseError>(())
        }
        .await;
        if let Err(error) = initialized {
            connection.close().await?;
            lock.release(&path)?;
            return Err(error);
        }
        Ok(Self {
            connection,
            lock,
            path,
        })
    }

    async fn close(mut self) -> Result<(), DatabaseError> {
        let checkpoint: Result<(i64, i64, i64), _> =
            sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
                .fetch_one(&mut self.connection)
                .await;
        self.connection.close().await?;
        self.lock.release(&self.path)?;
        if checkpoint?.0 != 0 {
            return Err(DatabaseError::CheckpointBusy);
        }
        Ok(())
    }
}

async fn run_transaction<T, F>(
    connection: &mut SqliteConnection,
    operation: F,
) -> Result<T, DatabaseError>
where
    T: Send,
    F: for<'a> FnOnce(&'a mut SqliteConnection) -> BoxFuture<'a, Result<T, DatabaseError>> + Send,
{
    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
    let outcome = contain_panic(async { operation(&mut transaction).await }).await;
    match outcome {
        Ok(value) => {
            transaction.commit().await?;
            Ok(value)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

async fn contain_panic<T>(
    future: impl std::future::Future<Output = Result<T, DatabaseError>>,
) -> Result<T, DatabaseError> {
    match std::panic::AssertUnwindSafe(future).catch_unwind().await {
        Ok(result) => result,
        Err(payload) => {
            dispose_panic_payload(payload);
            Err(DatabaseError::CallbackPanicked)
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

#[cfg(test)]
mod tests;
