//! Server error adapter for the shared sealed database value format.
use crate::application::error::StoreError;
use std::sync::Arc;
use submilli_shared::secret_store::{KEY_ID_LEN, SecretCipher};
#[derive(Clone, Default)]
pub(crate) struct Sealer(submilli_durable::codec::Sealer);
impl Sealer {
    pub fn new(cipher: Option<Arc<SecretCipher>>) -> Self {
        Self(submilli_durable::codec::Sealer::new(cipher))
    }
    pub fn key_id(&self) -> Option<[u8; KEY_ID_LEN]> {
        self.0.key_id()
    }
    pub fn sealed_key_id(blob: &[u8]) -> Option<&[u8]> {
        submilli_durable::codec::Sealer::sealed_key_id(blob)
    }
    pub fn seal(&self, owner: &str, plaintext: &[u8]) -> Result<Vec<u8>, StoreError> {
        self.0.seal(owner, plaintext).map_err(map_error)
    }
    pub fn open(&self, owner: &str, blob: &[u8], entry: &str) -> Result<Vec<u8>, StoreError> {
        self.0.open(owner, blob, entry).map_err(map_error)
    }
}
fn map_error(error: submilli_durable::unit_of_work::StoreError) -> StoreError {
    match error {
        submilli_durable::unit_of_work::StoreError::Credentials(message) => {
            StoreError::Credentials(message)
        }
        other => StoreError::Io(other.to_string()),
    }
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
