//! Runtime-side provider for `submilli:secrets`.

use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

/// Identifies the encryption key needed to decrypt a recorded secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretKeyId(pub String);

/// The only secret value permitted in effect history. Ciphertext includes the
/// encryption format's nonce and authentication data; it never contains a key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedSecret {
    pub key_id: SecretKeyId,
    #[serde(with = "super::effects::binary")]
    pub ciphertext: Vec<u8>,
}

/// Host-provided resolver for blueprint-declared secrets.
///
/// `Ok(None)` means the secret is not available to the script: undeclared,
/// declared but missing, or otherwise absent. Infrastructure/configuration
/// failures should be returned as `Err` so scripts cannot confuse them with
/// ordinary absence.
pub trait SecretProvider: Send + Sync {
    fn get<'a>(
        &'a self,
        name: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, String>> + Send + 'a>>;

    /// Retrieve and encrypt before returning to the effect recorder. Implementations
    /// must not include plaintext credentials in either the value or an error.
    /// Direct-only providers fail closed when used with recorded execution.
    fn get_encrypted<'a>(
        &'a self,
        _name: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<EncryptedSecret>, String>> + Send + 'a>> {
        Box::pin(async { Err("secret provider does not support encrypted retrieval".into()) })
    }

    /// Decrypt outside effect recording, selecting the key by the envelope's ID.
    /// A missing key is an error, never a reason to retrieve a different credential.
    /// Errors must not include plaintext credentials.
    fn decrypt(&self, _name: &str, _secret: &EncryptedSecret) -> Result<String, String> {
        Err("secret provider does not support decryption".into())
    }
}

#[derive(Default)]
pub struct NoopSecretProvider;

impl SecretProvider for NoopSecretProvider {
    fn get_encrypted<'a>(
        &'a self,
        _name: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<EncryptedSecret>, String>> + Send + 'a>> {
        Box::pin(async { Ok(None) })
    }

    fn get<'a>(
        &'a self,
        _name: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, String>> + Send + 'a>> {
        Box::pin(async { Ok(None) })
    }
}
