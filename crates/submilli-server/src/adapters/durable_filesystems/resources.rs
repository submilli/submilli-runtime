use super::*;
use interpreter::runtime::fs::recovery::{Content, ReaderPosition};

pub(crate) struct WriterRecord {
    pub path: String,
    pub chunks: Vec<Content>,
}

pub(crate) enum PendingResource {
    Reader {
        id: String,
        path: String,
    },
    Listing {
        id: String,
        path: String,
        recursive: bool,
    },
}

impl FilesystemJournal {
    pub async fn pending_resource(
        &self,
        workspace: &str,
    ) -> Result<Option<PendingResource>, FilesystemError> {
        let workspace = workspace.to_owned();
        let sealer = self.sealer.clone();
        self.database.read(move |connection| Box::pin(async move {
            if let Some(row) = sqlx::query("SELECT resource_id,request FROM durable_filesystem_readers WHERE workspace_id=? AND position IS NULL AND error IS NULL ORDER BY resource_id LIMIT 1")
                .bind(&workspace).fetch_optional(&mut *connection).await? {
                let id: String = row.try_get("resource_id")?;
                let path = decode(&sealer, &resource_owner(&workspace, &id), &row.try_get::<Vec<u8>, _>("request")?)?;
                return Ok(Some(PendingResource::Reader { id, path }));
            }
            if let Some(row) = sqlx::query("SELECT resource_id,request FROM durable_filesystem_listings WHERE workspace_id=? AND content IS NULL AND error IS NULL ORDER BY resource_id LIMIT 1")
                .bind(&workspace).fetch_optional(connection).await? {
                let id: String = row.try_get("resource_id")?;
                let (path, recursive) = decode(&sealer, &format!("listing:{workspace}:{id}"), &row.try_get::<Vec<u8>, _>("request")?)?;
                return Ok(Some(PendingResource::Listing { id, path, recursive }));
            }
            Ok(None)
        })).await.map_err(Into::into)
    }
    pub async fn pending_writer(
        &self,
        workspace: &str,
    ) -> Result<Option<(String, String)>, FilesystemError> {
        let workspace = workspace.to_owned();
        self.database.read(move |connection| Box::pin(async move {
            Ok(sqlx::query_as("SELECT resource_id,close_effect FROM durable_filesystem_writers WHERE workspace_id=? AND state='closing' ORDER BY resource_id LIMIT 1")
                .bind(workspace).fetch_optional(connection).await?)
        })).await.map_err(Into::into)
    }
    pub async fn reader(
        &self,
        workspace: &str,
        resource: &str,
        path: &str,
    ) -> Result<Option<ReaderPosition>, FilesystemError> {
        let (workspace, resource, path) =
            (workspace.to_owned(), resource.to_owned(), path.to_owned());
        let sealer = self.sealer.clone();
        self.database.transaction(move |connection| Box::pin(async move {
            require_active(connection, &workspace).await?;
            let key = resource_owner(&workspace, &resource);
            let row = sqlx::query("SELECT request,position,error FROM durable_filesystem_readers WHERE workspace_id=? AND resource_id=?")
                .bind(&workspace).bind(&resource).fetch_optional(&mut *connection).await?;
            if let Some(row) = row {
                if decode::<String>(&sealer, &key, &row.try_get::<Vec<u8>, _>("request")?)? != path { return Err(DatabaseError::SessionConflict); }
                if let Some(error) = row.try_get::<Option<Vec<u8>>, _>("error")? {
                    return Ok(Err(decode::<String>(&sealer, &key, &error)?));
                }
                return Ok(Ok(row.try_get::<Option<Vec<u8>>, _>("position")?.map(|bytes| decode(&sealer, &key, &bytes)).transpose()?));
            }
            require_idle(connection, &workspace).await?;
            require_resource_limit(connection, &workspace).await?;
            let request = encode(&sealer, &key, &path)?;
            sqlx::query("INSERT INTO durable_filesystem_readers(workspace_id,resource_id,request) VALUES(?,?,?)")
                .bind(workspace).bind(resource).bind(request).execute(connection).await?;
            Ok(Ok(None))
        })).await?.map_err(FilesystemError::Rejected)
    }

