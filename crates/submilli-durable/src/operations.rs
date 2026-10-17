//! Filesystem-only mutation receipts. Admission records fingerprints, not input bytes.
pub use crate::codec::MAX_RECORD;
use crate::{UnitOfWork, state::Operation, unit_of_work::StoreError};
pub const MAX_OPERATIONS: u64 = 10_000;
pub struct OperationKey<'a> {
    pub scope: &'a str,
    pub effect: &'a str,
    pub operation: &'a str,
    pub version: u32,
}

pub async fn load(
    unit: &mut dyn UnitOfWork,
    key: &OperationKey<'_>,
) -> Result<Option<Operation>, StoreError> {
    let record = unit.get_operation(key.scope, key.effect).await?;
    if record
        .as_ref()
        .is_some_and(|r| r.operation != key.operation || r.version != key.version)
    {
        return Err(StoreError::Conflict);
    }
    Ok(record)
}
pub async fn admit(
    unit: &mut dyn UnitOfWork,
    key: &OperationKey<'_>,
    request: &[u8],
) -> Result<Operation, StoreError> {
    if let Some(prior) = load(unit, key).await? {
        return if prior.request == request {
            Ok(prior)
        } else {
            Err(StoreError::Conflict)
        };
    }
    if unit.operation_count(key.scope).await? >= MAX_OPERATIONS
        || unit.pending_operation(key.scope).await?.is_some()
    {
        return Err(StoreError::Conflict);
    }
    let record = Operation {
        id: key.effect.into(),
        scope: key.scope.into(),
        operation: key.operation.into(),
        version: key.version,
        request: request.into(),
        recovery_payload: None,
        outcome: None,
        rejected: false,
    };
    record.validate_successor(None)?;
    unit.save_operation(record.clone()).await?;
    Ok(record)
}
pub async fn prepare(
    unit: &mut dyn UnitOfWork,
    key: &OperationKey<'_>,
    payload: &[u8],
) -> Result<(), StoreError> {
    let mut record = load(unit, key).await?.ok_or(StoreError::Conflict)?;
    if record.outcome.is_some() {
        return Err(StoreError::Conflict);
    }
    let prior = record.clone();
    record.recovery_payload = Some(payload.into());
    record.validate_successor(Some(&prior))?;
    unit.save_operation(record).await
}
pub async fn complete(
    unit: &mut dyn UnitOfWork,
    key: &OperationKey<'_>,
    outcome: &[u8],
    rejected: bool,
) -> Result<(), StoreError> {
    let mut record = load(unit, key).await?.ok_or(StoreError::Conflict)?;
    let prior = record.clone();
    record.outcome = Some(outcome.into());
    record.rejected = rejected;
    record.validate_successor(Some(&prior))?;
    unit.save_operation(record).await
}
