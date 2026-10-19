//! OAuth auth-state for blueprints that declare OAuth-protected MCP servers.
//!
//! A blueprint is ACTIVE only when every declared `auth: oauth2` MCP server has
//! a grant in the grant store; until then it is PENDING and agent
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

/// The grant held per OAuth-protected MCP server: the refresh token
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

/// A grant as it is on file, with the revision a conditional write is checked
/// against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredGrant {
    pub credential: OAuthCredential,
    pub revision: u64,
}

/// What a conditional rotation came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    /// The rotated grant is on file under this revision.
    Saved(u64),
    /// The grant was replaced or revoked since it was read; nothing was written.
    Changed,
}

/// Whether a grant is redeemed for access tokens or used as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GrantKind {
    Refresh,
    Static,
}

impl GrantKind {
    pub fn as_str(self) -> &'static str {
        match self {
            GrantKind::Refresh => "refresh",
            GrantKind::Static => "static",
        }
    }
}

/// A grant as listed: never its tokens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantSummary {
    pub blueprint: String,
    pub server: String,
    pub kind: GrantKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Unix milliseconds of the last write, when the store records one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_unix_ms: Option<i64>,
}

impl GrantSummary {
    fn of(blueprint: &str, server: &str, credential: &OAuthCredential) -> Self {
        Self {
            blueprint: blueprint.to_owned(),
            server: server.to_owned(),
            kind: if credential.refresh_token.is_some() {
                GrantKind::Refresh
            } else {
                GrantKind::Static
            },
            client_id: credential.client_id.clone(),
            scopes: credential.scopes.clone(),
            updated_unix_ms: None,
        }
    }
}

/// A grant read or write that did not complete. The message never carries a
/// token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantStoreError {
    /// The stored grant could not be read: a wrong key or a corrupt entry.
    Unreadable(String),
    /// The grant was refused as invalid.
    Invalid(String),
    /// The store could not be reached.
    Unavailable(String),
}

impl std::fmt::Display for GrantStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GrantStoreError::Unreadable(message)
            | GrantStoreError::Invalid(message)
            | GrantStoreError::Unavailable(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for GrantStoreError {}

/// Where MCP OAuth grants are kept, one per `(blueprint, server)`. Writes that
/// follow a read are conditional on the revision read, so a refresh cannot
/// overwrite a newer login or bring back a revoked grant.
#[async_trait::async_trait]
pub trait McpGrantStore: Send + Sync + 'static {
    async fn load(
        &self,
        blueprint: &str,
        server: &str,
    ) -> Result<Option<StoredGrant>, GrantStoreError>;
    /// The revision on file, without reading the grant's tokens.
    async fn revision(&self, blueprint: &str, server: &str)
    -> Result<Option<u64>, GrantStoreError>;
    /// Create the grant, or replace whatever is on file (a new login).
    async fn deposit(
        &self,
        blueprint: &str,
        server: &str,
        credential: &OAuthCredential,
    ) -> Result<u64, GrantStoreError>;
    /// Replace the grant only if it is still at `expected`.
    async fn rotate(
        &self,
        blueprint: &str,
        server: &str,
        expected: u64,
        credential: &OAuthCredential,
    ) -> Result<Rotation, GrantStoreError>;
    /// Remove the grant; whether one was on file.
    async fn revoke(&self, blueprint: &str, server: &str) -> Result<bool, GrantStoreError>;
    async fn list(&self, blueprint: Option<&str>) -> Result<Vec<GrantSummary>, GrantStoreError>;
}

/// Grants kept as JSON in a [`SecretStore`] under
/// `mcp_oauth/<blueprint>/<server>/credential`: the CLI's local layout. Its
/// revision is a hash of what is on file, so a rotation is refused once the
/// file changed.
pub struct SecretStoreGrants {
    store: Arc<dyn SecretStore>,
}

/// The [`SecretStore`] key prefix [`SecretStoreGrants`] keeps grants under.
pub const GRANT_KEY_PREFIX: &str = "mcp_oauth/";

/// The secret-store key a blueprint's grant for an MCP server is kept under.
pub fn grant_key(blueprint: &str, server: &str) -> String {
    format!("{GRANT_KEY_PREFIX}{blueprint}/{server}/credential")
}

/// The blueprint and MCP server a [`grant_key`] names, or `None` for any other
/// key.
pub fn parse_grant_key(key: &str) -> Option<(&str, &str)> {
    key.strip_prefix(GRANT_KEY_PREFIX)?
        .strip_suffix("/credential")?
        .split_once('/')
}

