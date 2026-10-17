//! Version-one sealed filesystem records. Adapters bind these opaque columns;
//! all associated-data names and serialized shapes are defined here.
use crate::{
    codec::{decode, effect_owner, encode, open, seal},
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
}
pub struct WorkspaceColumns {
    pub identity: Option<Vec<u8>>,
    pub layout: Option<Vec<u8>>,
    pub reservation: Option<Vec<u8>>,
}
pub struct EffectColumns {
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
    workspace: &str,
    value: &Reader,
) -> Result<ResourceColumns, StoreError> {
    let owner = format!("resource:{workspace}:{}", value.id);
    Ok(ResourceColumns {
        request: encode(sealer, &owner, &value.path)?,
        result: encode_optional(sealer, &owner, value.position.as_ref())?,
        error: encode_optional(sealer, &owner, value.error.as_ref())?,
    })
}
pub fn decode_reader(
    sealer: &Sealer,
    workspace: &str,
    id: String,
    columns: ResourceColumns,
) -> Result<Reader, StoreError> {
    let owner = format!("resource:{workspace}:{id}");
    let path: String = decode(sealer, &owner, &columns.request)?;
    Ok(Reader {
        id,
        path,
        position: decode_optional(sealer, &owner, columns.result)?,
        error: decode_optional(sealer, &owner, columns.error)?,
    })
}

pub fn encode_listing(
    sealer: &Sealer,
    workspace: &str,
    value: &Listing,
) -> Result<ResourceColumns, StoreError> {
    let owner = format!("listing:{workspace}:{}", value.id);
    Ok(ResourceColumns {
        request: encode(sealer, &owner, &(value.path.clone(), value.recursive))?,
        result: value
            .content
            .as_ref()
            .map(|entries| encode_listing_output(sealer, &owner, entries))
            .transpose()?,
        error: encode_optional(sealer, &owner, value.error.as_ref())?,
    })
}
pub fn decode_listing(
    sealer: &Sealer,
    workspace: &str,
    id: String,
    columns: ResourceColumns,
) -> Result<Listing, StoreError> {
    let owner = format!("listing:{workspace}:{id}");
    let (path, recursive): (String, bool) = decode(sealer, &owner, &columns.request)?;
    Ok(Listing {
        id,
        path,
        recursive,
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

pub fn encode_plan(
    sealer: &Sealer,
    workspace: &str,
    value: &Plan,
) -> Result<ResourceColumns, StoreError> {
    let owner = format!("plan:{workspace}:{}", value.id);
    Ok(ResourceColumns {
        request: encode(sealer, &owner, &value.steps)?,
        result: encode_optional(sealer, &owner, value.outcome.as_ref())?,
        error: None,
    })
}
pub fn decode_plan(
    sealer: &Sealer,
    workspace: &str,
    id: String,
    columns: ResourceColumns,
) -> Result<Plan, StoreError> {
    let owner = format!("plan:{workspace}:{id}");
    let steps = decode(sealer, &owner, &columns.request)?;
    Ok(Plan {
        id,
        steps,
        outcome: decode_optional(sealer, &owner, columns.result)?,
    })
}

pub fn encode_writer(
    sealer: &Sealer,
    workspace: &str,
    value: &Writer,
) -> Result<Vec<u8>, StoreError> {
    encode(
        sealer,
        &format!("resource:{workspace}:{}", value.id),
        &value.path,
    )
}
pub fn decode_writer_path(
    sealer: &Sealer,
    workspace: &str,
    id: &str,
    request: &[u8],
) -> Result<String, StoreError> {
    decode(sealer, &format!("resource:{workspace}:{id}"), request)
}
pub fn encode_chunk(
    sealer: &Sealer,
    workspace: &str,
    chunk: &Chunk,
) -> Result<Vec<u8>, StoreError> {
    encode(
        sealer,
        &format!("resource:{workspace}:{}:{}", chunk.writer, chunk.id),
        &chunk.content,
    )
}
pub fn decode_chunk_content(
    sealer: &Sealer,
    workspace: &str,
    writer: &str,
    id: &str,
    content: &[u8],
) -> Result<crate::recovery::Content, StoreError> {
    decode(
        sealer,
        &format!("resource:{workspace}:{writer}:{id}"),
        content,
    )
}

pub fn encode_workspace(
    sealer: &Sealer,
    workspace: &WorkspaceState,
) -> Result<WorkspaceColumns, StoreError> {
    let id = &workspace.id;
    Ok(WorkspaceColumns {
        identity: encode_optional(sealer, id, workspace.identity.as_ref())?,
        layout: encode_optional(sealer, &format!("layout:{id}"), workspace.layout.as_ref())?,
        reservation: encode_optional(
            sealer,
            &format!("reservation:{id}"),
            workspace.reservation.as_ref(),
        )?,
    })
}
pub fn decode_workspace(
    sealer: &Sealer,
    mut workspace: WorkspaceState,
    columns: WorkspaceColumns,
) -> Result<WorkspaceState, StoreError> {
    let id = &workspace.id;
    workspace.identity = decode_optional(sealer, id, columns.identity)?;
    workspace.layout = decode_optional(sealer, &format!("layout:{id}"), columns.layout)?;
    workspace.reservation =
        decode_optional(sealer, &format!("reservation:{id}"), columns.reservation)?;
    Ok(workspace)
}

pub fn encode_effect(sealer: &Sealer, effect: &Effect) -> Result<EffectColumns, StoreError> {
    Ok(EffectColumns {
        request: seal(sealer, &effect_owner(effect, "request")?, &effect.request)?,
        recovery: effect
            .recovery_payload
            .as_ref()
            .map(|v| seal(sealer, &effect_owner(effect, "recovery")?, v))
            .transpose()?,
        outcome: effect
            .outcome
            .as_ref()
            .map(|v| seal(sealer, &effect_owner(effect, "outcome")?, v))
            .transpose()?,
    })
}
pub fn decode_effect(
    sealer: &Sealer,
    mut effect: Effect,
    columns: EffectColumns,
) -> Result<Effect, StoreError> {
    effect.request = open(sealer, &effect_owner(&effect, "request")?, &columns.request)?;
    effect.recovery_payload = columns
        .recovery
        .map(|v| open(sealer, &effect_owner(&effect, "recovery")?, &v))
        .transpose()?;
    effect.outcome = columns
        .outcome
        .map(|v| open(sealer, &effect_owner(&effect, "outcome")?, &v))
        .transpose()?;
    Ok(effect)
}
