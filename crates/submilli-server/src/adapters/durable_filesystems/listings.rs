use super::*;
use interpreter::runtime::fs::recovery::Content;

impl FilesystemJournal {
    pub async fn listing(
        &self,
        workspace: &str,
        resource: &str,
        path: &str,
        recursive: bool,
        completed: Option<Content>,
    ) -> Result<Option<Content>, FilesystemError> {
        let (workspace, resource, request) = (
            workspace.to_owned(),
            resource.to_owned(),
            (path.to_owned(), recursive),
        );
        let sealer = self.sealer.clone();
        self.database.transaction(move |connection| Box::pin(async move {
            resources::require_active(connection, &workspace).await?;
            let key = format!("listing:{workspace}:{resource}");
            let row = sqlx::query("SELECT request,content,error FROM durable_filesystem_listings WHERE workspace_id=? AND resource_id=?")
                .bind(&workspace).bind(&resource).fetch_optional(&mut *connection).await?;
            if let Some(row) = row {
                if decode::<(String, bool)>(&sealer, &key, &row.try_get::<Vec<u8>, _>("request")?)? != request { return Err(DatabaseError::SessionConflict); }
                if let Some(error) = row.try_get::<Option<Vec<u8>>, _>("error")? { return Ok(Err(decode::<String>(&sealer, &key, &error)?)); }
                if let Some(content) = row.try_get::<Option<Vec<u8>>, _>("content")? {
                    let content: Content = decode(&sealer, &key, &content)?;
                    if completed.as_ref().is_some_and(|expected| *expected != content) { return Err(DatabaseError::SessionConflict); }
                    return Ok(Ok(Some(content)));
                }
                if let Some(content) = completed {
                    sqlx::query("UPDATE durable_filesystem_listings SET content=? WHERE workspace_id=? AND resource_id=?")
                        .bind(encode(&sealer, &key, &content)?).bind(workspace).bind(resource).execute(connection).await?;
                    return Ok(Ok(Some(content)));
                }
                return Ok(Ok(None));
            }
            if completed.is_some() { return Err(DatabaseError::SessionConflict); }
            resources::require_idle(connection, &workspace).await?;
            resources::require_resource_limit(connection, &workspace).await?;
            sqlx::query("INSERT INTO durable_filesystem_listings(workspace_id,resource_id,request) VALUES(?,?,?)")
                .bind(workspace).bind(resource).bind(encode(&sealer, &key, &request)?).execute(connection).await?;
            Ok(Ok(None))
        })).await?.map_err(FilesystemError::Rejected)
    }

    pub async fn reject_listing(
        &self,
        workspace: &str,
        resource: &str,
        reason: String,
    ) -> Result<(), FilesystemError> {
        let (workspace, resource) = (workspace.to_owned(), resource.to_owned());
        let sealer = self.sealer.clone();
        self.database.transaction(move |connection| Box::pin(async move {
            let error = encode(&sealer, &format!("listing:{workspace}:{resource}"), &reason)?;
            sqlx::query("UPDATE durable_filesystem_listings SET error=? WHERE workspace_id=? AND resource_id=? AND content IS NULL")
                .bind(error).bind(workspace).bind(resource).execute(connection).await?;
            Ok(())
        })).await.map_err(Into::into)
    }
}
