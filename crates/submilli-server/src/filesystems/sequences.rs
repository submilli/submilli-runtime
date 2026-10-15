use super::*;

impl DurableFilesystem {
    /// Publish already computed Git objects, worktree files, index and refs.
    /// The caller captures the exact commit/result before this call, checks the
    /// complete repository capability, and orders refs after their objects.
    /// Once admitted, failures keep the publication pending for roll-forward;
    /// they never report a definitive Git error with a partially updated repo.
    pub async fn publish_git_plan(
        &self,
        id: &str,
        repository: &str,
        steps: Vec<Mutation>,
    ) -> Result<Outcome, FilesystemError> {
        if !steps.iter().any(|step| step.repository().is_some())
            || steps
                .iter()
                .filter_map(Mutation::repository)
                .any(|scope| scope != repository)
        {
            return Err(RecoveryError::InvalidRecord.into());
        }
        let steps = steps
            .into_iter()
            .map(|step| match step {
                Mutation::Write { path, content } => Ok(Mutation::GitWrite {
                    repository: repository.into(),
                    path,
                    content,
                    executable: false,
                }),
                Mutation::Remove { path } => Ok(Mutation::GitRemove {
                    repository: repository.into(),
                    path,
                }),
                Mutation::CreateDirectory { path } => Ok(Mutation::GitDirectory {
                    repository: repository.into(),
                    path,
                }),
                Mutation::GitWrite { .. }
                | Mutation::GitRemove { .. }
                | Mutation::GitDirectory { .. } => Ok(step),
                _ => Err(RecoveryError::Unsupported(
                    "Git publication requires captured files",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.execute_plan(id, steps).await
    }
    /// Recoverable ordered mutations, for recursive copy/remove, cross-volume
    /// moves and prepared publications. Every source must already be captured.
    /// The whole plan's capabilities must be admitted before this call. The
    /// order and payloads are immutable; recovery never re-enumerates a tree.
    pub async fn execute_plan(
        &self,
        id: &str,
        steps: Vec<Mutation>,
    ) -> Result<Outcome, FilesystemError> {
        validate_id(id)?;
        if steps.len() > 1000 {
            return Err(RecoveryError::Limit.into());
        }
        for step in &steps {
            step.validate()?;
        }
        let gate = Arc::new(self.objects.filesystem_gate.clone().lock_owned().await);
        let record = self.journal.plan(&self.id, id, &steps).await?;
        if let Some(outcome) = record.outcome {
            return unpack(outcome);
        }
        self.execute_plan_held(gate, id, record.steps).await
    }

    pub(super) async fn execute_plan_held(
        &self,
        gate: Arc<tokio::sync::OwnedMutexGuard<()>>,
        id: &str,
        steps: Vec<Mutation>,
    ) -> Result<Outcome, FilesystemError> {
        let mut bytes = 0u64;
        let finish_publication = steps.iter().any(|step| step.repository().is_some());
        for (index, request) in steps.into_iter().enumerate() {
            let effect = crate::adapters::durable_filesystems::plan_effect_id(&self.id, id, index);
            match self
                .execute_step(gate.clone(), &effect, request, Some(id), finish_publication)
                .await
            {
                Ok(outcome) => bytes = bytes.saturating_add(outcome.bytes),
                Err(FilesystemError::Rejected(reason)) => {
                    // Recursive operations can have a committed prefix. Keep
                    // that definitive failure; replay must not repeat it.
                    let outcome = RecordedOutcome::Rejected(format!("step {index}: {reason}"));
                    self.journal.finish_plan(&self.id, id, &outcome).await?;
                    return unpack(outcome);
                }
                Err(error) => return Err(error),
            }
        }
        let outcome = Outcome { bytes };
        self.journal
            .finish_plan(&self.id, id, &RecordedOutcome::Completed(outcome.clone()))
            .await?;
        Ok(outcome)
    }
}
