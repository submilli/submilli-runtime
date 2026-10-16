//! Bounded, independently authenticated frames plus an authenticated end marker.
//! The envelope is private to the local adapter; references identify plaintext.
use crate::object::{ContentIdentity, ObjectId};
use crate::publications::ObjectStoreError;
use crate::publications::ports::PublishedContent;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use submilli_shared::secret_store::{KEY_ID_LEN, SecretCipher};

const MAGIC: &[u8; 8] = b"SMOBJ001";
pub(super) const CHUNK: usize = 64 * 1024;
const SEAL_OVERHEAD: usize = 24 + 16;
const HEADER: usize = 8 + KEY_ID_LEN + 16;
const FOOTER: usize = 8 + 8 + 32;
const END: u32 = u32::MAX;

pub(super) fn stored_bytes(plaintext: u64) -> Result<u64, ObjectStoreError> {
    plaintext
        .checked_add(
            plaintext
                .div_ceil(CHUNK as u64)
                .checked_mul((4 + SEAL_OVERHEAD) as u64)
                .ok_or(ObjectStoreError::Limit)?,
        )
        .and_then(|n| n.checked_add((HEADER + 4 + FOOTER + SEAL_OVERHEAD) as u64))
        .ok_or(ObjectStoreError::Limit)
}

pub(super) fn encode(
    source: &mut dyn Read,
    output: &mut dyn Write,
    cipher: &SecretCipher,
    id: &ObjectId,
    maximum: u64,
) -> Result<PublishedContent, ObjectStoreError> {
    let mut header = [0; HEADER];
    header[..8].copy_from_slice(MAGIC);
    header[8..8 + KEY_ID_LEN].copy_from_slice(&cipher.key_id());
    getrandom::getrandom(&mut header[8 + KEY_ID_LEN..])
        .map_err(|error| ObjectStoreError::Worker(error.to_string()))?;
    output.write_all(&header)?;
    let mut buffer = vec![0; CHUNK];
    let mut digest = Sha256::new();
    let mut bytes = 0u64;
    let mut index = 0u64;
    loop {
        let count = fill(source, &mut buffer)?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .filter(|size| *size <= maximum)
            .ok_or(ObjectStoreError::Limit)?;
        let value = &buffer[..count];
        digest.update(value);
        write_frame(output, cipher, &header, id, index, count as u32, value)?;
        crate::checkpoint("chunk");
        index = index.checked_add(1).ok_or(ObjectStoreError::Limit)?;
    }
    let content = ContentIdentity {
        bytes,
        sha256: digest.finalize().into(),
    };
    let mut footer = Vec::with_capacity(FOOTER);
    footer.extend_from_slice(&index.to_be_bytes());
    footer.extend_from_slice(&bytes.to_be_bytes());
    footer.extend_from_slice(&content.sha256);
    write_frame(output, cipher, &header, id, index, END, &footer)?;
    output.flush()?;
    Ok(PublishedContent {
        stored_bytes: stored_bytes(bytes)?,
        identity: content,
    })
}

pub(super) fn decode(
    input: &mut dyn Read,
    output: &mut dyn Write,
    cipher: &SecretCipher,
    id: &ObjectId,
    maximum: u64,
) -> Result<PublishedContent, ObjectStoreError> {
    let mut header = [0; HEADER];
    exact(input, &mut header)?;
    if &header[..8] != MAGIC {
        return Err(ObjectStoreError::Integrity);
    }
    if header[8..8 + KEY_ID_LEN] != cipher.key_id() {
        return Err(ObjectStoreError::Key);
    }
    let mut index = 0u64;
    let mut bytes = 0u64;
    let mut digest = Sha256::new();
    let mut short_frame = false;
    loop {
        let mut length = [0; 4];
        exact(input, &mut length)?;
        let marker = u32::from_be_bytes(length);
        let plain_length = if marker == END {
            FOOTER
        } else {
            marker as usize
        };
        if marker != END && (plain_length == 0 || plain_length > CHUNK || short_frame) {
            return Err(ObjectStoreError::Integrity);
        }
        let mut sealed = vec![0; plain_length + SEAL_OVERHEAD];
        exact(input, &mut sealed)?;
        let value = cipher
            .open(&sealed, &associated_data(&header, id, index, marker))
            .map_err(|_| ObjectStoreError::Integrity)?;
        if value.len() != plain_length {
            return Err(ObjectStoreError::Integrity);
        }
        if marker == END {
            let content = ContentIdentity {
                bytes,
                sha256: digest.finalize().into(),
            };
            verify_footer(input, &value, index, &content)?;
            output.flush()?;
            return Ok(PublishedContent {
                stored_bytes: stored_bytes(bytes)?,
                identity: content,
            });
        }
        bytes = bytes
            .checked_add(value.len() as u64)
            .filter(|size| *size <= maximum)
            .ok_or(ObjectStoreError::Integrity)?;
        short_frame = value.len() < CHUNK;
        digest.update(&value);
        output.write_all(&value)?;
        index = index.checked_add(1).ok_or(ObjectStoreError::Integrity)?;
    }
}

fn verify_footer(
    input: &mut dyn Read,
    value: &[u8],
    index: u64,
    content: &ContentIdentity,
) -> Result<(), ObjectStoreError> {
    // decode checks this frame's exact length before calling; checked slices
    // also make this helper safe if another caller is introduced.
    if value.get(..8) != Some(index.to_be_bytes().as_slice())
        || value.get(8..16) != Some(content.bytes.to_be_bytes().as_slice())
        || value.get(16..) != Some(content.sha256.as_slice())
        || input.read(&mut [0; 1])? != 0
    {
        return Err(ObjectStoreError::Integrity);
    }
    Ok(())
}

fn write_frame(
    output: &mut dyn Write,
    cipher: &SecretCipher,
    header: &[u8],
    id: &ObjectId,
    index: u64,
    marker: u32,
    bytes: &[u8],
) -> Result<(), ObjectStoreError> {
    let sealed = cipher
        .seal(bytes, &associated_data(header, id, index, marker))
        .map_err(|_| ObjectStoreError::Key)?;
    output.write_all(&marker.to_be_bytes())?;
    output.write_all(&sealed)?;
    Ok(())
}

fn associated_data(header: &[u8], id: &ObjectId, index: u64, marker: u32) -> Vec<u8> {
    let mut aad = b"submilli:durable-object:1:".to_vec();
    aad.extend_from_slice(id.as_str().as_bytes());
    aad.extend_from_slice(header);
    aad.extend_from_slice(&index.to_be_bytes());
    aad.extend_from_slice(&marker.to_be_bytes());
    aad
}

fn fill(source: &mut dyn Read, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match source.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(count) if count <= buffer.len() - filled => filled += count,
            Ok(_) => {
                return Err(std::io::Error::other(
                    "payload source returned an invalid byte count",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

fn exact(input: &mut dyn Read, bytes: &mut [u8]) -> Result<(), ObjectStoreError> {
    input.read_exact(bytes).map_err(|error| {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            ObjectStoreError::Integrity
        } else {
            ObjectStoreError::Io(error)
        }
    })
}

#[cfg(test)]
mod tests;
