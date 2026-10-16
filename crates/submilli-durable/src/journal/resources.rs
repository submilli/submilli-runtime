use super::*;

impl FilesystemJournal {
    pub async fn pending_resource(
        &self,
        workspace: &str,
    ) -> Result<Option<PendingResource>, FilesystemError> {
        Ok((self.units)().await?.pending_resource(workspace).await?)
    }
    pub async fn pending_writer(
        &self,
        workspace: &str,
    ) -> Result<Option<(String, String)>, FilesystemError> {
        let writer = (self.units)().await?.closing_writer(workspace).await?;
        writer
            .map(|w| Ok((w.id, w.close_effect.ok_or(StoreError::Conflict)?)))
            .transpose()
    }
    pub async fn reader(
        &self,
        workspace: &str,
        resource: &str,
        path: &str,
    ) -> Result<Option<ReaderPosition>, FilesystemError> {
        let (workspace, resource, path) =
            (workspace.to_owned(), resource.to_owned(), path.to_owned());
        self.transaction(move |unit| {
            Box::pin(async move {
                let state = require_active(unit, &workspace).await?;
                if let Some(reader) = unit.get_reader(&workspace, &resource).await? {
                    if reader.path != path {
                        return conflict();
                    }
                    if let Some(error) = reader.error {
                        return Err(FilesystemError::Rejected(error));
                    }
                    return Ok(reader.position);
                }
                require_idle(unit, &state).await?;
                require_resource_limit(unit, &workspace).await?;
                unit.save_reader(
                    &workspace,
                    Reader {
                        id: resource,
                        path,
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
        workspace: &str,
        resource: &str,
        error: String,
    ) -> Result<(), FilesystemError> {
        let (workspace, resource) = (workspace.to_owned(), resource.to_owned());
        self.transaction(move |unit| {
            Box::pin(async move {
                let mut reader = unit
                    .get_reader(&workspace, &resource)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                if reader.position.is_none() {
                    reader.error = Some(error);
                    unit.save_reader(&workspace, reader).await?;
                }
                Ok(())
            })
        })
        .await
    }
    pub async fn reader_opened(
        &self,
        workspace: &str,
        resource: &str,
        position: &ReaderPosition,
    ) -> Result<(), FilesystemError> {
        let (workspace, resource, position) =
            (workspace.to_owned(), resource.to_owned(), position.clone());
        self.transaction(move |unit| {
            Box::pin(async move {
                require_active(unit, &workspace).await?;
                let mut reader = unit
                    .get_reader(&workspace, &resource)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                if reader.position.is_none() {
                    reader.position = Some(position);
                    unit.save_reader(&workspace, reader).await?;
                }
                Ok(())
            })
        })
        .await
    }
    pub async fn reader_position(
        &self,
        workspace: &str,
        resource: &str,
    ) -> Result<ReaderPosition, FilesystemError> {
        let mut unit = (self.units)().await?;
        require_active(unit.as_mut(), workspace).await?;
        unit.get_reader(workspace, resource)
            .await?
            .and_then(|r| r.position)
            .ok_or_else(|| StoreError::Conflict.into())
    }
    pub async fn track_object(
        &self,
        workspace: &str,
        object: &str,
        request: [u8; 32],
    ) -> Result<(), FilesystemError> {
        let (workspace, object) = (workspace.to_owned(), object.to_owned());
        self.transaction(move |unit| {
            Box::pin(async move {
                require_active(unit, &workspace).await?;
                if let Some(prior) = unit.get_producer(&workspace, &object).await? {
                    return if prior.request == request {
                        Ok(())
                    } else {
                        conflict()
                    };
                }
                unit.save_producer(
                    &workspace,
                    Producer {
                        id: object,
                        request,
                    },
                )
                .await?;
                Ok(())
            })
        })
        .await
    }
    pub async fn listing(
        &self,
        workspace: &str,
        resource: &str,
        path: &str,
        recursive: bool,
        completed: Option<Content>,
    ) -> Result<Option<Content>, FilesystemError> {
        let (workspace, resource, path) =
            (workspace.to_owned(), resource.to_owned(), path.to_owned());
        self.transaction(move |unit| {
            Box::pin(async move {
                let state = require_active(unit, &workspace).await?;
                if let Some(mut listing) = unit.get_listing(&workspace, &resource).await? {
                    if listing.path != path || listing.recursive != recursive {
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
                        unit.save_listing(&workspace, listing).await?;
                        return Ok(Some(content));
                    }
                    return Ok(None);
                }
                if completed.is_some() {
                    return conflict();
                }
                require_idle(unit, &state).await?;
                require_resource_limit(unit, &workspace).await?;
                unit.save_listing(
                    &workspace,
                    Listing {
                        id: resource,
                        path,
                        recursive,
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
        workspace: &str,
        resource: &str,
        reason: String,
    ) -> Result<(), FilesystemError> {
        let (workspace, resource) = (workspace.to_owned(), resource.to_owned());
        self.transaction(move |unit| {
            Box::pin(async move {
                let mut listing = unit
                    .get_listing(&workspace, &resource)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                if listing.content.is_none() {
                    listing.error = Some(reason);
                    unit.save_listing(&workspace, listing).await?;
                }
                Ok(())
            })
        })
        .await
    }
    pub async fn open_writer(
        &self,
        workspace: &str,
        resource: &str,
        path: &str,
    ) -> Result<(), FilesystemError> {
        let (workspace, resource, path) =
            (workspace.to_owned(), resource.to_owned(), path.to_owned());
        self.transaction(move |unit| {
            Box::pin(async move {
                let state = require_active(unit, &workspace).await?;
                if let Some(prior) = unit.get_writer(&workspace, &resource).await? {
                    return if prior.path == path {
                        Ok(())
                    } else {
                        conflict()
                    };
                }
                require_idle(unit, &state).await?;
                require_resource_limit(unit, &workspace).await?;
                unit.save_writer(
                    &workspace,
                    Writer {
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
    pub async fn writer_chunk(
        &self,
        workspace: &str,
        resource: &str,
        chunk: &str,
        content: &Content,
    ) -> Result<(), FilesystemError> {
        let (workspace, resource, chunk, content) = (
            workspace.to_owned(),
            resource.to_owned(),
            chunk.to_owned(),
            content.clone(),
        );
        self.transaction(move |unit| {
            Box::pin(async move {
                let state = require_active(unit, &workspace).await?;
                if let Some(prior) = unit.get_chunk(&workspace, &resource, &chunk).await? {
                    return if prior.content == content {
                        Ok(())
                    } else {
                        conflict()
                    };
                }
                require_idle(unit, &state).await?;
                let writer = unit
                    .get_writer(&workspace, &resource)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                if writer.state != "open"
                    || unit.resource_counts(&workspace).await?.chunks >= MAX_EFFECTS
                {
                    return conflict();
                }
                let sequence = unit
                    .writer_chunks(&workspace, &resource, MAX_EFFECTS as u32 + 1)
                    .await?
                    .len();
                retain_content(
                    unit,
                    &state,
                    &Mutation::Write {
                        path: "/".into(),
                        content: content.clone(),
                    },
                )
                .await?;
                unit.save_chunk(
                    &workspace,
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
        workspace: &str,
        resource: &str,
        effect: &str,
        maximum_bytes: u64,
    ) -> Result<WriterRecord, FilesystemError> {
        self.writer_close_record(workspace, resource, effect, maximum_bytes, true)
            .await
    }
    pub async fn writer_for_close(
        &self,
        workspace: &str,
        resource: &str,
        effect: &str,
        maximum_bytes: u64,
    ) -> Result<WriterRecord, FilesystemError> {
        self.writer_close_record(workspace, resource, effect, maximum_bytes, false)
            .await
    }
    async fn writer_close_record(
        &self,
        workspace: &str,
        resource: &str,
        effect: &str,
        maximum_bytes: u64,
        admit: bool,
    ) -> Result<WriterRecord, FilesystemError> {
        let (workspace, resource, effect) =
            (workspace.to_owned(), resource.to_owned(), effect.to_owned());
        self.transaction(move |unit| {
            Box::pin(async move {
                let state = require_active(unit, &workspace).await?;
                let mut writer = unit
                    .get_writer(&workspace, &resource)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                if writer
                    .close_effect
                    .as_ref()
                    .is_some_and(|prior| *prior != effect)
                {
                    return conflict();
                }
                let chunks = unit
                    .writer_chunks(&workspace, &resource, MAX_EFFECTS as u32 + 1)
                    .await?;
                if chunks.len() > MAX_EFFECTS as usize {
                    return conflict();
                }
                let bytes = chunks
                    .iter()
                    .try_fold(0u64, |sum, chunk| sum.checked_add(chunk.content.bytes));
                if bytes.is_none_or(|bytes| bytes > maximum_bytes) {
                    return conflict();
                }
                if writer.close_effect.is_none() {
                    require_idle(unit, &state).await?;
                    require_capacity(unit, &state.run, 1).await?;
                    sequences::require_unused_effect(unit, &state, &effect).await?;
                    if admit {
                        writer.close_effect = Some(effect);
                        writer.state = "closing".into();
                        unit.save_writer(&workspace, writer.clone()).await?;
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
    pub async fn writer_closed(
        &self,
        workspace: &str,
        resource: &str,
    ) -> Result<(), FilesystemError> {
        let (workspace, resource) = (workspace.to_owned(), resource.to_owned());
        self.transaction(move |unit| {
            Box::pin(async move {
                let mut writer = unit
                    .get_writer(&workspace, &resource)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                writer.state = "closed".into();
                unit.save_writer(&workspace, writer).await?;
                Ok(())
            })
        })
        .await
    }
}
