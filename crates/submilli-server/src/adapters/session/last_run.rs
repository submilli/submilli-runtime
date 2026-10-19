//! Each session's last run: its `{result, console, error}` JSON, sealed when
//! the server has a key. A small one is a column of its `last_runs` row. A
//! large one is a spill file in the session's directory beside the database,
//! made durable before the row that names it commits, and deleted after no
//! row names it; startup deletes whatever a crash left between the two.
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use sqlx::SqliteConnection;
use submilli_shared::secret_store::{KEY_ID_LEN, SecretCipher};

use crate::adapters::sealing::Sealer;
use crate::adapters::sql::{database, unix_ms};
use crate::application::error::StoreError;
use crate::application::sessions::ports::{LastRunStaging, LastRunStorage, StagedLastRun};
use crate::database::ServerDatabase;
use crate::domain::session::SessionId;

/// Above this many bytes of JSON, a last run is kept in a spill file rather
/// than in its row.
pub(crate) const INLINE_BYTES: usize = 64 * 1024;

/// The largest last run kept by default; a larger one keeps only its size.
pub const DEFAULT_MAX_LAST_RUN_BYTES: u64 = 64 * 1024 * 1024;

/// The largest last run an in-memory database keeps in a row, well inside
/// SQLite's own limit on a value.
const MAX_ROW_BYTES: u64 = 256 * 1024 * 1024;

/// A session's last run as a reader gets it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StoredLastRun {
    /// The JSON, exactly as it was recorded.
    Json(Vec<u8>),
    /// The run's JSON was `bytes` long, over the retained `limit`.
    TooLarge { bytes: u64, limit: u64 },
}

pub(crate) struct LastRuns {
    database: Arc<ServerDatabase>,
    cipher: Option<Arc<SecretCipher>>,
    sealer: Sealer,
    /// Where large last runs are kept; `None` for an in-memory database, whose
    /// last runs all stay in their rows.
    spill_root: Option<PathBuf>,
    max_bytes: u64,
}

/// A `last_runs` row.
#[derive(sqlx::FromRow)]
struct Row {
    storage: String,
    encoded_bytes: i64,
    payload: Option<Vec<u8>>,
    spill_file: Option<String>,
    key_id: Option<Vec<u8>>,
    limit_bytes: Option<i64>,
}

impl LastRuns {
    pub fn new(
        database: Arc<ServerDatabase>,
        cipher: Option<Arc<SecretCipher>>,
        max_bytes: u64,
    ) -> Self {
        let spill_root = (!database.is_ephemeral()).then(|| spill_root(database.path()));
        let max_bytes = match spill_root {
            Some(_) => max_bytes,
            None => max_bytes.min(MAX_ROW_BYTES),
        };
        Self {
            database,
            sealer: Sealer::new(cipher.clone()),
            cipher,
            spill_root,
            max_bytes,
        }
    }

    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// These last runs with `max_bytes` as the retained limit.
    pub fn with_max_bytes(&self, max_bytes: u64) -> Self {
        Self::new(self.database.clone(), self.cipher.clone(), max_bytes)
    }

    /// The session's last run, or `None` when it has none. A spill file its
    /// row names that is missing or does not open is an error, never `None`.
    pub async fn get(&self, session_id: &str) -> Result<Option<StoredLastRun>, StoreError> {
        // A run that ends while this reads may replace the file the row named
        // and delete it; the row read again names the new one.
        for _ in 0..3 {
            let Some(row) = self.row(session_id).await? else {
                return Ok(None);
            };
            match (row.storage.as_str(), row.payload, row.spill_file) {
                ("too_large", None, None) => {
                    return Ok(Some(StoredLastRun::TooLarge {
                        bytes: size(row.encoded_bytes)?,
                        limit: size(row.limit_bytes.ok_or_else(|| corrupt_row("no limit"))?)?,
                    }));
                }
                ("inline", Some(payload), None) => {
                    let json = self.open(&owner(session_id, None), payload, row.key_id)?;
                    return Ok(Some(StoredLastRun::Json(json)));
                }
                ("file", None, Some(name)) => {
                    let Some(blob) = self.read_spill_file(session_id, &name).await? else {
                        continue;
                    };
                    let json = self.open(&owner(session_id, Some(&name)), blob, row.key_id)?;
                    return Ok(Some(StoredLastRun::Json(json)));
                }
                _ => return Err(corrupt_row("its storage columns disagree")),
            }
        }
        Err(StoreError::Io(format!(
            "the last run of session {session_id} names a spill file that is missing"
        )))
    }

