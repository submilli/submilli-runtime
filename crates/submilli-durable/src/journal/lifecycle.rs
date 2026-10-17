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
                state.state = "gone".into();
                unit.save_workspace(state).await?;
                Ok(())
            })
        })
        .await
    }
}
