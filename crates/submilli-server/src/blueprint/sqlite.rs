use std::path::PathBuf;
use std::sync::Arc;

use sqlx::SqliteConnection;
use submilli_blueprint::Blueprint;

use super::{BlueprintStore, StoreError, StoredBlueprint};
use crate::database::{DatabaseError, ServerDatabase};

mod archive;

/// SQLite is authoritative after initialization; imported files are archived.
pub struct SqliteBlueprintStore {
    database: Arc<ServerDatabase>,
    source: Option<PathBuf>,
}

impl SqliteBlueprintStore {
    pub fn new(database: Arc<ServerDatabase>, source: Option<PathBuf>) -> Self {
        Self { database, source }
    }

    /// Import file-backed blueprints and archive the source before exposing this store.
    pub async fn migrate(&self) -> Result<(), StoreError> {
        let source = self.source.clone();
        let source = self
            .database
            .transaction(move |connection| {
                Box::pin(async move { import_files(connection, source).await })
            })
            .await?;
        // This runs only after commit, on the database supervisor rather than a
        // request Tokio thread. A cancelled caller can retry archiving at startup.
        let database_path = self.database.path().to_path_buf();
        self.database
            .read(move |_| Box::pin(async move { archive_directory(source, &database_path) }))
            .await
            .map_err(Into::into)
    }

    async fn write(&self, stored: StoredBlueprint, replace: bool) -> Result<bool, StoreError> {
        self.database.transaction(move |connection| Box::pin(async move {
            let name = stored.blueprint.name;
            let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM blueprints WHERE name=?1)")
                .bind(&name).fetch_one(&mut *connection).await?;
            if exists && !replace {
                return Err(DatabaseError::AlreadyExists);
            }
            let previous: Option<i64> = sqlx::query_scalar("SELECT MAX(revision) FROM blueprint_revisions WHERE name=?1")
                .bind(&name).fetch_one(&mut *connection).await?;
            let revision = previous.unwrap_or(0).checked_add(1)
                .ok_or_else(|| DatabaseError::RevisionExhausted { name: name.clone() })?;
            sqlx::query("INSERT INTO blueprint_revisions (name, revision, yaml) VALUES (?1, ?2, ?3)")
                .bind(&name).bind(revision).bind(stored.yaml).execute(&mut *connection).await?;
            sqlx::query("INSERT INTO blueprints (name, current_revision) VALUES (?1, ?2) ON CONFLICT(name) DO UPDATE SET current_revision=excluded.current_revision")
                .bind(name).bind(revision).execute(connection).await?;
            Ok(!exists)
        })).await.map_err(Into::into)
    }

    async fn current<T, F>(&self, name: &str, decode: F) -> Result<Option<T>, StoreError>
    where
        T: Send + 'static,
        F: FnOnce(&str, String) -> T + Send + 'static,
    {
        let name = name.to_owned();
        self.database.read(move |connection| Box::pin(async move {
            let yaml: Option<String> = sqlx::query_scalar(
                "SELECT r.yaml FROM blueprints b JOIN blueprint_revisions r ON r.name=b.name AND r.revision=b.current_revision WHERE b.name=?1"
            ).bind(&name).fetch_optional(connection).await?;
            Ok(yaml.map(|yaml| decode(&name, yaml)))
        })).await.map_err(Into::into)
    }

    async fn active(&self) -> Result<Vec<Blueprint>, StoreError> {
        self.database.read(|connection| Box::pin(async move {
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT b.name, r.yaml FROM blueprints b JOIN blueprint_revisions r ON r.name=b.name AND r.revision=b.current_revision ORDER BY b.name"
            ).fetch_all(connection).await?;
            Ok(rows.into_iter().filter_map(|(name, yaml)| parse_current(&name, &yaml).ok()).collect())
        })).await.map_err(Into::into)
    }
}

