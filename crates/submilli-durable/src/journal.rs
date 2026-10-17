//! Filesystem admission and retention rules, independent of SQL and native I/O.
use crate::{
    UnitFactory, UnitOfWork, filesystems::FilesystemError, recovery::*, state::*,
    unit_of_work::StoreError,
};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
mod lifecycle;
mod resources;
mod sequences;
use crate::effects::{self, EffectKey, MAX_EFFECTS};
pub(crate) use crate::state::{PendingResource, RecordedOutcome};
pub use sequences::plan_effect_id;
const OPERATION: &str = "filesystem.mutation";
const VERSION: u32 = 1;

#[derive(Clone)]
pub struct FilesystemJournal {
    units: UnitFactory,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FilesystemRequest {
    workspace: String,
    plan: Option<String>,
    mutation: Mutation,
}

impl FilesystemJournal {
    pub async fn open(units: UnitFactory, expected_store: &str) -> Result<Self, FilesystemError> {
        let mut unit = units().await?;
        if unit.filesystem_store_identity().await?.as_deref() != Some(expected_store) {
            return Err(FilesystemError::Configuration(
                "filesystem belongs to another database",
            ));
        }
        Ok(Self { units })
    }
    async fn transaction<T: Send>(
        &self,
        operation: impl for<'a> FnOnce(
            &'a mut dyn UnitOfWork,
        ) -> BoxFuture<'a, Result<T, FilesystemError>>,
    ) -> Result<T, FilesystemError> {
        let mut unit = (self.units)().await?;
        let value = operation(unit.as_mut()).await?;
        unit.commit().await?;
        Ok(value)
    }
    pub async fn register(
        &self,
        workspace: &str,
        run: &str,
        owner: &str,
        layout: Option<WorkspaceLayout>,
    ) -> Result<Option<WorkspaceIdentity>, FilesystemError> {
        self.register_location(workspace, run, owner, layout, None)
            .await
    }
    pub async fn reserve_ephemeral(
        &self,
        workspace: &str,
        run: &str,
        owner: &str,
        reservation: EphemeralReservation,
    ) -> Result<Option<WorkspaceIdentity>, FilesystemError> {
        self.register_location(workspace, run, owner, None, Some(reservation))
            .await
    }
    async fn register_location(
        &self,
        workspace: &str,
        run: &str,
        owner: &str,
        layout: Option<WorkspaceLayout>,
        reservation: Option<EphemeralReservation>,
    ) -> Result<Option<WorkspaceIdentity>, FilesystemError> {
        let (workspace, run, owner) = (workspace.to_owned(), run.to_owned(), owner.to_owned());
        self.transaction(move |unit| {
            Box::pin(async move {
                let admitted = unit.get_run(&run).await?.ok_or(StoreError::Conflict)?;
                if let Some(mut existing) = unit.get_workspace(&workspace).await? {
                    if existing.run != run
                        || existing.owner != owner
                        || !matches!(existing.state.as_str(), "preparing" | "active")
                        || reservation
                            .as_ref()
                            .is_some_and(|r| existing.reservation.as_ref() != Some(r))
                        || layout
                            .as_ref()
                            .zip(existing.layout.as_ref())
                            .is_some_and(|(a, b)| a != b)
                    {
                        return conflict();
                    }
                    if layout.is_some() && existing.layout.is_none() {
                        existing.layout = layout;
                        unit.save_workspace(existing.clone()).await?;
                    }
                    return Ok(existing.identity);
                }
                if !admitted.live {
                    return conflict();
                }
                unit.save_workspace(WorkspaceState {
                    id: workspace,
                    run,
                    session: admitted.session,
                    owner,
                    identity: None,
                    layout,
                    reservation,
                    state: "preparing".into(),
                })
                .await?;
                Ok(None)
            })
        })
        .await
    }
    pub async fn identify(
        &self,
        workspace: &str,
        identity: &WorkspaceIdentity,
    ) -> Result<(), FilesystemError> {
        let (workspace, identity) = (workspace.to_owned(), identity.clone());
        self.transaction(move |unit| {
            Box::pin(async move {
                let mut state = required_workspace(unit, &workspace).await?;
                if state
                    .identity
                    .as_ref()
                    .is_some_and(|prior| *prior != identity)
                    || !matches!(state.state.as_str(), "preparing" | "active")
                {
                    return conflict();
                }
                state.identity = Some(identity);
                state.state = "active".into();
                unit.save_workspace(state).await?;
                Ok(())
            })
        })
        .await
    }
    #[cfg(feature = "test-support")]
    pub async fn admit(
        &self,
        workspace: &str,
        effect: &str,
        request: &Mutation,
    ) -> Result<EffectRecord, FilesystemError> {
        self.admit_step(workspace, effect, request, None).await
    }
    pub async fn admit_step(
        &self,
        workspace: &str,
        effect: &str,
        request: &Mutation,
        plan: Option<&str>,
    ) -> Result<EffectRecord, FilesystemError> {
        let (workspace, effect, request, plan) = (
            workspace.to_owned(),
            effect.to_owned(),
            request.clone(),
            plan.map(str::to_owned),
        );
        self.transaction(move |unit| {
            Box::pin(async move {
                let state = require_active(unit, &workspace).await?;
                if let Some(existing) = load_effect(unit, &state, &effect).await? {
                    if existing.request != request || existing.plan != plan {
                        return conflict();
                    }
                    return Ok(existing);
                }
                if unit
                    .pending_plan(&workspace)
                    .await?
                    .as_ref()
                    .map(|p| p.id.as_str())
                    != plan.as_deref()
                {
                    return conflict();
                }
                let running = unit
                    .get_run(&state.run)
                    .await?
                    .ok_or(StoreError::Conflict)?
                    .live;
                let closing = unit
                    .writer_for_effect(&workspace, &effect)
                    .await?
                    .is_some_and(|w| w.state == "closing");
                if !running && plan.is_none() && !closing {
                    return conflict();
                }
                require_ready(unit, &workspace, Some(&effect)).await?;
                let envelope = FilesystemRequest {
                    workspace: workspace.clone(),
                    plan: plan.clone(),
                    mutation: request.clone(),
                };
                effects::admit(
                    unit,
                    &effect_key(&state.run, &effect),
                    &serialize(&envelope)?,
                )
                .await?;
                Ok(EffectRecord {
                    request,
                    plan,
                    intent: None,
                    outcome: None,
                })
            })
        })
        .await
    }
    pub async fn prepared(&self, workspace: &str, intent: &Intent) -> Result<(), FilesystemError> {
        let (workspace, intent) = (workspace.to_owned(), intent.clone());
        self.transaction(move |unit| {
            Box::pin(async move {
                let state = required_workspace(unit, &workspace).await?;
                let existing = load_effect(unit, &state, intent.effect())
                    .await?
                    .ok_or(StoreError::Conflict)?;
                if existing.request != *intent.mutation() || existing.outcome.is_some() {
                    return conflict();
                }
                effects::prepare(
                    unit,
                    &effect_key(&state.run, intent.effect()),
                    &intent.encode()?,
                )
                .await?;
                Ok(())
            })
        })
        .await
    }
    pub async fn complete(
        &self,
        workspace: &str,
        effect: &str,
        outcome: &RecordedOutcome,
    ) -> Result<(), FilesystemError> {
        let (workspace, effect, outcome) =
            (workspace.to_owned(), effect.to_owned(), outcome.clone());
        self.transaction(move |unit| {
            Box::pin(async move {
                let state = required_workspace(unit, &workspace).await?;
                load_effect(unit, &state, &effect)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                effects::complete(
                    unit,
                    &effect_key(&state.run, &effect),
                    &serialize(&outcome)?,
                    matches!(outcome, RecordedOutcome::Rejected(_)),
                )
                .await?;
                Ok(())
            })
        })
        .await
    }
    pub async fn pending(
        &self,
        workspace: &str,
    ) -> Result<Option<(String, Mutation)>, FilesystemError> {
        let mut unit = (self.units)().await?;
        let state = required_workspace(unit.as_mut(), workspace).await?;
        let Some(id) = unit.pending_effect(&state.run).await? else {
            return Ok(None);
        };
        let record = load_effect(unit.as_mut(), &state, &id)
            .await?
            .ok_or(StoreError::Conflict)?;
        Ok(Some((id, record.request)))
    }
}

