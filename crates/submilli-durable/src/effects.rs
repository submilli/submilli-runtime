//! Shared effect lifecycle. The caller's unit also owns admission and object pins.
pub use crate::codec::MAX_RECORD;
use crate::{UnitOfWork, state::Effect, unit_of_work::StoreError};
pub const MAX_EFFECTS: u64 = 10_000;
pub struct EffectKey<'a> {
    pub run: &'a str,
    pub effect: &'a str,
    pub operation: &'a str,
    pub version: u32,
}

pub async fn load(
    unit: &mut dyn UnitOfWork,
    key: &EffectKey<'_>,
) -> Result<Option<Effect>, StoreError> {
    let record = unit.get_effect(key.run, key.effect).await?;
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
    key: &EffectKey<'_>,
    request: &[u8],
) -> Result<Effect, StoreError> {
    if let Some(prior) = load(unit, key).await? {
        return if prior.request == request {
            Ok(prior)
        } else {
            Err(StoreError::Conflict)
        };
    }
    if unit.effect_count(key.run).await? >= MAX_EFFECTS
        || unit.pending_effect(key.run).await?.is_some()
    {
        return Err(StoreError::Conflict);
    }
    let record = Effect {
        id: key.effect.into(),
        run: key.run.into(),
        operation: key.operation.into(),
        version: key.version,
        request: request.into(),
        recovery_payload: None,
        outcome: None,
        rejected: false,
    };
    record.validate_successor(None)?;
    unit.save_effect(record.clone()).await?;
    Ok(record)
}
pub async fn prepare(
    unit: &mut dyn UnitOfWork,
    key: &EffectKey<'_>,
    payload: &[u8],
) -> Result<(), StoreError> {
    let mut record = load(unit, key).await?.ok_or(StoreError::Conflict)?;
    if record.outcome.is_some() {
        return Err(StoreError::Conflict);
    }
    let prior = record.clone();
    record.recovery_payload = Some(payload.into());
    record.validate_successor(Some(&prior))?;
    unit.save_effect(record).await
}
pub async fn complete(
    unit: &mut dyn UnitOfWork,
    key: &EffectKey<'_>,
    outcome: &[u8],
    rejected: bool,
) -> Result<(), StoreError> {
    let mut record = load(unit, key).await?.ok_or(StoreError::Conflict)?;
    let prior = record.clone();
    record.outcome = Some(outcome.into());
    record.rejected = rejected;
    record.validate_successor(Some(&prior))?;
    unit.save_effect(record).await
}
