//! Shared identities for durable execution.

/// Identity of one run. Its effects and resource handles share this namespace.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
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
    /// Allocate an identity once at admission; retries retain the original ID.
    pub fn generate() -> std::io::Result<Self> {
        let mut bytes = [0; 16];
        getrandom::getrandom(&mut bytes)
            .map_err(|error| std::io::Error::other(format!("run ID entropy: {error}")))?;
        Ok(Self(
            uuid::Builder::from_random_bytes(bytes)
                .into_uuid()
                .to_string(),
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for RunId {
    fn borrow(&self) -> &str {
        self.as_str()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_runs_have_distinct_uuid_identities() {
        let first = RunId::generate().unwrap();
        let second = RunId::generate().unwrap();
        assert_ne!(first, second);
        assert_eq!(
            uuid::Uuid::parse_str(first.as_str())
                .unwrap()
                .get_version_num(),
            4
        );
    }

    #[test]
    fn stored_identities_remain_plain_strings() {
        for value in ["legacy-reservation", "00000000-0000-4000-8000-000000000001"] {
            let id = RunId::from(value);
            let encoded = serde_json::to_value(&id).unwrap();
            assert_eq!(encoded, value);
            assert_eq!(serde_json::from_value::<RunId>(encoded).unwrap(), id);
        }
    }
}
