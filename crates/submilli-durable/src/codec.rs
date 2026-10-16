//! Version-preserving database codecs shared by persistence adapters.
use crate::{sealing::Sealer, unit_of_work::StoreError};
use serde::{Serialize, de::DeserializeOwned};
pub const MAX_RECORD: usize = 64 * 1024;

pub fn encode<T: Serialize>(
    sealer: &Sealer,
    owner: &str,
    value: &T,
) -> Result<Vec<u8>, StoreError> {
    seal(sealer, owner, &serde_json::to_vec(value).map_err(invalid)?)
}
pub fn decode<T: DeserializeOwned>(
    sealer: &Sealer,
    owner: &str,
    bytes: &[u8],
) -> Result<T, StoreError> {
    serde_json::from_slice(&open(sealer, owner, bytes)?).map_err(invalid)
}
pub fn seal(sealer: &Sealer, owner: &str, bytes: &[u8]) -> Result<Vec<u8>, StoreError> {
    if bytes.len() > MAX_RECORD {
        return Err(invalid("record exceeds limit"));
    }
    sealer.seal(owner, bytes)
}
pub fn open(sealer: &Sealer, owner: &str, bytes: &[u8]) -> Result<Vec<u8>, StoreError> {
    if bytes.len() > MAX_RECORD + 128 {
        return Err(invalid("record exceeds limit"));
    }
    let plain = sealer.open(owner, bytes, "durable record")?;
    if plain.len() > MAX_RECORD {
        return Err(invalid("record exceeds limit"));
    }
    Ok(plain)
}
pub fn effect_owner(effect: &crate::state::Effect, field: &str) -> Result<String, StoreError> {
    serde_json::to_string(&(
        "durable-effect",
        &effect.run,
        &effect.id,
        &effect.operation,
        effect.version,
        field,
    ))
    .map_err(invalid)
}
fn invalid(error: impl std::fmt::Display) -> StoreError {
    StoreError::Io(error.to_string())
}
