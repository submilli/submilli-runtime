//! Auth-proxy rules: host-keyed rules that inject auth headers and query
//! parameters into outbound HTTP, with `${secrets.X}` resolved through a
//! [`SecretResolver`]. The program never sees the resolved values.
//!
//! This module computes what to inject; the embedder supplies the resolver and
//! adapts the result to the runtime's `AuthProxy` trait.

use std::collections::BTreeMap;
use std::fmt;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

const PLACEHOLDER_PREFIX: &str = "${secrets.";

/// Every auth-proxy rule in force, and whether cleartext HTTP may be used at
/// all.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthProxyPolicy {
    /// Permit cleartext requests; a rule must opt in separately to inject into
    /// one.
    pub allow_insecure_http: bool,
    pub rules: Vec<AuthProxyRule>,
}

/// One host-keyed injection rule. v1 matches `host` exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthProxyRule {
    pub host: String,
    /// Also requires the policy-wide opt-in to permit cleartext requests.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_insecure_http: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthSpec>,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "crate::serde_support::unique_map"
    )]
    pub headers: BTreeMap<String, String>,
    #[serde(
        default,
        skip_serializing_if = "BTreeMap::is_empty",
        deserialize_with = "crate::serde_support::unique_map"
    )]
    pub query: BTreeMap<String, String>,
}

/// A first-class auth method, lowered into an `Authorization` header at resolve
/// time. Exactly one of `bearer`/`basic` is set (the format that reads rules
/// enforces it). `bearer` / basic `password` name declared secrets; basic
/// `username` is a literal. Basic auth can't be expressed as an
/// interpolation-only header value — its `base64(user:pass)` is computed *after*
/// the password secret resolves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basic: Option<BasicAuth>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BasicAuth {
    pub username: String,
    pub password: String,
}

/// Resolves a secret, by name, to its value. Implemented by the embedder, which
/// knows which names are declared and where each value comes from: a name it
/// does not declare is [`AuthError::UndeclaredSecret`]. Async so a store can be
/// awaited without blocking the executor.
#[async_trait::async_trait]
pub trait SecretResolver: Send + Sync {
    async fn resolve(&self, name: &str) -> Result<String, AuthError>;
}

/// What an auth-proxy rule contributes to a request. Header/query *names* are
/// taken verbatim; values have their `${secrets.X}` placeholders resolved.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Injections {
    pub headers: Vec<(String, String)>,
    pub query: Vec<(String, String)>,
}

