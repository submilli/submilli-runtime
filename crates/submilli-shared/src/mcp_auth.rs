//! OAuth auth-state for blueprints that declare OAuth-protected MCP servers.
//!
//! A blueprint is ACTIVE only when every declared `auth: oauth2` MCP server has
//! a credential in the SecretStore; until then it is PENDING and agent
//! sessions can't bind it (see the gate in `mcp::router`). State is *derived* on
//! demand from the store — never persisted on the blueprint, so the operator's
//! declaration stays the single source of truth.
//!
//! Depositing the credential (the OAuth dance) is `submilli server mcp
//! authenticate` (handlers in `handlers::mcp_auth`); minting access tokens from
//! the stored refresh token at call time is `mcp_token`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use submilli_blueprint::{Blueprint, McpAuth, McpServer};

use crate::secret_store::{SecretStore, SecretStoreError};

/// SecretStore key holding a server's OAuth credential record.
pub fn credential_key(blueprint: &str, server: &str) -> String {
    format!("mcp_oauth/{blueprint}/{server}/credential")
}

/// The single secret persisted per OAuth-protected MCP server: the refresh token
/// plus the inputs the runtime needs to redeem it for access tokens at call time
/// (so refresh needs no `.well-known` discovery or `${secrets.X}` resolution on
/// the hot path). Access tokens are never stored — they live only in the
/// in-memory cache in `mcp_token`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OAuthCredential {
    /// Standard OAuth2: the refresh token redeemed for short-lived access tokens.
    /// `None` for providers that only issue a static access token (e.g. GitHub
    /// OAuth Apps) — see `access_token`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    /// A non-expiring access token used as-is, for providers that don't issue a
    /// refresh token. Exactly one of `refresh_token` / `access_token` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    /// The exact client id used at authenticate time (a Dynamic-Client-
    /// Registered one, or the blueprint's configured `client_id`). `None` only
    /// for token endpoints that need no client authentication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Where refresh tokens are redeemed. `None` in the static-access-token case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_endpoint: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// A credential read/write through the store: a store fault, or a stored blob
/// that isn't the JSON we wrote (corruption).
#[derive(Debug)]
pub enum CredentialError {
    Store(SecretStoreError),
    Corrupt(serde_json::Error),
}

impl std::fmt::Display for CredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CredentialError::Store(e) => write!(f, "{e}"),
            CredentialError::Corrupt(e) => write!(f, "stored credential is not valid JSON: {e}"),
        }
    }
}

impl std::error::Error for CredentialError {}

/// Read and parse a server's credential record, if one is on file.
pub async fn read_credential(
    blueprint: &str,
    server: &str,
    store: &Arc<dyn SecretStore>,
) -> Result<Option<OAuthCredential>, CredentialError> {
    match store
        .get(&credential_key(blueprint, server))
        .await
        .map_err(CredentialError::Store)?
    {
        Some(raw) => Ok(Some(
            serde_json::from_str(&raw).map_err(CredentialError::Corrupt)?,
        )),
        None => Ok(None),
    }
}

/// Serialize and store a server's credential record (overwrites any existing).
pub async fn write_credential(
    blueprint: &str,
    server: &str,
    credential: &OAuthCredential,
    store: &Arc<dyn SecretStore>,
) -> Result<(), CredentialError> {
    let raw = serde_json::to_string(credential).map_err(CredentialError::Corrupt)?;
    store
        .put(&credential_key(blueprint, server), &raw)
        .await
        .map_err(CredentialError::Store)
}

/// Whether a blueprint can be bound: ACTIVE, or PENDING with the OAuth servers
/// still awaiting `submilli server mcp authenticate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthState {
    Active,
    Pending { unauthenticated: Vec<String> },
}

impl AuthState {
    pub fn is_pending(&self) -> bool {
        matches!(self, AuthState::Pending { .. })
    }
}

/// How a declared server authenticates — for `auth-status` rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerKind {
    /// `auth: oauth2`.
    OAuth,
    /// Static `headers:` (typically `Authorization: Bearer ...`).
    StaticKey,
    /// No auth configured.
    None,
}

