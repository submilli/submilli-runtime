use super::*;
use crate::database::{DatabaseError, ServerDatabase};

mod mcp;
pub(crate) mod records;

pub struct SqliteSessionStore {
    database: Arc<ServerDatabase>,
    source: Option<PathBuf>,
    workspace_root: PathBuf,
}

impl SqliteSessionStore {
    pub fn new(
        database: Arc<ServerDatabase>,
        source: Option<PathBuf>,
        workspace_root: PathBuf,
    ) -> Self {
        Self {
            database,
            source,
            workspace_root,
        }
    }
}

#[async_trait::async_trait]
impl DurableSessionStore for SqliteSessionStore {
    fn database(&self) -> Option<Arc<ServerDatabase>> {
        Some(self.database.clone())
    }

    fn durable(&self) -> bool {
        true
    }

    async fn initialize(&self) -> Result<(), StoreError> {
        let Some(source) = self.source.clone() else {
            return Ok(());
        };
        let root = self.workspace_root.clone();
        let database_path = self.database.path().to_path_buf();
        let import_source = source.clone();
        let files = self
            .database
            .transaction(move |connection| {
                Box::pin(async move {
                    validate_archive_paths(&import_source, &database_path, &root)?;
                    import_files(connection, &import_source, &root).await
                })
            })
            .await?;
        let database_path = self.database.path().to_path_buf();
        let workspace_root = self.workspace_root.clone();
        self.database
            .read(move |_| {
                Box::pin(async move {
                    archive_directory(&source, files, &database_path, &workspace_root)
                })
            })
            .await?;
        Ok(())
    }

    async fn now(&self) -> Result<SystemTime, StoreError> {
        let millis: i64 = self
            .database
            .read(|connection| {
                Box::pin(async move {
                    Ok(
                        sqlx::query_scalar("SELECT CAST(unixepoch('subsec') * 1000 AS INTEGER)")
                            .fetch_one(connection)
                            .await?,
                    )
                })
            })
            .await?;
        let millis =
            u64::try_from(millis).map_err(|_| StoreError::Io("invalid database clock".into()))?;
        UNIX_EPOCH
            .checked_add(Duration::from_millis(millis))
            .ok_or_else(|| StoreError::Io("database clock overflow".into()))
    }

    async fn put(&self, record: SessionRecord) -> Result<(), StoreError> {
        if record.ephemeral_bindings.is_some() {
            return Err(StoreError::Io(
                "durable session bindings must be encrypted".into(),
            ));
        }
        self.database
            .transaction(move |connection| {
                Box::pin(async move { write_record(connection, record).await })
            })
            .await?;
        Ok(())
    }

    async fn load(&self, session_id: &str) -> Result<Option<SessionRecord>, StoreError> {
        let id = session_id.to_owned();
        self.database
            .read(move |connection| Box::pin(async move { records::load(connection, &id).await }))
            .await
            .map_err(Into::into)
    }

    async fn remove(&self, session_id: &str) -> Result<(), StoreError> {
        if let Some(mut record) = self.load(session_id).await?
            && record.status == SessionStatus::Active
        {
            record = crate::adapters::session::repository::close_record(
                record,
                ClosedReason::Deleted,
                &self.workspace_root,
            )?;
            self.put(record).await?;
        }
        Ok(())
    }