/// Runtime auth-proxy failure. Maps onto the runtime's `AuthProxyError`.
#[derive(Debug)]
pub enum AuthError {
    /// `${secrets.X}` referenced an undeclared name.
    UndeclaredSecret(String),
    /// Secret declared but its source produced no value.
    MissingSecret(String),
    Other(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuthError::UndeclaredSecret(name) => write!(f, "undeclared secret '{name}'"),
            AuthError::MissingSecret(name) => write!(f, "missing secret '{name}'"),
            AuthError::Other(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for AuthError {}

/// The `${secrets.NAME}` names referenced by a value, in order.
pub fn secret_refs(value: &str) -> Vec<&str> {
    let mut names = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find(PLACEHOLDER_PREFIX) {
        let after = &rest[start + PLACEHOLDER_PREFIX.len()..];
        match after.find('}') {
            Some(end) => {
                names.push(&after[..end]);
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    names
}

/// The first rule whose `host` matches exactly, with `${secrets.X}` resolved.
/// `Ok(None)` when no rule matches.
pub async fn resolve_injections(
    rules: &[AuthProxyRule],
    host: &str,
    resolver: &dyn SecretResolver,
) -> Result<Option<Injections>, AuthError> {
    let Some(rule) = rules.iter().find(|r| r.host == host) else {
        return Ok(None);
    };
    let mut headers = interpolate_pairs(&rule.headers, resolver).await?;
    if let Some(auth) = &rule.auth {
        headers.push((
            "Authorization".to_string(),
            resolve_authorization(auth, resolver).await?,
        ));
    }
    Ok(Some(Injections {
        headers,
        query: interpolate_pairs(&rule.query, resolver).await?,
    }))
}

/// The `Authorization` header value for an `auth:` method: `Bearer <secret>` or
/// `Basic <base64(user:secret)>`. `bearer`/`password` name declared secrets and
/// are resolved directly (not `${secrets.X}` interpolation).
async fn resolve_authorization(
    auth: &AuthSpec,
    resolver: &dyn SecretResolver,
) -> Result<String, AuthError> {
    if let Some(secret) = &auth.bearer {
        return Ok(format!("Bearer {}", resolver.resolve(secret).await?));
    }
    if let Some(basic) = &auth.basic {
        let password = resolver.resolve(&basic.password).await?;
        let token = STANDARD.encode(format!("{}:{password}", basic.username));
        return Ok(format!("Basic {token}"));
    }
    Err(AuthError::Other(
        "auth_proxy rule has an empty `auth:` block".into(),
    ))
}

async fn interpolate_pairs(
    pairs: &BTreeMap<String, String>,
    resolver: &dyn SecretResolver,
) -> Result<Vec<(String, String)>, AuthError> {
    let mut out = Vec::with_capacity(pairs.len());
    for (name, value) in pairs {
        out.push((name.clone(), interpolate(value, resolver).await?));
    }
    Ok(out)
}

/// Replace every `${secrets.NAME}` in `value` with its resolved secret. Public so
/// other settings that interpolate secrets can reuse the exact placeholder
/// semantics auth-proxy rules use.
pub async fn interpolate(value: &str, resolver: &dyn SecretResolver) -> Result<String, AuthError> {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find(PLACEHOLDER_PREFIX) {
        out.push_str(&rest[..start]);
        let after = &rest[start + PLACEHOLDER_PREFIX.len()..];
        let end = after.find('}').ok_or_else(|| {
            AuthError::Other(format!(
                "unterminated '{PLACEHOLDER_PREFIX}...}}' in '{value}'"
            ))
        })?;
        out.push_str(&resolver.resolve(&after[..end]).await?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Knows `K` and `P`, resolving each to `"<name>-value"`.
    struct Stub;

    #[async_trait::async_trait]
    impl SecretResolver for Stub {
        async fn resolve(&self, name: &str) -> Result<String, AuthError> {
            match name {
                "K" | "P" => Ok(format!("{name}-value")),
                _ => Err(AuthError::UndeclaredSecret(name.to_string())),
            }
        }
    }

    fn rule(host: &str) -> AuthProxyRule {
        AuthProxyRule {
            host: host.to_string(),
            allow_insecure_http: false,
            auth: None,
            headers: BTreeMap::new(),
            query: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn rules_resolve_by_name_without_a_blueprint() {
        let rules = vec![
            AuthProxyRule {
                headers: BTreeMap::from([("X-Key".to_string(), "k=${secrets.K}".to_string())]),
                auth: Some(AuthSpec {
                    bearer: None,
                    basic: Some(BasicAuth {
                        username: "alice".to_string(),
                        password: "P".to_string(),
                    }),
                }),
                ..rule("h")
            },
            rule("other"),
        ];
        let injections = resolve_injections(&rules, "h", &Stub)
            .await
            .expect("resolves")
            .expect("rule matches");
        let basic = STANDARD.encode("alice:P-value");
        assert_eq!(
            injections.headers,
            [
                ("X-Key".to_string(), "k=K-value".to_string()),
                ("Authorization".to_string(), format!("Basic {basic}")),
            ]
        );
        assert!(
            resolve_injections(&rules, "none", &Stub)
                .await
                .expect("ok")
                .is_none()
        );
    }

    #[tokio::test]
    async fn an_undeclared_name_surfaces_from_the_resolver() {
        let err = interpolate("${secrets.NOPE}", &Stub).await.unwrap_err();
        assert!(matches!(err, AuthError::UndeclaredSecret(name) if name == "NOPE"));
    }
}
