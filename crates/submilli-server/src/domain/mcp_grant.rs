//! The OAuth grant an operator gave a blueprint's MCP server: what the server
//! redeems for access tokens when a program calls that server.
//!
//! Each login is identified, and every change moves the grant to its next
//! revision; together they are the grant's [`GrantVersion`]. Persisting a
//! change is conditional on the version it was made from, so a refresh cannot
//! overwrite a newer login or bring back a revoked grant, and a token minted
//! from a revoked grant is never mistaken for one from a later login.
use std::time::SystemTime;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GrantError {
    #[error("a grant needs a blueprint, a server and a login id")]
    MissingName,
    #[error("a grant's token must not be empty")]
    EmptyToken,
    #[error("a refresh grant needs a token endpoint")]
    MissingTokenEndpoint,
    #[error("an OAuth scope must be non-empty and hold no whitespace")]
    InvalidScope,
    #[error("a static grant has no refresh token to rotate")]
    NotRefreshable,
    #[error("the grant revision counter is exhausted")]
    RevisionExhausted,
}

/// What the server holds to authenticate to the MCP server.
#[derive(Clone, PartialEq, Eq)]
pub enum GrantCredential {
    /// Standard OAuth2: a refresh token redeemed for short-lived access tokens.
    Refresh {
        refresh_token: String,
        /// The client the grant was issued to, sent when redeeming it.
        client_id: Option<String>,
        token_endpoint: String,
    },
    /// A non-expiring access token, for providers that issue no refresh token.
    Static { access_token: String },
}

impl GrantCredential {
    fn validate(&self) -> Result<(), GrantError> {
        match self {
            GrantCredential::Refresh {
                refresh_token,
                token_endpoint,
                ..
            } => {
                if refresh_token.is_empty() {
                    return Err(GrantError::EmptyToken);
                }
                if token_endpoint.is_empty() {
                    return Err(GrantError::MissingTokenEndpoint);
                }
                Ok(())
            }
            GrantCredential::Static { access_token } if access_token.is_empty() => {
                Err(GrantError::EmptyToken)
            }
            GrantCredential::Static { .. } => Ok(()),
        }
    }

    /// The token the server keeps secret.
    pub fn token(&self) -> &str {
        match self {
            GrantCredential::Refresh { refresh_token, .. } => refresh_token,
            GrantCredential::Static { access_token } => access_token,
        }
    }
}

impl std::fmt::Debug for GrantCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GrantCredential::Refresh {
                client_id,
                token_endpoint,
                ..
            } => f
                .debug_struct("Refresh")
                .field("refresh_token", &"[redacted]")
                .field("client_id", client_id)
                .field("token_endpoint", token_endpoint)
                .finish(),
            GrantCredential::Static { .. } => f
                .debug_struct("Static")
                .field("access_token", &"[redacted]")
                .finish(),
        }
    }
}

/// OAuth sends scopes space-separated (RFC 6749 §3.3), and a scope is printable
/// ASCII without spaces, so none can be empty or hold whitespace.
fn validate_scopes(scopes: &[String]) -> Result<(), GrantError> {
    if scopes
        .iter()
        .any(|scope| scope.is_empty() || scope.contains(char::is_whitespace))
    {
        return Err(GrantError::InvalidScope);
    }
    Ok(())
}

/// Which login a grant holds, and how far it has changed since.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GrantVersion {
    pub login_id: String,
    pub revision: u64,
}

/// Whether a grant is redeemed for access tokens or used as one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantKind {
    Refresh,
    Static,
}

/// A grant as listed: everything but its token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantOverview {
    pub blueprint: String,
    pub server: String,
    pub kind: GrantKind,
    pub client_id: Option<String>,
    pub scopes: Vec<String>,
    pub updated_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpGrant {
    blueprint: String,
    server: String,
    login_id: String,
    credential: GrantCredential,
    scopes: Vec<String>,
    revision: u64,
    updated_at: SystemTime,
    /// The version on file this grant was changed from, which persisting it
    /// must still find; `None` for a grant not yet on file.
    replaces: Option<GrantVersion>,
}

