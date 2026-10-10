//! The blueprint `auth_proxy:` block. The rules and their resolution live in
//! `submilli-policy`; this ties them to the blueprint's `secrets:` block —
//! a `${secrets.X}` must name a declared secret, resolved from where the
//! declaration says — and validates the rules with the YAML path of the
//! offending key.

use submilli_policy::{AuthError, AuthSpec, Injections, SecretResolver, secret_refs};

use crate::{Blueprint, BlueprintError, Fault, SecretSource, yaml_path};

/// Resolves a declared secret from its declaration's source. Implemented by the
/// embedder (store or harness). Async so a `store:` source can `await` the
/// `SecretStore` without blocking the executor.
#[async_trait::async_trait]
pub trait SourceSecretResolver: Sync {
    async fn resolve(&self, name: &str, source: &SecretSource) -> Result<String, AuthError>;
}

/// A blueprint's declared secrets as a policy [`SecretResolver`]: a name the
/// `secrets:` block does not declare is [`AuthError::UndeclaredSecret`], and a
/// declared one resolves from its source through `inner`.
pub struct DeclaredSecrets<'a> {
    pub blueprint: &'a Blueprint,
    pub inner: &'a dyn SourceSecretResolver,
}

#[async_trait::async_trait]
impl SecretResolver for DeclaredSecrets<'_> {
    async fn resolve(&self, name: &str) -> Result<String, AuthError> {
        let source = self
            .blueprint
            .secrets
            .get(name)
            .ok_or_else(|| AuthError::UndeclaredSecret(name.to_string()))?;
        self.inner.resolve(name, source).await
    }
}

/// The first `auth_proxy` rule whose `host` matches exactly, with
/// `${secrets.X}` resolved against the declared secrets. `Ok(None)` when no
/// rule matches.
pub async fn resolve_injections(
    blueprint: &Blueprint,
    host: &str,
    resolver: &dyn SourceSecretResolver,
) -> Result<Option<Injections>, AuthError> {
    let secrets = DeclaredSecrets {
        blueprint,
        inner: resolver,
    };
    submilli_policy::resolve_injections(&blueprint.auth_proxy, host, &secrets).await
}

/// Replace every `${secrets.NAME}` in `value` with its resolved declared
/// secret. Public so other blueprint blocks that interpolate secrets (e.g. the
/// `mcp:` OAuth `client_id`) reuse the exact placeholder semantics `auth_proxy`
/// uses.
pub async fn interpolate(
    value: &str,
    blueprint: &Blueprint,
    resolver: &dyn SourceSecretResolver,
) -> Result<String, AuthError> {
    let secrets = DeclaredSecrets {
        blueprint,
        inner: resolver,
    };
    submilli_policy::interpolate(value, &secrets).await
}

/// Resolve **every declared secret**, to verify (at blueprint apply/add time)
/// that each currently produces a value on this server. Independent of
/// `auth_proxy:` — the `secrets:` block is the operator's declaration of what
/// exists in this sandbox, so all of it is checked (whether or not auth_proxy
/// references it). Point-in-time only: a later deletion from the store is the
/// runtime's concern, not a reason to reject the registration. IO lives in the embedder's [`SourceSecretResolver`].
pub async fn verify_secrets(
    blueprint: &Blueprint,
    resolver: &dyn SourceSecretResolver,
) -> Result<(), AuthError> {
    for (name, source) in &blueprint.secrets {
        // Harness secrets are bound when a session opens — there is nothing to
        // resolve at registration time, so verifying one here would reject
        // every blueprint that declares one.
        if matches!(source, SecretSource::Harness(_)) {
            continue;
        }
        resolver.resolve(name, source).await?;
    }
    Ok(())
}

