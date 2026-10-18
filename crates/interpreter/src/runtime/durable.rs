//! Shared identities for durable execution.

/// Identity of one logical call. Retries must reuse it; distinct calls must not.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct EffectId(pub String);
impl From<String> for EffectId {
    fn from(value: String) -> Self {
        Self(value)
    }
}
impl std::ops::Deref for EffectId {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl EffectId {
    /// Temporary per-call identity until durable execution supplies stable IDs.
    /// This stub is not replay-stable; backend retries must retain their ID.
    pub fn stub() -> std::io::Result<Self> {
        let mut bytes = [0; 16];
        getrandom::getrandom(&mut bytes)
            .map_err(|error| std::io::Error::other(format!("effect ID entropy: {error}")))?;
        Ok(uuid::Builder::from_random_bytes(bytes)
            .into_uuid()
            .to_string()
            .into())
    }
}
