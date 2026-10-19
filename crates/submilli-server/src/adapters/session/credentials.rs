//! Credential storage codec. The session domain does not know the cipher format.
use std::sync::Arc;
use submilli_blueprint::HarnessSecretBindings;
use submilli_shared::secret_store::SecretCipher;

use crate::adapters::sealing::Sealer;

fn owner(id: &str) -> String {
    format!("submilli:session:v1:{id}")
}

#[derive(Debug)]
pub struct CredentialError(String);
impl std::fmt::Display for CredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for CredentialError {}

#[derive(Clone, Default)]
pub enum SessionCredentials {
    #[default]
    Absent,
    Encrypted(Vec<u8>),
    Transient(HarnessSecretBindings),
}

impl std::fmt::Debug for SessionCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionCredentials([redacted])")
    }
}

#[derive(Default)]
pub struct CredentialCodec {
    cipher: Option<Arc<SecretCipher>>,
}

impl CredentialCodec {
    pub fn new(cipher: Option<Arc<SecretCipher>>) -> Self {
        Self { cipher }
    }
    pub fn seal(
        &self,
        id: &str,
        bindings: &HarnessSecretBindings,
        durable: bool,
    ) -> Result<SessionCredentials, CredentialError> {
        if !durable {
            return Ok(SessionCredentials::Transient(bindings.clone()));
        }
        if bindings.is_empty() {
            return Ok(SessionCredentials::Absent);
        }
        if self.cipher.is_none() {
            return Err(CredentialError(
                "configure the secret-store encryption key before supplying session secrets".into(),
            ));
        }
        let plain = serde_json::to_vec(bindings)
            .map_err(|_| CredentialError("cannot encode session bindings".into()))?;
        let blob = Sealer::new(self.cipher.clone())
            .seal(&owner(id), &plain)
            .map_err(|_| CredentialError("cannot encrypt session bindings".into()))?;
        Ok(SessionCredentials::Encrypted(blob))
    }
    pub fn open(
        &self,
        id: &str,
        credentials: &SessionCredentials,
    ) -> Result<Option<Arc<HarnessSecretBindings>>, CredentialError> {
        let blob = match credentials {
            SessionCredentials::Absent => return Ok(None),
            SessionCredentials::Transient(bindings) => return Ok(Some(Arc::new(bindings.clone()))),
            SessionCredentials::Encrypted(blob) => blob,
        };
        let plain = match blob.split_first() {
            // Sealed before blobs carried a key id.
            Some((&1, sealed)) => {
                let cipher = self.cipher.as_ref().ok_or_else(|| {
                    CredentialError(
                        "restore the configured secret-store encryption key to resume this session"
                            .into(),
                    )
                })?;
                cipher.open(sealed, owner(id).as_bytes()).map_err(|_| {
                    CredentialError(
                        "cannot decrypt session secrets; restore the original secret-store encryption key"
                            .into(),
                    )
                })?
            }
            Some((&2, _)) => Sealer::new(self.cipher.clone())
                .open(&owner(id), blob, "session secrets")
                .map_err(|error| CredentialError(error.to_string()))?,
            _ => {
                return Err(CredentialError(
                    "unsupported session binding encryption version".into(),
                ));
            }
        };
        let bindings = serde_json::from_slice(&plain)
            .map_err(|_| CredentialError("invalid encrypted session bindings".into()))?;
        Ok(Some(Arc::new(bindings)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_sealed_before_key_ids_still_open() {
        let cipher = Arc::new(SecretCipher::from_key([4; 32]));
        let bindings: HarnessSecretBindings = [("TOKEN".to_owned(), "t".to_owned())].into();
        let mut legacy = cipher
            .seal(
                &serde_json::to_vec(&bindings).unwrap(),
                owner("s").as_bytes(),
            )
            .unwrap();
        legacy.insert(0, 1);
        let codec = CredentialCodec::new(Some(cipher));
        let opened = codec
            .open("s", &SessionCredentials::Encrypted(legacy))
            .unwrap()
            .unwrap();
        assert_eq!(opened.as_ref(), &bindings);
        let SessionCredentials::Encrypted(current) = codec.seal("s", &bindings, true).unwrap()
        else {
            panic!("durable bindings are encrypted");
        };
        assert_eq!(current[0], 2);
        let other = CredentialCodec::new(Some(Arc::new(SecretCipher::from_key([5; 32]))));
        let error = other
            .open("s", &SessionCredentials::Encrypted(current))
            .unwrap_err();
        assert!(error.to_string().contains("was sealed with key"), "{error}");
    }
}
