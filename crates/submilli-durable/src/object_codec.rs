//! Stable encrypted representation of object content identity and retention.
//! These envelopes retain the original object format and AAD names.
use crate::{
    object::{ContentIdentity, DurableObject, ObjectId, Retention},
    sealing::Sealer,
    unit_of_work::StoreError,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
pub fn encode(
    object: &DurableObject,
    sealer: &Sealer,
) -> Result<(Option<Vec<u8>>, Vec<u8>), StoreError> {
    let owner = format!("durable-object:{}", object.id().as_str());
    let content = object
        .content()
        .map(|content| {
            let bytes = serde_json::to_vec(&StoredContent {
                bytes: content.bytes,
                sha256: content.sha256,
            })
            .map_err(invalid)?;
            sealer.seal(&format!("{owner}:content"), &bytes)
        })
        .transpose()?;
    let retention = serde_json::to_vec(
        &object
            .retention()
            .iter()
            .map(StoredRetention::from)
            .collect::<Vec<_>>(),
    )
    .map_err(invalid)?;
    Ok((
        content,
        sealer.seal(&format!("{owner}:retention"), &retention)?,
    ))
}
pub fn decode(
    id: &ObjectId,
    content: Option<&[u8]>,
    retention: &[u8],
    sealer: &Sealer,
) -> Result<(Option<ContentIdentity>, BTreeSet<Retention>), StoreError> {
    let owner = format!("durable-object:{}", id.as_str());
    let content = content
        .map(|blob| {
            let bytes = sealer.open(&format!("{owner}:content"), blob, "durable object content")?;
            let value: StoredContent = serde_json::from_slice(&bytes).map_err(invalid)?;
            Ok::<_, StoreError>(ContentIdentity {
                bytes: value.bytes,
                sha256: value.sha256,
            })
        })
        .transpose()?;
    let bytes = sealer.open(
        &format!("{owner}:retention"),
        retention,
        "durable object retention",
    )?;
    let retention: Vec<StoredRetention> = serde_json::from_slice(&bytes).map_err(invalid)?;
    Ok((
        content,
        retention.into_iter().map(Retention::from).collect(),
    ))
}
fn invalid(error: impl std::fmt::Display) -> StoreError {
    StoreError::Io(format!("invalid durable object: {error}"))
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredContent {
    bytes: u64,
    sha256: [u8; 32],
}

#[derive(Serialize, Deserialize)]
enum StoredRetention {
    Run(String),
    Response(String),
    Workspace(String),
    Backup(String),
}

impl From<&Retention> for StoredRetention {
    fn from(value: &Retention) -> Self {
        match value {
            Retention::Run(id) => Self::Run(id.clone()),
            Retention::Response(id) => Self::Response(id.clone()),
            Retention::Workspace(id) => Self::Workspace(id.clone()),
            Retention::Backup(id) => Self::Backup(id.clone()),
        }
    }
}
impl From<StoredRetention> for Retention {
    fn from(value: StoredRetention) -> Self {
        match value {
            StoredRetention::Run(id) => Self::Run(id),
            StoredRetention::Response(id) => Self::Response(id),
            StoredRetention::Workspace(id) => Self::Workspace(id),
            StoredRetention::Backup(id) => Self::Backup(id),
        }
    }
}