#[async_trait::async_trait]
impl BlueprintStore for SqliteBlueprintStore {
    async fn add_yaml(&self, stored: StoredBlueprint) -> Result<(), StoreError> {
        self.write(stored, false).await.map(|_| ())
    }

    async fn upsert_yaml(&self, stored: StoredBlueprint) -> Result<bool, StoreError> {
        self.write(stored, true).await
    }

    async fn get(&self, name: &str) -> Result<Option<Blueprint>, StoreError> {
        Ok(self
            .current(name, |name, yaml| parse_current(name, &yaml).ok())
            .await?
            .flatten())
    }

    async fn get_yaml(&self, name: &str) -> Result<Option<String>, StoreError> {
        self.current(name, |_, yaml| yaml).await
    }

    async fn unusable_reason(&self, name: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .current(name, |name, yaml| parse_current(name, &yaml).err())
            .await?
            .flatten())
    }

    async fn list(&self) -> Result<Vec<String>, StoreError> {
        Ok(self
            .active()
            .await?
            .into_iter()
            .map(|blueprint| blueprint.name)
            .collect())
    }

    async fn list_blueprints(&self) -> Result<Vec<Blueprint>, StoreError> {
        self.active().await
    }

    async fn remove(&self, name: &str) -> Result<bool, StoreError> {
        let name = name.to_owned();
        self.database
            .transaction(move |connection| {
                Box::pin(async move {
                    Ok(sqlx::query("DELETE FROM blueprints WHERE name=?1")
                        .bind(name)
                        .execute(connection)
                        .await?
                        .rows_affected()
                        != 0)
                })
            })
            .await
            .map_err(Into::into)
    }
}

fn parse_current(name: &str, yaml: &str) -> Result<Blueprint, String> {
    let blueprint = submilli_blueprint::parse(yaml).map_err(|error| error.to_string())?;
    if blueprint.name != name {
        return Err(format!(
            "stored blueprint name does not match registered name '{name}'"
        ));
    }
    Ok(blueprint)
}

/// Called on the database supervisor, inside the import transaction. Filesystem
/// reads and YAML parsing never execute on the request runtime.
async fn import_files(
    connection: &mut SqliteConnection,
    source: Option<PathBuf>,
) -> Result<Option<PathBuf>, DatabaseError> {
    let Some(source) = source else {
        return Ok(None);
    };
    let Some(SourceRecords { revisions, active }) = read_source(&source)? else {
        return Ok(None);
    };
    let populated: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM blueprint_revisions)")
        .fetch_one(&mut *connection)
        .await?;
    if populated {
        // A committed import may have been interrupted before the directory moved.
        // Immutable history proves the source revisions are already in SQLite;
        // active selections may since have changed or been deleted.
        verify_imported(connection, &revisions, &active).await?;
        return Ok(Some(source));
    }
    for (name, revision, yaml) in &revisions {
        sqlx::query("INSERT INTO blueprint_revisions VALUES (?1, ?2, ?3)")
            .bind(name)
            .bind(revision)
            .bind(yaml)
            .execute(&mut *connection)
            .await?;
    }
    for (name, revision) in &active {
        let revision = checked_revision(name, *revision)?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM blueprint_revisions WHERE name=?1 AND revision=?2)",
        )
        .bind(name)
        .bind(revision)
        .fetch_one(&mut *connection)
        .await?;
        if !exists {
            return Err(DatabaseError::Import(format!(
                "missing indexed revision for '{name}' ({revision})"
            )));
        }
        sqlx::query("INSERT INTO blueprints VALUES (?1, ?2)")
            .bind(name)
            .bind(revision)
            .execute(&mut *connection)
            .await?;
    }
    tracing::info!(
        revisions = revisions.len(),
        active = active.len(),
        "blueprint file import prepared"
    );
    Ok(Some(source))
}