impl McpGrant {
    /// A first login for `server` in `blueprint`, identified by `login_id`.
    pub fn deposit(
        blueprint: String,
        server: String,
        login_id: String,
        credential: GrantCredential,
        scopes: Vec<String>,
        now: SystemTime,
    ) -> Result<Self, GrantError> {
        Self::restore(blueprint, server, login_id, credential, scopes, 0, now)
    }

    /// A new login replacing the grant on file at `previous`, whose token it
    /// never needs to read.
    pub fn supersede(
        previous: GrantVersion,
        blueprint: String,
        server: String,
        login_id: String,
        credential: GrantCredential,
        scopes: Vec<String>,
        now: SystemTime,
    ) -> Result<Self, GrantError> {
        let revision = previous
            .revision
            .checked_add(1)
            .ok_or(GrantError::RevisionExhausted)?;
        let mut grant = Self::restore(
            blueprint, server, login_id, credential, scopes, revision, now,
        )?;
        grant.replaces = Some(previous);
        Ok(grant)
    }

    /// A grant as it was persisted.
    pub fn restore(
        blueprint: String,
        server: String,
        login_id: String,
        credential: GrantCredential,
        scopes: Vec<String>,
        revision: u64,
        updated_at: SystemTime,
    ) -> Result<Self, GrantError> {
        if blueprint.is_empty() || server.is_empty() || login_id.is_empty() {
            return Err(GrantError::MissingName);
        }
        credential.validate()?;
        validate_scopes(&scopes)?;
        Ok(Self {
            blueprint,
            server,
            login_id,
            credential,
            scopes,
            revision,
            updated_at,
            replaces: None,
        })
    }

    /// A new login, identified by `login_id`, replaces whatever the grant held.
    pub fn replace(
        &mut self,
        login_id: String,
        credential: GrantCredential,
        scopes: Vec<String>,
        now: SystemTime,
    ) -> Result<(), GrantError> {
        if login_id.is_empty() {
            return Err(GrantError::MissingName);
        }
        credential.validate()?;
        validate_scopes(&scopes)?;
        let revision = self.next_revision()?;
        self.remember_replaced();
        self.login_id = login_id;
        self.credential = credential;
        self.scopes = scopes;
        self.revision = revision;
        self.updated_at = now;
        Ok(())
    }

    /// The provider issued a new refresh token when the current one was redeemed.
    pub fn rotate_refresh_token(
        &mut self,
        token: String,
        now: SystemTime,
    ) -> Result<(), GrantError> {
        if token.is_empty() {
            return Err(GrantError::EmptyToken);
        }
        let revision = self.next_revision()?;
        if !matches!(self.credential, GrantCredential::Refresh { .. }) {
            return Err(GrantError::NotRefreshable);
        }
        self.remember_replaced();
        if let GrantCredential::Refresh { refresh_token, .. } = &mut self.credential {
            *refresh_token = token;
        }
        self.revision = revision;
        self.updated_at = now;
        Ok(())
    }

    /// The first change to a grant read from file records the version it
    /// replaces; later changes in the same unit replace that same version.
    fn remember_replaced(&mut self) {
        if self.replaces.is_none() {
            self.replaces = Some(self.version());
        }
    }

    fn next_revision(&self) -> Result<u64, GrantError> {
        self.revision
            .checked_add(1)
            .ok_or(GrantError::RevisionExhausted)
    }

    pub fn blueprint(&self) -> &str {
        &self.blueprint
    }

    pub fn server(&self) -> &str {
        &self.server
    }

