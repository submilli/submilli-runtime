//! Encoding and encryption for journal records, independent of the SQL adapter.
//!
//! Each sealed value is authenticated against its owner: run and resource/effect.
//! Mutation records also bind the operation kind, version and field. Decoding
//! checks that binding before treating the plaintext as retry evidence.
//! Size checks bound decoding; directory snapshots have their own larger bound.
//! Codecs preserve stored formats and do not decide operation state transitions.
use crate::unit_of_work::*;
use interpreter::runtime::durable::{EffectId, RunId};
use serde::{Serialize, de::DeserializeOwned};
use std::sync::Arc;
use submilli_shared::secret_store::{KEY_ID_LEN, SecretCipher};
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
pub fn journal_entry_owner(
    journal_entry: &JournalEntry,
    field: &str,
) -> Result<String, StoreError> {
    serde_json::to_string(&(
        "filesystem-operation",
        &journal_entry.run_id,
        &journal_entry.id,
        &journal_entry.operation,
        journal_entry.version,
        field,
    ))
    .map_err(invalid)
}
fn invalid(error: impl std::fmt::Display) -> StoreError {
    StoreError::Io(error.to_string())
}

/// The format with a key id. Format 1 (session bindings before key ids) has
/// none.
const FORMAT: u8 = 2;

/// Encryption boundary supplied with the application's secret-store key.
/// The encoded key ID identifies the key required to reopen existing records.
#[derive(Clone, Default)]
pub struct Sealer {
    cipher: Option<Arc<SecretCipher>>,
}

impl Sealer {
    pub fn new(cipher: Option<Arc<SecretCipher>>) -> Self {
        Self { cipher }
    }

    fn cipher(&self) -> Result<&SecretCipher, StoreError> {
        self.cipher.as_deref().ok_or_else(|| {
            StoreError::Credentials(
                "configure the secret-store encryption key before storing secrets".into(),
            )
        })
    }

    /// The id of the key [`Self::seal`] seals with, or `None` without a key.
    pub fn key_id(&self) -> Option<[u8; KEY_ID_LEN]> {
        self.cipher.as_deref().map(SecretCipher::key_id)
    }

    /// The key id a sealed blob names, without opening it.
    pub fn sealed_key_id(blob: &[u8]) -> Option<&[u8]> {
        match blob.split_first() {
            Some((&FORMAT, rest)) => rest.get(..KEY_ID_LEN),
            _ => None,
        }
    }

    pub fn seal(&self, owner: &str, plaintext: &[u8]) -> Result<Vec<u8>, StoreError> {
        let cipher = self.cipher()?;
        let sealed = cipher
            .seal(plaintext, owner.as_bytes())
            .map_err(|_| StoreError::Credentials("cannot encrypt the value".into()))?;
        let mut blob = Vec::with_capacity(1 + KEY_ID_LEN + sealed.len());
        blob.push(FORMAT);
        blob.extend_from_slice(&cipher.key_id());
        blob.extend_from_slice(&sealed);
        Ok(blob)
    }

    /// Open a value sealed for `owner`. `entry` names it in errors, which never
    /// carry the value.
    pub fn open(&self, owner: &str, blob: &[u8], entry: &str) -> Result<Vec<u8>, StoreError> {
        let Some((&FORMAT, rest)) = blob.split_first() else {
            return Err(corrupt(entry));
        };
        let (key_id, sealed) = rest
            .split_at_checked(KEY_ID_LEN)
            .ok_or_else(|| corrupt(entry))?;
        let cipher = self.cipher.as_deref().ok_or_else(|| {
            StoreError::Credentials(format!(
                "{entry} is encrypted; configure the secret-store key that sealed it (key {})",
                hex(key_id)
            ))
        })?;
        if key_id != cipher.key_id() {
            return Err(StoreError::Credentials(format!(
                "{entry} was sealed with key {}, but the configured secret-store key is {}; \
                 restore the key that sealed it",
                hex(key_id),
                hex(&cipher.key_id())
            )));
        }
        cipher
            .open(sealed, owner.as_bytes())
            .map_err(|_| corrupt(entry))
    }
}

fn corrupt(entry: &str) -> StoreError {
    StoreError::Credentials(format!("{entry} is corrupt and cannot be decrypted"))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sealer(key: u8) -> Sealer {
        Sealer::new(Some(Arc::new(SecretCipher::from_key([key; 32]))))
    }

    #[test]
    fn a_value_opens_only_for_its_owner_under_its_key() {
        let blob = sealer(1).seal("secret:a", b"canary").unwrap();
        assert!(!blob.windows(6).any(|window| window == b"canary"));
        assert_eq!(sealer(1).open("secret:a", &blob, "a").unwrap(), b"canary");
        let moved = sealer(1).open("secret:b", &blob, "b").unwrap_err();
        assert!(moved.to_string().contains("corrupt"), "{moved}");
        let wrong_key = sealer(2).open("secret:a", &blob, "a").unwrap_err();
        assert!(
            wrong_key.to_string().contains("was sealed with key"),
            "{wrong_key}"
        );
        assert!(!wrong_key.to_string().contains("canary"));
        let mut tampered = blob.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(
            sealer(1)
                .open("secret:a", &tampered, "a")
                .unwrap_err()
                .to_string()
                .contains("corrupt")
        );
        assert!(sealer(1).open("secret:a", &[9, 9], "a").is_err());
    }
}