    async fn schedule_cleanup(&self, task: SessionCleanup) -> Result<(), StoreError> {
        self.database
            .transaction(move |connection| {
                Box::pin(async move { queue_cleanup(connection, &task).await })
            })
            .await?;
        Ok(())
    }
    async fn active_ids(&self) -> Result<Vec<String>, StoreError> {
        self.database
            .read(|connection| {
                Box::pin(async move {
                    Ok(
                        sqlx::query_scalar("SELECT session_id FROM sessions WHERE status='active'")
                            .fetch_all(connection)
                            .await?,
                    )
                })
            })
            .await
            .map_err(Into::into)
    }
    async fn cleanup_task(&self, id: &str) -> Result<Option<SessionCleanup>, StoreError> {
        let id = id.to_owned();
        self.database
            .read(move |connection| {
                Box::pin(async move {
                    let row: Option<(String, Option<String>)> = sqlx::query_as(
                        "SELECT session_id,folder_path FROM session_cleanup WHERE session_id=?",
                    )
                    .bind(id)
                    .fetch_optional(connection)
                    .await?;
                    Ok(row.map(|(session_id, folder)| SessionCleanup {
                        session_id,
                        folder: folder.map(PathBuf::from),
                    }))
                })
            })
            .await
            .map_err(Into::into)
    }
    async fn pending_cleanup(&self) -> Result<Vec<SessionCleanup>, StoreError> {
        self.database
            .read(|connection| {
                Box::pin(async move {
                    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
                        "SELECT session_id,folder_path FROM session_cleanup ORDER BY session_id",
                    )
                    .fetch_all(connection)
                    .await?;
                    Ok(rows
                        .into_iter()
                        .map(|(session_id, folder)| SessionCleanup {
                            session_id,
                            folder: folder.map(PathBuf::from),
                        })
                        .collect())
                })
            })
            .await
            .map_err(Into::into)
    }
    async fn complete_cleanup(&self, id: &str) -> Result<(), StoreError> {
        let id = id.to_owned();
        self.database
            .transaction(move |connection| {
                Box::pin(async move {
                    sqlx::query("DELETE FROM session_cleanup WHERE session_id=?")
                        .bind(id)
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .await?;
        Ok(())
    }

    async fn active_count(&self) -> Result<usize, StoreError> {
        self.database
            .read(|connection| {
                Box::pin(async move {
                    let count: i64 =
                        sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE status='active'")
                            .fetch_one(connection)
                            .await?;
                    usize::try_from(count)
                        .map_err(|_| DatabaseError::Import("invalid active session count".into()))
                })
            })
            .await
            .map_err(Into::into)
    }
    async fn for_blueprint(&self, name: &str) -> Result<Vec<SessionRecord>, StoreError> {
        let name = name.to_owned();
        self.database
            .read(move |connection| {
                Box::pin(async move { records::for_blueprint(connection, &name).await })
            })
            .await
            .map_err(Into::into)
    }
    async fn expiry_candidates(&self, now: SystemTime) -> Result<Vec<SessionRecord>, StoreError> {
        let now = i64::try_from(
            now.duration_since(UNIX_EPOCH)
                .map_err(|_| StoreError::Io("invalid expiry clock".into()))?
                .as_millis(),
        )
        .map_err(|_| StoreError::Io("expiry clock overflow".into()))?;
        self.database
            .read(move |connection| {
                Box::pin(async move { records::expiry_candidates(connection, now).await })
            })
            .await
            .map_err(Into::into)
    }

    async fn load_all(&self) -> Result<Vec<SessionRecord>, StoreError> {
        self.database
            .read(|connection| Box::pin(async move { records::load_all(connection).await }))
            .await
            .map_err(Into::into)
    }
}

async fn queue_cleanup(
    connection: &mut sqlx::SqliteConnection,
    task: &SessionCleanup,
) -> Result<(), DatabaseError> {
    let folder = task
        .folder
        .as_ref()
        .map(|path| {
            path.to_str()
                .ok_or_else(|| DatabaseError::Import("cleanup path is not UTF-8".into()))
        })
        .transpose()?;
    sqlx::query("INSERT INTO session_cleanup(session_id,folder_path) VALUES (?,?) ON CONFLICT(session_id) DO UPDATE SET folder_path=COALESCE(excluded.folder_path,session_cleanup.folder_path)")
        .bind(&task.session_id).bind(folder).execute(connection).await?;
    Ok(())
}

pub(crate) async fn write_record(
    connection: &mut sqlx::SqliteConnection,
    mut record: SessionRecord,
) -> Result<(), DatabaseError> {
    record
        .validate_lifecycle()
        .map_err(|error| DatabaseError::Import(error.to_string()))?;
    if record.status == SessionStatus::Active {
        let blueprint_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM blueprints WHERE name=?)")
                .bind(&record.blueprint_name)
                .fetch_one(&mut *connection)
                .await?;
        if !blueprint_exists {
            return Err(DatabaseError::SessionConflict);
        }
    }
    let previous: Option<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT record_version, status, closed_reason FROM sessions WHERE session_id=?",
    )
    .bind(&record.session_id)
    .fetch_optional(&mut *connection)
    .await?;
    if previous.as_ref().map_or(0, |(revision, _, _)| *revision) != record.revision
        || previous.as_ref().is_some_and(|(_, status, reason)| {
            status == "closed"
                && (record.status == SessionStatus::Active
                    || reason.as_deref() != record.closed_reason.and_then(ClosedReason::as_storage))
        })
    {
        return Err(DatabaseError::SessionConflict);
    }
    if previous.is_some() && record.status == SessionStatus::Active {
        let (last_activity, idle_timeout): (i64, i64) = sqlx::query_as(
            "SELECT last_activity_unix_ms,idle_timeout_ms FROM sessions WHERE session_id=?",
        )
        .bind(&record.session_id)
        .fetch_one(&mut *connection)
        .await?;
        let now: i64 = sqlx::query_scalar("SELECT CAST(unixepoch('subsec') * 1000 AS INTEGER)")
            .fetch_one(&mut *connection)
            .await?;
        let lifetime = crate::domain::session::SessionLifetime::new(
            Duration::from_millis(records::millis(idle_timeout)?),
            records::timestamp(last_activity)?,
        );
        if lifetime.expired_at(records::timestamp(now)?) {
            return Err(DatabaseError::SessionConflict);
        }
    }
    record.revision = record
        .revision
        .checked_add(1)
        .ok_or_else(|| DatabaseError::Import("session revision exhausted".into()))?;
    records::write(connection, &record).await
}

async fn import_files(
    connection: &mut sqlx::SqliteConnection,
    source: &Path,
    root: &Path,
) -> Result<Vec<(PathBuf, Vec<u8>)>, DatabaseError> {
    if let Ok(metadata) = fs::symlink_metadata(source)
        && metadata.file_type().is_symlink()
    {
        return Err(DatabaseError::Import(
            "session import does not follow source symlinks".into(),
        ));
    }
    let entries = match fs::read_dir(source) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(import_io(error)),
    };
    let ImportedCleanup {
        mut files,
        session_ids: cleanup_ids,
    } = import_cleanup_files(connection, source, root).await?;
    for entry in entries {
        let path = entry.map_err(import_io)?.path();
        if !is_record_file(&path) {
            continue;
        }
        if fs::symlink_metadata(&path)
            .map_err(import_io)?
            .file_type()
            .is_symlink()
        {
            return Err(DatabaseError::Import(
                "session import does not follow symlinks".into(),
            ));
        }
        let bytes = fs::read(&path).map_err(import_io)?;
        let stored: StoredRecord = serde_json::from_slice(&bytes).map_err(|_| {
            DatabaseError::Import(format!("invalid session JSON in {}", path.display()))
        })?;
        let mut record = stored
            .into_record()
            .map_err(|_| DatabaseError::Import("invalid imported timestamp".into()))?;
        crate::domain::session::SessionId::parse(record.session_id.clone())
            .map_err(|error| DatabaseError::Import(error.to_string()))?;
        record.cleanup_pending |= cleanup_ids.contains(&record.session_id);
        record.revision = 1;
        record.root_vfs_path = match record.root_vfs_type {
            RootVfsType::PerSession => Some(root.join(&record.session_id)),
            RootVfsType::Named => record.root_vfs_path,
            RootVfsType::None | RootVfsType::Ephemeral => None,
        };
        if let Some(existing) = records::load(connection, &record.session_id).await? {
            // Startup does not admit requests until archival succeeds. An exact
            // committed record proves an interrupted import can safely finish.
            if serde_json::to_value(StoredRecord::from_record(&existing))
                .map_err(|e| DatabaseError::Import(e.to_string()))?
                != serde_json::to_value(StoredRecord::from_record(&record))
                    .map_err(|e| DatabaseError::Import(e.to_string()))?
            {
                return Err(DatabaseError::Import(
                    "session import conflicts with database state; source directory was not archived".into()));
            }
        } else {
            record.revision = 0;
            write_record(connection, record).await?;
        }
        files.push((path, bytes));
    }
    Ok(files)
}