    pub fn credential(&self) -> &GrantCredential {
        &self.credential
    }

    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }

    pub fn login_id(&self) -> &str {
        &self.login_id
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn version(&self) -> GrantVersion {
        GrantVersion {
            login_id: self.login_id.clone(),
            revision: self.revision,
        }
    }

    pub fn updated_at(&self) -> SystemTime {
        self.updated_at
    }

    pub fn replaces(&self) -> Option<&GrantVersion> {
        self.replaces.as_ref()
    }

    pub fn kind(&self) -> GrantKind {
        match self.credential {
            GrantCredential::Refresh { .. } => GrantKind::Refresh,
            GrantCredential::Static { .. } => GrantKind::Static,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refresh(token: &str) -> GrantCredential {
        GrantCredential::Refresh {
            refresh_token: token.into(),
            client_id: Some("cid".into()),
            token_endpoint: "https://auth.example.com/token".into(),
        }
    }

    #[test]
    fn every_change_moves_to_the_next_revision() {
        let now = SystemTime::UNIX_EPOCH;
        let mut grant = McpGrant::deposit(
            "bp".into(),
            "srv".into(),
            "l1".into(),
            refresh("one"),
            vec![],
            now,
        )
        .unwrap();
        assert_eq!(grant.revision(), 0);
        grant.rotate_refresh_token("two".into(), now).unwrap();
        assert_eq!(grant.revision(), 1);
        assert_eq!(grant.credential().token(), "two");
        grant
            .replace(
                "l2".into(),
                GrantCredential::Static {
                    access_token: "static".into(),
                },
                vec!["read".into()],
                now,
            )
            .unwrap();
        assert_eq!(
            grant.version(),
            GrantVersion {
                login_id: "l2".into(),
                revision: 2
            }
        );
        assert_eq!(
            grant.rotate_refresh_token("three".into(), now),
            Err(GrantError::NotRefreshable)
        );
        assert_eq!(grant.revision(), 2);
    }

    #[test]
    fn a_grant_needs_names_a_token_and_an_endpoint_to_redeem_it() {
        let now = SystemTime::UNIX_EPOCH;
        assert_eq!(
            McpGrant::deposit(
                String::new(),
                "srv".into(),
                "l".into(),
                refresh("t"),
                vec![],
                now
            ),
            Err(GrantError::MissingName)
        );
        assert_eq!(
            McpGrant::deposit(
                "bp".into(),
                "srv".into(),
                "l".into(),
                refresh(""),
                vec![],
                now
            ),
            Err(GrantError::EmptyToken)
        );
        assert_eq!(
            McpGrant::deposit(
                "bp".into(),
                "srv".into(),
                "l".into(),
                GrantCredential::Refresh {
                    refresh_token: "t".into(),
                    client_id: None,
                    token_endpoint: String::new(),
                },
                vec![],
                now,
            ),
            Err(GrantError::MissingTokenEndpoint)
        );
    }

    #[test]
    fn a_scope_is_one_non_empty_word() {
        let now = SystemTime::UNIX_EPOCH;
        for scope in ["", "read write", "read\twrite"] {
            assert_eq!(
                McpGrant::deposit(
                    "bp".into(),
                    "srv".into(),
                    "l".into(),
                    refresh("t"),
                    vec![scope.into()],
                    now
                ),
                Err(GrantError::InvalidScope),
                "{scope:?}"
            );
        }
        let mut grant = McpGrant::deposit(
            "bp".into(),
            "srv".into(),
            "l".into(),
            refresh("t"),
            vec!["read:user".into()],
            now,
        )
        .unwrap();
        assert_eq!(
            grant.replace("l2".into(), refresh("t2"), vec!["a b".into()], now),
            Err(GrantError::InvalidScope)
        );
    }

    #[test]
    fn a_change_replaces_the_version_it_was_read_at() {
        let now = SystemTime::UNIX_EPOCH;
        let mut grant = McpGrant::restore(
            "bp".into(),
            "srv".into(),
            "l1".into(),
            refresh("one"),
            vec![],
            4,
            now,
        )
        .unwrap();
        assert_eq!(grant.replaces(), None);
        grant.rotate_refresh_token("two".into(), now).unwrap();
        grant.rotate_refresh_token("three".into(), now).unwrap();
        assert_eq!(
            grant.replaces(),
            Some(&GrantVersion {
                login_id: "l1".into(),
                revision: 4
            }),
            "two changes in one unit still replace what was read"
        );
        let superseding = McpGrant::supersede(
            grant.version(),
            "bp".into(),
            "srv".into(),
            "l2".into(),
            refresh("new"),
            vec![],
            now,
        )
        .unwrap();
        assert_eq!(superseding.revision(), 7);
        assert_eq!(superseding.replaces(), Some(&grant.version()));
        assert_eq!(superseding.kind(), GrantKind::Refresh);
    }

    #[test]
    fn a_credential_never_prints_its_token() {
        assert!(!format!("{:?}", refresh("canary")).contains("canary"));
    }
}