#[derive(Default)]
pub struct ResourceColumns {
    pub request: Vec<u8>,
    pub result: Option<Vec<u8>>,
    pub error: Option<Vec<u8>>,
    pub progress: Option<Vec<u8>>,
}
pub struct JournalEntryColumns {
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
    run_id: &RunId,
    value: &Reader,
) -> Result<ResourceColumns, StoreError> {
    let owner = format!("resource:{run_id}:{}", value.id);
    Ok(ResourceColumns {
        request: encode(
            sealer,
            &owner,
            &(
                1u32,
                value.filesystem_config_fingerprint,
                &value.path,
                &value.options,
            ),
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
    run_id: &RunId,
    id: EffectId,
    columns: ResourceColumns,
) -> Result<Reader, StoreError> {
    let owner = format!("resource:{run_id}:{id}");
    let (version, filesystem_config_fingerprint, path, options): (
        u32,
        [u8; 32],
        String,
        interpreter::runtime::filesystem::ReaderOptions,
    ) = decode(sealer, &owner, &columns.request)?;
    if version != 1 {
        return Err(StoreError::Conflict);
    }
    Ok(Reader {
        filesystem_config_fingerprint,
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
    run_id: &RunId,
    value: &Listing,
) -> Result<ResourceColumns, StoreError> {
    let owner = format!("listing:{run_id}:{}", value.id);
    Ok(ResourceColumns {
        request: encode(
            sealer,
            &owner,
            &(
                1u32,
                value.filesystem_config_fingerprint,
                &value.path,
                value.recursive,
            ),
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
    run_id: &RunId,
    id: EffectId,
    columns: ResourceColumns,
) -> Result<Listing, StoreError> {
    let owner = format!("listing:{run_id}:{id}");
    let (version, filesystem_config_fingerprint, path, recursive): (u32, [u8; 32], String, bool) =
        decode(sealer, &owner, &columns.request)?;
    if version != 1 {
        return Err(StoreError::Conflict);
    }
    Ok(Listing {
        filesystem_config_fingerprint,
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
    entries: &[crate::readers::DirectoryEntry],
) -> Result<Vec<u8>, StoreError> {
    let bytes = serde_json::to_vec(entries).map_err(|e| StoreError::Io(e.to_string()))?;
    if bytes.len() > crate::readers::MAX_LISTING_BYTES {
        return Err(StoreError::Conflict);
    }
    sealer.seal(owner, &bytes)
}
fn decode_listing_output(
    sealer: &Sealer,
    owner: &str,
    bytes: &[u8],
) -> Result<Vec<crate::readers::DirectoryEntry>, StoreError> {
    if bytes.len() > crate::readers::MAX_LISTING_BYTES + 128 {
        return Err(StoreError::Conflict);
    }
    let plain = sealer.open(owner, bytes, "directory output")?;
    if plain.len() > crate::readers::MAX_LISTING_BYTES {
        return Err(StoreError::Conflict);
    }
    serde_json::from_slice(&plain).map_err(|e| StoreError::Io(e.to_string()))
}

pub fn encode_writer(
    sealer: &Sealer,
    run_id: &RunId,
    value: &Writer,
) -> Result<Vec<u8>, StoreError> {
    encode(
        sealer,
        &format!("resource:{run_id}:{}", value.id),
        &(value.filesystem_config_fingerprint, &value.path),
    )
}
pub fn decode_writer_path(
    sealer: &Sealer,
    run_id: &RunId,
    id: &EffectId,
    request: &[u8],
) -> Result<([u8; 32], String), StoreError> {
    decode(sealer, &format!("resource:{run_id}:{id}"), request)
}
pub fn encode_chunk(sealer: &Sealer, run_id: &RunId, chunk: &Chunk) -> Result<Vec<u8>, StoreError> {
    encode(
        sealer,
        &format!("resource:{run_id}:{}:{}", chunk.writer, chunk.id),
        &chunk.content,
    )
}
pub fn decode_chunk_content(
    sealer: &Sealer,
    run_id: &RunId,
    writer: &EffectId,
    id: &EffectId,
    content: &[u8],
) -> Result<crate::unit_of_work::Content, StoreError> {
    decode(sealer, &format!("resource:{run_id}:{writer}:{id}"), content)
}

pub fn encode_journal_entry(
    sealer: &Sealer,
    journal_entry: &JournalEntry,
) -> Result<JournalEntryColumns, StoreError> {
    Ok(JournalEntryColumns {
        request: seal(
            sealer,
            &journal_entry_owner(journal_entry, "request")?,
            &journal_entry.request,
        )?,
        recovery: journal_entry
            .recovery_payload
            .as_ref()
            .map(|v| seal(sealer, &journal_entry_owner(journal_entry, "recovery")?, v))
            .transpose()?,
        outcome: journal_entry
            .outcome
            .as_ref()
            .map(|v| seal(sealer, &journal_entry_owner(journal_entry, "outcome")?, v))
            .transpose()?,
    })
}
pub fn decode_journal_entry(
    sealer: &Sealer,
    mut journal_entry: JournalEntry,
    columns: JournalEntryColumns,
) -> Result<JournalEntry, StoreError> {
    journal_entry.request = open(
        sealer,
        &journal_entry_owner(&journal_entry, "request")?,
        &columns.request,
    )?;
    journal_entry.recovery_payload = columns
        .recovery
        .map(|v| {
            open(
                sealer,
                &journal_entry_owner(&journal_entry, "recovery")?,
                &v,
            )
        })
        .transpose()?;
    journal_entry.outcome = columns
        .outcome
        .map(|v| open(sealer, &journal_entry_owner(&journal_entry, "outcome")?, &v))
        .transpose()?;
    Ok(journal_entry)
}
