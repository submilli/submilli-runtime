use super::*;
use crate::domain::durable_object::Retention;
use interpreter::runtime::fs::recovery::{DirectoryEntry, MAX_LISTING_BYTES};
use std::collections::BTreeSet;

pub struct DirectorySnapshot {
    entries: std::vec::IntoIter<DirectoryEntry>,
    offset: usize,
}
impl DirectorySnapshot {
    pub fn position(&self) -> usize {
        self.offset
    }
}
impl Iterator for DirectorySnapshot {
    type Item = DirectoryEntry;
    fn next(&mut self) -> Option<Self::Item> {
        let entry = self.entries.next()?;
        self.offset += 1;
        Some(entry)
    }
}

impl DurableFilesystem {
    /// `offset` comes from the same recorded observation as the iterator's
    /// resource ID. The initial enumeration is captured once in the object
    /// store, so later mutations cannot change replayed order or metadata.
    pub async fn open_directory(
        &self,
        resource: &str,
        path: &str,
        recursive: bool,
        offset: usize,
    ) -> Result<DirectorySnapshot, FilesystemError> {
        validate_id(resource)?;
        Mutation::Remove { path: path.into() }.validate()?;
        let gate = Arc::new(self.objects.filesystem_gate.clone().lock_owned().await);
        let content = match self
            .journal
            .listing(&self.id, resource, path, recursive, None)
            .await?
        {
            Some(content) => content,
            None => {
                self.capture_listing(gate.clone(), resource, path, recursive)
                    .await?
            }
        };
        if content.bytes > MAX_LISTING_BYTES as u64 {
            return Err(RecoveryError::Limit.into());
        }
        let reference = ObjectReference {
            id: ObjectId::parse(content.id).map_err(ObjectStoreError::from)?,
            content: ContentIdentity {
                bytes: content.bytes,
                sha256: content.sha256,
            },
        };
        let reader = self.objects.stream(reference);
        let entries: Vec<DirectoryEntry> = blocking(gate, move || {
            let mut bytes = Vec::new();
            reader
                .take(MAX_LISTING_BYTES as u64 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > MAX_LISTING_BYTES {
                return Err(RecoveryError::Limit);
            }
            serde_json::from_slice(&bytes).map_err(|_| RecoveryError::InvalidRecord)
        })
        .await?;
        if offset > entries.len() {
            return Err(RecoveryError::InvalidRecord.into());
        }
        let mut entries = entries.into_iter();
        for _ in 0..offset {
            entries.next();
        }
        Ok(DirectorySnapshot { entries, offset })
    }

    async fn capture_listing(
        &self,
        gate: Arc<tokio::sync::OwnedMutexGuard<()>>,
        resource: &str,
        path: &str,
        recursive: bool,
    ) -> Result<Content, FilesystemError> {
        let id = ObjectId::parse(workspace_id(&format!("listing:{resource}"), &self.id))
            .map_err(ObjectStoreError::from)?;
        let fingerprint = Sha256::digest(
            serde_json::to_vec(&(path, recursive)).map_err(|_| RecoveryError::InvalidRecord)?,
        )
        .into();
        self.journal
            .track_object(&self.id, id.as_str(), fingerprint)
            .await?;
        let reserved = self
            .objects
            .reservation_limit(&id, &self.owner, &fingerprint)
            .await?;
        let reference = match self.objects.reference(&id).await {
            Ok(reference) => reference,
            Err(ObjectStoreError::State(
                crate::domain::durable_object::ObjectError::Unavailable,
            )) => {
                let recovered = if reserved.is_some() {
                    self.objects.recover_publication(&id).await?
                } else {
                    None
                };
                let prepared = if let Some(prepared) = recovered {
                    prepared
                } else {
                    let bytes = self
                        .enumerate_listing(gate, resource, path, recursive)
                        .await?;
                    let result = self
                        .objects
                        .reserve(
                            id.clone(),
                            self.owner.clone(),
                            fingerprint,
                            reserved.unwrap_or(bytes.len() as u64),
                        )
                        .await;
                    if matches!(result, Err(ObjectStoreError::Limit)) {
                        return self
                            .reject_listing(
                                resource,
                                "directory capture exceeds available object quota".into(),
                            )
                            .await;
                    }
                    result?;
                    self.objects
                        .publish(&id, std::io::Cursor::new(bytes))
                        .await?
                };
                self.objects
                    .bind(
                        &prepared,
                        BTreeSet::from([Retention::Workspace(self.id.clone())]),
                    )
                    .await?
            }
            Err(error) => return Err(error.into()),
        };
        let content = Content {
            id: reference.id.as_str().into(),
            bytes: reference.content.bytes,
            sha256: reference.content.sha256,
        };
        self.journal
            .listing(&self.id, resource, path, recursive, Some(content.clone()))
            .await?;
        Ok(content)
    }

    async fn enumerate_listing(
        &self,
        gate: Arc<tokio::sync::OwnedMutexGuard<()>>,
        resource: &str,
        path: &str,
        recursive: bool,
    ) -> Result<Vec<u8>, FilesystemError> {
        let workspace = self.workspace.clone();
        let path = path.to_owned();
        let listing = blocking(gate, move || workspace.directory_listing(&path, recursive)).await;
        let listing = match listing {
            Ok(listing) => listing,
            Err(FilesystemError::Recovery(error)) if preflight_refusal(&error) => {
                return self.reject_listing(resource, error.to_string()).await;
            }
            Err(error) => return Err(error),
        };
        let bytes = serde_json::to_vec(&listing).map_err(|_| RecoveryError::InvalidRecord)?;
        if bytes.len() as u64 > self.objects.maximum_bytes().min(MAX_LISTING_BYTES as u64) {
            return self
                .reject_listing(
                    resource,
                    "directory snapshot exceeds the object size limit".into(),
                )
                .await;
        }
        Ok(bytes)
    }

    async fn reject_listing<T>(
        &self,
        resource: &str,
        reason: String,
    ) -> Result<T, FilesystemError> {
        self.journal
            .reject_listing(&self.id, resource, reason.clone())
            .await?;
        Err(FilesystemError::Rejected(reason))
    }
}
