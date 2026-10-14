//! A secret-store directory from before secrets were kept in the database: one
//! sealed file per key, MCP OAuth grants among them under
//! `mcp_oauth/<blueprint>/<server>/credential`. Read for
//! [`ImportSecrets`](crate::application::secrets::import::ImportSecrets), then
//! moved under `<parent>/archive/`.
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use submilli_shared::mcp_auth::{GRANT_KEY_PREFIX, OAuthCredential, parse_grant_key};
use submilli_shared::secret_store::{FileSecretStore, SealedFile, SecretCipher};

use crate::application::error::StoreError;
use crate::application::secrets::import::{
    PreviousEntry, PreviousSecretStore, PreviousSecrets, SkippedEntry,
};
use crate::domain::secret::{Secret, SecretName, SecretValue};

pub(crate) struct LegacySecretDirectory {
    source: PathBuf,
    cipher: Option<Arc<SecretCipher>>,
    database: PathBuf,
    /// The files [`PreviousSecretStore::read`] opened, so archiving can
    /// confirm nothing changed since.
    read: Mutex<Vec<SealedFile>>,
}

impl LegacySecretDirectory {
    pub fn new(source: PathBuf, cipher: Option<Arc<SecretCipher>>, database: PathBuf) -> Self {
        Self {
            source,
            cipher,
            database,
            read: Mutex::new(Vec::new()),
        }
    }

    /// Whether `source` holds sealed files to import. Without any, the import
    /// is a no-op and needs neither the key nor a durable database.
    pub fn holds_sealed_files(source: &Path) -> Result<bool, StoreError> {
        Ok(!Self::sealed_files(source)?.is_empty())
    }

    fn sealed_files(source: &Path) -> Result<Vec<SealedFile>, StoreError> {
        if !source.try_exists().map_err(io)? {
            return Ok(Vec::new());
        }
        FileSecretStore::sealed_files(source).map_err(|error| {
            StoreError::Io(format!(
                "cannot read secrets in `{}`: {error}",
                source.display()
            ))
        })
    }

    /// The archives earlier imports left beside `source`. They hold the old
    /// sealed values, so a secret deleted or a grant revoked since stays
    /// recoverable from them with the key.
    pub fn archives(source: &Path) -> Vec<PathBuf> {
        let Some(archive) = resolved(source)
            .ok()
            .and_then(|source| Some(source.parent()?.join("archive")))
        else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(&archive) else {
            return Vec::new();
        };
        let mut archives: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name == "secrets" || name.starts_with("secrets-"))
            })
            .map(|entry| entry.path())
            .collect();
        archives.sort();
        archives
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<SealedFile>> {
        // A poisoned list may be partial. AGENTS.md's poisoned-lock exception
        // accepts a panic instead of recovery.
        self.read.lock().expect("legacy secret files poisoned")
    }
}

#[async_trait::async_trait]
impl PreviousSecretStore for LegacySecretDirectory {
    async fn read(&self) -> Result<PreviousSecrets, StoreError> {
        let files = Self::sealed_files(&self.source)?;
        if files.is_empty() {
            return Ok(PreviousSecrets::default());
        }
        // Refused before anything is imported: archiving would move the
        // database too.
        if resolved(&self.database)?.starts_with(resolved(&self.source)?) {
            return Err(StoreError::Io(
                "the secret-store directory contains the database; move one of them".into(),
            ));
        }
        let cipher = self.cipher.clone().ok_or_else(|| {
            StoreError::Credentials(format!(
                "importing secrets from `{}` needs the secret-store key that sealed them",
                self.source.display()
            ))
        })?;
        let store = FileSecretStore::existing(self.source.clone(), cipher);
        let mut previous = PreviousSecrets::default();
        for file in &files {
            match entry(&store, file)? {
                FileEntry::Importable(entry) => previous.entries.push(entry),
                FileEntry::Skipped(skipped) => previous.skipped.push(skipped),
            }
        }
        *self.lock() = files;
        Ok(previous)
    }

    async fn archive(&self) -> Result<(), StoreError> {
        let read = std::mem::take(&mut *self.lock());
        archive(&self.source, &read)
    }
}

/// What one sealed file holds for the import.
enum FileEntry {
    Importable(PreviousEntry),
    /// Left behind in the archive, and why.
    Skipped(SkippedEntry),
}

