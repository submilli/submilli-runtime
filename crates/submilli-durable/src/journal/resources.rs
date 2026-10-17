use super::*;
use crate::state::{ListingPosition, Progress};
use interpreter::runtime::filesystem::{ReadPosition, ReaderOptions};

impl FilesystemJournal {
    pub async fn reader(
        &self,
        scope: &str,
        resource: &str,
        path: &str,
        options: &ReaderOptions,
    ) -> Result<Option<ReaderPosition>, FilesystemError> {
        let (scope, resource, path) = (scope.to_owned(), resource.to_owned(), path.to_owned());
        let options = options.clone();
        let binding = self.binding;
        self.transaction(move |unit| {
            Box::pin(async move {
                if let Some(reader) = unit.get_reader(&scope, &resource).await? {
                    if reader.binding != binding || reader.path != path || reader.options != options
                    {
                        return conflict();
                    }
                    if let Some(error) = reader.error {
                        return Err(FilesystemError::Rejected(error));
                    }
                    return Ok(reader.position);
                }
                require_idle(unit, &scope).await?;
                require_resource_limit(unit, &scope).await?;
                unit.save_reader(
                    &scope,
                    Reader {
                        binding,
                        id: resource,
                        path,
                        options,
                        progress: None,
                        position: None,
                        error: None,
                    },
                )
                .await?;
                Ok(None)
            })
        })
        .await
    }
    pub async fn reject_reader(
        &self,
        scope: &str,
        resource: &str,
        error: String,
    ) -> Result<(), FilesystemError> {
        let (scope, resource) = (scope.to_owned(), resource.to_owned());
        let binding = self.binding;
        self.transaction(move |unit| {
            Box::pin(async move {
                let mut reader = unit
                    .get_reader(&scope, &resource)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                if reader.binding != binding {
                    return conflict();
                }
                if reader.position.is_none() {
                    reader.error = Some(error);
                    unit.save_reader(&scope, reader).await?;
                }
                Ok(())
            })
        })
        .await
    }
    pub async fn reader_opened(
        &self,
        scope: &str,
        resource: &str,
        position: &ReaderPosition,
    ) -> Result<(), FilesystemError> {
        let (scope, resource, position) = (scope.to_owned(), resource.to_owned(), position.clone());
        let binding = self.binding;
        self.transaction(move |unit| {
            Box::pin(async move {
                let mut reader = unit
                    .get_reader(&scope, &resource)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                if reader.binding != binding {
                    return conflict();
                }
                if reader.position.is_none() {
                    reader.position = Some(position);
                    unit.save_reader(&scope, reader).await?;
                }
                Ok(())
            })
        })
        .await
    }
    pub async fn reader_record(
        &self,
        scope: &str,
        resource: &str,
    ) -> Result<Reader, FilesystemError> {
        let mut unit = (self.units)().await?;
        unit.get_reader(scope, resource)
            .await?
            .filter(|r| r.binding == self.binding && r.position.is_some())
            .ok_or_else(|| StoreError::Conflict.into())
    }
    pub async fn reader_progress(
        &self,
        scope: &str,
        resource: &str,
        expected: Option<Progress<ReadPosition>>,
        progress: Progress<ReadPosition>,
    ) -> Result<(), FilesystemError> {
        let (scope, resource, binding) = (scope.to_owned(), resource.to_owned(), self.binding);
        self.transaction(move |unit| {
            Box::pin(async move {
                let mut reader = unit
                    .get_reader(&scope, &resource)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                if reader.binding != binding || reader.progress != expected {
                    return conflict();
                }
                reader.progress = Some(progress);
                unit.save_reader(&scope, reader).await?;
                Ok(())
            })
        })
        .await
    }
    pub async fn listing_bytes(&self, scope: &str, resource: &str) -> Result<u64, FilesystemError> {
        let mut unit = (self.units)().await?;
        Ok(unit.listing_bytes(scope, resource).await?)
    }
    pub async fn listing_record(
        &self,
        scope: &str,
        resource: &str,
    ) -> Result<Listing, FilesystemError> {
        let mut unit = (self.units)().await?;
        unit.get_listing(scope, resource)
            .await?
            .filter(|r| r.binding == self.binding && r.content.is_some())
            .ok_or_else(|| StoreError::Conflict.into())
    }
    pub async fn listing_progress(
        &self,
        scope: &str,
        resource: &str,
        expected: Option<Progress<ListingPosition>>,
        progress: Progress<ListingPosition>,
    ) -> Result<(), FilesystemError> {
        let (scope, resource, binding) = (scope.to_owned(), resource.to_owned(), self.binding);
        self.transaction(move |unit| {
            Box::pin(async move {
                let mut listing = unit
                    .get_listing(&scope, &resource)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                if listing.binding != binding || listing.progress != expected {
                    return conflict();
                }
                listing.progress = Some(progress);
                unit.save_listing(&scope, listing).await?;
                Ok(())
            })
        })
        .await
    }
    pub async fn writer_is_closed(
        &self,
        scope: &str,
        resource: &str,
    ) -> Result<bool, FilesystemError> {
        let mut unit = (self.units)().await?;
        let writer = unit
            .get_writer(scope, resource)
            .await?
            .ok_or(StoreError::Conflict)?;
        if writer.binding != self.binding {
            return conflict();
        }
        Ok(writer.state == "closed")
    }
    pub async fn listing(
        &self,
        scope: &str,
        resource: &str,
        path: &str,
        recursive: bool,
        completed: Option<Vec<DirectoryEntry>>,
    ) -> Result<Option<Vec<DirectoryEntry>>, FilesystemError> {
        let (scope, resource, path) = (scope.to_owned(), resource.to_owned(), path.to_owned());
        let binding = self.binding;
        self.transaction(move |unit| {
            Box::pin(async move {
                if let Some(mut listing) = unit.get_listing(&scope, &resource).await? {
                    if listing.binding != binding
                        || listing.path != path
                        || listing.recursive != recursive
                    {
                        return conflict();
                    }
                    if let Some(error) = listing.error {
                        return Err(FilesystemError::Rejected(error));
                    }
                    if let Some(content) = listing.content {
                        if completed.as_ref().is_some_and(|v| *v != content) {
                            return conflict();
                        }
                        return Ok(Some(content));
                    }
                    if let Some(content) = completed {
                        listing.content = Some(content.clone());
                        unit.save_listing(&scope, listing).await?;
                        return Ok(Some(content));
                    }
                    return Ok(None);
                }
                if completed.is_some() {
                    return conflict();
                }
                require_idle(unit, &scope).await?;
                require_resource_limit(unit, &scope).await?;
                unit.save_listing(
                    &scope,
                    Listing {
                        binding,
                        id: resource,
                        path,
                        recursive,
                        progress: None,
                        content: None,
                        error: None,
                    },
                )
                .await?;
                Ok(None)
            })
        })
        .await
    }
    pub async fn reject_listing(
        &self,
        scope: &str,
        resource: &str,
        reason: String,
    ) -> Result<(), FilesystemError> {
        let (scope, resource) = (scope.to_owned(), resource.to_owned());
        let binding = self.binding;
        self.transaction(move |unit| {
            Box::pin(async move {
                let mut listing = unit
                    .get_listing(&scope, &resource)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                if listing.binding != binding {
                    return conflict();
                }
                if listing.content.is_none() {
                    listing.error = Some(reason);
                    unit.save_listing(&scope, listing).await?;
                }
                Ok(())
            })
        })
        .await
    }
    pub async fn open_writer(
        &self,
        scope: &str,
        resource: &str,
        path: &str,
    ) -> Result<(), FilesystemError> {
        let (scope, resource, path) = (scope.to_owned(), resource.to_owned(), path.to_owned());
        let binding = self.binding;
        self.transaction(move |unit| {
            Box::pin(async move {
                if let Some(prior) = unit.get_writer(&scope, &resource).await? {
                    return if prior.binding == binding && prior.path == path {
                        Ok(())
                    } else {
                        conflict()
                    };
                }
                require_idle(unit, &scope).await?;
                require_resource_limit(unit, &scope).await?;
                unit.save_writer(
                    &scope,
                    Writer {
                        binding,
                        id: resource,
                        path,
                        close_effect: None,
                        state: "open".into(),
                    },
                )
                .await?;
                Ok(())
            })
        })
        .await
    }
    pub async fn chunk_position(
        &self,
        scope: &str,
        resource: &str,
        chunk: &str,
        content: &Content,
    ) -> Result<(String, bool, u64), FilesystemError> {
        let binding = self.binding;
        let mut unit = (self.units)().await?;
        let writer = unit
            .get_writer(scope, resource)
            .await?
            .ok_or(StoreError::Conflict)?;
        if writer.binding != binding {
            return conflict();
        }
        if let Some(prior) = unit.get_chunk(scope, resource, chunk).await? {
            if prior.content != *content {
                return conflict();
            }
            return Ok((writer.path, true, 0));
        }
        require_idle(unit.as_mut(), scope).await?;
        if writer.state != "open" || unit.resource_counts(scope).await?.chunks >= MAX_OPERATIONS {
            return conflict();
        }
        let chunks = unit
            .writer_chunks(scope, resource, MAX_OPERATIONS as u32 + 1)
            .await?;
        let offset = chunks
            .iter()
            .try_fold(0u64, |sum, chunk| sum.checked_add(chunk.content.bytes))
            .ok_or(StoreError::Conflict)?;
        Ok((writer.path, false, offset))
    }
    pub async fn writer_outcome(
        &self,
        scope: &str,
        effect: &str,
    ) -> Result<Option<RecordedOutcome>, FilesystemError> {
        let binding = self.binding;
        let mut unit = (self.units)().await?;
        Ok(load_operation(unit.as_mut(), scope, effect, binding)
            .await?
            .and_then(|effect| effect.outcome))
    }
    pub async fn writer_chunk(
        &self,
        scope: &str,
        resource: &str,
        chunk: &str,
        content: &Content,
    ) -> Result<(), FilesystemError> {
        let (scope, resource, chunk, content) = (
            scope.to_owned(),
            resource.to_owned(),
            chunk.to_owned(),
            content.clone(),
        );
        let binding = self.binding;
        self.transaction(move |unit| {
            Box::pin(async move {
                let writer = unit
                    .get_writer(&scope, &resource)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                if writer.binding != binding {
                    return conflict();
                }
                if let Some(prior) = unit.get_chunk(&scope, &resource, &chunk).await? {
                    return if prior.content == content {
                        Ok(())
                    } else {
                        conflict()
                    };
                }
                require_idle(unit, &scope).await?;
                if writer.state != "open"
                    || unit.resource_counts(&scope).await?.chunks >= MAX_OPERATIONS
                {
                    return conflict();
                }
                let sequence = unit
                    .writer_chunks(&scope, &resource, MAX_OPERATIONS as u32 + 1)
                    .await?
                    .len();
                unit.save_chunk(
                    &scope,
                    Chunk {
                        writer: resource,
                        id: chunk,
                        sequence: u32::try_from(sequence).map_err(invalid)?,
                        content,
                    },
                )
                .await?;
                Ok(())
            })
        })
        .await
    }
    pub async fn closing_writer(
        &self,
        scope: &str,
        resource: &str,
        effect: &str,
        maximum_bytes: u64,
    ) -> Result<WriterRecord, FilesystemError> {
        self.writer_close_record(scope, resource, effect, maximum_bytes, true)
            .await
    }
    pub async fn writer_for_close(
        &self,
        scope: &str,
        resource: &str,
        effect: &str,
        maximum_bytes: u64,
    ) -> Result<WriterRecord, FilesystemError> {
        self.writer_close_record(scope, resource, effect, maximum_bytes, false)
            .await
    }
    async fn writer_close_record(
        &self,
        scope: &str,
        resource: &str,
        effect: &str,
        maximum_bytes: u64,
        admit: bool,
    ) -> Result<WriterRecord, FilesystemError> {
        let (scope, resource, effect) = (scope.to_owned(), resource.to_owned(), effect.to_owned());
        let binding = self.binding;
        self.transaction(move |unit| {
            Box::pin(async move {
                let mut writer = unit
                    .get_writer(&scope, &resource)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                if writer.binding != binding {
                    return conflict();
                }
                if writer
                    .close_effect
                    .as_ref()
                    .is_some_and(|prior| *prior != effect)
                {
                    return conflict();
                }
                let chunks = unit
                    .writer_chunks(&scope, &resource, MAX_OPERATIONS as u32 + 1)
                    .await?;
                if chunks.len() > MAX_OPERATIONS as usize {
                    return conflict();
                }
                let bytes = chunks
                    .iter()
                    .try_fold(0u64, |sum, chunk| sum.checked_add(chunk.content.bytes));
                if bytes.is_none_or(|bytes| bytes > maximum_bytes) {
                    return conflict();
                }
                if writer.close_effect.is_none() {
                    require_idle(unit, &scope).await?;
                    require_capacity(unit, &scope).await?;
                    require_unused_operation(unit, &scope, &effect).await?;
                    if admit {
                        writer.close_effect = Some(effect);
                        writer.state = "closing".into();
                        unit.save_writer(&scope, writer.clone()).await?;
                    }
                }
                Ok(WriterRecord {
                    path: writer.path,
                    chunks: chunks.into_iter().map(|chunk| chunk.content).collect(),
                })
            })
        })
        .await
    }
    pub async fn writer_closed(&self, scope: &str, resource: &str) -> Result<(), FilesystemError> {
        let (scope, resource) = (scope.to_owned(), resource.to_owned());
        let binding = self.binding;
        self.transaction(move |unit| {
            Box::pin(async move {
                let mut writer = unit
                    .get_writer(&scope, &resource)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                if writer.binding != binding {
                    return conflict();
                }
                writer.state = "closed".into();
                unit.save_writer(&scope, writer).await?;
                Ok(())
            })
        })
        .await
    }
}
