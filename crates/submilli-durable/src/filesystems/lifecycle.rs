use super::*;
use crate::recovery::EphemeralReservation;
use std::path::Path;

impl DurableFilesystem {
    /// Reserve run ownership in SQL before creating a retained ephemeral root.
    /// `parent` is the configured `<volume_dir>/ephemeral`, never a new runs/ root.
    pub async fn retained_ephemeral(
        parent: &Path,
        run: &str,
        owner: &str,
        units: UnitFactory,
        objects: DurableObjects,
    ) -> Result<Self, FilesystemError> {
        validate_id(run)?;
        validate_owner(owner)?;
        let id = workspace_id(run, objects.store_identity());
        let journal = FilesystemJournal::open(units.clone(), objects.store_identity()).await?;
        let gate = Arc::new(objects.filesystem_gate.clone().lock_owned().await);
        let parent = parent.to_owned();
        let reserved_id = id.clone();
        let reservation = blocking(gate.clone(), move || {
            EphemeralReservation::new(&parent, &reserved_id)
        })
        .await?;
        let existing = journal
            .reserve_ephemeral(&id, run, owner, reservation.clone())
            .await?;
        checkpoint("workspace-reserved");
        let vfs = blocking(gate.clone(), move || reservation.open(existing.is_none())).await?;
        checkpoint("workspace-root");
        drop(gate);
        Self::open(vfs, run, owner, units, objects).await
    }

    /// Reconstruct retained filesystem access from SQL, without rebuilding the
    /// program or starting execution. Capability policy remains the caller's job.
    pub async fn resume(
        workspace: &str,
        owner: &str,
        units: UnitFactory,
        objects: DurableObjects,
    ) -> Result<Self, FilesystemError> {
        validate_id(workspace)?;
        let journal = FilesystemJournal::open(units, objects.store_identity()).await?;
        let gate = Arc::new(objects.filesystem_gate.clone().lock_owned().await);
        let stored = journal.workspace(workspace, owner).await?;
        if stored.state != "active" {
            return Err(FilesystemError::Configuration("workspace is not active"));
        }
        stored
            .layout
            .as_ref()
            .ok_or(RecoveryError::InvalidRecord)?
            .exclude(objects.directory())?;
        let (vfs, identity) = blocking(gate.clone(), move || {
            let layout = stored.layout.ok_or(RecoveryError::InvalidRecord)?;
            let identity = stored.identity.ok_or(RecoveryError::InvalidRecord)?;
            let vfs = layout.restore(&identity)?;
            Ok((vfs, identity))
        })
        .await?;
        let vfs = quotas::share(&objects, vfs).await?;
        let recovered = blocking(gate, move || Workspace::reopen(vfs, &identity)).await?;
        Ok(Self {
            work: None,
            workspace: Arc::new(recovered),
            journal,
            objects,
            id: workspace.into(),
            owner: owner.into(),
        })
    }

    /// Terminal, retryable cleanup. Call only after native resources and all
    /// workers have drained. SQL checks terminal run state and pending effects;
    /// the shared gate drains cancelled blocking mutation attempts.
    pub async fn retire(&self, owner: &str) -> Result<(), FilesystemError> {
        let gate = Arc::new(self.objects.filesystem_gate.clone().lock_owned().await);
        self.journal.retire(&self.id, owner).await?;
        checkpoint("retirement");
        let stored = self.journal.workspace(&self.id, owner).await?;
        if stored.state == "gone" {
            return Ok(());
        }
        let access = self
            .objects
            .filesystem_resources
            .clone()
            .write_owned()
            .await;
        let id = self.id.clone();
        let quotas = quotas::live(&self.objects).await;
        blocking(gate.clone(), move || {
            let _access = access;
            let layout = stored.layout.ok_or(RecoveryError::InvalidRecord)?;
            let result = layout.retire(&id);
            layout.reconcile_quotas(&quotas)?;
            result
        })
        .await?;
        checkpoint("filesystem-removed");
        self.journal.finish_retirement(&self.id).await?;
        checkpoint("retired");
        Ok(())
    }

    /// Finish a cleanup interrupted by process death, including after an
    /// ephemeral root was removed but before its SQL tombstone committed.
    pub async fn recover_cleanup(
        workspace: &str,
        owner: &str,
        units: UnitFactory,
        objects: DurableObjects,
    ) -> Result<(), FilesystemError> {
        validate_id(workspace)?;
        let journal = FilesystemJournal::open(units, objects.store_identity()).await?;
        let gate = Arc::new(objects.filesystem_gate.clone().lock_owned().await);
        let stored = journal.workspace(workspace, owner).await?;
        if stored.state == "gone" {
            return Ok(());
        }
        journal.retire(workspace, owner).await?;
        let access = objects.filesystem_resources.clone().write_owned().await;
        let id = workspace.to_owned();
        let quotas = quotas::live(&objects).await;
        blocking(gate.clone(), move || {
            let _access = access;
            if let Some(layout) = stored.layout {
                let result = layout.retire(&id);
                layout.reconcile_quotas(&quotas)?;
                result
            } else if let Some(reservation) = stored.reservation {
                reservation.remove_uninitialized()
            } else {
                Err(RecoveryError::InvalidRecord)
            }
        })
        .await?;
        journal.finish_retirement(workspace).await
    }
}