    async fn row(&self, session_id: &str) -> Result<Option<Row>, StoreError> {
        let id = session_id.to_owned();
        Ok(self
            .database
            .read(move |connection| {
                Box::pin(async move {
                    Ok(sqlx::query_as(
                        "SELECT storage, encoded_bytes, payload, spill_file, key_id, limit_bytes \
                         FROM last_runs WHERE session_id=?",
                    )
                    .bind(id)
                    .fetch_optional(connection)
                    .await?)
                })
            })
            .await?)
    }

    /// A spill file's contents, or `None` when it is gone.
    async fn read_spill_file(
        &self,
        session_id: &str,
        name: &str,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        let path = self.spill_path(session_id, name)?;
        match blocking(move || std::fs::read(path)).await {
            Ok(blob) => Ok(Some(blob)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(StoreError::Io(error.to_string())),
        }
    }

    fn seal(&self, owner: &str, json: Vec<u8>) -> Result<Vec<u8>, StoreError> {
        match self.sealer.key_id() {
            Some(_) => self.sealer.seal(owner, &json),
            None => Ok(json),
        }
    }

    fn stage_inline(
        &self,
        session_id: &str,
        encoded_bytes: u64,
        json: Vec<u8>,
    ) -> Result<StagedLastRun, StoreError> {
        Ok(StagedLastRun {
            encoded_bytes,
            storage: LastRunStorage::Inline {
                payload: self.seal(&owner(session_id, None), json)?,
                key_id: self.sealer.key_id(),
            },
        })
    }

    /// Seal `json` into a new spill file of the session under `root`,
    /// durably. Returns its name.
    async fn write_spill_file(
        &self,
        root: &Path,
        session_id: &str,
        json: Vec<u8>,
    ) -> Result<String, StoreError> {
        let name = uuid::Uuid::new_v4().to_string();
        let payload = self.seal(&owner(session_id, Some(&name)), json)?;
        let (root, dir) = (root.to_path_buf(), session_dir(root, session_id)?);
        let file = name.clone();
        blocking(move || write_durably(&root, &dir, &file, &payload))
            .await
            .map_err(|error| StoreError::Io(format!("writing the last run: {error}")))?;
        Ok(name)
    }

    async fn remove_spill_file(&self, session_id: &str, name: &str) -> std::io::Result<()> {
        let path = self
            .spill_path(session_id, name)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        blocking(move || std::fs::remove_file(path)).await
    }

    fn open(
        &self,
        owner: &str,
        blob: Vec<u8>,
        key_id: Option<Vec<u8>>,
    ) -> Result<Vec<u8>, StoreError> {
        let Some(key_id) = key_id else {
            return Ok(blob);
        };
        if Sealer::sealed_key_id(&blob) != Some(key_id.as_slice()) {
            return Err(StoreError::Credentials(
                "the last run is corrupt: its key id does not match its value".into(),
            ));
        }
        self.sealer.open(owner, &blob, "the last run")
    }

    fn spill_path(&self, session_id: &str, name: &str) -> Result<PathBuf, StoreError> {
        let root = self
            .spill_root
            .as_ref()
            .ok_or_else(|| corrupt_row("it names a spill file, but the database is in memory"))?;
        Ok(session_dir(root, session_id)?.join(name))
    }

    /// Delete a session's spill files once it has closed. Its row went with
    /// the transaction that closed it, so a file left here is unnamed and
    /// startup deletes it.
    pub async fn remove_session_files(&self, session_id: &str) {
        let Some(root) = &self.spill_root else {
            return;
        };
        let dir = match session_dir(root, session_id) {
            Ok(dir) => dir,
            Err(error) => return warn_left_in_place(root, &error.to_string()),
        };
        let removed = blocking({
            let dir = dir.clone();
            move || std::fs::remove_dir_all(dir)
        })
        .await;
        if let Err(error) = removed
            && error.kind() != std::io::ErrorKind::NotFound
        {
            warn_left_in_place(&dir, &error.to_string());
        }
    }

    /// Delete every spill file no row names: one a crash left before its row
    /// committed, or after its row moved on but before the file went. Runs
    /// before the server takes requests, so no run is writing one. A file that
    /// cannot be deleted is reported and left.
    pub async fn sweep(&self) -> Result<(), StoreError> {
        let Some(root) = self.spill_root.clone() else {
            return Ok(());
        };
        let named: Vec<(String, String)> = self
            .database
            .read(|connection| {
                Box::pin(async move {
                    Ok(sqlx::query_as(
                        "SELECT session_id, spill_file FROM last_runs WHERE storage='file'",
                    )
                    .fetch_all(connection)
                    .await?)
                })
            })
            .await?;
        let named: HashSet<(String, String)> = named.into_iter().collect();
        tokio::task::spawn_blocking(move || sweep_unnamed_files(&root, &named))
            .await
            .map_err(|error| StoreError::Io(error.to_string()))
    }
}

#[async_trait::async_trait]
impl LastRunStaging for LastRuns {
    async fn stage(&self, session_id: &str, json: Vec<u8>) -> Result<StagedLastRun, StoreError> {
        let encoded_bytes = json.len() as u64;
        if encoded_bytes > self.max_bytes {
            return Ok(StagedLastRun {
                encoded_bytes,
                storage: LastRunStorage::TooLarge {
                    limit: self.max_bytes,
                },
            });
        }
        let root = match &self.spill_root {
            Some(root) if json.len() > INLINE_BYTES => root,
            _ => return self.stage_inline(session_id, encoded_bytes, json),
        };
        let name = self.write_spill_file(root, session_id, json).await?;
        Ok(StagedLastRun {
            encoded_bytes,
            storage: LastRunStorage::SpillFile {
                name,
                key_id: self.sealer.key_id(),
            },
        })
    }

