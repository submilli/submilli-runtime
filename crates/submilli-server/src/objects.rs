//! Durable immutable payload storage shared by execution history and downloads.
//!
//! A prepared object is not yet a replayable outcome. History consumers bind it
//! in their own SQL unit of work before exposing a result. The standalone `bind`
//! operation is useful for consumers whose durable reference is the object row.
use crate::adapters::durable_objects::files::LocalObjects;
use crate::adapters::sealing::Sealer;
use crate::application::durable_objects::ObjectPublications;
pub use crate::application::durable_objects::{
    ObjectLimits, ObjectRecovery, ObjectStoreError, PreparedObject,
};
use crate::application::unit_of_work::{UnitOfWork, UnitOfWorkFactory};
use crate::database::ServerDatabase;
use crate::domain::durable_object::{ContentIdentity, ObjectId, ObjectReference, Retention};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::{Cursor, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use submilli_shared::secret_store::SecretCipher;

mod reader;

pub const DEFAULT_INLINE_BYTES: usize = 64 * 1024;
pub const MAX_INLINE_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug)]
pub struct DurableObjectConfig {
    /// Must already exist when explicitly configured.
    pub directory: PathBuf,
    pub inline_bytes: usize,
    pub limits: ObjectLimits,
}

impl Default for DurableObjectConfig {
    fn default() -> Self {
        Self {
            directory: crate::config::default_server_root().join("objects"),
            inline_bytes: DEFAULT_INLINE_BYTES,
            limits: ObjectLimits::default(),
        }
    }
}

