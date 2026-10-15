use super::*;

pub(crate) struct StoredWorkspace {
    pub identity: Option<WorkspaceIdentity>,
    pub layout: Option<WorkspaceLayout>,
    pub reservation: Option<EphemeralReservation>,
    pub state: String,
}

impl FilesystemJournal {
    pub async fn workspace(
        &self,
        workspace: &str,
        owner: &str,
    ) -> Result<StoredWorkspace, FilesystemError> {
        let workspace = workspace.to_owned();
        let owner = owner.to_owned();
        let sealer = self.sealer.clone();
        self.database.read(move |connection| Box::pin(async move {
            let row = sqlx::query("SELECT identity,layout,reservation,state FROM durable_workspaces WHERE workspace_id=? AND owner=?")
                .bind(&workspace).bind(owner).fetch_one(connection).await?;
            Ok(StoredWorkspace {
                identity: row.try_get::<Option<Vec<u8>>, _>("identity")?.map(|bytes| decode(&sealer, &workspace, &bytes)).transpose()?,
                layout: row.try_get::<Option<Vec<u8>>, _>("layout")?.map(|bytes| decode(&sealer, &format!("layout:{workspace}"), &bytes)).transpose()?,
                reservation: row.try_get::<Option<Vec<u8>>, _>("reservation")?.map(|bytes| decode(&sealer, &format!("reservation:{workspace}"), &bytes)).transpose()?,
                state: row.try_get("state")?,
            })
        })).await.map_err(Into::into)
    }

    pub async fn retire(&self, workspace: &str, owner: &str) -> Result<(), FilesystemError> {
        let workspace = workspace.to_owned();
        let owner = owner.to_owned();
        self.database.transaction(move |connection| Box::pin(async move {
            let state: String = sqlx::query_scalar("SELECT runs.state FROM runs JOIN durable_workspaces w ON w.run_id=runs.run_id WHERE workspace_id=? AND w.owner=?")
                .bind(&workspace).bind(owner).fetch_one(&mut *connection).await?;
            if !matches!(state.as_str(), "completed" | "finished" | "cancelled" | "aborted" | "expired") { return Err(DatabaseError::SessionConflict); }
            let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM durable_filesystem_effects WHERE workspace_id=? AND state IN ('preparing','pending'))")
                .bind(&workspace).fetch_one(&mut *connection).await?;
            if pending { return Err(DatabaseError::SessionConflict); }
            let closing: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM durable_filesystem_writers WHERE workspace_id=? AND state='closing')").bind(&workspace).fetch_one(&mut *connection).await?;
            if closing { return Err(DatabaseError::SessionConflict); }
            let plan: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM durable_filesystem_plans WHERE workspace_id=? AND outcome IS NULL)").bind(&workspace).fetch_one(&mut *connection).await?;
            if plan { return Err(DatabaseError::SessionConflict); }
            sqlx::query("UPDATE durable_workspaces SET state='retiring' WHERE workspace_id=? AND state IN ('preparing','active')")
                .bind(workspace).execute(connection).await?;
            Ok(())
        })).await.map_err(Into::into)
    }

    pub async fn finish_retirement(&self, workspace: &str) -> Result<(), FilesystemError> {
        let workspace = workspace.to_owned();
        let sealer = self.sealer.clone();
        self.database.transaction(move |connection| Box::pin(async move {
            let state: String = sqlx::query_scalar("SELECT state FROM durable_workspaces WHERE workspace_id=?").bind(&workspace).fetch_one(&mut *connection).await?;
            if state == "gone" { return Ok(()); }
            if state != "retiring" { return Err(DatabaseError::SessionConflict); }
            let mut after = String::new();
            loop {
                let rows = sqlx::query("SELECT effect_id,request FROM durable_filesystem_effects WHERE workspace_id=? AND effect_id>? ORDER BY effect_id LIMIT 64")
                    .bind(&workspace).bind(&after).fetch_all(&mut *connection).await?;
                if rows.is_empty() { break; }
                for row in rows {
                    after = row.try_get("effect_id")?;
                    let request: Mutation = decode(&sealer, &effect_owner(&workspace, &after), &row.try_get::<Vec<u8>, _>("request")?)?;
                    release_content(connection, &sealer, &workspace, request).await?;
                }
            }
            let mut after = String::new();
            loop {
                let rows = sqlx::query("SELECT plan_id,request FROM durable_filesystem_plans WHERE workspace_id=? AND plan_id>? ORDER BY plan_id LIMIT 64")
                    .bind(&workspace).bind(&after).fetch_all(&mut *connection).await?;
                if rows.is_empty() { break; }
                for row in rows {
                    after = row.try_get("plan_id")?;
                    let steps: Vec<Mutation> = decode(&sealer, &sequences::plan_owner(&workspace, &after), &row.try_get::<Vec<u8>, _>("request")?)?;
                    for step in steps { release_content(connection, &sealer, &workspace, step).await?; }
                }
            }
            let mut after = (String::new(), String::new());
            loop {
                let rows = sqlx::query("SELECT resource_id,chunk_id,content FROM durable_filesystem_chunks WHERE workspace_id=? AND (resource_id,chunk_id)>(?,?) ORDER BY resource_id,chunk_id LIMIT 64")
                    .bind(&workspace).bind(&after.0).bind(&after.1).fetch_all(&mut *connection).await?;
                if rows.is_empty() { break; }
                for row in rows {
                    after = (row.try_get("resource_id")?, row.try_get("chunk_id")?);
                    let key = format!("resource:{workspace}:{}:{}", after.0, after.1);
                    let content = decode(&sealer, &key, &row.try_get::<Vec<u8>, _>("content")?)?;
                    release_content(connection, &sealer, &workspace, Mutation::Write { path: "/".into(), content }).await?;
                }
            }
            let owner: String = sqlx::query_scalar("SELECT owner FROM durable_workspaces WHERE workspace_id=?").bind(&workspace).fetch_one(&mut *connection).await?;
            let mut after = String::new();
            loop {
                let rows = sqlx::query("SELECT object_id,request FROM durable_filesystem_objects WHERE workspace_id=? AND object_id>? ORDER BY object_id LIMIT 64")
                    .bind(&workspace).bind(&after).fetch_all(&mut *connection).await?;
                if rows.is_empty() { break; }
                for row in rows {
                    after = row.try_get("object_id")?;
                    retention::retire_producer(connection, &sealer, &workspace, &owner, after.clone(), row.try_get("request")?).await?;
                }
            }
            sqlx::query("UPDATE durable_workspaces SET state='gone' WHERE workspace_id=?").bind(workspace).execute(connection).await?;
            Ok(())
        })).await.map_err(Into::into)
    }
}