    async fn spill_file(&self, session_id: &str) -> Result<Option<String>, StoreError> {
        let id = session_id.to_owned();
        self.database
            .read(move |connection| Box::pin(async move { Ok(spill_file(connection, &id).await?) }))
            .await
            .map_err(Into::into)
    }

    async fn release(&self, session_id: &str, files: Vec<String>) {
        if files.is_empty() {
            return;
        }
        // A name, once no row names it, is never named again: each is new.
        let named = match self.spill_file(session_id).await {
            Ok(named) => named,
            Err(error) => {
                tracing::warn!(session = session_id, %error, "last-run file left in place");
                return;
            }
        };
        for file in files
            .into_iter()
            .filter(|file| Some(file) != named.as_ref())
        {
            if let Err(error) = self.remove_spill_file(session_id, &file).await
                && error.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(session = session_id, %error, "last-run file left in place");
            }
        }
    }
}

/// Record a session's last run, replacing the one before. Returns the spill
/// file the one before was kept in, to delete once this commits.
pub(crate) async fn put(
    connection: &mut SqliteConnection,
    session_id: &str,
    run_id: &str,
    completed_at: SystemTime,
    staged: &StagedLastRun,
) -> Result<Option<String>, StoreError> {
    let replaced = spill_file(&mut *connection, session_id)
        .await
        .map_err(database)?;
    let (storage, payload, spill_file, key_id, limit) = match &staged.storage {
        LastRunStorage::Inline { payload, key_id } => (
            "inline",
            Some(payload.as_slice()),
            None,
            key_id.as_ref(),
            None,
        ),
        LastRunStorage::SpillFile { name, key_id } => {
            ("file", None, Some(name.as_str()), key_id.as_ref(), None)
        }
        LastRunStorage::TooLarge { limit } => {
            ("too_large", None, None, None, Some(sql_size(*limit)?))
        }
    };
    sqlx::query(
        "INSERT INTO last_runs(session_id,run_id,completed_at_unix_ms,storage,encoded_bytes,\
         payload,spill_file,key_id,limit_bytes) VALUES (?,?,?,?,?,?,?,?,?) \
         ON CONFLICT(session_id) DO UPDATE SET run_id=excluded.run_id,\
         completed_at_unix_ms=excluded.completed_at_unix_ms,storage=excluded.storage,\
         encoded_bytes=excluded.encoded_bytes,payload=excluded.payload,\
         spill_file=excluded.spill_file,key_id=excluded.key_id,limit_bytes=excluded.limit_bytes",
    )
    .bind(session_id)
    .bind(run_id)
    .bind(unix_ms(completed_at)?)
    .bind(storage)
    .bind(sql_size(staged.encoded_bytes)?)
    .bind(payload)
    .bind(spill_file)
    .bind(key_id.map(<[u8; KEY_ID_LEN]>::as_slice))
    .bind(limit)
    .execute(connection)
    .await
    .map_err(database)?;
    Ok(replaced)
}

/// Forget a session's last run. Returns the spill file it was kept in, to
/// delete once this commits.
pub(crate) async fn remove(
    connection: &mut SqliteConnection,
    session_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let removed: Option<Option<String>> =
        sqlx::query_scalar("DELETE FROM last_runs WHERE session_id=? RETURNING spill_file")
            .bind(session_id)
            .fetch_optional(connection)
            .await?;
    Ok(removed.flatten())
}

