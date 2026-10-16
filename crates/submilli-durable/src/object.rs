//! Immutable payloads and the retention that makes their collection safe.
use std::collections::BTreeSet;

const MAX_OWNER_BYTES: usize = 1024;
const MAX_RETENTION_ROOTS: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectStoreIdentity(ObjectId);

impl ObjectStoreIdentity {
    pub fn parse(value: String) -> Result<Self, ObjectError> {
        Ok(Self(ObjectId::parse(value)?))
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId(String);

impl ObjectId {
    pub fn parse(value: String) -> Result<Self, ObjectError> {
        let id = uuid::Uuid::parse_str(&value).map_err(|_| ObjectError::InvalidIdentity)?;
        if id.hyphenated().to_string() != value {
            return Err(ObjectError::InvalidIdentity);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentIdentity {
    pub bytes: u64,
    pub sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectReference {
    pub id: ObjectId,
    pub content: ContentIdentity,
}

/// Releasing a response does not release a run's reconstruction inputs.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Retention {
    Run(String),
    Response(String),
    Workspace(String),
    Backup(String),
}

impl Retention {
    fn validate(&self) -> Result<(), ObjectError> {
        let (Self::Run(id) | Self::Response(id) | Self::Workspace(id) | Self::Backup(id)) = self;
        if id.is_empty() || id.len() > MAX_OWNER_BYTES {
            return Err(ObjectError::InvalidState);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicationState {
    Pending,
    Bound,
    Abandoned,
    Retired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionState {
    Available,
    Deleting,
    Gone,
}

#[derive(Debug, thiserror::Error)]
pub enum ObjectError {
    #[error("invalid durable object identity")]
    InvalidIdentity,
    #[error("durable object does not match its immutable request")]
    RequestConflict,
    #[error("durable object is not available for this transition")]
    Unavailable,
    #[error("durable object has no retention owner")]
    Unretained,
    #[error("durable object exceeds its reservation")]
    TooLarge,
    #[error("durable object revision exhausted")]
    RevisionExhausted,
    #[error("invalid durable object state")]
    InvalidState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DurableObject {
    id: ObjectId,
    owner: String,
    request: [u8; 32],
    maximum_bytes: u64,
    reserved_bytes: u64,
    content: Option<ContentIdentity>,
    publication: PublicationState,
    collection: CollectionState,
    retention: BTreeSet<Retention>,
    revision: i64,
}

impl DurableObject {
    pub fn reserve(
        id: ObjectId,
        owner: String,
        request: [u8; 32],
        maximum_bytes: u64,
        reserved_bytes: u64,
    ) -> Result<Self, ObjectError> {
        if owner.is_empty()
            || owner.len() > MAX_OWNER_BYTES
            || reserved_bytes < maximum_bytes
            || reserved_bytes > i64::MAX as u64
        {
            return Err(ObjectError::InvalidState);
        }
        Ok(Self {
            id,
            owner,
            request,
            maximum_bytes,
            reserved_bytes,
            content: None,
            publication: PublicationState::Pending,
            collection: CollectionState::Available,
            retention: BTreeSet::new(),
            revision: 0,
        })
    }

    pub fn id(&self) -> &ObjectId {
        &self.id
    }
    pub fn owner(&self) -> &str {
        &self.owner
    }
    pub fn request(&self) -> &[u8; 32] {
        &self.request
    }
    pub fn maximum_bytes(&self) -> u64 {
        self.maximum_bytes
    }
    pub fn reserved_bytes(&self) -> u64 {
        self.reserved_bytes
    }
    pub fn content(&self) -> Option<&ContentIdentity> {
        self.content.as_ref()
    }
    pub fn publication(&self) -> PublicationState {
        self.publication
    }
    pub fn collection(&self) -> CollectionState {
        self.collection
    }
    pub fn retention(&self) -> &BTreeSet<Retention> {
        &self.retention
    }
    pub fn revision(&self) -> i64 {
        self.revision
    }

    pub fn matches_request(&self, other: &Self) -> Result<(), ObjectError> {
        if self.owner != other.owner
            || self.request != other.request
            || self.maximum_bytes != other.maximum_bytes
        {
            return Err(ObjectError::RequestConflict);
        }
        Ok(())
    }

    /// The caller establishes publication durability before invoking this transition.
    /// Its transaction also stores the consumer's reference and accounting.
    pub fn bind(
        &mut self,
        content: ContentIdentity,
        stored_bytes: u64,
        retention: BTreeSet<Retention>,
    ) -> Result<(), ObjectError> {
        validate_retention(&retention)?;
        if self.collection != CollectionState::Available {
            return Err(ObjectError::Unavailable);
        }
        if self.publication == PublicationState::Bound
            && self.content.as_ref() == Some(&content)
            && retention.is_subset(&self.retention)
        {
            return Ok(());
        }
        if self.publication != PublicationState::Pending {
            return Err(ObjectError::Unavailable);
        }
        if retention.is_empty() {
            return Err(ObjectError::Unretained);
        }
        if content.bytes > self.maximum_bytes
            || stored_bytes > self.reserved_bytes
            || stored_bytes < content.bytes
        {
            return Err(ObjectError::TooLarge);
        }
        self.advance()?;
        self.reserved_bytes = stored_bytes;
        self.content = Some(content);
        self.retention = retention;
        self.publication = PublicationState::Bound;
        Ok(())
    }

    pub fn reference(&self) -> Result<ObjectReference, ObjectError> {
        if self.publication != PublicationState::Bound
            || self.collection != CollectionState::Available
        {
            return Err(ObjectError::Unavailable);
        }
        let content = self.content.clone().ok_or(ObjectError::InvalidState)?;
        Ok(ObjectReference {
            id: self.id.clone(),
            content,
        })
    }

    pub fn retain(&mut self, retention: Retention) -> Result<(), ObjectError> {
        retention.validate()?;
        self.reference()?;
        if !self.retention.contains(&retention) {
            if self.retention.len() >= MAX_RETENTION_ROOTS {
                return Err(ObjectError::TooLarge);
            }
            self.advance()?;
            self.retention.insert(retention);
        }
        Ok(())
    }

    pub fn release(&mut self, retention: &Retention) -> Result<(), ObjectError> {
        if !self.retention.contains(retention) {
            return Ok(());
        }
        self.advance()?;
        self.retention.remove(retention);
        if self.retention.is_empty() {
            self.publication = PublicationState::Retired;
        }
        Ok(())
    }

    /// Requires exclusion and draining of the publication writer.
    pub fn abandon(&mut self) -> Result<(), ObjectError> {
        if self.publication == PublicationState::Abandoned {
            return Ok(());
        }
        if self.publication != PublicationState::Pending {
            return Err(ObjectError::Unavailable);
        }
        self.advance()?;
        self.publication = PublicationState::Abandoned;
        Ok(())
    }

    /// The adapter excludes readers/writers before committing this claim.
    pub fn collect(&mut self) -> Result<(), ObjectError> {
        if self.collection != CollectionState::Available {
            return Ok(());
        }
        if !matches!(
            self.publication,
            PublicationState::Abandoned | PublicationState::Retired
        ) || !self.retention.is_empty()
        {
            return Err(ObjectError::Unavailable);
        }
        self.advance()?;
        self.collection = CollectionState::Deleting;
        Ok(())
    }

    /// Requires acknowledged durable removal of both staging and final names.
    pub fn finish_collection(&mut self) -> Result<(), ObjectError> {
        if self.collection == CollectionState::Gone {
            return Ok(());
        }
        if self.collection != CollectionState::Deleting {
            return Err(ObjectError::Unavailable);
        }
        self.advance()?;
        self.collection = CollectionState::Gone;
        Ok(())
    }

    fn advance(&mut self) -> Result<(), ObjectError> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(ObjectError::RevisionExhausted)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        id: ObjectId,
        owner: String,
        request: [u8; 32],
        maximum_bytes: u64,
        reserved_bytes: u64,
        content: Option<ContentIdentity>,
        publication: PublicationState,
        collection: CollectionState,
        retention: BTreeSet<Retention>,
        revision: i64,
    ) -> Result<Self, ObjectError> {
        let mut object = Self::reserve(
            id,
            owner,
            request,
            maximum_bytes,
            reserved_bytes.max(maximum_bytes),
        )?;
        validate_retention(&retention)?;
        if revision < 0
            || content
                .as_ref()
                .is_some_and(|c| c.bytes > maximum_bytes || c.bytes > reserved_bytes)
            || (content.is_none() && reserved_bytes < maximum_bytes)
            || (matches!(
                publication,
                PublicationState::Bound | PublicationState::Retired
            ) != content.is_some())
            || (publication == PublicationState::Bound) == retention.is_empty()
            || (collection != CollectionState::Available
                && !matches!(
                    publication,
                    PublicationState::Abandoned | PublicationState::Retired
                ))
        {
            return Err(ObjectError::InvalidState);
        }
        object.content = content;
        object.reserved_bytes = reserved_bytes;
        object.publication = publication;
        object.collection = collection;
        object.retention = retention;
        object.revision = revision;
        Ok(object)
    }
}

fn validate_retention(retention: &BTreeSet<Retention>) -> Result<(), ObjectError> {
    if retention.len() > MAX_RETENTION_ROOTS {
        return Err(ObjectError::TooLarge);
    }
    for root in retention {
        root.validate()?;
    }
    Ok(())
}