/// Every rule injects something, and every `${secrets.X}` names a declared
/// secret. The undeclared-reference check is the spec's load-time error.
pub(crate) fn validate_auth_proxy(blueprint: &Blueprint) -> Result<(), BlueprintError> {
    for (i, rule) in blueprint.auth_proxy.iter().enumerate() {
        if rule.auth.is_none() && rule.headers.is_empty() && rule.query.is_empty() {
            return Err(BlueprintError::InvalidAuthProxy(Fault::at(
                yaml_path!["auth_proxy", i],
                format!(
                    "auth_proxy rule for host '{}' must set auth, headers, and/or query",
                    rule.host
                ),
            )));
        }
        if let Some(auth) = &rule.auth {
            if auth.bearer.is_some() == auth.basic.is_some() {
                return Err(BlueprintError::InvalidAuthProxy(Fault::at(
                    yaml_path!["auth_proxy", i, "auth"],
                    format!(
                        "auth_proxy rule for host '{}' must set exactly one of `auth.bearer` \
                         or `auth.basic`",
                        rule.host
                    ),
                )));
            }
            if rule
                .headers
                .keys()
                .any(|k| k.eq_ignore_ascii_case("authorization"))
            {
                return Err(BlueprintError::InvalidAuthProxy(Fault::at(
                    yaml_path!["auth_proxy", i, "auth"],
                    format!(
                        "auth_proxy rule for host '{}' sets both `auth:` and an explicit \
                         `Authorization` header — use one or the other",
                        rule.host
                    ),
                )));
            }
            for name in auth_secret_refs(auth) {
                if !blueprint.secrets.contains_key(name) {
                    let field = if auth.bearer.is_some() {
                        yaml_path!["auth_proxy", i, "auth", "bearer"]
                    } else {
                        yaml_path!["auth_proxy", i, "auth", "basic", "password"]
                    };
                    return Err(BlueprintError::InvalidAuthProxy(Fault::at(
                        field,
                        format!(
                            "auth_proxy rule for host '{}' references undeclared secret '{name}'",
                            rule.host
                        ),
                    )));
                }
            }
        }
        for (field, injections) in [("headers", &rule.headers), ("query", &rule.query)] {
            for (key, value) in injections {
                for name in secret_refs(value) {
                    if !blueprint.secrets.contains_key(name) {
                        return Err(BlueprintError::InvalidAuthProxy(Fault::at(
                            yaml_path!["auth_proxy", i, field, key],
                            format!(
                                "auth_proxy rule for host '{}' references undeclared secret '{name}'",
                                rule.host
                            ),
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

/// The declared-secret names an `auth:` method references (bearer token / basic
/// password). The basic `username` is a literal, not a secret.
fn auth_secret_refs(auth: &AuthSpec) -> Vec<&str> {
    auth.bearer
        .as_deref()
        .into_iter()
        .chain(auth.basic.as_ref().map(|b| b.password.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use base64::{Engine as _, engine::general_purpose::STANDARD};

    use super::*;
    use crate::parse;

    /// Resolves any declared secret to `"<name>-value"`, or fails a named one.
    struct StubResolver {
        missing: Option<&'static str>,
    }
    #[async_trait::async_trait]
    impl SourceSecretResolver for StubResolver {
        async fn resolve(&self, name: &str, _src: &SecretSource) -> Result<String, AuthError> {
            if self.missing == Some(name) {
                return Err(AuthError::MissingSecret(name.to_string()));
            }
            Ok(format!("{name}-value"))
        }
    }

    const BP: &str = "name: x\nsecrets:\n  K: { store: K_ENV }\n  Q: { store: Q_ENV }\nauth_proxy:\n  - host: api.example.com\n    headers:\n      Authorization: \"Bearer ${secrets.K}\"\n    query:\n      appid: \"${secrets.Q}\"\n  - host: api.example.com\n    headers:\n      X-Second: \"${secrets.K}\"\n";

    #[tokio::test]
    async fn verify_secrets_checks_all_declared_regardless_of_auth_proxy() {
        // Two declared secrets, no auth_proxy block referencing them.
        let bp =
            parse("name: x\nsecrets:\n  A: { store: A_ENV }\n  B: { store: B_ENV }\n").unwrap();
        assert!(
            verify_secrets(&bp, &StubResolver { missing: None })
                .await
                .is_ok()
        );
        let err = verify_secrets(&bp, &StubResolver { missing: Some("B") })
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::MissingSecret(n) if n == "B"));
    }

    #[tokio::test]
    async fn verify_secrets_skips_harness_declarations() {
        // A harness secret has no value until a session binds one — apply-time
        // verification must not try (and fail) to resolve it.
        let bp =
            parse("name: x\nsecrets:\n  TOKEN:\n    harness:\n      required: true\n").unwrap();
        assert!(
            verify_secrets(
                &bp,
                &StubResolver {
                    missing: Some("TOKEN")
                }
            )
            .await
            .is_ok()
        );
    }

    #[test]
    fn secret_refs_extracts_names() {
        assert_eq!(secret_refs("Bearer ${secrets.K}"), vec!["K"]);
        assert_eq!(secret_refs("${secrets.A}-${secrets.B}"), vec!["A", "B"]);
        assert!(secret_refs("no placeholders").is_empty());
    }

    #[tokio::test]
    async fn resolves_headers_and_query() {
        let bp = parse(BP).unwrap();
        let inj = resolve_injections(&bp, "api.example.com", &StubResolver { missing: None })
            .await
            .unwrap()
            .expect("rule matches");
        assert_eq!(
            inj.headers,
            vec![("Authorization".to_string(), "Bearer K-value".to_string())]
        );
        assert_eq!(
            inj.query,
            vec![("appid".to_string(), "Q-value".to_string())]
        );
    }

    #[tokio::test]
    async fn first_matching_rule_wins() {
        let bp = parse(BP).unwrap();
        let inj = resolve_injections(&bp, "api.example.com", &StubResolver { missing: None })
            .await
            .unwrap()
            .unwrap();
        // The second rule (X-Second) is shadowed by the first.
        assert!(inj.headers.iter().all(|(k, _)| k == "Authorization"));
    }

    #[tokio::test]
    async fn no_matching_host_is_none() {
        let bp = parse(BP).unwrap();
        let inj = resolve_injections(&bp, "other.host", &StubResolver { missing: None })
            .await
            .unwrap();
        assert!(inj.is_none());
    }

    #[tokio::test]
    async fn missing_secret_surfaces() {
        let bp = parse(BP).unwrap();
        let err = resolve_injections(&bp, "api.example.com", &StubResolver { missing: Some("K") })
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::MissingSecret(n) if n == "K"));
    }

    #[tokio::test]
    async fn bearer_auth_lowers_to_authorization_header() {
        let bp = parse(
            "name: x\nsecrets:\n  K: { store: K_ENV }\nauth_proxy:\n  - host: h\n    auth:\n      bearer: K\n",
        )
        .unwrap();
        let inj = resolve_injections(&bp, "h", &StubResolver { missing: None })
            .await
            .unwrap()
            .expect("rule matches");
        assert_eq!(
            inj.headers,
            vec![("Authorization".to_string(), "Bearer K-value".to_string())]
        );
    }

    #[tokio::test]
    async fn basic_auth_base64_encodes_user_and_password() {
        let bp = parse(
            "name: x\nsecrets:\n  P: { store: P_ENV }\nauth_proxy:\n  - host: h\n    auth:\n      basic:\n        username: alice\n        password: P\n",
        )
        .unwrap();
        let inj = resolve_injections(&bp, "h", &StubResolver { missing: None })
            .await
            .unwrap()
            .expect("rule matches");
        let expected = STANDARD.encode("alice:P-value");
        assert_eq!(
            inj.headers,
            vec![("Authorization".to_string(), format!("Basic {expected}"))]
        );
    }

    #[test]
    fn auth_only_rule_is_valid() {
        assert!(
            parse(
                "name: x\nsecrets:\n  K: { store: K_ENV }\nauth_proxy:\n  - host: h\n    auth:\n      bearer: K\n"
            )
            .is_ok()
        );
    }

    #[test]
    fn auth_with_explicit_authorization_header_is_rejected() {
        let err = parse(
            "name: x\nsecrets:\n  K: { store: K_ENV }\nauth_proxy:\n  - host: h\n    auth:\n      bearer: K\n    headers:\n      authorization: \"Bearer ${secrets.K}\"\n",
        )
        .unwrap_err();
        assert!(
            matches!(err, BlueprintError::InvalidAuthProxy(ref m) if m.message.contains("both")),
            "{err:?}"
        );
    }

    #[test]
    fn auth_referencing_undeclared_secret_is_rejected() {
        let err = parse(
            "name: x\nauth_proxy:\n  - host: h\n    auth:\n      basic:\n        username: alice\n        password: MISSING\n",
        )
        .unwrap_err();
        assert!(
            matches!(err, BlueprintError::InvalidAuthProxy(ref m) if m.message.contains("MISSING")),
            "{err:?}"
        );
    }

    #[test]
    fn empty_rule_is_rejected() {
        let err = parse("name: x\nauth_proxy:\n  - host: h\n").unwrap_err();
        assert!(
            matches!(err, BlueprintError::InvalidAuthProxy(ref m) if m.message.contains("auth, headers")),
            "{err:?}"
        );
    }
}
