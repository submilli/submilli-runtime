//! Credential storage codec. The session domain does not know the cipher format.
use std::sync::Arc;
use submilli_blueprint::HarnessSecretBindings;
use submilli_shared::secret_store::SecretCipher;

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
        let cipher = self.cipher.as_ref().ok_or_else(|| {
            CredentialError(
                "configure the secret-store encryption key before supplying session secrets".into(),
            )
        })?;
        let plain = serde_json::to_vec(bindings)
            .map_err(|_| CredentialError("cannot encode session bindings".into()))?;
        let owner = format!("submilli:session:v1:{id}");
        let mut blob = cipher
            .seal(&plain, owner.as_bytes())
            .map_err(|_| CredentialError("cannot encrypt session bindings".into()))?;
        blob.insert(0, 1);
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
        let Some((&1, blob)) = blob.split_first() else {
            return Err(CredentialError(
                "unsupported session binding encryption version".into(),
            ));
        };
        let cipher = self.cipher.as_ref().ok_or_else(|| {
            CredentialError(
                "restore the configured secret-store encryption key to resume this session".into(),
            )
        })?;
        let owner = format!("submilli:session:v1:{id}");
        let plain = cipher.open(blob, owner.as_bytes()).map_err(|_| {
            CredentialError(
                "cannot decrypt session secrets; restore the original secret-store encryption key"
                    .into(),
            )
        })?;
        let bindings = serde_json::from_slice(&plain)
            .map_err(|_| CredentialError("invalid encrypted session bindings".into()))?;
        Ok(Some(Arc::new(bindings)))
    }
}
