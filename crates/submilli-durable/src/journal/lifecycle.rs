use super::*;
impl FilesystemJournal {
    pub async fn workspace(
        &self,
        workspace: &str,
        owner: &str,
    ) -> Result<WorkspaceState, FilesystemError> {
        let mut unit = (self.units)().await?;
        let state = required_workspace(unit.as_mut(), workspace).await?;
        if state.owner != owner {
            return conflict();
        }
        Ok(state)
    }
    pub async fn retire(&self, workspace: &str, owner: &str) -> Result<(), FilesystemError> {
        let (workspace, owner) = (workspace.to_owned(), owner.to_owned());
        self.transaction(move |unit| {
            Box::pin(async move {
                let mut state = required_workspace(unit, &workspace).await?;
                if state.owner != owner
                    || !unit
                        .get_run(&state.run)
                        .await?
                        .ok_or(StoreError::Conflict)?
                        .terminal
                {
                    return conflict();
                }
                if unit.pending_effect(&state.run).await?.is_some()
                    || unit.closing_writer(&workspace).await?.is_some()
                    || unit.pending_plan(&workspace).await?.is_some()
                {
                    return conflict();
                }
                if matches!(state.state.as_str(), "preparing" | "active") {
                    state.state = "retiring".into();
                    unit.save_workspace(state).await?;
                }
                Ok(())
            })
        })
        .await
    }
    pub async fn finish_retirement(&self, workspace: &str) -> Result<(), FilesystemError> {
        let workspace = workspace.to_owned();
        self.transaction(move |unit| {
            Box::pin(async move {
                let mut state = required_workspace(unit, &workspace).await?;
                if state.state == "gone" {
                    return Ok(());
                }
                if state.state != "retiring" {
                    return conflict();
                }
                release_effects(unit, &state).await?;
                release_plans(unit, &workspace).await?;
                release_chunks(unit, &workspace).await?;
                retire_producers(unit, &state).await?;
                state.state = "gone".into();
                unit.save_workspace(state).await?;
                Ok(())
            })
        })
        .await
    }
}
async fn release_effects(
    unit: &mut dyn UnitOfWork,
    state: &WorkspaceState,
) -> Result<(), FilesystemError> {
    let mut after = String::new();
    loop {
        let page = unit
            .effects_after(&state.run, OPERATION, &after, 64)
            .await?;
        if page.is_empty() {
            return Ok(());
        }
        for effect in page {
            after = effect.id.clone();
            release_content(unit, &state.id, &decode_effect(effect, &state.id)?.request).await?;
        }
    }
}
async fn release_plans(unit: &mut dyn UnitOfWork, workspace: &str) -> Result<(), FilesystemError> {
    let mut after = String::new();
    loop {
        let page = unit.plans_after(workspace, &after, 64).await?;
        if page.is_empty() {
            return Ok(());
        }
        for plan in page {
            after = plan.id;
            for step in plan.steps {
                release_content(unit, workspace, &step).await?;
            }
        }
    }
}
async fn release_chunks(unit: &mut dyn UnitOfWork, workspace: &str) -> Result<(), FilesystemError> {
    let mut after = (String::new(), String::new());
    loop {
        let page = unit
            .chunks_after(workspace, (&after.0, &after.1), 64)
            .await?;
        if page.is_empty() {
            return Ok(());
        }
        for chunk in page {
            after = (chunk.writer, chunk.id);
            release_content(
                unit,
                workspace,
                &Mutation::Write {
                    path: "/".into(),
                    content: chunk.content,
                },
            )
            .await?;
        }
    }
}
async fn retire_producers(
    unit: &mut dyn UnitOfWork,
    workspace: &WorkspaceState,
) -> Result<(), FilesystemError> {
    let mut after = String::new();
    loop {
        let page = unit.producers_after(&workspace.id, &after, 64).await?;
        if page.is_empty() {
            return Ok(());
        }
        for producer in page {
            after = producer.id;
            crate::publications::workspaces::retire_producer(
                unit,
                &workspace.id,
                &workspace.owner,
                &ObjectId::parse(after.clone()).map_err(invalid)?,
                &producer.request,
            )
            .await?;
        }
    }
}
