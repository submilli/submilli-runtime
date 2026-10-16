use super::*;
impl FilesystemJournal {
    pub async fn plan(
        &self,
        workspace: &str,
        id: &str,
        steps: &[Mutation],
    ) -> Result<Plan, FilesystemError> {
        let (workspace, id, steps) = (workspace.to_owned(), id.to_owned(), steps.to_vec());
        self.transaction(move |unit| {
            Box::pin(async move {
                let state = require_active(unit, &workspace).await?;
                if let Some(prior) = unit.get_plan(&workspace, &id).await? {
                    return if prior.steps == steps {
                        Ok(prior)
                    } else {
                        conflict()
                    };
                }
                require_live(unit, &state).await?;
                if unit.resource_counts(&workspace).await?.plans >= MAX_EFFECTS {
                    return conflict();
                }
                require_ready(unit, &workspace, None).await?;
                if unit.pending_effect(&state.run).await?.is_some()
                    || unit.pending_plan(&workspace).await?.is_some()
                {
                    return conflict();
                }
                require_capacity(unit, &state.run, steps.len()).await?;
                for index in 0..steps.len() {
                    require_unused_effect(unit, &state, &plan_effect_id(&workspace, &id, index))
                        .await?;
                }
                serialize(&steps)?;
                for step in &steps {
                    retain_content(unit, &state, step).await?;
                }
                let plan = Plan {
                    id,
                    steps,
                    outcome: None,
                };
                unit.save_plan(&workspace, plan.clone()).await?;
                Ok(plan)
            })
        })
        .await
    }
    pub async fn pending_plan(
        &self,
        workspace: &str,
    ) -> Result<Option<(String, Plan)>, FilesystemError> {
        Ok((self.units)()
            .await?
            .pending_plan(workspace)
            .await?
            .map(|p| (p.id.clone(), p)))
    }
    pub async fn finish_plan(
        &self,
        workspace: &str,
        id: &str,
        outcome: &RecordedOutcome,
    ) -> Result<(), FilesystemError> {
        let (workspace, id, outcome) = (workspace.to_owned(), id.to_owned(), outcome.clone());
        self.transaction(move |unit| {
            Box::pin(async move {
                let state = required_workspace(unit, &workspace).await?;
                if unit.pending_effect(&state.run).await?.is_some() {
                    return conflict();
                }
                let mut plan = unit
                    .get_plan(&workspace, &id)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                if plan.outcome.is_none() {
                    plan.outcome = Some(outcome);
                    unit.save_plan(&workspace, plan).await?;
                }
                Ok(())
            })
        })
        .await
    }
}
pub(super) async fn require_unused_effect(
    unit: &mut dyn UnitOfWork,
    workspace: &WorkspaceState,
    effect: &str,
) -> Result<(), FilesystemError> {
    if unit.get_effect(&workspace.run, effect).await?.is_some()
        || unit
            .writer_for_effect(&workspace.id, effect)
            .await?
            .is_some()
    {
        return conflict();
    }
    Ok(())
}
pub fn plan_effect_id(workspace: &str, plan: &str, index: usize) -> String {
    crate::filesystems::workspace_id(&format!("plan:{plan}:{index}"), workspace)
}