    pub async fn reject_reader(
        &self,
        workspace: &str,
        resource: &str,
        error: String,
    ) -> Result<(), FilesystemError> {
        let (workspace, resource) = (workspace.to_owned(), resource.to_owned());
        let sealer = self.sealer.clone();
        self.database.transaction(move |connection| Box::pin(async move {
            let error = encode(&sealer, &resource_owner(&workspace, &resource), &error)?;
            sqlx::query("UPDATE durable_filesystem_readers SET error=? WHERE workspace_id=? AND resource_id=? AND position IS NULL")
                .bind(error).bind(workspace).bind(resource).execute(connection).await?;
            Ok(())
        })).await.map_err(Into::into)
    }

    pub async fn track_object(
        &self,
        workspace: &str,
        object: &str,
        request: [u8; 32],
    ) -> Result<(), FilesystemError> {
        let (workspace, object) = (workspace.to_owned(), object.to_owned());
        self.database.transaction(move |connection| Box::pin(async move {
            require_active(connection, &workspace).await?;
            let prior: Option<Vec<u8>> = sqlx::query_scalar("SELECT request FROM durable_filesystem_objects WHERE workspace_id=? AND object_id=?")
                .bind(&workspace).bind(&object).fetch_optional(&mut *connection).await?;
            if let Some(prior) = prior { return if prior == request { Ok(()) } else { Err(DatabaseError::SessionConflict) }; }
            sqlx::query("INSERT INTO durable_filesystem_objects(workspace_id,object_id,request) VALUES(?,?,?)")
                .bind(workspace).bind(object).bind(request.as_slice()).execute(connection).await?;
            Ok(())
        })).await.map_err(Into::into)
    }

    pub async fn reader_opened(
        &self,
        workspace: &str,
        resource: &str,
        position: &ReaderPosition,
    ) -> Result<(), FilesystemError> {
        let (workspace, resource, position) =
            (workspace.to_owned(), resource.to_owned(), position.clone());
        let sealer = self.sealer.clone();
        self.database.transaction(move |connection| Box::pin(async move {
            require_active(connection, &workspace).await?;
            let key = resource_owner(&workspace, &resource);
            let bytes = encode(&sealer, &key, &position)?;
            sqlx::query("UPDATE durable_filesystem_readers SET position=? WHERE workspace_id=? AND resource_id=? AND position IS NULL")
                .bind(bytes).bind(workspace).bind(resource).execute(connection).await?;
            Ok(())
        })).await.map_err(Into::into)
    }

    pub async fn reader_position(
        &self,
        workspace: &str,
        resource: &str,
    ) -> Result<ReaderPosition, FilesystemError> {
        let (workspace, resource) = (workspace.to_owned(), resource.to_owned());
        let sealer = self.sealer.clone();
        self.database.read(move |connection| Box::pin(async move {
            require_active(connection, &workspace).await?;
            let bytes: Vec<u8> = sqlx::query_scalar("SELECT position FROM durable_filesystem_readers WHERE workspace_id=? AND resource_id=?")
                .bind(&workspace).bind(&resource).fetch_one(connection).await?;
            decode(&sealer, &resource_owner(&workspace, &resource), &bytes)
        })).await.map_err(Into::into)
    }

