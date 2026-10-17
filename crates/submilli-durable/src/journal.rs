//! Filesystem operation admission, independent of run lifecycle and execution history.
use crate::{
    UnitFactory, UnitOfWork, filesystems::FilesystemError, recovery::*, state::*,
    unit_of_work::StoreError,
};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
mod resources;
use crate::operations::{self, MAX_OPERATIONS, OperationKey};
pub(crate) use crate::state::RecordedOutcome;
const OPERATION: &str = "filesystem.mutation";
const VERSION: u32 = 1;

#[derive(Clone)]
pub struct FilesystemJournal {
    units: UnitFactory,
    binding: [u8; 32],
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FilesystemRequest {
    binding: [u8; 32],
    mutation: Mutation,
}

impl FilesystemJournal {
    pub fn new(units: UnitFactory, binding: [u8; 32]) -> Self {
        Self { units, binding }
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
    pub async fn admit(
        &self,
        scope: &str,
        id: &str,
        request: &Mutation,
    ) -> Result<OperationRecord, FilesystemError> {
        self.admit_operation(scope, id, request, None).await
    }
    pub(crate) async fn admit_writer_close(
        &self,
        scope: &str,
        id: &str,
        request: &Mutation,
        writer: &str,
    ) -> Result<OperationRecord, FilesystemError> {
        self.admit_operation(scope, id, request, Some(writer)).await
    }
    async fn admit_operation(
        &self,
        scope: &str,
        id: &str,
        request: &Mutation,
        writer: Option<&str>,
    ) -> Result<OperationRecord, FilesystemError> {
        let (scope, id, request) = (scope.to_owned(), id.to_owned(), request.clone());
        let writer = writer.map(str::to_owned);
        let binding = self.binding;
        self.transaction(move |unit| {
            Box::pin(async move {
                if let Some(existing) = load_operation(unit, &scope, &id, binding).await? {
                    if existing.request != request {
                        return conflict();
                    }
                    return Ok(existing);
                }
                require_ready(
                    unit,
                    &scope,
                    writer.as_deref().map(|writer| (writer, id.as_str())),
                )
                .await?;
                let envelope = FilesystemRequest {
                    binding,
                    mutation: request.clone(),
                };
                operations::admit(unit, &operation_key(&scope, &id), &serialize(&envelope)?)
                    .await?;
                Ok(OperationRecord {
                    request,
                    intent: None,
                    outcome: None,
                })
            })
        })
        .await
    }
    pub async fn prepared(&self, scope: &str, intent: &Intent) -> Result<(), FilesystemError> {
        let (scope, intent) = (scope.to_owned(), intent.clone());
        let binding = self.binding;
        self.transaction(move |unit| {
            Box::pin(async move {
                let existing = load_operation(unit, &scope, intent.effect(), binding)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                if existing.request != *intent.mutation() || existing.outcome.is_some() {
                    return conflict();
                }
                operations::prepare(
                    unit,
                    &operation_key(&scope, intent.effect()),
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
        scope: &str,
        id: &str,
        outcome: &RecordedOutcome,
    ) -> Result<(), FilesystemError> {
        let (scope, id, outcome) = (scope.to_owned(), id.to_owned(), outcome.clone());
        let binding = self.binding;
        self.transaction(move |unit| {
            Box::pin(async move {
                load_operation(unit, &scope, &id, binding)
                    .await?
                    .ok_or(StoreError::Conflict)?;
                operations::complete(
                    unit,
                    &operation_key(&scope, &id),
                    &serialize(&outcome)?,
                    matches!(outcome, RecordedOutcome::Rejected(_)),
                )
                .await?;
                Ok(())
            })
        })
        .await
    }
}
fn operation_key<'a>(scope: &'a str, id: &'a str) -> OperationKey<'a> {
    OperationKey {
        scope,
        effect: id,
        operation: OPERATION,
        version: VERSION,
    }
}
fn decode_operation(
    record: Operation,
    binding: [u8; 32],
) -> Result<OperationRecord, FilesystemError> {
    if record.operation != OPERATION || record.version != VERSION {
        return conflict();
    }
    let request: FilesystemRequest = serde_json::from_slice(&record.request).map_err(invalid)?;
    if request.binding != binding {
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
    Ok(OperationRecord {
        request: request.mutation,
        intent,
        outcome,
    })
}
async fn load_operation(
    unit: &mut dyn UnitOfWork,
    scope: &str,
    id: &str,
    binding: [u8; 32],
) -> Result<Option<OperationRecord>, FilesystemError> {
    unit.get_operation(scope, id)
        .await?
        .map(|r| decode_operation(r, binding))
        .transpose()
}
async fn require_ready(
    unit: &mut dyn UnitOfWork,
    scope: &str,
    writer_close: Option<(&str, &str)>,
) -> Result<(), FilesystemError> {
    if unit.pending_resource(scope).await?.is_some() {
        return conflict();
    }
    if let Some(writer) = unit.closing_writer(scope).await?
        && writer_close
            != writer
                .close_effect
                .as_deref()
                .map(|effect| (writer.id.as_str(), effect))
    {
        return conflict();
    }
    Ok(())
}
async fn require_idle(unit: &mut dyn UnitOfWork, scope: &str) -> Result<(), FilesystemError> {
    require_ready(unit, scope, None).await?;
    if unit.pending_operation(scope).await?.is_some() {
        return conflict();
    }
    Ok(())
}
async fn require_capacity(unit: &mut dyn UnitOfWork, scope: &str) -> Result<(), FilesystemError> {
    if unit.operation_count(scope).await? >= MAX_OPERATIONS {
        return conflict();
    }
    Ok(())
}
async fn require_unused_operation(
    unit: &mut dyn UnitOfWork,
    scope: &str,
    id: &str,
) -> Result<(), FilesystemError> {
    if unit.get_operation(scope, id).await?.is_some()
        || unit.writer_for_effect(scope, id).await?.is_some()
    {
        return conflict();
    }
    Ok(())
}
async fn require_resource_limit(
    unit: &mut dyn UnitOfWork,
    scope: &str,
) -> Result<(), FilesystemError> {
    let count = unit.resource_counts(scope).await?;
    if count
        .readers
        .saturating_add(count.listings)
        .saturating_add(count.writers)
        >= MAX_OPERATIONS
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