pub fn server_kind(server: &McpServer) -> ServerKind {
    match server.auth {
        Some(McpAuth::Oauth2 { .. }) => ServerKind::OAuth,
        None if !server.headers.is_empty() => ServerKind::StaticKey,
        None => ServerKind::None,
    }
}

fn oauth_servers(blueprint: &Blueprint) -> impl Iterator<Item = &String> {
    blueprint
        .mcp
        .iter()
        .filter(|(_, s)| server_kind(s) == ServerKind::OAuth)
        .map(|(name, _)| name)
}

/// Derive a blueprint's auth state from the SecretStore. Short-circuits to
/// `Active` without touching the store when the blueprint declares no OAuth
/// servers (the common case). A store read error is treated as "not
/// authenticated" — the gate stays conservative when it can't confirm.
pub async fn blueprint_auth_state(
    blueprint: &Blueprint,
    store: Option<&Arc<dyn SecretStore>>,
) -> AuthState {
    let mut unauthenticated = Vec::new();
    for server in oauth_servers(blueprint) {
        if !is_authenticated(&blueprint.name, server, store).await {
            unauthenticated.push(server.clone());
        }
    }
    if unauthenticated.is_empty() {
        AuthState::Active
    } else {
        AuthState::Pending { unauthenticated }
    }
}

/// Whether a single OAuth server has a credential on file.
pub async fn is_authenticated(
    blueprint: &str,
    server: &str,
    store: Option<&Arc<dyn SecretStore>>,
) -> bool {
    match store {
        Some(store) => store
            .get(&credential_key(blueprint, server))
            .await
            .ok()
            .flatten()
            .is_some(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use submilli_blueprint::parse;

    use super::*;
    use crate::secret_store::{FileSecretStore, KeySource};

    fn temp_store() -> (tempfile::TempDir, Arc<dyn SecretStore>) {
        let tmp = tempfile::tempdir().unwrap();
        let key_path = tmp.path().join("key.b64");
        fs::write(&key_path, STANDARD.encode([7u8; 32])).unwrap();
        let store =
            FileSecretStore::open(tmp.path().join("secrets"), &KeySource::File(key_path)).unwrap();
        (tmp, Arc::new(store))
    }

    const STATIC_BP: &str = "\
name: bp
secrets:
  K: { store: K }
mcp:
  linear:
    url: https://mcp.linear.app/mcp
    headers:
      Authorization: \"Bearer ${secrets.K}\"
";

    const OAUTH_BP: &str = "\
name: bp
secrets:
  CID: { store: CID }
mcp:
  salesforce:
    url: https://sf.example.com/mcp
    auth:
      type: oauth2
      client_id: \"${secrets.CID}\"
";

    #[tokio::test]
    async fn no_oauth_servers_is_active_without_touching_store() {
        let bp = parse(STATIC_BP).unwrap();
        // `None` store would panic if read; Active must short-circuit.
        assert_eq!(blueprint_auth_state(&bp, None).await, AuthState::Active);
    }

    #[tokio::test]
    async fn oauth_server_without_token_is_pending() {
        let (_tmp, store) = temp_store();
        let bp = parse(OAUTH_BP).unwrap();
        assert_eq!(
            blueprint_auth_state(&bp, Some(&store)).await,
            AuthState::Pending {
                unauthenticated: vec!["salesforce".to_string()]
            }
        );
    }

    #[tokio::test]
    async fn oauth_server_with_token_is_active() {
        let (_tmp, store) = temp_store();
        store
            .put(&credential_key("bp", "salesforce"), "refresh-xyz")
            .await
            .unwrap();
        let bp = parse(OAUTH_BP).unwrap();
        assert_eq!(
            blueprint_auth_state(&bp, Some(&store)).await,
            AuthState::Active
        );
    }

    #[tokio::test]
    async fn oauth_server_pending_when_no_store_configured() {
        let bp = parse(OAUTH_BP).unwrap();
        assert!(blueprint_auth_state(&bp, None).await.is_pending());
    }

    #[test]
    fn server_kind_classifies() {
        let bp = parse(STATIC_BP).unwrap();
        assert_eq!(server_kind(&bp.mcp["linear"]), ServerKind::StaticKey);
        let bp = parse(OAUTH_BP).unwrap();
        assert_eq!(server_kind(&bp.mcp["salesforce"]), ServerKind::OAuth);
    }
}
