//! One local immutable-object directory, exclusively owned by this server.
use super::codec;
use crate::application::durable_objects::{
    ObjectStoreError,
    ports::{ObjectAccess, ObjectStorage, PayloadSource, PayloadTarget, PublishedContent},
};
use crate::domain::durable_object::{ContentIdentity, ObjectId, ObjectStoreIdentity};
use async_trait::async_trait;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use submilli_shared::secret_store::SecretCipher;
use tokio::sync::{Mutex, OwnedMutexGuard};

pub(crate) struct LocalObjects {
    files: Arc<Files>,
    gates: Mutex<HashMap<ObjectId, Weak<Mutex<()>>>>,
}

struct Files {
    path: PathBuf,
    directory: File,
    cipher: Arc<SecretCipher>,
    // Every access and detached blocking job retains Files, including this lock.
    // Another store cannot recover/collect while old I/O still runs.
    _owner: File,
}

impl LocalObjects {
    /// Called while the initial SQL transaction excludes competing registrations.
    /// The identity file is durable before SQL names this directory. A crash
    /// before the SQL commit leaves only an empty store that may be adopted.
    pub(crate) fn identify(
        &self,
        expected: Option<ObjectStoreIdentity>,
    ) -> Result<ObjectStoreIdentity, ObjectStoreError> {
        let path = self.files.path.join(".identity");
        let existing = if regular_or_missing(&path)? {
            let mut bytes = String::new();
            File::open(&path)?.take(128).read_to_string(&mut bytes)?;
            File::open(&path)?.sync_all()?;
            self.files.directory.sync_all()?;
            Some(ObjectStoreIdentity::parse(bytes)?)
        } else {
            None
        };
        if let Some(expected) = expected {
            if existing.as_ref() != Some(&expected) {
                return Err(ObjectStoreError::Configuration(
                    "object directory does not match the database's store identity".into(),
                ));
            }
            return Ok(expected);
        }
        // No database identity means initialization or rollback of initialization.
        // Existing payloads must never be adopted without their accounting/pins.
        for entry in std::fs::read_dir(&self.files.path)? {
            let name = entry?.file_name();
            if name != ".owner" && name != ".identity" && name != ".identity-staging" {
                return Err(ObjectStoreError::Configuration(
                    "cannot adopt object files without their original database".into(),
                ));
            }
        }
        // Even an empty directory may have belonged to another database. Mint
        // a new identity so that database can no longer reopen this store with
        // obsolete accounting. A rolled-back initialization has no SQL owner.
        let mut bytes = [0; 16];
        getrandom::getrandom(&mut bytes)
            .map_err(|error| ObjectStoreError::Worker(error.to_string()))?;
        let identity = ObjectStoreIdentity::parse(
            uuid::Builder::from_random_bytes(bytes)
                .into_uuid()
                .to_string(),
        )?;
        let staging = self.files.path.join(".identity-staging");
        regular_or_missing(&staging)?;
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        private_file(&mut options);
        let mut file = options.open(&staging)?;
        file.write_all(identity.as_str().as_bytes())?;
        file.sync_all()?;
        std::fs::rename(staging, path)?;
        self.files.directory.sync_all()?;
        Ok(identity)
    }

    /// Explicitly configured directories must exist. Startup must not replace
    /// a missing mount with an empty local directory.
    pub(crate) fn open(path: PathBuf, cipher: Arc<SecretCipher>) -> Result<Self, ObjectStoreError> {
        if !std::fs::symlink_metadata(&path)?.file_type().is_dir() {
            return Err(ObjectStoreError::Configuration(
                "object directory must be a real directory".into(),
            ));
        }
        let directory = File::open(&path)?;
        directory.sync_all()?;
        let owner_path = path.join(".owner");
        regular_or_missing(&owner_path)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        private_file(&mut options);
        let owner = options.open(&owner_path)?;
        owner.try_lock().map_err(|error| {
            ObjectStoreError::Configuration(format!(
                "object directory is already owned or cannot be locked: {error}"
            ))
        })?;
        owner.sync_all()?;
        directory.sync_all()?;
        Ok(Self {
            files: Arc::new(Files {
                path,
                directory,
                cipher,
                _owner: owner,
            }),
            gates: Mutex::new(HashMap::new()),
        })
    }
}

#[async_trait]
impl ObjectStorage for LocalObjects {
    fn maximum_stored_bytes(&self, plaintext_bytes: u64) -> Result<u64, ObjectStoreError> {
        codec::stored_bytes(plaintext_bytes)
    }

    async fn access(&self, id: &ObjectId) -> Result<Box<dyn ObjectAccess>, ObjectStoreError> {
        let gate = {
            let mut gates = self.gates.lock().await;
            gates.retain(|_, gate| gate.strong_count() > 0);
            if let Some(gate) = gates.get(id).and_then(Weak::upgrade) {
                gate
            } else {
                let gate = Arc::new(Mutex::new(()));
                gates.insert(id.clone(), Arc::downgrade(&gate));
                gate
            }
        };
        Ok(Box::new(LocalAccess {
            files: self.files.clone(),
            id: id.clone(),
            guard: Arc::new(gate.lock_owned().await),
        }))
    }
}

struct LocalAccess {
    files: Arc<Files>,
    id: ObjectId,
    guard: Arc<OwnedMutexGuard<()>>,
}

impl LocalAccess {
    async fn work<T: Send + 'static>(
        &self,
        job: impl FnOnce(&Files, &ObjectId) -> Result<T, ObjectStoreError> + Send + 'static,
    ) -> Result<T, ObjectStoreError> {
        let files = self.files.clone();
        let id = self.id.clone();
        let guard = self.guard.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            job(&files, &id)
        })
        .await
        .map_err(|error| ObjectStoreError::Worker(error.to_string()))?
    }
}