impl SecretStoreGrants {
    pub fn new(store: Arc<dyn SecretStore>) -> Self {
        Self { store }
    }

    async fn raw(&self, blueprint: &str, server: &str) -> Result<Option<String>, GrantStoreError> {
        self.store
            .get(&grant_key(blueprint, server))
            .await
            .map_err(store_error)
    }

    async fn write(
        &self,
        blueprint: &str,
        server: &str,
        credential: &OAuthCredential,
    ) -> Result<u64, GrantStoreError> {
        let raw = serde_json::to_string(credential)
            .map_err(|error| GrantStoreError::Invalid(error.to_string()))?;
        self.store
            .put(&grant_key(blueprint, server), &raw)
            .await
            .map_err(store_error)?;
        Ok(content_revision(&raw))
    }
}

fn store_error(error: SecretStoreError) -> GrantStoreError {
    match error {
        SecretStoreError::Crypto(message) => GrantStoreError::Unreadable(message),
        other => GrantStoreError::Unavailable(other.to_string()),
    }
}

/// A revision derived from the stored JSON. A byte-identical credential
/// deposited again after a revoke gets the same revision; providers issue new
/// tokens per login, so that only happens when an operator re-pastes the same
/// one.
fn content_revision(raw: &str) -> u64 {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(raw.as_bytes());
    // A SHA-256 digest is 32 bytes, so its first eight always exist.
    let first: [u8; 8] = digest.first_chunk().copied().unwrap_or_default();
    u64::from_be_bytes(first)
}

fn parse_grant(raw: &str) -> Result<OAuthCredential, GrantStoreError> {
    serde_json::from_str(raw).map_err(|error| {
        GrantStoreError::Unreadable(format!("stored credential is not valid JSON: {error}"))
    })
}

#[async_trait::async_trait]
impl McpGrantStore for SecretStoreGrants {
    async fn load(
        &self,
        blueprint: &str,
        server: &str,
    ) -> Result<Option<StoredGrant>, GrantStoreError> {
        let Some(raw) = self.raw(blueprint, server).await? else {
            return Ok(None);
        };
        Ok(Some(StoredGrant {
            credential: parse_grant(&raw)?,
            revision: content_revision(&raw),
        }))
    }

    async fn revision(
        &self,
        blueprint: &str,
        server: &str,
    ) -> Result<Option<u64>, GrantStoreError> {
        Ok(self
            .raw(blueprint, server)
            .await?
            .map(|raw| content_revision(&raw)))
    }

    async fn deposit(
        &self,
        blueprint: &str,
        server: &str,
        credential: &OAuthCredential,
    ) -> Result<u64, GrantStoreError> {
        self.write(blueprint, server, credential).await
    }

    async fn rotate(
        &self,
        blueprint: &str,
        server: &str,
        expected: u64,
        credential: &OAuthCredential,
    ) -> Result<Rotation, GrantStoreError> {
        if self.revision(blueprint, server).await? != Some(expected) {
            return Ok(Rotation::Changed);
        }
        self.write(blueprint, server, credential)
            .await
            .map(Rotation::Saved)
    }

    async fn revoke(&self, blueprint: &str, server: &str) -> Result<bool, GrantStoreError> {
        self.store
            .delete(&grant_key(blueprint, server))
            .await
            .map_err(store_error)
    }

