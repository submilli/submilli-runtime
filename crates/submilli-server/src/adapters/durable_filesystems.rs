//! SQLite persistence for the local filesystem recovery protocol. Filesystem
//! publication and outcome commit are deliberately separate operations.
use crate::adapters::durable_effects::{self as effects, EffectKey, MAX_EFFECTS, MAX_RECORD};
use crate::adapters::sealing::Sealer;
use crate::database::{DatabaseError, ServerDatabase};
use crate::domain::durable_object::{ContentIdentity, ObjectId, ObjectReference};
use crate::filesystems::FilesystemError;
use interpreter::runtime::fs::recovery::{
    EphemeralReservation, Intent, Mutation, Outcome, WorkspaceIdentity, WorkspaceLayout,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqliteConnection};
use std::sync::Arc;

const OPERATION: &str = "filesystem.mutation";
const VERSION: u32 = 1;

mod lifecycle;
mod listings;
mod resources;
pub(crate) use resources::PendingResource;
mod retention;
mod sequences;
use retention::{release_content, retain_content};
pub(crate) use sequences::plan_effect_id;

#[derive(Clone)]
pub(crate) struct FilesystemJournal {
    database: Arc<ServerDatabase>,
    sealer: Sealer,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum RecordedOutcome {
    Completed(Outcome),
    Rejected(String),
}

pub(crate) struct EffectRecord {
    pub request: Mutation,
    pub plan: Option<String>,
    pub intent: Option<Intent>,
    pub outcome: Option<RecordedOutcome>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FilesystemRequest {
    workspace: String,
    plan: Option<String>,
    mutation: Mutation,
}

impl FilesystemJournal {
    pub async fn open(
        database: Arc<ServerDatabase>,
        sealer: Sealer,
        expected_store: &str,
    ) -> Result<Self, FilesystemError> {
        if database.is_ephemeral() {
            return Err(FilesystemError::Configuration(
                "persistent database required",
            ));
        }
        let expected = expected_store.to_owned();
        let matches = database
            .read(move |connection| {
                Box::pin(async move {
                    let actual: Option<String> = sqlx::query_scalar(
                        "SELECT identity FROM durable_object_store WHERE singleton=1",
                    )
                    .fetch_optional(connection)
                    .await?;
                    Ok(actual == Some(expected))
                })
            })
            .await?;
        if !matches {
            return Err(FilesystemError::Configuration(
                "object store belongs to another database",
            ));
        }
        Ok(Self { database, sealer })
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
        let workspace = workspace.to_owned();
        let run = run.to_owned();
        let owner = owner.to_owned();
        let sealer = self.sealer.clone();
        self.database.transaction(move |connection| Box::pin(async move {
            let admitted = sqlx::query("SELECT session_id,state,principal FROM runs WHERE run_id=?")
                .bind(&run).fetch_optional(&mut *connection).await?.ok_or(DatabaseError::SessionConflict)?;
            let session: String = admitted.try_get("session_id")?;
            let state: String = admitted.try_get("state")?;
            let existing = sqlx::query("SELECT run_id,owner,identity,layout,reservation,state FROM durable_workspaces WHERE workspace_id=?")
                .bind(&workspace).fetch_optional(&mut *connection).await?;
            if let Some(existing) = existing {
                if existing.try_get::<String, _>("run_id")? != run || existing.try_get::<String, _>("owner")? != owner ||
                    !matches!(existing.try_get::<&str, _>("state")?, "preparing" | "active") { return Err(DatabaseError::SessionConflict); }
                if let Some(reservation) = &reservation {
                    let bytes = existing.try_get::<Option<Vec<u8>>, _>("reservation")?.ok_or(DatabaseError::SessionConflict)?;
                    if decode::<EphemeralReservation>(&sealer, &format!("reservation:{workspace}"), &bytes)? != *reservation { return Err(DatabaseError::SessionConflict); }
                }
                if let Some(layout) = &layout {
                    if let Some(bytes) = existing.try_get::<Option<Vec<u8>>, _>("layout")? {
                        if decode::<WorkspaceLayout>(&sealer, &format!("layout:{workspace}"), &bytes)? != *layout { return Err(DatabaseError::SessionConflict); }
                    } else {
                        let sealed = encode(&sealer, &format!("layout:{workspace}"), layout)?;
                        sqlx::query("UPDATE durable_workspaces SET layout=? WHERE workspace_id=?")
                            .bind(sealed).bind(&workspace).execute(&mut *connection).await?;
                    }
                }
                return existing.try_get::<Option<Vec<u8>>, _>("identity")?.map(|bytes| decode(&sealer, &workspace, &bytes)).transpose();
            }
            if !matches!(state.as_str(), "reserved" | "running" | "blocked") { return Err(DatabaseError::SessionConflict); }
            let layout = layout.as_ref().map(|layout| encode(&sealer, &format!("layout:{workspace}"), layout)).transpose()?;
            let reservation = reservation.as_ref().map(|value| encode(&sealer, &format!("reservation:{workspace}"), value)).transpose()?;
            sqlx::query("INSERT INTO durable_workspaces(workspace_id,run_id,session_id,owner,layout,reservation,state) VALUES(?,?,?,?,?,?,'preparing')")
                .bind(&workspace).bind(run).bind(session).bind(owner).bind(layout).bind(reservation).execute(connection).await?;
            Ok(None)
        })).await.map_err(Into::into)
    }