#[async_trait]
impl ObjectAccess for LocalAccess {
    async fn publish(
        &mut self,
        mut source: PayloadSource,
        maximum_bytes: u64,
    ) -> Result<PublishedContent, ObjectStoreError> {
        self.work(move |files, id| files.publish(id, source.as_mut(), maximum_bytes))
            .await
    }
    async fn inspect(
        &mut self,
        maximum_bytes: u64,
    ) -> Result<Option<PublishedContent>, ObjectStoreError> {
        self.work(move |files, id| files.inspect(id, maximum_bytes))
            .await
    }
    async fn copy_into(
        &mut self,
        content: ContentIdentity,
        mut target: PayloadTarget,
    ) -> Result<PayloadTarget, ObjectStoreError> {
        self.work(move |files, id| {
            let path = files.final_path(id);
            if !regular_or_missing(&path)? {
                return Err(ObjectStoreError::Integrity);
            }
            // Validate before giving any bytes to the consumer. Published files
            // are immutable, and the access guard prevents retirement/collection.
            let validated = files.validate(&path, id, content.bytes)?;
            if validated.identity != content {
                return Err(ObjectStoreError::Integrity);
            }
            let mut file = File::open(&path)?;
            let copied =
                codec::decode(&mut file, target.as_mut(), &files.cipher, id, content.bytes)?;
            if copied.identity != content {
                return Err(ObjectStoreError::Integrity);
            }
            Ok(target)
        })
        .await
    }
    async fn remove(&mut self) -> Result<(), ObjectStoreError> {
        self.work(|files, id| {
            for path in [files.staging_path(id), files.final_path(id)] {
                if regular_or_missing(&path)? {
                    std::fs::remove_file(path)?;
                }
            }
            #[cfg(test)]
            crate::objects::tests::process::checkpoint("unlink");
            files.directory.sync_all()?;
            #[cfg(test)]
            crate::objects::tests::process::checkpoint("deletion-sync");
            Ok(())
        })
        .await
    }
}

impl Files {
    fn final_path(&self, id: &ObjectId) -> PathBuf {
        self.path.join(format!("{}.object", id.as_str()))
    }
    fn staging_path(&self, id: &ObjectId) -> PathBuf {
        self.path.join(format!("{}.staging", id.as_str()))
    }

    fn publish(
        &self,
        id: &ObjectId,
        source: &mut dyn Read,
        maximum: u64,
    ) -> Result<PublishedContent, ObjectStoreError> {
        if let Some(content) = self.inspect(id, maximum)? {
            return Ok(content);
        }
        let staging = self.staging_path(id);
        regular_or_missing(&staging)?;
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        private_file(&mut options);
        let mut file = options.open(&staging)?;
        let content = codec::encode(source, &mut file, &self.cipher, id, maximum)?;
        file.sync_all()?;
        #[cfg(test)]
        crate::objects::tests::process::checkpoint("file-sync");
        drop(file);
        if self.validate(&staging, id, maximum)?.identity != content.identity {
            return Err(ObjectStoreError::Integrity);
        }
        self.publish_name(id)?;
        Ok(content)
    }

    fn inspect(
        &self,
        id: &ObjectId,
        maximum: u64,
    ) -> Result<Option<PublishedContent>, ObjectStoreError> {
        let final_path = self.final_path(id);
        if regular_or_missing(&final_path)? {
            let content = self.validate(&final_path, id, maximum)?;
            File::open(&final_path)?.sync_all()?;
            self.directory.sync_all()?;
            return Ok(Some(content));
        }
        let staging = self.staging_path(id);
        if !regular_or_missing(&staging)? {
            return Ok(None);
        }
        match self.validate(&staging, id, maximum) {
            Ok(content) => {
                File::open(&staging)?.sync_all()?;
                self.publish_name(id)?;
                Ok(Some(content))
            }
            // The application authenticated the reservation with the configured
            // key before inspecting staging. In this single-key store a damaged
            // staging key-ID is therefore corrupt uncommitted data, not evidence
            // of an unavailable historical key. Final objects still fail closed.
            Err(ObjectStoreError::Integrity | ObjectStoreError::Key) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn validate(
        &self,
        path: &Path,
        id: &ObjectId,
        maximum: u64,
    ) -> Result<PublishedContent, ObjectStoreError> {
        let mut file = File::open(path)?;
        codec::decode(&mut file, &mut std::io::sink(), &self.cipher, id, maximum)
    }

    fn publish_name(&self, id: &ObjectId) -> Result<(), ObjectStoreError> {
        let destination = self.final_path(id);
        if regular_or_missing(&destination)? {
            return Err(ObjectStoreError::Integrity);
        }
        // The directory's process lock and this ID's access guard exclude every
        // publisher/collector. Guests cannot access this private root. Thus no
        // writer can introduce a destination between this check and rename.
        std::fs::rename(self.staging_path(id), destination)?;
        #[cfg(test)]
        crate::objects::tests::process::checkpoint("rename");
        self.directory.sync_all()?;
        #[cfg(test)]
        crate::objects::tests::process::checkpoint("publication-sync");
        Ok(())
    }
}

fn regular_or_missing(path: &Path) -> Result<bool, ObjectStoreError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.nlink() != 1 {
                    return Err(ObjectStoreError::Integrity);
                }
            }
            Ok(true)
        }
        Ok(_) => Err(ObjectStoreError::Integrity),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn private_file(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = options;
}
