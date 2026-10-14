//! The sealed format for values kept in the database: a format byte, the id of
//! the key that sealed it, then the ciphertext. The ciphertext is bound to its
//! owner (the row it belongs to), so a value copied to another row does not
//! open. Values are sealed before they reach SQL, so the database, its WAL and
//! its backups hold only ciphertext.
use std::sync::Arc;

use submilli_shared::secret_store::{KEY_ID_LEN, SecretCipher};

use crate::application::error::StoreError;

/// The format with a key id. Format 1 (session bindings before key ids) has
/// none.
const FORMAT: u8 = 2;

#[derive(Clone, Default)]
pub(crate) struct Sealer {
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