/// The entry a file holds, or why the import leaves it behind. A file that
/// cannot be opened is an error: a wrong key or corruption must not lose it.
fn entry(store: &FileSecretStore, file: &SealedFile) -> Result<FileEntry, StoreError> {
    let value = store.open_file(file).map_err(|error| {
        StoreError::Credentials(format!(
            "secret '{}' in `{}` cannot be read: {error}; nothing was imported",
            file.key,
            file.path.display()
        ))
    })?;
    let skip = |reason: String| {
        Ok(FileEntry::Skipped(SkippedEntry {
            entry: format!("'{}' (`{}`)", file.key, file.path.display()),
            reason,
        }))
    };
    if !file.key.starts_with(GRANT_KEY_PREFIX) {
        return match SecretName::parse(file.key.clone()) {
            Ok(name) => Ok(FileEntry::Importable(PreviousEntry::Secret(Secret::new(
                name,
                SecretValue::new(value),
            )))),
            Err(error) => skip(error.to_string()),
        };
    }
    let Some((blueprint, server)) = parse_grant_key(&file.key) else {
        return skip("not an MCP grant key".into());
    };
    let Ok(credential) = serde_json::from_str::<OAuthCredential>(&value) else {
        return skip("not an MCP grant record".into());
    };
    let grant = match crate::adapters::mcp_grant_store::grant_credential(&credential) {
        Ok(grant) => grant,
        Err(error) => return skip(error.to_string()),
    };
    Ok(FileEntry::Importable(PreviousEntry::Grant {
        blueprint: blueprint.to_owned(),
        server: server.to_owned(),
        credential: grant,
        scopes: credential.scopes,
    }))
}

/// Move the imported directory under `<parent>/archive/`, once its files are
/// confirmed unchanged since they were read. An earlier archive is kept; this
/// one takes the next free name.
fn archive(source: &Path, read: &[SealedFile]) -> Result<(), StoreError> {
    // Another server sharing the directory imported the same rows and
    // archived it first.
    if !read.is_empty() && !source.try_exists().map_err(io)? {
        return Ok(());
    }
    let source = resolved(source)?;
    let parent = source
        .parent()
        .ok_or_else(|| StoreError::Io("invalid secret-store directory".into()))?;
    let now = FileSecretStore::sealed_files(&source)
        .map_err(|error| StoreError::Io(error.to_string()))?;
    let unchanged = now.len() == read.len()
        && now
            .iter()
            .zip(read)
            .all(|(now, read)| now.key == read.key && now.bytes == read.bytes);
    if !unchanged {
        return Err(StoreError::Io(format!(
            "`{}` changed during the import, after the database took what was read; move the \
             changed entries out of it, then restart",
            source.display()
        )));
    }
    let archive = parent.join("archive");
    std::fs::create_dir_all(&archive).map_err(io)?;
    let destination = free_name(&archive)?;
    crate::import_archive::move_directory(&source, &destination).map_err(io)?;
    crate::import_archive::sync_directory_ancestry(&destination).map_err(io)?;
    crate::import_archive::sync_directory_ancestry(parent).map_err(io)?;
    Ok(())
}

/// `archive/secrets`, or `archive/secrets-<n>` beside earlier archives. The
/// move itself refuses an existing destination, so a race loses safely.
fn free_name(archive: &Path) -> Result<PathBuf, StoreError> {
    let first = archive.join("secrets");
    if !first.try_exists().map_err(io)? {
        return Ok(first);
    }
    for n in 2..1000 {
        let next = archive.join(format!("secrets-{n}"));
        if !next.try_exists().map_err(io)? {
            return Ok(next);
        }
    }
    Err(StoreError::Io(format!(
        "`{}` holds too many secret archives; remove old ones",
        archive.display()
    )))
}

fn resolved(path: &Path) -> Result<PathBuf, StoreError> {
    crate::import_archive::resolved_path(path).map_err(io)
}

fn io(error: std::io::Error) -> StoreError {
    StoreError::Io(format!("secret import I/O: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_another_server_archived_first_counts_as_archived() {
        let root = tempfile::tempdir().unwrap();
        let read = [SealedFile {
            key: "k".into(),
            path: root.path().join("secrets/aw"),
            bytes: vec![1],
        }];
        archive(&root.path().join("secrets"), &read).unwrap();
        assert!(!root.path().join("archive").exists());
    }
}