#[derive(Default)]
struct ImportedCleanup {
    files: Vec<(PathBuf, Vec<u8>)>,
    session_ids: std::collections::HashSet<String>,
}

async fn import_cleanup_files(
    connection: &mut sqlx::SqliteConnection,
    source: &Path,
    root: &Path,
) -> Result<ImportedCleanup, DatabaseError> {
    let directory = source.join("cleanup");
    if let Ok(metadata) = fs::symlink_metadata(&directory)
        && metadata.file_type().is_symlink()
    {
        return Err(DatabaseError::Import(
            "cleanup import does not follow symlinks".into(),
        ));
    }
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ImportedCleanup::default());
        }
        Err(error) => return Err(import_io(error)),
    };
    let mut files = Vec::new();
    let mut ids = std::collections::HashSet::new();
    for entry in entries {
        let path = entry.map_err(import_io)?.path();
        if !is_record_file(&path) {
            continue;
        }
        if fs::symlink_metadata(&path)
            .map_err(import_io)?
            .file_type()
            .is_symlink()
        {
            return Err(DatabaseError::Import(
                "cleanup import does not follow symlinks".into(),
            ));
        }
        let bytes = fs::read(&path).map_err(import_io)?;
        let mut task: SessionCleanup = serde_json::from_slice(&bytes)
            .map_err(|error| DatabaseError::Import(error.to_string()))?;
        crate::domain::session::SessionId::parse(task.session_id.clone())
            .map_err(|error| DatabaseError::Import(error.to_string()))?;
        if task.folder.is_some() {
            task.folder = Some(root.join(&task.session_id));
        }
        ids.insert(task.session_id.clone());
        queue_cleanup(connection, &task).await?;
        files.push((path, bytes));
    }
    Ok(ImportedCleanup {
        files,
        session_ids: ids,
    })
}

fn archive_directory(
    source: &Path,
    files: Vec<(PathBuf, Vec<u8>)>,
    database_path: &Path,
    workspace_root: &Path,
) -> Result<(), DatabaseError> {
    let parent = source
        .parent()
        .ok_or_else(|| DatabaseError::Import("invalid session source directory".into()))?;
    let archive = parent.join("archive");
    let destination = archive.join("sessions");
    if !files.is_empty() {
        validate_archive_paths(source, database_path, workspace_root)?;
        for (path, expected) in files {
            if fs::read(&path).map_err(import_io)? != expected {
                return Err(DatabaseError::Import(
                    "session source changed before archive".into(),
                ));
            }
        }
        fs::create_dir_all(&archive).map_err(import_io)?;
        crate::import_archive::move_directory(source, &destination).map_err(import_io)?;
    }
    if destination.try_exists().map_err(import_io)? {
        sync_directory_ancestry(&destination)?;
    }
    if parent.try_exists().map_err(import_io)? {
        sync_directory_ancestry(parent)?;
    }
    restore_idempotency_ledger(source, &destination)?;
    if source.try_exists().map_err(import_io)? {
        sync_directory_ancestry(source)?;
    }
    Ok(())
}