    pub async fn identify(
        &self,
        workspace: &str,
        identity: &WorkspaceIdentity,
    ) -> Result<(), FilesystemError> {
        let workspace = workspace.to_owned();
        let identity = identity.clone();
        let sealer = self.sealer.clone();
        self.database.transaction(move |connection| Box::pin(async move {
            let row = sqlx::query("SELECT identity,state FROM durable_workspaces WHERE workspace_id=?")
                .bind(&workspace).fetch_one(&mut *connection).await?;
            if let Some(bytes) = row.try_get::<Option<Vec<u8>>, _>("identity")?
                && decode::<WorkspaceIdentity>(&sealer, &workspace, &bytes)? != identity { return Err(DatabaseError::SessionConflict); }
            if !matches!(row.try_get::<&str, _>("state")?, "preparing" | "active") { return Err(DatabaseError::SessionConflict); }
            let bytes = encode(&sealer, &workspace, &identity)?;
            sqlx::query("UPDATE durable_workspaces SET identity=?,state='active' WHERE workspace_id=?")
                .bind(bytes).bind(workspace).execute(connection).await?;
            Ok(())
        })).await.map_err(Into::into)
    }

    #[cfg(test)]
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
        let plan = plan.map(str::to_owned);
        let workspace = workspace.to_owned();
        let effect = effect.to_owned();
        let request = request.clone();
        let sealer = self.sealer.clone();
        self.database.transaction(move |connection| Box::pin(async move {
            let owner: String = sqlx::query_scalar("SELECT owner FROM durable_workspaces WHERE workspace_id=? AND state='active'")
                .bind(&workspace).fetch_optional(&mut *connection).await?.ok_or(DatabaseError::SessionConflict)?;
            sequences::require_plan(connection, &sealer, &workspace, &effect, plan.as_deref()).await?;
            if let Some(existing) = load_effect(connection, &sealer, &workspace, &effect).await? {
                if existing.request != request { return Err(DatabaseError::SessionConflict); }
                return Ok(existing);
            }
            let running: bool = sqlx::query_scalar("SELECT runs.state IN ('reserved','running','blocked') FROM runs JOIN durable_workspaces w ON runs.run_id=w.run_id WHERE workspace_id=?")
                .bind(&workspace).fetch_one(&mut *connection).await?;
            let closing: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM durable_filesystem_writers WHERE workspace_id=? AND state='closing' AND close_effect=?)")
                .bind(&workspace).bind(&effect).fetch_one(&mut *connection).await?;
            if !running && plan.is_none() && !closing { return Err(DatabaseError::SessionConflict); }
            resources::require_ready(connection, &workspace, Some(&effect)).await?;
            let run = workspace_run(connection, &workspace).await?;
            retain_content(connection, &sealer, &workspace, &owner, &request).await?;
            let envelope = FilesystemRequest { workspace: workspace.clone(), plan: plan.clone(), mutation: request.clone() };
            let bytes = serde_json::to_vec(&envelope).map_err(invalid)?;
            effects::admit(connection, &sealer, &effect_key(&run, &effect), &bytes).await?;
            Ok(EffectRecord { request, plan, intent: None, outcome: None })
        })).await.map_err(Into::into)
    }

    pub async fn prepared(&self, workspace: &str, intent: &Intent) -> Result<(), FilesystemError> {
        let workspace = workspace.to_owned();
        let intent = intent.clone();
        let sealer = self.sealer.clone();
        self.database
            .transaction(move |connection| {
                Box::pin(async move {
                    let effect = intent.effect();
                    let existing = load_effect(connection, &sealer, &workspace, effect)
                        .await?
                        .ok_or(DatabaseError::SessionConflict)?;
                    if existing.request != *intent.mutation() {
                        return Err(DatabaseError::SessionConflict);
                    }
                    let run = workspace_run(connection, &workspace).await?;
                    let bytes = intent.encode().map_err(invalid)?;
                    effects::prepare(connection, &sealer, &effect_key(&run, effect), &bytes).await
                })
            })
            .await
            .map_err(Into::into)
    }

