//! Version-one sealed filesystem records. Adapters bind these opaque columns;
//! all associated-data names and serialized shapes are defined here.
use crate::{
    codec::{decode, encode, open, operation_owner, seal},
    sealing::Sealer,
    state::*,
    unit_of_work::StoreError,
};
use serde::{Serialize, de::DeserializeOwned};
#[derive(Default)]
pub struct ResourceColumns {
    pub request: Vec<u8>,
    pub result: Option<Vec<u8>>,
    pub error: Option<Vec<u8>>,
    pub progress: Option<Vec<u8>>,
}
pub struct OperationColumns {
    pub request: Vec<u8>,
    pub recovery: Option<Vec<u8>>,
    pub outcome: Option<Vec<u8>>,
}
fn encode_optional<T: Serialize>(
    sealer: &Sealer,
    owner: &str,
    value: Option<&T>,
) -> Result<Option<Vec<u8>>, StoreError> {
    value.map(|v| encode(sealer, owner, v)).transpose()
}
fn decode_optional<T: DeserializeOwned>(
    sealer: &Sealer,
    owner: &str,
    bytes: Option<Vec<u8>>,
) -> Result<Option<T>, StoreError> {
    bytes.map(|v| decode(sealer, owner, &v)).transpose()
}

pub fn encode_reader(
    sealer: &Sealer,
    scope: &str,
    value: &Reader,
) -> Result<ResourceColumns, StoreError> {
    let owner = format!("resource:{scope}:{}", value.id);
    Ok(ResourceColumns {
        request: encode(
            sealer,
            &owner,
            &(1u32, value.binding, &value.path, &value.options),
        )?,
        result: encode_optional(sealer, &owner, value.position.as_ref())?,
        error: encode_optional(sealer, &owner, value.error.as_ref())?,
        progress: encode_optional(
            sealer,
            &format!("{owner}:progress"),
            value.progress.as_ref(),
        )?,
    })
}
pub fn decode_reader(
    sealer: &Sealer,
    scope: &str,
    id: String,
    columns: ResourceColumns,
) -> Result<Reader, StoreError> {
    let owner = format!("resource:{scope}:{id}");
    let (version, binding, path, options): (
        u32,
        [u8; 32],
        String,
        interpreter::runtime::filesystem::ReaderOptions,
    ) = decode(sealer, &owner, &columns.request)?;
    if version != 1 {
        return Err(StoreError::Conflict);
    }
    Ok(Reader {
        binding,
        id,
        path,
        options,
        progress: decode_optional(sealer, &format!("{owner}:progress"), columns.progress)?,
        position: decode_optional(sealer, &owner, columns.result)?,
        error: decode_optional(sealer, &owner, columns.error)?,
    })
}

pub fn encode_listing(
    sealer: &Sealer,
    scope: &str,
    value: &Listing,
) -> Result<ResourceColumns, StoreError> {
    let owner = format!("listing:{scope}:{}", value.id);
    Ok(ResourceColumns {
        request: encode(
            sealer,
            &owner,
            &(1u32, value.binding, &value.path, value.recursive),
        )?,
        result: value
            .content
            .as_ref()
            .map(|entries| encode_listing_output(sealer, &owner, entries))
            .transpose()?,
        error: encode_optional(sealer, &owner, value.error.as_ref())?,
        progress: encode_optional(
            sealer,
            &format!("{owner}:progress"),
            value.progress.as_ref(),
        )?,
    })
}
pub fn decode_listing(
    sealer: &Sealer,
    scope: &str,
    id: String,
    columns: ResourceColumns,
) -> Result<Listing, StoreError> {
    let owner = format!("listing:{scope}:{id}");
    let (version, binding, path, recursive): (u32, [u8; 32], String, bool) =
        decode(sealer, &owner, &columns.request)?;
    if version != 1 {
        return Err(StoreError::Conflict);
    }
    Ok(Listing {
        binding,
        id,
        path,
        recursive,
        progress: decode_optional(sealer, &format!("{owner}:progress"), columns.progress)?,
        content: columns
            .result
            .map(|bytes| decode_listing_output(sealer, &owner, &bytes))
            .transpose()?,
        error: decode_optional(sealer, &owner, columns.error)?,
    })
}

fn encode_listing_output(
    sealer: &Sealer,
    owner: &str,
    entries: &[crate::recovery::DirectoryEntry],
) -> Result<Vec<u8>, StoreError> {
    let bytes = serde_json::to_vec(entries).map_err(|e| StoreError::Io(e.to_string()))?;
    if bytes.len() > crate::recovery::MAX_LISTING_BYTES {
        return Err(StoreError::Conflict);
    }
    sealer.seal(owner, &bytes)
}
fn decode_listing_output(
    sealer: &Sealer,
    owner: &str,
    bytes: &[u8],
) -> Result<Vec<crate::recovery::DirectoryEntry>, StoreError> {
    if bytes.len() > crate::recovery::MAX_LISTING_BYTES + 128 {
        return Err(StoreError::Conflict);
    }
    let plain = sealer.open(owner, bytes, "directory output")?;
    if plain.len() > crate::recovery::MAX_LISTING_BYTES {
        return Err(StoreError::Conflict);
    }
    serde_json::from_slice(&plain).map_err(|e| StoreError::Io(e.to_string()))
}

pub fn encode_writer(sealer: &Sealer, scope: &str, value: &Writer) -> Result<Vec<u8>, StoreError> {
    encode(
        sealer,
        &format!("resource:{scope}:{}", value.id),
        &(value.binding, &value.path),
    )
}
pub fn decode_writer_path(
    sealer: &Sealer,
    scope: &str,
    id: &str,
    request: &[u8],
) -> Result<([u8; 32], String), StoreError> {
    decode(sealer, &format!("resource:{scope}:{id}"), request)
}
pub fn encode_chunk(sealer: &Sealer, scope: &str, chunk: &Chunk) -> Result<Vec<u8>, StoreError> {
    encode(
        sealer,
        &format!("resource:{scope}:{}:{}", chunk.writer, chunk.id),
        &chunk.content,
    )
}
pub fn decode_chunk_content(
    sealer: &Sealer,
    scope: &str,
    writer: &str,
    id: &str,
    content: &[u8],
) -> Result<crate::recovery::Content, StoreError> {
    decode(sealer, &format!("resource:{scope}:{writer}:{id}"), content)
}

pub fn encode_operation(
    sealer: &Sealer,
    effect: &Operation,
) -> Result<OperationColumns, StoreError> {
    Ok(OperationColumns {
        request: seal(
            sealer,
            &operation_owner(effect, "request")?,
            &effect.request,
        )?,
        recovery: effect
            .recovery_payload
            .as_ref()
            .map(|v| seal(sealer, &operation_owner(effect, "recovery")?, v))
            .transpose()?,
        outcome: effect
            .outcome
            .as_ref()
            .map(|v| seal(sealer, &operation_owner(effect, "outcome")?, v))
            .transpose()?,
    })
}
pub fn decode_operation(
    sealer: &Sealer,
    mut effect: Operation,
    columns: OperationColumns,
) -> Result<Operation, StoreError> {
    effect.request = open(
        sealer,
        &operation_owner(&effect, "request")?,
        &columns.request,
    )?;
    effect.recovery_payload = columns
        .recovery
        .map(|v| open(sealer, &operation_owner(&effect, "recovery")?, &v))
        .transpose()?;
    effect.outcome = columns
        .outcome
        .map(|v| open(sealer, &operation_owner(&effect, "outcome")?, &v))
        .transpose()?;
    Ok(effect)
}