fn restore_idempotency_ledger(source: &Path, archive: &Path) -> Result<(), DatabaseError> {
    let archived_ledger = archive.join("idempotency");
    match fs::symlink_metadata(&archived_ledger) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => {
            return Err(DatabaseError::Import(
                "invalid archived idempotency directory".into(),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(import_io(error)),
    }
    fs::create_dir_all(source).map_err(import_io)?;
    let active_ledger = source.join("idempotency");
    // App construction may create an empty ledger root before import. Never
    // replace nonempty state if a prior restore or another process wrote it.
    match fs::remove_dir(&active_ledger) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(import_io(error)),
    }
    crate::import_archive::move_directory(&archived_ledger, &active_ledger).map_err(import_io)?;
    sync_directory_ancestry(archive)?;
    sync_directory_ancestry(&active_ledger)
}

fn validate_archive_paths(
    source: &Path,
    database: &Path,
    workspace: &Path,
) -> Result<(), DatabaseError> {
    let source = resolved_path(source)?;
    if resolved_path(database)?.starts_with(&source)
        || resolved_path(workspace)?.starts_with(&source)
    {
        return Err(DatabaseError::Import(
            "session source contains active database or workspace storage".into(),
        ));
    }
    Ok(())
}

// Resolve each existing component so relative paths, parent components, and
// symlink ancestors cannot hide active storage beneath the import directory.
fn resolved_path(path: &Path) -> Result<PathBuf, DatabaseError> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_err(import_io)?.join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                resolved.pop();
            }
            component => {
                resolved.push(component.as_os_str());
                match fs::canonicalize(&resolved) {
                    Ok(canonical) => resolved = canonical,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(import_io(error)),
                }
            }
        }
    }
    Ok(resolved)
}

fn sync_directory_ancestry(path: &Path) -> Result<(), DatabaseError> {
    let absolute = fs::canonicalize(path).map_err(import_io)?;
    let mut current = Some(absolute.as_path());
    while let Some(directory) = current {
        File::open(directory)
            .and_then(|file| file.sync_all())
            .map_err(import_io)?;
        current = directory
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
    }
    Ok(())
}

