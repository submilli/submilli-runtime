//! Publication, retention and recovery of immutable execution payloads.
pub mod ports;
pub mod workspaces;
use crate::object::{
    CollectionState, ContentIdentity, DurableObject, ObjectError, ObjectId, ObjectReference,
    ObjectStoreIdentity, PublicationState, Retention,
};
use crate::unit_of_work::StoreError;
use crate::unit_of_work::{UnitFactory, UnitOfWork};
use ports::{ObjectStorage, PayloadSource, PayloadTarget, PublishedContent};
use std::collections::BTreeSet;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum ObjectStoreError {
    #[error(transparent)]
    State(#[from] ObjectError),
    #[error(transparent)]
    Persistence(#[from] StoreError),
    #[error("durable object I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("durable object is missing or corrupt")]
    Integrity,
    #[error("durable object encryption key is missing or incorrect")]
    Key,
    #[error("durable object storage limit exceeded")]
    Limit,
    #[error("durable object worker failed: {0}")]
    Worker(String),
    #[error("invalid durable object configuration: {0}")]
    Configuration(String),
}

#[derive(Clone, Copy, Debug)]
pub struct ObjectLimits {
    pub maximum_object_bytes: u64,
    pub maximum_owner_bytes: u64,
    pub maximum_store_bytes: u64,
}

impl Default for ObjectLimits {
    fn default() -> Self {
        Self {
            maximum_object_bytes: 4 << 30,
            maximum_owner_bytes: 8 << 30,
            maximum_store_bytes: 64 << 30,
        }
    }
}

impl ObjectLimits {
    pub fn validate(&self) -> Result<(), ObjectStoreError> {
        if self.maximum_object_bytes > self.maximum_owner_bytes
            || self.maximum_owner_bytes > self.maximum_store_bytes
            || self.maximum_store_bytes > i64::MAX as u64
            || self.maximum_store_bytes == 0
        {
            return Err(ObjectStoreError::Configuration(
                "object <= owner <= store limits must fit SQLite integers".into(),
            ));
        }
        Ok(())
    }
}

/// A publication acknowledgement, not yet an authoritative consumer reference.
/// Only a storage adapter can establish the required persistence barriers.
pub struct PreparedObject {
    issuer: Arc<()>,
    id: ObjectId,
    request: [u8; 32],
    content: PublishedContent,
}

#[derive(Default, Debug)]
pub struct ObjectRecovery {
    pub ready: u64,
    pub incomplete: u64,
    pub validated: u64,
    pub collected: u64,
}

impl PreparedObject {
    pub fn id(&self) -> &ObjectId {
        &self.id
    }
    pub fn content(&self) -> &ContentIdentity {
        &self.content.identity
    }
}

pub(crate) struct ObjectPublications {
    identity: ObjectStoreIdentity,
    issuer: Arc<()>,
    units: UnitFactory,
    storage: Arc<dyn ObjectStorage>,
    limits: ObjectLimits,
}

impl ObjectPublications {
    pub fn identity(&self) -> &ObjectStoreIdentity {
        &self.identity
    }

    pub async fn begin(&self) -> Result<Box<dyn UnitOfWork>, ObjectStoreError> {
        Ok((self.units)().await?)
    }

    pub async fn check_owner(&self, id: &ObjectId, owner: &str) -> Result<(), ObjectStoreError> {
        if self.load(id).await?.owner() != owner {
            return Err(ObjectError::RequestConflict.into());
        }
        Ok(())
    }

    pub fn new(
        identity: ObjectStoreIdentity,
        units: UnitFactory,
        storage: Arc<dyn ObjectStorage>,
        limits: ObjectLimits,
    ) -> Result<Self, ObjectStoreError> {
        limits.validate()?;
        Ok(Self {
            identity,
            issuer: Arc::new(()),
            units,
            storage,
            limits,
        })
    }

    pub async fn reserve(
        &self,
        id: ObjectId,
        owner: String,
        request: [u8; 32],
        maximum_bytes: u64,
    ) -> Result<(), ObjectStoreError> {
        if maximum_bytes > self.limits.maximum_object_bytes {
            return Err(ObjectStoreError::Limit);
        }
        let reserved = self.storage.maximum_stored_bytes(maximum_bytes)?;
        let candidate = DurableObject::reserve(id, owner, request, maximum_bytes, reserved)?;
        let mut unit = (self.units)().await?;
        if let Some(existing) = unit.get_object(candidate.id()).await? {
            existing.matches_request(&candidate)?;
            if existing.collection() != CollectionState::Available
                || !matches!(
                    existing.publication(),
                    PublicationState::Pending | PublicationState::Bound
                )
            {
                return Err(ObjectError::Unavailable.into());
            }
            return Ok(());
        }
        let (total, owned) = unit.object_usage(candidate.owner()).await?;
        if reserved > self.limits.maximum_store_bytes.saturating_sub(total)
            || reserved > self.limits.maximum_owner_bytes.saturating_sub(owned)
        {
            return Err(ObjectStoreError::Limit);
        }
        unit.save_object(candidate).await?;
        unit.commit().await?;
        crate::checkpoint("reservation");
        Ok(())
    }

    pub async fn publish(
        &self,
        id: &ObjectId,
        source: PayloadSource,
    ) -> Result<PreparedObject, ObjectStoreError> {
        let mut access = self.storage.access(id).await?;
        let object = self.load(id).await?;
        if object.collection() != CollectionState::Available
            || !matches!(
                object.publication(),
                PublicationState::Pending | PublicationState::Bound
            )
        {
            return Err(ObjectError::Unavailable.into());
        }
        let content = if object.publication() == PublicationState::Bound {
            access
                .inspect(object.maximum_bytes())
                .await?
                .ok_or(ObjectStoreError::Integrity)?
        } else {
            access.publish(source, object.maximum_bytes()).await?
        };
        if object
            .content()
            .is_some_and(|expected| *expected != content.identity)
        {
            return Err(ObjectStoreError::Integrity);
        }
        Ok(PreparedObject {
            issuer: self.issuer.clone(),
            id: id.clone(),
            request: *object.request(),
            content,
        })
    }

    /// Use this inside the history/result transaction. It must not acquire the
    /// filesystem gate: the SQL state predicate fences abandonment/collection.
    pub async fn bind_in(
        &self,
        unit: &mut dyn UnitOfWork,
        prepared: &PreparedObject,
        retention: BTreeSet<Retention>,
    ) -> Result<ObjectReference, ObjectStoreError> {
        if !Arc::ptr_eq(&self.issuer, &prepared.issuer)
            || unit.object_store_identity().await?.as_ref() != Some(&self.identity)
        {
            return Err(ObjectError::RequestConflict.into());
        }
        let mut object = unit
            .get_object(&prepared.id)
            .await?
            .ok_or(ObjectError::Unavailable)?;
        if object.request() != &prepared.request {
            return Err(ObjectError::RequestConflict.into());
        }
        object.bind(
            prepared.content.identity.clone(),
            prepared.content.stored_bytes,
            retention,
        )?;
        let reference = object.reference()?;
        unit.save_object(object).await?;
        Ok(reference)
    }

    pub async fn bind(
        &self,
        prepared: &PreparedObject,
        retention: BTreeSet<Retention>,
    ) -> Result<ObjectReference, ObjectStoreError> {
        let mut unit = (self.units)().await?;
        let reference = self.bind_in(unit.as_mut(), prepared, retention).await?;
        unit.commit().await?;
        crate::checkpoint("reference");
        Ok(reference)
    }

    pub async fn reference(&self, id: &ObjectId) -> Result<ObjectReference, ObjectStoreError> {
        Ok(self.load(id).await?.reference()?)
    }

    pub async fn copy_into(
        &self,
        reference: &ObjectReference,
        target: PayloadTarget,
    ) -> Result<PayloadTarget, ObjectStoreError> {
        let mut access = self.storage.access(&reference.id).await?;
        let object = self.load(&reference.id).await?;
        if object.reference()? != *reference {
            return Err(ObjectStoreError::Integrity);
        }
        access.copy_into(reference.content.clone(), target).await
    }

    pub async fn retain(
        &self,
        id: &ObjectId,
        retention: Retention,
    ) -> Result<(), ObjectStoreError> {
        let mut unit = (self.units)().await?;
        let mut object = unit.get_object(id).await?.ok_or(ObjectError::Unavailable)?;
        object.retain(retention)?;
        unit.save_object(object).await?;
        unit.commit().await?;
        Ok(())
    }

    pub async fn release(
        &self,
        id: &ObjectId,
        retention: &Retention,
    ) -> Result<(), ObjectStoreError> {
        let mut unit = (self.units)().await?;
        let mut object = unit.get_object(id).await?.ok_or(ObjectError::Unavailable)?;
        object.release(retention)?;
        unit.save_object(object).await?;
        unit.commit().await?;
        Ok(())
    }

    pub async fn abandon(&self, id: &ObjectId) -> Result<(), ObjectStoreError> {
        let _access = self.storage.access(id).await?;
        let mut unit = (self.units)().await?;
        let mut object = unit.get_object(id).await?.ok_or(ObjectError::Unavailable)?;
        object.abandon()?;
        unit.save_object(object).await?;
        unit.commit().await?;
        Ok(())
    }

    pub async fn collect(&self, id: &ObjectId) -> Result<(), ObjectStoreError> {
        let mut access = self.storage.access(id).await?;
        let mut unit = (self.units)().await?;
        let mut object = unit.get_object(id).await?.ok_or(ObjectError::Unavailable)?;
        if object.collection() == CollectionState::Gone {
            return Ok(());
        }
        object.collect()?;
        unit.save_object(object).await?;
        unit.commit().await?;
        crate::checkpoint("deletion-claim");
        access.remove().await?;
        let mut unit = (self.units)().await?;
        let mut object = unit.get_object(id).await?.ok_or(ObjectError::Unavailable)?;
        object.finish_collection()?;
        unit.save_object(object).await?;
        unit.commit().await?;
        crate::checkpoint("deletion-complete");
        Ok(())
    }

    /// Validate bound objects, rediscover complete pending publications, and
    /// resume deletion. Incomplete candidates remain pinned for their producer.
    pub async fn recover(&self) -> Result<ObjectRecovery, ObjectStoreError> {
        let mut after = None;
        let mut recovered = ObjectRecovery::default();
        loop {
            let mut unit = (self.units)().await?;
            let page = unit.objects_after(after, 64).await?;
            drop(unit);
            if page.is_empty() {
                break;
            }
            after = page.last().map(|object| object.id().clone());
            for object in page {
                let mut access = self.storage.access(object.id()).await?;
                let object = self.load(object.id()).await?;
                if object.collection() == CollectionState::Gone {
                    continue;
                }
                match object.publication() {
                    PublicationState::Abandoned | PublicationState::Retired => {
                        drop(access);
                        self.collect(object.id()).await?;
                        recovered.collected = recovered.collected.saturating_add(1);
                    }
                    PublicationState::Pending => {
                        if access.inspect(object.maximum_bytes()).await?.is_some() {
                            recovered.ready = recovered.ready.saturating_add(1);
                        } else {
                            recovered.incomplete = recovered.incomplete.saturating_add(1);
                        }
                    }
                    PublicationState::Bound => {
                        let content = access
                            .inspect(object.maximum_bytes())
                            .await?
                            .ok_or(ObjectStoreError::Integrity)?;
                        if object.content() != Some(&content.identity) {
                            return Err(ObjectStoreError::Integrity);
                        }
                        recovered.validated = recovered.validated.saturating_add(1);
                    }
                }
            }
        }
        Ok(recovered)
    }

    pub async fn recover_publication(
        &self,
        id: &ObjectId,
    ) -> Result<Option<PreparedObject>, ObjectStoreError> {
        let mut access = self.storage.access(id).await?;
        let object = self.load(id).await?;
        if object.publication() != PublicationState::Pending
            || object.collection() != CollectionState::Available
        {
            return Err(ObjectError::Unavailable.into());
        }
        Ok(access
            .inspect(object.maximum_bytes())
            .await?
            .map(|content| PreparedObject {
                issuer: self.issuer.clone(),
                id: id.clone(),
                request: *object.request(),
                content,
            }))
    }

    /// Reuse an interrupted producer's original reservation when its exact
    /// payload size was discovered before publication. Foreign IDs fail closed.
    pub async fn reservation_limit(
        &self,
        id: &ObjectId,
        owner: &str,
        request: &[u8; 32],
    ) -> Result<Option<u64>, ObjectStoreError> {
        let mut unit = (self.units)().await?;
        let Some(object) = unit.get_object(id).await? else {
            return Ok(None);
        };
        if object.owner() != owner || object.request() != request {
            return Err(ObjectError::RequestConflict.into());
        }
        if object.collection() != CollectionState::Available
            || !matches!(
                object.publication(),
                PublicationState::Pending | PublicationState::Bound
            )
        {
            return Err(ObjectError::Unavailable.into());
        }
        Ok(Some(object.maximum_bytes()))
    }

    async fn load(&self, id: &ObjectId) -> Result<DurableObject, ObjectStoreError> {
        let mut unit = (self.units)().await?;
        Ok(unit.get_object(id).await?.ok_or(ObjectError::Unavailable)?)
    }
}