    pub async fn open_writer(
        &self,
        workspace: &str,
        resource: &str,
        path: &str,
    ) -> Result<(), FilesystemError> {
        let (workspace, resource, path) =
            (workspace.to_owned(), resource.to_owned(), path.to_owned());
        let sealer = self.sealer.clone();
        self.database.transaction(move |connection| Box::pin(async move {
            require_active(connection, &workspace).await?;
            let key = resource_owner(&workspace, &resource);
            let prior: Option<Vec<u8>> = sqlx::query_scalar("SELECT request FROM durable_filesystem_writers WHERE workspace_id=? AND resource_id=?")
                .bind(&workspace).bind(&resource).fetch_optional(&mut *connection).await?;
            if let Some(prior) = prior {
                return if decode::<String>(&sealer, &key, &prior)? == path { Ok(()) } else { Err(DatabaseError::SessionConflict) };
            }
            require_idle(connection, &workspace).await?;
            require_resource_limit(connection, &workspace).await?;
            sqlx::query("INSERT INTO durable_filesystem_writers(workspace_id,resource_id,request,state) VALUES(?,?,?,'open')")
                .bind(workspace).bind(resource).bind(encode(&sealer, &key, &path)?).execute(connection).await?;
            Ok(())
        })).await.map_err(Into::into)
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
        let sealer = self.sealer.clone();
        self.database.transaction(move |connection| Box::pin(async move {
            let owner = require_active(connection, &workspace).await?;
            let key = resource_owner(&workspace, &format!("{resource}:{chunk}"));
            let prior: Option<Vec<u8>> = sqlx::query_scalar("SELECT content FROM durable_filesystem_chunks WHERE workspace_id=? AND resource_id=? AND chunk_id=?")
                .bind(&workspace).bind(&resource).bind(&chunk).fetch_optional(&mut *connection).await?;
            if let Some(prior) = prior {
                return if decode::<Content>(&sealer, &key, &prior)? == content { Ok(()) } else { Err(DatabaseError::SessionConflict) };
            }
            require_idle(connection, &workspace).await?;
            let state: String = sqlx::query_scalar("SELECT state FROM durable_filesystem_writers WHERE workspace_id=? AND resource_id=?")
                .bind(&workspace).bind(&resource).fetch_one(&mut *connection).await?;
            if state != "open" { return Err(DatabaseError::SessionConflict); }
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM durable_filesystem_chunks WHERE workspace_id=?")
                .bind(&workspace).fetch_one(&mut *connection).await?;
            if count >= MAX_EFFECTS { return Err(DatabaseError::SessionConflict); }
            let sequence: i64 = sqlx::query_scalar("SELECT count(*) FROM durable_filesystem_chunks WHERE workspace_id=? AND resource_id=?")
                .bind(&workspace).bind(&resource).fetch_one(&mut *connection).await?;
            retain_content(connection, &sealer, &workspace, &owner, &Mutation::Write { path: "/".into(), content: content.clone() }).await?;
            sqlx::query("INSERT INTO durable_filesystem_chunks(workspace_id,resource_id,chunk_id,sequence,content) VALUES(?,?,?,?,?)")
                .bind(workspace).bind(resource).bind(chunk).bind(sequence).bind(encode(&sealer, &key, &content)?).execute(connection).await?;
            Ok(())
        })).await.map_err(Into::into)
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
        let sealer = self.sealer.clone();
        self.database.transaction(move |connection| Box::pin(async move {
            require_active(connection, &workspace).await?;
            let row = sqlx::query("SELECT request,close_effect,state FROM durable_filesystem_writers WHERE workspace_id=? AND resource_id=?")
                .bind(&workspace).bind(&resource).fetch_one(&mut *connection).await?;
            let prior: Option<String> = row.try_get("close_effect")?;
            if prior.as_ref().is_some_and(|prior| *prior != effect) { return Err(DatabaseError::SessionConflict); }
            let rows = sqlx::query("SELECT chunk_id,content FROM durable_filesystem_chunks WHERE workspace_id=? AND resource_id=? ORDER BY sequence LIMIT 10001")
                .bind(&workspace).bind(&resource).fetch_all(&mut *connection).await?;
            if rows.len() > MAX_EFFECTS as usize { return Err(DatabaseError::SessionConflict); }
            let chunks = rows.into_iter().map(|row| {
                let chunk: String = row.try_get("chunk_id")?;
                decode(&sealer, &resource_owner(&workspace, &format!("{resource}:{chunk}")), &row.try_get::<Vec<u8>, _>("content")?)
            }).collect::<Result<Vec<Content>, DatabaseError>>()?;
            let bytes = chunks.iter().try_fold(0u64, |sum, chunk| sum.checked_add(chunk.bytes));
            if bytes.is_none_or(|bytes| bytes > maximum_bytes) {
                return Err(DatabaseError::SessionConflict);
            }
            if prior.is_none() {
                require_idle(connection, &workspace).await?;
                sequences::require_effect_capacity(connection, &workspace, 1).await?;
                sequences::require_unused_effect(connection, &workspace, &effect).await?;
                if admit {
                    sqlx::query("UPDATE durable_filesystem_writers SET close_effect=?,state='closing' WHERE workspace_id=? AND resource_id=?")
                        .bind(&effect).bind(&workspace).bind(&resource).execute(&mut *connection).await?;
                }
            }
            Ok(WriterRecord { path: decode(&sealer, &resource_owner(&workspace, &resource), &row.try_get::<Vec<u8>, _>("request")?)?, chunks })
        })).await.map_err(Into::into)
    }