    pub async fn complete(
        &self,
        workspace: &str,
        effect: &str,
        outcome: &RecordedOutcome,
    ) -> Result<(), FilesystemError> {
        let workspace = workspace.to_owned();
        let effect = effect.to_owned();
        let sealer = self.sealer.clone();
        let outcome = outcome.clone();
        self.database
            .transaction(move |connection| {
                Box::pin(async move {
                    load_effect(connection, &sealer, &workspace, &effect)
                        .await?
                        .ok_or(DatabaseError::SessionConflict)?;
                    let run = workspace_run(connection, &workspace).await?;
                    let bytes = serde_json::to_vec(&outcome).map_err(invalid)?;
                    effects::complete(
                        connection,
                        &sealer,
                        &effect_key(&run, &effect),
                        &bytes,
                        matches!(outcome, RecordedOutcome::Rejected(_)),
                    )
                    .await
                })
            })
            .await
            .map_err(Into::into)
    }

    pub async fn pending(
        &self,
        workspace: &str,
    ) -> Result<Option<(String, Mutation)>, FilesystemError> {
        let workspace = workspace.to_owned();
        let sealer = self.sealer.clone();
        self.database
            .read(move |connection| {
                Box::pin(async move {
                    let run = workspace_run(connection, &workspace).await?;
                    let Some(effect) = effects::pending(connection, &run).await? else {
                        return Ok(None);
                    };
                    let record = load_effect(connection, &sealer, &workspace, &effect)
                        .await?
                        .ok_or(DatabaseError::SessionConflict)?;
                    Ok(Some((effect, record.request)))
                })
            })
            .await
            .map_err(Into::into)
    }
}

async fn load_effect(
    connection: &mut SqliteConnection,
    sealer: &Sealer,
    workspace: &str,
    effect: &str,
) -> Result<Option<EffectRecord>, DatabaseError> {
    let run = workspace_run(connection, workspace).await?;
    let Some(record) = effects::load(connection, sealer, &effect_key(&run, effect)).await? else {
        return Ok(None);
    };
    let request: FilesystemRequest = serde_json::from_slice(&record.request).map_err(invalid)?;
    if request.workspace != workspace {
        return Err(DatabaseError::SessionConflict);
    }
    let intent = record
        .recovery_payload
        .map(|bytes| Intent::decode(&bytes).map_err(invalid))
        .transpose()?;
    let outcome = record
        .outcome
        .map(|bytes| serde_json::from_slice::<RecordedOutcome>(&bytes).map_err(invalid))
        .transpose()?;
    if let Some(intent) = &intent
        && (intent.effect() != effect || *intent.mutation() != request.mutation)
    {
        return Err(DatabaseError::SessionConflict);
    }
    if outcome
        .as_ref()
        .is_some_and(|outcome| matches!(outcome, RecordedOutcome::Rejected(_)) != record.rejected)
    {
        return Err(DatabaseError::SessionConflict);
    }
    Ok(Some(EffectRecord {
        request: request.mutation,
        plan: request.plan,
        intent,
        outcome,
    }))
}

async fn workspace_run(
    connection: &mut SqliteConnection,
    workspace: &str,
) -> Result<String, DatabaseError> {
    sqlx::query_scalar("SELECT run_id FROM durable_workspaces WHERE workspace_id=?")
        .bind(workspace)
        .fetch_optional(connection)
        .await?
        .ok_or(DatabaseError::SessionConflict)
}

fn effect_key<'a>(run: &'a str, effect: &'a str) -> EffectKey<'a> {
    EffectKey {
        run,
        effect,
        operation: OPERATION,
        version: VERSION,
    }
}

fn encode<T: Serialize>(sealer: &Sealer, owner: &str, value: &T) -> Result<Vec<u8>, DatabaseError> {
    let bytes = serde_json::to_vec(value).map_err(invalid)?;
    if bytes.len() > MAX_RECORD {
        return Err(invalid("filesystem record exceeds limit"));
    }
    sealer.seal(owner, &bytes).map_err(invalid)
}

fn decode<T: serde::de::DeserializeOwned>(
    sealer: &Sealer,
    owner: &str,
    bytes: &[u8],
) -> Result<T, DatabaseError> {
    if bytes.len() > MAX_RECORD + 128 {
        return Err(invalid("filesystem record exceeds limit"));
    }
    let plain = sealer
        .open(owner, bytes, "filesystem record")
        .map_err(invalid)?;
    serde_json::from_slice(&plain).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> DatabaseError {
    DatabaseError::Import(error.to_string())
}
