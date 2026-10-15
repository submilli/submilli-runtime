use super::*;

pub(crate) struct PlanRecord {
    pub steps: Vec<Mutation>,
    pub outcome: Option<RecordedOutcome>,
}

impl FilesystemJournal {
    /// All capture references are pinned in the same commit as the immutable
    /// plan. A later step may resolve parents created by an earlier step.
    pub async fn plan(
        &self,
        workspace: &str,
        id: &str,
        steps: &[Mutation],
    ) -> Result<PlanRecord, FilesystemError> {
        let (workspace, id, steps) = (workspace.to_owned(), id.to_owned(), steps.to_vec());
        let sealer = self.sealer.clone();
        self.database.transaction(move |connection| Box::pin(async move {
            let owner = resources::require_active(connection, &workspace).await?;
            if let Some(record) = load_plan(connection, &sealer, &workspace, &id).await? {
                if record.steps != steps { return Err(DatabaseError::SessionConflict); }
                return Ok(record);
            }
            resources::require_live(connection, &workspace).await?;
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM durable_filesystem_plans WHERE workspace_id=?")
                .bind(&workspace).fetch_one(&mut *connection).await?;
            if count >= MAX_EFFECTS { return Err(DatabaseError::SessionConflict); }
            resources::require_ready(connection, &workspace, None).await?;
            let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM durable_filesystem_effects WHERE workspace_id=? AND state IN ('preparing','pending'))")
                .bind(&workspace).fetch_one(&mut *connection).await?;
            if pending { return Err(DatabaseError::SessionConflict); }
            // Pending plans exclude all unrelated effects, so these available
            // slots remain reserved for this plan until its outcome commits.
            require_effect_capacity(connection, &workspace, steps.len()).await?;
            for index in 0..steps.len() {
                require_unused_effect(connection, &workspace, &plan_effect_id(&workspace, &id, index)).await?;
            }
            let key = plan_owner(&workspace, &id);
            let request = encode(&sealer, &key, &steps)?;
            for step in &steps { retain_content(connection, &sealer, &workspace, &owner, step).await?; }
            sqlx::query("INSERT INTO durable_filesystem_plans(workspace_id,plan_id,request) VALUES(?,?,?)")
                .bind(workspace).bind(id).bind(request).execute(connection).await?;
            Ok(PlanRecord { steps, outcome: None })
        })).await.map_err(Into::into)
    }

    pub async fn pending_plan(
        &self,
        workspace: &str,
    ) -> Result<Option<(String, PlanRecord)>, FilesystemError> {
        let workspace = workspace.to_owned();
        let sealer = self.sealer.clone();
        self.database.read(move |connection| Box::pin(async move {
            let id: Option<String> = sqlx::query_scalar("SELECT plan_id FROM durable_filesystem_plans WHERE workspace_id=? AND outcome IS NULL")
                .bind(&workspace).fetch_optional(&mut *connection).await?;
            let Some(id) = id else { return Ok(None); };
            let record = load_plan(connection, &sealer, &workspace, &id).await?.ok_or(DatabaseError::SessionConflict)?;
            Ok(Some((id, record)))
        })).await.map_err(Into::into)
    }

    pub async fn finish_plan(
        &self,
        workspace: &str,
        id: &str,
        outcome: &RecordedOutcome,
    ) -> Result<(), FilesystemError> {
        let (workspace, id, outcome) = (workspace.to_owned(), id.to_owned(), outcome.clone());
        let sealer = self.sealer.clone();
        self.database.transaction(move |connection| Box::pin(async move {
            let pending: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM durable_filesystem_effects WHERE workspace_id=? AND state IN ('preparing','pending'))")
                .bind(&workspace).fetch_one(&mut *connection).await?;
            if pending { return Err(DatabaseError::SessionConflict); }
            sqlx::query("UPDATE durable_filesystem_plans SET outcome=? WHERE workspace_id=? AND plan_id=? AND outcome IS NULL")
                .bind(encode(&sealer, &plan_owner(&workspace, &id), &outcome)?).bind(workspace).bind(id).execute(connection).await?;
            Ok(())
        })).await.map_err(Into::into)
    }
}

pub(super) async fn require_effect_capacity(
    connection: &mut SqliteConnection,
    workspace: &str,
    additional: usize,
) -> Result<(), DatabaseError> {
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM durable_filesystem_effects WHERE workspace_id=?")
            .bind(workspace)
            .fetch_one(connection)
            .await?;
    let additional = i64::try_from(additional).map_err(invalid)?;
    if additional > MAX_EFFECTS.saturating_sub(count) {
        return Err(DatabaseError::SessionConflict);
    }
    Ok(())
}

pub(super) async fn require_unused_effect(
    connection: &mut SqliteConnection,
    workspace: &str,
    effect: &str,
) -> Result<(), DatabaseError> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM durable_filesystem_effects WHERE workspace_id=? AND effect_id=?) OR EXISTS(SELECT 1 FROM durable_filesystem_writers WHERE workspace_id=? AND close_effect=?)")
        .bind(workspace).bind(effect).bind(workspace).bind(effect).fetch_one(connection).await?;
    if exists {
        return Err(DatabaseError::SessionConflict);
    }
    Ok(())
}

pub(crate) fn plan_effect_id(workspace: &str, plan: &str, index: usize) -> String {
    crate::filesystems::workspace_id(&format!("plan:{plan}:{index}"), workspace)
}

pub(super) async fn require_plan(
    connection: &mut SqliteConnection,
    workspace: &str,
    effect: &str,
    plan: Option<&str>,
) -> Result<(), DatabaseError> {
    let existing: Option<Option<String>> = sqlx::query_scalar(
        "SELECT plan_id FROM durable_filesystem_effects WHERE workspace_id=? AND effect_id=?",
    )
    .bind(workspace)
    .bind(effect)
    .fetch_optional(&mut *connection)
    .await?;
    if let Some(existing) = existing {
        return if existing.as_deref() == plan {
            Ok(())
        } else {
            Err(DatabaseError::SessionConflict)
        };
    }
    let pending: Option<String> = sqlx::query_scalar(
        "SELECT plan_id FROM durable_filesystem_plans WHERE workspace_id=? AND outcome IS NULL",
    )
    .bind(workspace)
    .fetch_optional(connection)
    .await?;
    if pending.as_deref() != plan {
        return Err(DatabaseError::SessionConflict);
    }
    Ok(())
}

async fn load_plan(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    id: &str,
) -> Result<Option<PlanRecord>, DatabaseError> {
    let row = sqlx::query(
        "SELECT request,outcome FROM durable_filesystem_plans WHERE workspace_id=? AND plan_id=?",
    )
    .bind(workspace)
    .bind(id)
    .fetch_optional(connection)
    .await?;
    row.map(|row| {
        let key = plan_owner(workspace, id);
        Ok(PlanRecord {
            steps: decode(sealer, &key, &row.try_get::<Vec<u8>, _>("request")?)?,
            outcome: row
                .try_get::<Option<Vec<u8>>, _>("outcome")?
                .map(|bytes| decode(sealer, &key, &bytes))
                .transpose()?,
        })
    })
    .transpose()
}

pub(super) fn plan_owner(workspace: &str, id: &str) -> String {
    format!("plan:{workspace}:{id}")
}