fn import_io(error: io::Error) -> DatabaseError {
    DatabaseError::Import(format!("session import I/O: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn database(root: &Path) -> Arc<ServerDatabase> {
        let database = Arc::new(ServerDatabase::open(&root.join("server.db")).await.unwrap());
        database
            .transaction(|connection| {
                Box::pin(async move {
                    sqlx::query("INSERT INTO blueprint_revisions VALUES ('test',1,'name: test')")
                        .execute(&mut *connection)
                        .await?;
                    sqlx::query("INSERT INTO blueprints VALUES ('test',1)")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .await
            .unwrap();
        database
    }

    fn record(id: &str) -> SessionRecord {
        SessionRecord {
            session_id: id.into(),
            blueprint_name: "test".into(),
            idle_timeout: Duration::from_secs(3600),
            last_activity: SystemTime::now(),
            ..SessionRecord::default()
        }
    }

    #[tokio::test]
    async fn root_vfs_types_and_named_paths_survive_import_and_restart() {
        let root = tempfile::tempdir().unwrap();
        let database = database(root.path()).await;
        let source = root.path().join("sessions");
        fs::create_dir_all(&source).unwrap();
        let named_path = root.path().join("volume/users/ada");
        let mut named = record("named");
        named.root_vfs_type = RootVfsType::Named;
        named.root_vfs_path = Some(named_path.clone());
        fs::write(
            source.join("named.json"),
            serde_json::to_vec(&StoredRecord::from_record(&named)).unwrap(),
        )
        .unwrap();
        let store = SqliteSessionStore::new(
            database.clone(),
            Some(source),
            root.path().join("workspaces"),
        );
        store.initialize().await.unwrap();
        for (id, mode) in [
            ("none", RootVfsType::None),
            ("ephemeral", RootVfsType::Ephemeral),
            ("owned", RootVfsType::PerSession),
        ] {
            let mut entry = record(id);
            entry.root_vfs_type = mode;
            store.put(entry).await.unwrap();
        }
        database.close().await.unwrap();
        drop(store);
        drop(database);
        let database = Arc::new(
            ServerDatabase::open(&root.path().join("server.db"))
                .await
                .unwrap(),
        );
        let store = SqliteSessionStore::new(database.clone(), None, root.path().join("workspaces"));
        let loaded = store.load("named").await.unwrap().unwrap();
        assert_eq!(loaded.root_vfs_type, RootVfsType::Named);
        assert_eq!(loaded.root_vfs_path, Some(named_path));
        for (id, mode) in [
            ("none", RootVfsType::None),
            ("ephemeral", RootVfsType::Ephemeral),
            ("owned", RootVfsType::PerSession),
        ] {
            assert_eq!(store.load(id).await.unwrap().unwrap().root_vfs_type, mode);
        }
        database.close().await.unwrap();
    }

    #[tokio::test]
    async fn orphan_cleanup_import_survives_restart_without_a_session_row() {
        let root = tempfile::tempdir().unwrap();
        let database = database(root.path()).await;
        let source = root.path().join("sessions");
        let folder = root.path().join("workspaces/orphan");
        fs::create_dir_all(&folder).unwrap();
        let legacy = FileDurableSessionStore::new(source.clone()).unwrap();
        legacy
            .schedule_cleanup(SessionCleanup {
                session_id: "orphan".into(),
                folder: Some(folder.clone()),
            })
            .await
            .unwrap();
        let store = Arc::new(SqliteSessionStore::new(
            database.clone(),
            Some(source.clone()),
            root.path().join("workspaces"),
        ));
        store.initialize().await.unwrap();
        assert!(store.load("orphan").await.unwrap().is_none());
        assert_eq!(store.pending_cleanup().await.unwrap().len(), 1);
        drop(store);
        database.close().await.unwrap();
        drop(database);
        let database = Arc::new(
            ServerDatabase::open(&root.path().join("server.db"))
                .await
                .unwrap(),
        );
        let store = Arc::new(SqliteSessionStore::new(
            database.clone(),
            Some(source),
            root.path().join("workspaces"),
        ));
        manager(root.path(), store.clone()).boot().await.unwrap();
        assert!(!folder.exists());
        assert!(store.load("orphan").await.unwrap().is_none());
        assert!(store.pending_cleanup().await.unwrap().is_empty());
        database.close().await.unwrap();
    }

    #[tokio::test]
    async fn explicit_named_root_requires_a_path() {
        let root = tempfile::tempdir().unwrap();
        let database = database(root.path()).await;
        let store = SqliteSessionStore::new(database.clone(), None, root.path().join("workspaces"));
        let mut invalid = record("invalid");
        invalid.root_vfs_type = RootVfsType::Named;
        assert!(
            crate::adapters::session::repository::restore_domain(&invalid, root.path()).is_err()
        );
        assert!(store.put(invalid).await.is_err());
        database.close().await.unwrap();
    }

    #[tokio::test]
    async fn revisions_reject_stale_writes_and_retired_resurrection() {
        let root = tempfile::tempdir().unwrap();
        let database = database(root.path()).await;
        let store = SqliteSessionStore::new(database.clone(), None, root.path().join("workspaces"));
        store.put(record("one")).await.unwrap();
        let old = store.load("one").await.unwrap().unwrap();
        let mut current = old.clone();
        current.variables.insert("tenant".into(), "new".into());
        store.put(current).await.unwrap();
        assert!(store.put(old).await.is_err());
        let current = store.load("one").await.unwrap().unwrap();
        store.remove("one").await.unwrap();
        assert!(store.put(current).await.is_err());
        assert!(store.load("one").await.unwrap().unwrap().status == SessionStatus::Closed);
        database.close().await.unwrap();
    }

    #[tokio::test]
    async fn closure_status_and_reason_survive_cleanup_and_restart() {
        let root = tempfile::tempdir().unwrap();
        let database = database(root.path()).await;
        let store = Arc::new(SqliteSessionStore::new(
            database.clone(),
            None,
            root.path().join("workspaces"),
        ));
        let sessions = manager(root.path(), store.clone());
        store.put(record("deleted")).await.unwrap();
        let mut expiring = record("expired");
        expiring.idle_timeout = Duration::from_secs(60);
        expiring.last_activity -= Duration::from_secs(120);
        store.put(expiring).await.unwrap();
        sessions.wipe_now("deleted").await.unwrap();
        sessions.reap_now().await.unwrap();
        for (id, reason) in [
            ("deleted", ClosedReason::Deleted),
            ("expired", ClosedReason::Expired),
        ] {
            let loaded = store.load(id).await.unwrap().unwrap();
            assert_eq!(loaded.status, SessionStatus::Closed);
            assert_eq!(loaded.closed_reason, Some(reason));
            assert!(!loaded.cleanup_pending);
        }
        drop(sessions);
        drop(store);
        database.close().await.unwrap();
        drop(database);
        let database = Arc::new(
            ServerDatabase::open(&root.path().join("server.db"))
                .await
                .unwrap(),
        );
        let store = SqliteSessionStore::new(database.clone(), None, root.path().join("workspaces"));
        assert_eq!(
            store.load("deleted").await.unwrap().unwrap().closed_reason,
            Some(ClosedReason::Deleted)
        );
        let expired = store.load("expired").await.unwrap().unwrap();
        store.remove("expired").await.unwrap();
        let after_removal = store.load("expired").await.unwrap().unwrap();
        assert_eq!(after_removal.revision, expired.revision);
        assert_eq!(after_removal.closed_reason, Some(ClosedReason::Expired));
        let mut changed_reason = after_removal;
        changed_reason.closed_reason = Some(ClosedReason::Deleted);
        assert!(store.put(changed_reason).await.is_err());
        database.read(|connection| Box::pin(async move {
            assert!(sqlx::query("UPDATE sessions SET status='completed' WHERE session_id='deleted'").execute(&mut *connection).await.is_err());
            assert!(sqlx::query("UPDATE sessions SET status='active',closed_reason='expired' WHERE session_id='expired'").execute(&mut *connection).await.is_err());
            Ok(())
        })).await.unwrap();
        database.close().await.unwrap();
    }

    #[tokio::test]
    async fn independent_stores_share_committed_bindings() {
        let root = tempfile::tempdir().unwrap();
        let database = database(root.path()).await;
        let first = SqliteSessionStore::new(database.clone(), None, root.path().join("workspaces"));
        let second =
            SqliteSessionStore::new(database.clone(), None, root.path().join("workspaces"));
        first.put(record("one")).await.unwrap();
        let mut updated = second.load("one").await.unwrap().unwrap();
        updated.variables.insert("tenant".into(), "two".into());
        second.put(updated).await.unwrap();
        assert_eq!(
            first.load("one").await.unwrap().unwrap().variables["tenant"],
            "two"
        );
        database.close().await.unwrap();
    }

    #[tokio::test]
    async fn interrupted_archive_with_separate_cleanup_task_can_retry() {
        let root = tempfile::tempdir().unwrap();
        let database = database(root.path()).await;
        let source = root.path().join("sessions");
        let legacy = FileDurableSessionStore::new(source.clone()).unwrap();
        let mut closed = record("one");
        closed.status = SessionStatus::Closed;
        closed.closed_reason = Some(ClosedReason::Deleted);
        legacy.put(closed).await.unwrap();
        legacy
            .schedule_cleanup(SessionCleanup {
                session_id: "one".into(),
                folder: Some(root.path().join("workspaces/one")),
            })
            .await
            .unwrap();
        fs::write(root.path().join("archive"), b"blocked").unwrap();
        let store = SqliteSessionStore::new(
            database.clone(),
            Some(source.clone()),
            root.path().join("workspaces"),
        );
        assert!(store.initialize().await.is_err());
        assert!(store.load("one").await.unwrap().unwrap().cleanup_pending);
        fs::remove_file(root.path().join("archive")).unwrap();
        store.initialize().await.unwrap();
        assert!(root.path().join("archive/sessions/cleanup").is_dir());
        assert_eq!(store.pending_cleanup().await.unwrap().len(), 1);
        database.close().await.unwrap();
    }

    #[tokio::test]
    async fn interrupted_archive_retries_the_directory_move() {
        let root = tempfile::tempdir().unwrap();
        let database = database(root.path()).await;
        let source = root.path().join("sessions");
        fs::create_dir_all(&source).unwrap();
        let path = source.join("6f6e65.json");
        let original = serde_json::to_vec(&StoredRecord::from_record(&record("one"))).unwrap();
        fs::write(&path, &original).unwrap();
        fs::create_dir_all(source.join("idempotency")).unwrap();
        fs::write(source.join("idempotency/entry"), b"ledger").unwrap();
        // The import commits, but creating the archive directory fails.
        fs::write(root.path().join("archive"), b"blocked").unwrap();
        let store = SqliteSessionStore::new(
            database.clone(),
            Some(source.clone()),
            root.path().join("workspaces"),
        );
        assert!(store.initialize().await.is_err());
        assert!(path.exists());
        assert!(store.load("one").await.unwrap().is_some());
        fs::remove_file(root.path().join("archive")).unwrap();
        store.initialize().await.unwrap();
        store.initialize().await.unwrap();
        assert!(!path.exists());
        assert_eq!(
            fs::read(source.join("idempotency/entry")).unwrap(),
            b"ledger"
        );
        assert_eq!(
            fs::read(root.path().join("archive/sessions/6f6e65.json")).unwrap(),
            original
        );
        database.close().await.unwrap();
    }

    #[tokio::test]
    async fn relative_source_directory_is_archived_with_ledger_recovery() {
        let cwd = std::env::current_dir().unwrap();
        let root = tempfile::tempdir_in(&cwd).unwrap();
        let database = database(root.path()).await;
        let source = root.path().join("sessions");
        fs::create_dir_all(source.join("idempotency")).unwrap();
        fs::write(source.join("idempotency/entry"), b"ledger").unwrap();
        fs::write(
            source.join("one.json"),
            serde_json::to_vec(&StoredRecord::from_record(&record("one"))).unwrap(),
        )
        .unwrap();
        let relative = source.strip_prefix(&cwd).unwrap().to_path_buf();
        let store = SqliteSessionStore::new(
            database.clone(),
            Some(relative),
            root.path().join("workspaces"),
        );
        store.initialize().await.unwrap();
        store.initialize().await.unwrap();
        assert!(root.path().join("archive/sessions/one.json").exists());
        assert_eq!(
            fs::read(source.join("idempotency/entry")).unwrap(),
            b"ledger"
        );
        database.close().await.unwrap();
    }

    #[tokio::test]
    async fn archive_retry_refuses_newer_database_state() {
        let root = tempfile::tempdir().unwrap();
        let database = database(root.path()).await;
        let source = root.path().join("sessions");
        fs::create_dir_all(&source).unwrap();
        let path = source.join("one.json");
        fs::write(
            &path,
            serde_json::to_vec(&StoredRecord::from_record(&record("one"))).unwrap(),
        )
        .unwrap();
        fs::write(root.path().join("archive"), b"blocked").unwrap();
        let store = SqliteSessionStore::new(
            database.clone(),
            Some(source.clone()),
            root.path().join("workspaces"),
        );
        assert!(store.initialize().await.is_err());
        let mut updated = store.load("one").await.unwrap().unwrap();
        updated.variables.insert("tenant".into(), "newer".into());
        store.put(updated).await.unwrap();
        fs::remove_file(root.path().join("archive")).unwrap();
        assert!(store.initialize().await.is_err());
        assert!(path.exists());
        assert_eq!(
            store.load("one").await.unwrap().unwrap().variables["tenant"],
            "newer"
        );
        database.close().await.unwrap();
    }

    #[tokio::test]
    async fn interrupted_ledger_restore_preserves_existing_entries() {
        let root = tempfile::tempdir().unwrap();
        let database = database(root.path()).await;
        let source = root.path().join("sessions");
        let archived = root.path().join("archive/sessions/idempotency");
        fs::create_dir_all(&archived).unwrap();
        fs::write(archived.join("entry"), b"ledger").unwrap();
        // App construction recreates the empty active root after the crash.
        fs::create_dir_all(source.join("idempotency")).unwrap();
        let store = SqliteSessionStore::new(
            database.clone(),
            Some(source.clone()),
            root.path().join("workspaces"),
        );
        store.initialize().await.unwrap();
        store.initialize().await.unwrap();
        assert_eq!(
            fs::read(source.join("idempotency/entry")).unwrap(),
            b"ledger"
        );
        assert!(!archived.exists());
        database.close().await.unwrap();
    }

    #[test]
    fn relative_workspace_inside_source_is_rejected() {
        let cwd = std::env::current_dir().unwrap();
        let root = tempfile::tempdir_in(&cwd).unwrap();
        let source = root.path().join("sessions");
        fs::create_dir_all(&source).unwrap();
        let workspace = source.join("workspaces");
        let relative = workspace.strip_prefix(&cwd).unwrap();
        assert!(validate_archive_paths(&source, &root.path().join("server.db"), relative).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_workspace_inside_source_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("sessions");
        fs::create_dir_all(&source).unwrap();
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&source, &alias).unwrap();
        assert!(
            validate_archive_paths(
                &source,
                &root.path().join("server.db"),
                &alias.join("new/workspaces")
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn relational_rows_preserve_mcp_and_replace_variables_atomically() {
        let root = tempfile::tempdir().unwrap();
        let database = database(root.path()).await;
        let store = SqliteSessionStore::new(database.clone(), None, root.path().join("workspaces"));
        let mut original = record("one");
        original.variables.insert("old".into(), "value".into());
        original.sealed_bindings = Some(vec![1, 2, 3]);
        original.mcp_state = Some(serde_json::from_value(serde_json::json!({
            "initialize_params": {
                "protocolVersion":"2025-03-26", "capabilities": {"roots":{"listChanged":true}, "experimental":{"nested":{"items":[1,true,null]}}},
                "clientInfo":{"name":"test", "version":"1", "title":"Title", "description":"Description", "websiteUrl":"https://example.com", "icons":[{"src":"icon.svg", "mimeType":"image/svg+xml", "theme":"dark", "sizes":["16x16","any"]}]},
                "_meta":{"custom":{"nested":true}, "secrets":{"TOKEN":"never-store-me"}}
            }
        })).unwrap());
        store.put(original.clone()).await.unwrap();
        let mut loaded = store.load("one").await.unwrap().unwrap();
        let expected_mcp = original.mcp_state.as_mut().unwrap();
        sanitize_mcp_state(expected_mcp);
        assert_eq!(
            serde_json::to_value(&loaded.mcp_state).unwrap(),
            serde_json::to_value(&original.mcp_state).unwrap()
        );
        database.read(|connection| Box::pin(async move {
            let row: (i64, String, String, Vec<u8>) = sqlx::query_as("SELECT record_version,status,blueprint_name,encrypted_harness_bindings FROM sessions WHERE session_id='one'").fetch_one(&mut *connection).await?;
            assert_eq!(row, (1,"active".into(),"test".into(),vec![1,2,3]));
            let names: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('sessions')").fetch_all(&mut *connection).await?;
            assert!(!names.iter().any(|name| name == "record"));
            let meta: String = sqlx::query_scalar("SELECT metadata_json FROM session_mcp WHERE session_id='one'").fetch_one(connection).await?;
            assert!(!meta.contains("never-store-me"));
            Ok(())
        })).await.unwrap();
        loaded.variables.clear();
        loaded.variables.insert("new".into(), "replacement".into());
        store.put(loaded).await.unwrap();
        let loaded = store.load("one").await.unwrap().unwrap();
        assert_eq!(
            loaded.variables,
            BTreeMap::from([("new".into(), "replacement".into())])
        );
        store.remove("one").await.unwrap();
        database
            .read(|connection| {
                Box::pin(async move {
                    let count: i64 =
                        sqlx::query_scalar("SELECT COUNT(*) FROM session_mcp_icon_sizes")
                            .fetch_one(connection)
                            .await?;
                    assert_eq!(count, 0);
                    Ok(())
                })
            })
            .await
            .unwrap();
        database.close().await.unwrap();
    }

    #[tokio::test]
    async fn failed_import_rolls_back_every_record() {
        let root = tempfile::tempdir().unwrap();
        let database = database(root.path()).await;
        let source = root.path().join("sessions");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("one.json"),
            serde_json::to_vec(&StoredRecord::from_record(&record("one"))).unwrap(),
        )
        .unwrap();
        fs::write(source.join("two.json"), b"invalid JSON").unwrap();
        let store = SqliteSessionStore::new(
            database.clone(),
            Some(source.clone()),
            root.path().join("workspaces"),
        );
        assert!(store.initialize().await.is_err());
        assert!(store.load_all().await.unwrap().is_empty());
        assert!(source.join("one.json").exists());
        database.close().await.unwrap();
    }
    fn manager(
        root: &Path,
        store: Arc<SqliteSessionStore>,
    ) -> crate::session_manager::SessionManager {
        crate::session_manager::SessionManager::new(
            root.join("workspaces"),
            None,
            Arc::default(),
            Arc::new(|| panic!("test does not use HTTP")),
            store,
            Arc::new(crate::idempotency_store::InMemoryIdempotencyStore::default()),
            crate::session_manager::CapabilitySettings::default(),
        )
    }

    #[tokio::test]
    async fn execution_start_does_not_prevent_closure_or_expiry() {
        let root = tempfile::tempdir().unwrap();
        let database = database(root.path()).await;
        let store = Arc::new(SqliteSessionStore::new(
            database.clone(),
            None,
            root.path().join("workspaces"),
        ));
        let manager = manager(root.path(), store.clone());
        let mut initial = record("one");
        initial.idle_timeout = Duration::from_secs(10);
        store.put(initial).await.unwrap();
        manager.start_execution("one").await.unwrap();
        assert!(manager.wipe_now("one").await.unwrap());
        assert!(!manager.touch("one").await.unwrap());

        store.put(record("expired")).await.unwrap();
        manager.start_execution("expired").await.unwrap();
        let mut old = store.load("expired").await.unwrap().unwrap();
        old.last_activity = UNIX_EPOCH;
        store.put(old).await.unwrap();
        assert!(!manager.touch("expired").await.unwrap());
        assert_eq!(manager.reap_now().await.unwrap(), 1);
        assert_eq!(
            store.load("expired").await.unwrap().unwrap().closed_reason,
            Some(ClosedReason::Expired)
        );
        database.close().await.unwrap();
    }

    #[tokio::test]
    async fn failed_rebind_preserves_committed_secrets_and_other_instance_reads() {
        use submilli_shared::secret_store::{KeySource, SecretCipher};
        let root = tempfile::tempdir().unwrap();
        let key = root.path().join("key");
        fs::write(&key, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").unwrap();
        let cipher = Arc::new(SecretCipher::new(&KeySource::File(key)).unwrap());
        let database = database(root.path()).await;
        let store = Arc::new(SqliteSessionStore::new(
            database.clone(),
            None,
            root.path().join("workspaces"),
        ));
        let first = manager(root.path(), store.clone()).with_cipher(Some(cipher.clone()));
        let second = manager(root.path(), store.clone()).with_cipher(Some(cipher));
        let blueprint = submilli_blueprint::Blueprint {
            name: "test".into(),
            ..Default::default()
        };
        let bindings = |value: &str| Arc::new(BTreeMap::from([("TOKEN".into(), value.into())]));
        first
            .bind("one", &blueprint, Arc::default(), bindings("old"))
            .await
            .unwrap();
        database.transaction(|connection| Box::pin(async move {
            sqlx::query("CREATE TRIGGER fail_session_update BEFORE UPDATE ON sessions BEGIN SELECT RAISE(FAIL, 'injected'); END")
                .execute(connection).await?; Ok(())
        })).await.unwrap();
        assert!(
            first
                .rebind_harness_secrets("one", bindings("new"))
                .await
                .is_err()
        );
        assert_eq!(
            second.harness_secrets("one").await.unwrap().unwrap()["TOKEN"],
            "old"
        );
        database
            .transaction(|connection| {
                Box::pin(async move {
                    sqlx::query("DROP TRIGGER fail_session_update")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .await
            .unwrap();
        first
            .rebind_harness_secrets("one", bindings("new"))
            .await
            .unwrap();
        assert_eq!(
            second.harness_secrets("one").await.unwrap().unwrap()["TOKEN"],
            "new"
        );
        database.close().await.unwrap();
    }
}