async fn verify_imported(
    connection: &mut SqliteConnection,
    revisions: &[(String, i64, String)],
    active: &std::collections::BTreeMap<String, u64>,
) -> Result<(), DatabaseError> {
    for (name, revision, yaml) in revisions {
        let stored: Option<String> = sqlx::query_scalar(
            "SELECT yaml FROM blueprint_revisions WHERE name=?1 AND revision=?2",
        )
        .bind(name)
        .bind(revision)
        .fetch_optional(&mut *connection)
        .await?;
        if stored.as_ref() != Some(yaml) {
            return Err(DatabaseError::Import(format!(
                "source revision '{name}' ({revision}) conflicts with existing database history; source files were not archived"
            )));
        }
    }
    for (name, revision) in active {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM blueprint_revisions WHERE name=?1 AND revision=?2)",
        )
        .bind(name)
        .bind(checked_revision(name, *revision)?)
        .fetch_one(&mut *connection)
        .await?;
        if !exists {
            return Err(DatabaseError::Import(format!(
                "indexed revision '{name}' ({revision}) is missing from existing database history"
            )));
        }
    }
    Ok(())
}

struct SourceRecords {
    revisions: Vec<(String, i64, String)>,
    active: std::collections::BTreeMap<String, u64>,
}

fn read_source(source: &std::path::Path) -> Result<Option<SourceRecords>, DatabaseError> {
    match std::fs::symlink_metadata(source) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(DatabaseError::Import(format!(
                "blueprint source {} is a symbolic link; configure its real directory before migration",
                source.display()
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(import_io(source, error)),
    }
    let entries = match std::fs::read_dir(source) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(error) => return Err(import_io(source, error)),
    };
    let active = super::read_index(source).map_err(|error| import_io(source, error))?;
    let mut revisions = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in entries {
        let entry = entry.map_err(|error| import_io(source, error))?;
        let path = entry.path();
        let Some((name, revision)) = super::parse_revision_filename(&path) else {
            continue;
        };
        if !seen.insert((name.clone(), revision)) {
            return Err(DatabaseError::Import(format!(
                "duplicate revision for '{name}' ({revision})"
            )));
        }
        let revision = checked_revision(&name, revision)?;
        let yaml = std::fs::read_to_string(&path).map_err(|error| import_io(&path, error))?;
        revisions.push((name, revision, yaml));
    }
    Ok(Some(SourceRecords {
        revisions,
        active: active.unwrap_or_default(),
    }))
}

fn archive_directory(
    source: Option<PathBuf>,
    database_path: &std::path::Path,
) -> Result<(), DatabaseError> {
    let Some(source) = source else {
        return Ok(());
    };
    // Repeated initialization may already have moved this directory.
    if !source
        .try_exists()
        .map_err(|error| import_io(&source, error))?
    {
        return Ok(());
    }
    let canonical_source =
        std::fs::canonicalize(&source).map_err(|error| import_io(&source, error))?;
    if database_path.starts_with(&canonical_source) {
        return Err(DatabaseError::Import(
            "blueprint source directory contains the open database and cannot be archived".into(),
        ));
    }
    let parent = source
        .parent()
        .ok_or_else(|| DatabaseError::InvalidPath(source.clone()))?;
    let archive = parent.join("archive");
    let destination = archive.join("blueprints");
    std::fs::create_dir_all(&archive).map_err(|error| import_io(&archive, error))?;
    archive::move_directory(&source, &destination).map_err(|error| import_io(&destination, error))
}

fn checked_revision(name: &str, revision: u64) -> Result<i64, DatabaseError> {
    i64::try_from(revision).map_err(|_| {
        DatabaseError::Import(format!(
            "revision {revision} for '{name}' exceeds the supported integer range"
        ))
    })
}

fn import_io(path: &std::path::Path, error: std::io::Error) -> DatabaseError {
    DatabaseError::Io {
        path: path.to_path_buf(),
        source: error,
    }
}

#[cfg(test)]
mod tests;
