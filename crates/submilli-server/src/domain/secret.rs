//! Operator-managed secrets: a name blueprints refer to with `store:`, and a
//! value no API returns.

/// The longest secret name accepted, in bytes.
pub const MAX_NAME_BYTES: usize = 512;

/// Names under this prefix belong to MCP OAuth grants, which are managed as
/// grants rather than as secrets.
pub const RESERVED_PREFIX: &str = "mcp_oauth/";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    #[error("secret name must contain between 1 and {MAX_NAME_BYTES} bytes")]
    NameLength,
    #[error("secret name must not contain control characters")]
    ControlCharacter,
    #[error("secret names under `{RESERVED_PREFIX}` are reserved for MCP OAuth grants")]
    Reserved,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SecretName(String);

impl SecretName {
    pub fn parse(name: String) -> Result<Self, SecretError> {
        if name.is_empty() || name.len() > MAX_NAME_BYTES {
            return Err(SecretError::NameLength);
        }
        if name.chars().any(char::is_control) {
            return Err(SecretError::ControlCharacter);
        }
        if name.starts_with(RESERVED_PREFIX) {
            return Err(SecretError::Reserved);
        }
        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A secret's value. It never prints.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretValue([redacted])")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Secret {
    name: SecretName,
    value: SecretValue,
}

impl Secret {
    pub fn new(name: SecretName, value: SecretValue) -> Self {
        Self { name, value }
    }

    pub fn name(&self) -> &SecretName {
        &self.name
    }

    pub fn value(&self) -> &SecretValue {
        &self.value
    }

    pub fn into_value(self) -> SecretValue {
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_bounded_printable_and_outside_the_grant_namespace() {
        assert!(SecretName::parse("api/TOKEN".into()).is_ok());
        assert_eq!(
            SecretName::parse(String::new()),
            Err(SecretError::NameLength)
        );
        assert_eq!(
            SecretName::parse("x".repeat(MAX_NAME_BYTES + 1)),
            Err(SecretError::NameLength)
        );
        assert_eq!(
            SecretName::parse("a\nb".into()),
            Err(SecretError::ControlCharacter)
        );
        assert_eq!(
            SecretName::parse("mcp_oauth/bp/srv/credential".into()),
            Err(SecretError::Reserved)
        );
    }

    #[test]
    fn a_value_never_prints() {
        let value = SecretValue::new("canary".into());
        assert!(!format!("{value:?}").contains("canary"));
    }
}