impl DurableObjectConfig {
    pub fn validate(&self) -> Result<(), ObjectStoreError> {
        self.limits.validate()?;
        if self.inline_bytes > MAX_INLINE_BYTES {
            return Err(ObjectStoreError::Configuration(
                "inline threshold exceeds 1 MiB".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct DurableObjects {
    publications: Arc<ObjectPublications>,
    sealer: Sealer,
    inline_bytes: usize,
    maximum_bytes: u64,
    directory: PathBuf,
    pub(crate) filesystem_gate: Arc<tokio::sync::Mutex<()>>,
    pub(crate) filesystem_resources: Arc<tokio::sync::RwLock<()>>,
    pub(crate) filesystem_quotas: Arc<tokio::sync::Mutex<crate::filesystems::quotas::SharedQuotas>>,
}

/// The inline representation is sealed before it can reach a persistence adapter.
/// Object references contain no node-local paths.
#[derive(Clone)]
pub struct StoredPayload(PayloadRecord);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum PayloadEncoding {
    V1(PayloadRecord),
}

pub enum PreparedPayload {
    Inline(StoredPayload),
    Object(PreparedObject),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum PayloadRecord {
    Inline {
        id: String,
        bytes: u64,
        sealed: Vec<u8>,
    },
    Object {
        id: String,
        bytes: u64,
        sha256: [u8; 32],
    },
}

impl StoredPayload {
    pub fn is_object(&self) -> bool {
        matches!(self.0, PayloadRecord::Object { .. })
    }
    pub fn bytes(&self) -> u64 {
        match self.0 {
            PayloadRecord::Inline { bytes, .. } | PayloadRecord::Object { bytes, .. } => bytes,
        }
    }
    pub fn encode(&self) -> Result<Vec<u8>, ObjectStoreError> {
        serde_json::to_vec(&PayloadEncoding::V1(self.0.clone()))
            .map_err(|_| ObjectStoreError::Integrity)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, ObjectStoreError> {
        // A JSON byte array needs at most four bytes per ciphertext byte.
        if bytes.len() > (MAX_INLINE_BYTES + 256) * 4 {
            return Err(ObjectStoreError::Limit);
        }
        let PayloadEncoding::V1(record) =
            serde_json::from_slice(bytes).map_err(|_| ObjectStoreError::Integrity)?;
        let id = match &record {
            PayloadRecord::Inline { id, bytes, sealed } => {
                if *bytes > MAX_INLINE_BYTES as u64 || sealed.len() > MAX_INLINE_BYTES + 256 {
                    return Err(ObjectStoreError::Limit);
                }
                id
            }
            PayloadRecord::Object { id, .. } => id,
        };
        ObjectId::parse(id.clone())?;
        Ok(Self(record))
    }
}

impl DurableObjects {
    pub async fn open(
        config: DurableObjectConfig,
        database: Arc<ServerDatabase>,
        cipher: Arc<SecretCipher>,
    ) -> Result<Self, ObjectStoreError> {
        if database.is_ephemeral() {
            return Err(ObjectStoreError::Configuration(
                "durable objects require a persistent database".into(),
            ));
        }
        let units = crate::adapters::unit_of_work::for_sessions(
            database,
            PathBuf::new(),
            Some(cipher.clone()),
        );
        Self::with_units(config, units, cipher).await
    }

    pub(crate) async fn with_units(
        config: DurableObjectConfig,
        units: Arc<dyn UnitOfWorkFactory>,
        cipher: Arc<SecretCipher>,
    ) -> Result<Self, ObjectStoreError> {
        config.validate()?;
        let protected_directory = std::fs::canonicalize(&config.directory)?;
        let directory = config.directory;
        let local_cipher = cipher.clone();
        let storage =
            tokio::task::spawn_blocking(move || LocalObjects::open(directory, local_cipher))
                .await
                .map_err(|error| ObjectStoreError::Worker(error.to_string()))??;
        let mut unit = units.begin().await?;
        let expected = unit.object_store_identity().await?;
        let (storage, identity) = tokio::task::spawn_blocking(move || {
            let identity = storage.identify(expected)?;
            Ok::<_, ObjectStoreError>((storage, identity))
        })
        .await
        .map_err(|error| ObjectStoreError::Worker(error.to_string()))??;
        unit.identify_object_store(identity.clone()).await?;
        unit.commit().await?;
        Ok(Self {
            publications: Arc::new(ObjectPublications::new(
                identity,
                units,
                Arc::new(storage),
                config.limits,
            )?),
            sealer: Sealer::new(Some(cipher)),
            inline_bytes: config.inline_bytes,
            maximum_bytes: config.limits.maximum_object_bytes,
            directory: protected_directory,
            filesystem_gate: Arc::new(tokio::sync::Mutex::new(())),
            filesystem_resources: Arc::new(tokio::sync::RwLock::new(())),
            filesystem_quotas: Arc::new(tokio::sync::Mutex::new(Default::default())),
        })
    }

    pub(crate) fn maximum_bytes(&self) -> u64 {
        self.maximum_bytes
    }

    pub(crate) async fn reservation_limit(
        &self,
        id: &ObjectId,
        owner: &str,
        request: &[u8; 32],
    ) -> Result<Option<u64>, ObjectStoreError> {
        self.publications
            .reservation_limit(id, owner, request)
            .await
    }

    pub fn new_id() -> Result<ObjectId, ObjectStoreError> {
        let mut bytes = [0; 16];
        getrandom::getrandom(&mut bytes)
            .map_err(|error| ObjectStoreError::Worker(error.to_string()))?;
        Ok(ObjectId::parse(
            uuid::Builder::from_random_bytes(bytes)
                .into_uuid()
                .to_string(),
        )?)
    }

    pub(crate) fn store_identity(&self) -> &str {
        self.publications.identity().as_str()
    }

    pub(crate) fn directory(&self) -> &std::path::Path {
        &self.directory
    }

    pub(crate) fn sealer(&self) -> Sealer {
        self.sealer.clone()
    }

    pub async fn reserve(
        &self,
        id: ObjectId,
        owner: String,
        request: [u8; 32],
        maximum_bytes: u64,
    ) -> Result<(), ObjectStoreError> {
        self.publications
            .reserve(id, owner, request, maximum_bytes)
            .await
    }

    pub async fn publish(
        &self,
        id: &ObjectId,
        source: impl Read + Send + 'static,
    ) -> Result<PreparedObject, ObjectStoreError> {
        self.publications.publish(id, Box::new(source)).await
    }

    pub async fn bind(
        &self,
        prepared: &PreparedObject,
        retention: BTreeSet<Retention>,
    ) -> Result<ObjectReference, ObjectStoreError> {
        self.publications.bind(prepared, retention).await
    }

    pub async fn reference(&self, id: &ObjectId) -> Result<ObjectReference, ObjectStoreError> {
        self.publications.reference(id).await
    }

    pub async fn copy_into(
        &self,
        reference: &ObjectReference,
        target: impl Write + Send + 'static,
    ) -> Result<(), ObjectStoreError> {
        self.publications
            .copy_into(reference, Box::new(target))
            .await?;
        Ok(())
    }

    pub async fn retain(
        &self,
        id: &ObjectId,
        retention: Retention,
    ) -> Result<(), ObjectStoreError> {
        self.publications.retain(id, retention).await
    }
    pub async fn release(
        &self,
        id: &ObjectId,
        retention: &Retention,
    ) -> Result<(), ObjectStoreError> {
        self.publications.release(id, retention).await
    }
    pub async fn abandon(&self, id: &ObjectId) -> Result<(), ObjectStoreError> {
        self.publications.abandon(id).await
    }
    pub async fn collect(&self, id: &ObjectId) -> Result<(), ObjectStoreError> {
        self.publications.collect(id).await
    }
    pub async fn recover(&self) -> Result<ObjectRecovery, ObjectStoreError> {
        self.publications.recover().await
    }
    pub async fn recover_publication(
        &self,
        id: &ObjectId,
    ) -> Result<Option<PreparedObject>, ObjectStoreError> {
        self.publications.recover_publication(id).await
    }

    /// Buffer only threshold+1 bytes, then spill directly to encrypted staging.
    /// `always_object` is mandatory for file downloads, including empty bodies.
    #[allow(clippy::too_many_arguments)]
    pub async fn prepare_payload(
        &self,
        id: ObjectId,
        owner: String,
        request: [u8; 32],
        mut source: impl Read + Send + 'static,
        maximum: u64,
        always_object: bool,
    ) -> Result<PreparedPayload, ObjectStoreError> {
        if maximum > self.maximum_bytes || owner.is_empty() || owner.len() > 1024 {
            return Err(ObjectStoreError::Limit);
        }
        if always_object {
            self.reserve(id.clone(), owner, request, maximum).await?;
            return Ok(PreparedPayload::Object(self.publish(&id, source).await?));
        }
        let limit = self.inline_bytes;
        let (prefix, source) = tokio::task::spawn_blocking(move || {
            let mut prefix = Vec::with_capacity(limit + 1);
            source
                .by_ref()
                .take((limit + 1) as u64)
                .read_to_end(&mut prefix)?;
            Ok::<_, std::io::Error>((prefix, source))
        })
        .await
        .map_err(|error| ObjectStoreError::Worker(error.to_string()))??;
        if prefix.len() as u64 > maximum {
            return Err(ObjectStoreError::Limit);
        }
        if prefix.len() <= limit {
            let sealed = self.sealer.seal(&payload_owner(&owner, &id), &prefix)?;
            return Ok(PreparedPayload::Inline(StoredPayload(
                PayloadRecord::Inline {
                    id: id.as_str().to_owned(),
                    bytes: prefix.len() as u64,
                    sealed,
                },
            )));
        }
        self.reserve(id.clone(), owner, request, maximum).await?;
        Ok(PreparedPayload::Object(
            self.publish(&id, Cursor::new(prefix).chain(source)).await?,
        ))
    }

    pub async fn bind_payload(
        &self,
        prepared: PreparedPayload,
        retention: BTreeSet<Retention>,
    ) -> Result<StoredPayload, ObjectStoreError> {
        let mut unit = self.publications.begin().await?;
        let payload = self
            .bind_payload_in(unit.as_mut(), prepared, retention)
            .await?;
        unit.commit().await?;
        Ok(payload)
    }

    /// The caller stores this payload in its event/result and commits the same
    /// unit of work. A returned value alone is not permission to expose success.
    pub(crate) async fn bind_payload_in(
        &self,
        unit: &mut dyn UnitOfWork,
        prepared: PreparedPayload,
        retention: BTreeSet<Retention>,
    ) -> Result<StoredPayload, ObjectStoreError> {
        match prepared {
            PreparedPayload::Inline(payload) => Ok(payload),
            PreparedPayload::Object(prepared) => {
                let reference = self
                    .publications
                    .bind_in(unit, &prepared, retention)
                    .await?;
                Ok(StoredPayload(PayloadRecord::Object {
                    id: reference.id.as_str().to_owned(),
                    bytes: reference.content.bytes,
                    sha256: reference.content.sha256,
                }))
            }
        }
    }

    pub async fn copy_payload(
        &self,
        owner: &str,
        payload: &StoredPayload,
        mut target: impl Write + Send + 'static,
    ) -> Result<(), ObjectStoreError> {
        match &payload.0 {
            PayloadRecord::Inline { id, bytes, sealed } => {
                let id = ObjectId::parse(id.clone())?;
                let plaintext =
                    self.sealer
                        .open(&payload_owner(owner, &id), sealed, "durable payload")?;
                if plaintext.len() as u64 != *bytes {
                    return Err(ObjectStoreError::Integrity);
                }
                tokio::task::spawn_blocking(move || {
                    target.write_all(&plaintext)?;
                    target.flush()
                })
                .await
                .map_err(|error| ObjectStoreError::Worker(error.to_string()))??;
                Ok(())
            }
            PayloadRecord::Object { id, bytes, sha256 } => {
                let reference = ObjectReference {
                    id: ObjectId::parse(id.clone())?,
                    content: ContentIdentity {
                        bytes: *bytes,
                        sha256: *sha256,
                    },
                };
                self.publications.check_owner(&reference.id, owner).await?;
                self.copy_into(&reference, target).await
            }
        }
    }
}

fn payload_owner(owner: &str, id: &ObjectId) -> String {
    format!("durable-payload:{}:{owner}:{}", owner.len(), id.as_str())
}

#[cfg(test)]
pub(crate) mod tests;