/// The spill file a session's last run is kept in, if it is kept in one.
async fn spill_file(
    connection: &mut SqliteConnection,
    session_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    let named: Option<Option<String>> =
        sqlx::query_scalar("SELECT spill_file FROM last_runs WHERE session_id=?")
            .bind(session_id)
            .fetch_optional(connection)
            .await?;
    Ok(named.flatten())
}

/// Where the last runs of the database at `database` are kept: beside it, as
/// its `-wal` is, so they share its disk and its backups.
fn spill_root(database: &Path) -> PathBuf {
    let mut root = database.as_os_str().to_owned();
    root.push("-last-runs");
    PathBuf::from(root)
}

fn session_dir(root: &Path, session_id: &str) -> Result<PathBuf, StoreError> {
    SessionId::parse(session_id.to_owned())
        .map_err(|error| StoreError::Io(format!("invalid session id: {error}")))?;
    Ok(root.join(session_id))
}

/// The sealed JSON's owner: its session, and the spill file it is kept in, so
/// a value moved to another session or file does not open.
fn owner(session_id: &str, spill_file: Option<&str>) -> String {
    match spill_file {
        None => format!("submilli:last-run:v1:{session_id}"),
        Some(file) => format!("submilli:last-run:v1:{session_id}:{file}"),
    }
}

fn size(bytes: i64) -> Result<u64, StoreError> {
    u64::try_from(bytes).map_err(|_| corrupt_row("a size is negative"))
}

fn sql_size(bytes: u64) -> Result<i64, StoreError> {
    i64::try_from(bytes).map_err(|_| StoreError::Io("size overflow".into()))
}

fn corrupt_row(reason: &str) -> StoreError {
    StoreError::Io(format!("a last-run record is corrupt: {reason}"))
}

/// Write `payload` to `dir/name` so that, once this returns, the file is there
/// whole after a crash: written under a temporary name, synced, renamed, and
/// the rename synced, with the directories above it that this or a concurrent
/// write may have just created.
fn write_durably(root: &Path, dir: &Path, name: &str, payload: &[u8]) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)?;
    let staging = dir.join(format!("{name}.tmp"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = (|| {
        let mut file = options.open(&staging)?;
        file.write_all(payload)?;
        file.sync_all()?;
        std::fs::rename(&staging, dir.join(name))
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_file(&staging);
        return Err(error);
    }
    sync_directory(dir)?;
    sync_directory(root)?;
    match root.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => sync_directory(parent),
        _ => Ok(()),
    }
}

/// Run blocking file work off the async workers.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> std::io::Result<T> + Send + 'static,
) -> std::io::Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|error| Err(std::io::Error::other(error)))
}

/// Make the entries of `dir` durable. Windows offers no directory handle to
/// sync; its rename is as durable as the file system makes it.
fn sync_directory(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    std::fs::File::open(dir)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

/// Delete every file under `root` that `named` does not hold, and the session
/// directories that leaves empty.
fn sweep_unnamed_files(root: &Path, named: &HashSet<(String, String)>) {
    let sessions = match std::fs::read_dir(root) {
        Ok(sessions) => sessions,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => return warn_left_in_place(root, &error.to_string()),
    };
    for session in sessions.flatten() {
        if !session.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let session_id = session.file_name().to_string_lossy().into_owned();
        let files = match std::fs::read_dir(session.path()) {
            Ok(files) => files,
            Err(error) => {
                warn_left_in_place(&session.path(), &error.to_string());
                continue;
            }
        };
        for file in files.flatten() {
            let name = file.file_name().to_string_lossy().into_owned();
            if file.file_type().is_ok_and(|kind| kind.is_file())
                && !named.contains(&(session_id.clone(), name))
                && let Err(error) = std::fs::remove_file(file.path())
            {
                warn_left_in_place(&file.path(), &error.to_string());
            }
        }
        // Only an empty directory goes; one a file is still named in stays.
        let _ = std::fs::remove_dir(session.path());
    }
}

fn warn_left_in_place(path: &Path, error: &str) {
    tracing::warn!(path = %path.display(), error, "last-run file left in place");
}

#[cfg(test)]
impl StoredLastRun {
    /// The recorded JSON, parsed.
    pub fn value(&self) -> serde_json::Value {
        match self {
            Self::Json(json) => serde_json::from_slice(json).expect("a last run is JSON"),
            Self::TooLarge { .. } => panic!("the last run kept only its size"),
        }
    }
}

#[cfg(test)]
mod tests;