fn decode_effect(record: Effect, workspace: &str) -> Result<EffectRecord, FilesystemError> {
    if record.operation != OPERATION || record.version != VERSION {
        return conflict();
    }
    let request: FilesystemRequest = serde_json::from_slice(&record.request).map_err(invalid)?;
    if request.workspace != workspace {
        return conflict();
    }
    let intent = record
        .recovery_payload
        .map(|v| Intent::decode(&v))
        .transpose()?;
    let outcome = record
        .outcome
        .map(|v| serde_json::from_slice::<RecordedOutcome>(&v).map_err(invalid))
        .transpose()?;
    if intent
        .as_ref()
        .is_some_and(|i| i.effect() != record.id || *i.mutation() != request.mutation)
        || outcome
            .as_ref()
            .is_some_and(|o| matches!(o, RecordedOutcome::Rejected(_)) != record.rejected)
    {
        return conflict();
    }
    Ok(EffectRecord {
        request: request.mutation,
        plan: request.plan,
        intent,
        outcome,
    })
}
async fn load_effect(
    unit: &mut dyn UnitOfWork,
    workspace: &WorkspaceState,
    id: &str,
) -> Result<Option<EffectRecord>, FilesystemError> {
    unit.get_effect(&workspace.run, id)
        .await?
        .map(|r| decode_effect(r, &workspace.id))
        .transpose()
}
async fn required_workspace(
    unit: &mut dyn UnitOfWork,
    id: &str,
) -> Result<WorkspaceState, FilesystemError> {
    unit.get_workspace(id)
        .await?
        .ok_or_else(|| StoreError::Conflict.into())
}
async fn require_active(
    unit: &mut dyn UnitOfWork,
    id: &str,
) -> Result<WorkspaceState, FilesystemError> {
    let workspace = required_workspace(unit, id).await?;
    if workspace.state != "active" {
        return conflict();
    }
    Ok(workspace)
}
async fn require_live(
    unit: &mut dyn UnitOfWork,
    workspace: &WorkspaceState,
) -> Result<(), FilesystemError> {
    if !unit
        .get_run(&workspace.run)
        .await?
        .ok_or(StoreError::Conflict)?
        .live
    {
        return conflict();
    }
    Ok(())
}
async fn require_ready(
    unit: &mut dyn UnitOfWork,
    workspace: &str,
    effect: Option<&str>,
) -> Result<(), FilesystemError> {
    if unit.pending_resource(workspace).await?.is_some() {
        return conflict();
    }
    if let Some(writer) = unit.closing_writer(workspace).await?
        && writer.close_effect.as_deref() != effect
    {
        return conflict();
    }
    Ok(())
}
async fn require_idle(
    unit: &mut dyn UnitOfWork,
    workspace: &WorkspaceState,
) -> Result<(), FilesystemError> {
    require_live(unit, workspace).await?;
    require_ready(unit, &workspace.id, None).await?;
    if unit.pending_effect(&workspace.run).await?.is_some()
        || unit.pending_plan(&workspace.id).await?.is_some()
    {
        return conflict();
    }
    Ok(())
}
async fn require_capacity(
    unit: &mut dyn UnitOfWork,
    run: &str,
    additional: usize,
) -> Result<(), FilesystemError> {
    if additional as u64 > MAX_EFFECTS.saturating_sub(unit.effect_count(run).await?) {
        return conflict();
    }
    Ok(())
}
async fn require_resource_limit(
    unit: &mut dyn UnitOfWork,
    workspace: &str,
) -> Result<(), FilesystemError> {
    let count = unit.resource_counts(workspace).await?;
    if count
        .readers
        .saturating_add(count.listings)
        .saturating_add(count.writers)
        >= MAX_EFFECTS
    {
        return conflict();
    }
    Ok(())
}
fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, FilesystemError> {
    let bytes = serde_json::to_vec(value).map_err(invalid)?;
    if bytes.len() > crate::codec::MAX_RECORD {
        return Err(invalid("filesystem record exceeds limit"));
    }
    Ok(bytes)
}
fn invalid(error: impl std::fmt::Display) -> FilesystemError {
    StoreError::Io(error.to_string()).into()
}
fn conflict<T>() -> Result<T, FilesystemError> {
    Err(StoreError::Conflict.into())
}

fn effect_key<'a>(run: &'a str, effect: &'a str) -> EffectKey<'a> {
    EffectKey {
        run,
        effect,
        operation: OPERATION,
        version: VERSION,
    }
}
