//! Runtime-side provider for `submilli:secrets`.

use std::future::Future;
use std::pin::Pin;

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
}

#[derive(Default)]
pub struct NoopSecretProvider;

impl SecretProvider for NoopSecretProvider {
    fn get<'a>(
        &'a self,
        _name: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>, String>> + Send + 'a>> {
        Box::pin(async { Ok(None) })
    }
}
