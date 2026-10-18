//! Shared identities for durable execution.

/// Identity of one run. Its effects and resource handles share this namespace.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct RunId(pub String);

impl From<String> for RunId {
    fn from(value: String) -> Self {
        Self(value)
    }
}
impl From<&str> for RunId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}
impl std::ops::Deref for RunId {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}
impl std::fmt::Display for RunId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}
impl RunId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Identity of one logical call. Retries must reuse it; distinct calls must not.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct EffectId(pub String);
impl From<String> for EffectId {
    fn from(value: String) -> Self {
        Self(value)
    }
}
impl From<&str> for EffectId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}
impl std::ops::Deref for EffectId {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Display for EffectId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl EffectId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

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