    async fn list(&self, blueprint: Option<&str>) -> Result<Vec<GrantSummary>, GrantStoreError> {
        let prefix = match blueprint {
            Some(blueprint) => format!("{GRANT_KEY_PREFIX}{blueprint}/"),
            None => GRANT_KEY_PREFIX.to_owned(),
        };
        let mut grants = Vec::new();
        for key in self.store.list(Some(&prefix)).await.map_err(store_error)? {
            let Some((blueprint, server)) = parse_grant_key(&key) else {
                continue;
            };
            // An unreadable grant fails the listing: under a wrong key every
            // grant is unreadable, and an empty list would hide that.
            if let Some(stored) = self.load(blueprint, server).await? {
                grants.push(GrantSummary::of(blueprint, server, &stored.credential));
            }
        }
        Ok(grants)
    }
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

/// Derive a blueprint's auth state from the grant store. Short-circuits to
/// `Active` without touching the store when the blueprint declares no OAuth
/// servers (the common case). A store read error is treated as "not
/// authenticated" — the gate stays conservative when it can't confirm.
pub async fn blueprint_auth_state(
    blueprint: &Blueprint,
    grants: Option<&Arc<dyn McpGrantStore>>,
) -> AuthState {
    let mut unauthenticated = Vec::new();
    for server in oauth_servers(blueprint) {
        if !is_authenticated(&blueprint.name, server, grants).await {
            unauthenticated.push(server.clone());
        }
    }
    if unauthenticated.is_empty() {
        AuthState::Active
    } else {
        AuthState::Pending { unauthenticated }
    }
}

/// Whether a single OAuth server has a grant on file.
pub async fn is_authenticated(
    blueprint: &str,
    server: &str,
    grants: Option<&Arc<dyn McpGrantStore>>,
) -> bool {
    match grants {
        Some(grants) => grants
            .revision(blueprint, server)
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

    fn temp_store() -> (
        tempfile::TempDir,
        Arc<dyn SecretStore>,
        Arc<dyn McpGrantStore>,
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let key_path = tmp.path().join("key.b64");
        fs::write(&key_path, STANDARD.encode([7u8; 32])).unwrap();
        let store: Arc<dyn SecretStore> = Arc::new(
            FileSecretStore::open(tmp.path().join("secrets"), &KeySource::File(key_path)).unwrap(),
        );
        let grants = Arc::new(SecretStoreGrants::new(store.clone()));
        (tmp, store, grants)
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
        let (_tmp, _store, grants) = temp_store();
        let bp = parse(OAUTH_BP).unwrap();
        assert_eq!(
            blueprint_auth_state(&bp, Some(&grants)).await,
            AuthState::Pending {
                unauthenticated: vec!["salesforce".to_string()]
            }
        );
    }

    #[tokio::test]
    async fn oauth_server_with_token_is_active() {
        let (_tmp, store, grants) = temp_store();
        store
            .put("mcp_oauth/bp/salesforce/credential", "refresh-xyz")
            .await
            .unwrap();
        let bp = parse(OAUTH_BP).unwrap();
        assert_eq!(
            blueprint_auth_state(&bp, Some(&grants)).await,
            AuthState::Active
        );
    }

    #[tokio::test]
    async fn oauth_server_pending_when_no_store_configured() {
        let bp = parse(OAUTH_BP).unwrap();
        assert!(blueprint_auth_state(&bp, None).await.is_pending());
    }

    fn refresh_grant(token: &str) -> OAuthCredential {
        OAuthCredential {
            refresh_token: Some(token.into()),
            access_token: None,
            client_id: Some("cid".into()),
            token_endpoint: Some("https://auth.example.com/token".into()),
            scopes: vec!["read".into()],
        }
    }

    #[tokio::test]
    async fn a_rotation_against_a_changed_grant_writes_nothing() {
        let (_tmp, _store, grants) = temp_store();
        let read = grants
            .deposit("bp", "srv", &refresh_grant("one"))
            .await
            .unwrap();
        let newer = grants
            .deposit("bp", "srv", &refresh_grant("two"))
            .await
            .unwrap();
        assert_ne!(read, newer);
        assert_eq!(
            grants
                .rotate("bp", "srv", read, &refresh_grant("rotated"))
                .await
                .unwrap(),
            Rotation::Changed
        );
        let stored = grants.load("bp", "srv").await.unwrap().unwrap();
        assert_eq!(stored.credential.refresh_token.as_deref(), Some("two"));
        assert!(matches!(
            grants
                .rotate("bp", "srv", newer, &refresh_grant("rotated"))
                .await
                .unwrap(),
            Rotation::Saved(_)
        ));
        assert!(grants.revoke("bp", "srv").await.unwrap());
        assert_eq!(
            grants
                .rotate("bp", "srv", newer, &refresh_grant("late"))
                .await
                .unwrap(),
            Rotation::Changed
        );
        assert!(grants.load("bp", "srv").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn listed_grants_carry_no_tokens() {
        let (_tmp, _store, grants) = temp_store();
        grants
            .deposit("bp", "srv", &refresh_grant("secret-token"))
            .await
            .unwrap();
        let listed = grants.list(None).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].kind, GrantKind::Refresh);
        assert!(
            !serde_json::to_string(&listed)
                .unwrap()
                .contains("secret-token")
        );
        assert!(grants.list(Some("other")).await.unwrap().is_empty());
    }

    #[test]
    fn server_kind_classifies() {
        let bp = parse(STATIC_BP).unwrap();
        assert_eq!(server_kind(&bp.mcp["linear"]), ServerKind::StaticKey);
        let bp = parse(OAUTH_BP).unwrap();
        assert_eq!(server_kind(&bp.mcp["salesforce"]), ServerKind::OAuth);
    }
}
