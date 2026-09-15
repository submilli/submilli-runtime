//! The blueprint `secrets:` block — the operator's allow-list of secret names
//! and where each value comes from. A `${secrets.X}` reference (in `auth_proxy`
//! today, `submilli:secrets.get` later) must name a secret declared here.

use std::collections::BTreeMap;

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer, de};

/// Session-scoped values supplied by a trusted harness. These bindings are
/// deliberately separate from [`Blueprint`] so callers cannot persist them as
/// part of a blueprint or durable session record.
pub type HarnessSecretBindings = BTreeMap<String, String>;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Configuration for a session-scoped harness secret.
pub struct HarnessSecret {
    /// Whether session creation and rebind reject a missing or empty value.
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Why a harness-supplied secret bag did not match the blueprint declaration.
pub enum HarnessSecretError {
    /// The harness supplied a name absent from the blueprint.
    Undeclared(String),
    /// The harness attempted to override an env, file, or store secret.
    WrongSource(String),
    /// A required harness declaration had no non-empty supplied value.
    MissingRequired(String),
}

impl std::fmt::Display for HarnessSecretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Undeclared(name) => write!(f, "secret '{name}' is not declared"),
            Self::WrongSource(name) => {
                write!(f, "secret '{name}' is not declared with a harness source")
            }
            Self::MissingRequired(name) => {
                write!(f, "required harness secret '{name}' is missing or empty")
            }
        }
    }
}

impl std::error::Error for HarnessSecretError {}

/// Where a declared secret's value is read from. The YAML is a one-key map,
/// e.g. `{ env: STRIPE_API_KEY }` or `{ file: /run/secrets/x }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretSource {
    /// Read from the server process environment variable of this name.
    Env(String),
    /// Read from the file at this path (e.g. a k8s secret mount), trimmed.
    File(String),
    /// Read from the configured `SecretStore` backend by this key, resolved per
    /// call so rotated values are picked up without a restart.
    Store(String),
    /// Supplied by the trusted harness when a session is opened or rebound.
    Harness(HarnessSecret),
}

// Hand-written so the YAML is the `{ env: X }` one-key map the spec uses —
// `serde_yml` would otherwise render the enum with `!env` tag syntax.
impl<'de> Deserialize<'de> for SecretSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Raw {
            #[serde(default)]
            env: Option<String>,
            #[serde(default)]
            file: Option<String>,
            #[serde(default)]
            store: Option<String>,
            #[serde(default)]
            harness: Option<HarnessSecret>,
        }
        let raw = Raw::deserialize(deserializer)?;
        let set = [
            raw.env.is_some(),
            raw.file.is_some(),
            raw.store.is_some(),
            raw.harness.is_some(),
        ]
        .into_iter()
        .filter(|b| *b)
        .count();
        if set != 1 {
            return Err(de::Error::custom(
                "a secret needs exactly one source: env / file / store / harness",
            ));
        }
        Ok(if let Some(v) = raw.env {
            SecretSource::Env(v)
        } else if let Some(v) = raw.file {
            SecretSource::File(v)
        } else if let Some(v) = raw.store {
            SecretSource::Store(v)
        } else {
            SecretSource::Harness(raw.harness.expect("exactly one set"))
        })
    }
}

impl Serialize for SecretSource {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            SecretSource::Env(v) => map.serialize_entry("env", v)?,
            SecretSource::File(v) => map.serialize_entry("file", v)?,
            SecretSource::Store(v) => map.serialize_entry("store", v)?,
            SecretSource::Harness(v) => map.serialize_entry("harness", v)?,
        }
        map.end()
    }
}

impl SecretSource {
    /// The source tag as it appears in YAML.
    pub fn kind(&self) -> &'static str {
        match self {
            SecretSource::Env(_) => "env",
            SecretSource::File(_) => "file",
            SecretSource::Store(_) => "store",
            SecretSource::Harness(_) => "harness",
        }
    }
}

/// Validate and normalize values supplied by a harness. Only secrets explicitly
/// declared with a `harness:` source may be bound; empty values are treated as
/// absent and required declarations must be present.
pub fn resolve_harness_secrets(
    declarations: &BTreeMap<String, SecretSource>,
    supplied: &HarnessSecretBindings,
) -> Result<HarnessSecretBindings, HarnessSecretError> {
    let mut resolved = HarnessSecretBindings::new();
    for (name, value) in supplied {
        match declarations.get(name) {
            None => return Err(HarnessSecretError::Undeclared(name.clone())),
            Some(SecretSource::Harness(_)) => {
                if !value.is_empty() {
                    resolved.insert(name.clone(), value.clone());
                }
            }
            Some(_) => return Err(HarnessSecretError::WrongSource(name.clone())),
        }
    }
    for (name, source) in declarations {
        if let SecretSource::Harness(config) = source
            && config.required
            && !resolved.contains_key(name)
        {
            return Err(HarnessSecretError::MissingRequired(name.clone()));
        }
    }
    Ok(resolved)
}

/// Names whose harness declarations require a binding before execution.
pub fn required_harness_secrets(declarations: &BTreeMap<String, SecretSource>) -> Vec<String> {
    declarations
        .iter()
        .filter_map(|(name, source)| match source {
            SecretSource::Harness(config) if config.required => Some(name.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declarations() -> BTreeMap<String, SecretSource> {
        BTreeMap::from([
            (
                "REQUIRED".into(),
                SecretSource::Harness(HarnessSecret { required: true }),
            ),
            (
                "OPTIONAL".into(),
                SecretSource::Harness(HarnessSecret::default()),
            ),
            ("SERVER".into(), SecretSource::Env("SERVER_ENV".into())),
        ])
    }

    #[test]
    fn resolves_only_declared_harness_values() {
        let supplied = BTreeMap::from([
            ("REQUIRED".into(), "one".into()),
            ("OPTIONAL".into(), "two".into()),
        ]);
        assert_eq!(
            resolve_harness_secrets(&declarations(), &supplied).unwrap(),
            supplied
        );
    }

    #[test]
    fn rejects_missing_or_empty_required_value() {
        assert!(matches!(
            resolve_harness_secrets(&declarations(), &BTreeMap::new()),
            Err(HarnessSecretError::MissingRequired(name)) if name == "REQUIRED"
        ));
        let empty = BTreeMap::from([("REQUIRED".into(), String::new())]);
        assert!(matches!(
            resolve_harness_secrets(&declarations(), &empty),
            Err(HarnessSecretError::MissingRequired(_))
        ));
    }

    #[test]
    fn rejects_undeclared_and_non_harness_values() {
        let undeclared = BTreeMap::from([
            ("REQUIRED".into(), "one".into()),
            ("OTHER".into(), "two".into()),
        ]);
        assert!(matches!(
            resolve_harness_secrets(&declarations(), &undeclared),
            Err(HarnessSecretError::Undeclared(name)) if name == "OTHER"
        ));
        let server = BTreeMap::from([
            ("REQUIRED".into(), "one".into()),
            ("SERVER".into(), "override".into()),
        ]);
        assert!(matches!(
            resolve_harness_secrets(&declarations(), &server),
            Err(HarnessSecretError::WrongSource(name)) if name == "SERVER"
        ));
    }
}