    pub async fn writer_closed(
        &self,
        workspace: &str,
        resource: &str,
    ) -> Result<(), FilesystemError> {
        let (workspace, resource) = (workspace.to_owned(), resource.to_owned());
        self.database.transaction(move |connection| Box::pin(async move {
            sqlx::query("UPDATE durable_filesystem_writers SET state='closed' WHERE workspace_id=? AND resource_id=?").bind(workspace).bind(resource).execute(connection).await?;
            Ok(())
        })).await.map_err(Into::into)
    }
}

pub(super) async fn require_active(
    connection: &mut SqliteConnection,
    workspace: &str,
) -> Result<String, DatabaseError> {
    sqlx::query_scalar(
        "SELECT owner FROM durable_workspaces WHERE workspace_id=? AND state='active'",
    )
    .bind(workspace)
    .fetch_optional(connection)
    .await?
    .ok_or(DatabaseError::SessionConflict)
}

pub(super) async fn require_resource_limit(
    connection: &mut SqliteConnection,
    workspace: &str,
) -> Result<(), DatabaseError> {
    let count: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM durable_filesystem_readers WHERE workspace_id=?)+(SELECT count(*) FROM durable_filesystem_writers WHERE workspace_id=?)+(SELECT count(*) FROM durable_filesystem_listings WHERE workspace_id=?)")
        .bind(workspace).bind(workspace).bind(workspace).fetch_one(connection).await?;
    if count >= MAX_EFFECTS {
        return Err(DatabaseError::SessionConflict);
    }
    Ok(())
}

pub(super) async fn require_ready(
    connection: &mut SqliteConnection,
    workspace: &str,
    effect: Option<&str>,
) -> Result<(), DatabaseError> {
    let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM durable_filesystem_readers WHERE workspace_id=? AND position IS NULL AND error IS NULL) OR EXISTS(SELECT 1 FROM durable_filesystem_writers WHERE workspace_id=? AND state='closing' AND close_effect!=?) OR EXISTS(SELECT 1 FROM durable_filesystem_listings WHERE workspace_id=? AND content IS NULL AND error IS NULL)")
        .bind(workspace).bind(workspace).bind(effect.unwrap_or("")).bind(workspace).fetch_one(connection).await?;
    if pending {
        return Err(DatabaseError::SessionConflict);
    }
    Ok(())
}

pub(super) async fn require_idle(
    connection: &mut SqliteConnection,
    workspace: &str,
) -> Result<(), DatabaseError> {
    require_live(connection, workspace).await?;
    require_ready(connection, workspace, None).await?;
    let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM durable_filesystem_effects WHERE workspace_id=? AND state IN ('preparing','pending')) OR EXISTS(SELECT 1 FROM durable_filesystem_plans WHERE workspace_id=? AND outcome IS NULL)")
        .bind(workspace).bind(workspace).fetch_one(connection).await?;
    if pending {
        return Err(DatabaseError::SessionConflict);
    }
    Ok(())
}

pub(super) async fn require_live(
    connection: &mut SqliteConnection,
    workspace: &str,
) -> Result<(), DatabaseError> {
    let live: bool = sqlx::query_scalar("SELECT runs.state IN ('reserved','running','blocked') FROM runs JOIN durable_workspaces w ON runs.run_id=w.run_id WHERE workspace_id=?")
        .bind(workspace).fetch_one(connection).await?;
    if !live {
        return Err(DatabaseError::SessionConflict);
    }
    Ok(())
}

fn resource_owner(workspace: &str, resource: &str) -> String {
    format!("resource:{workspace}:{resource}")
}
